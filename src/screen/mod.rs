//! 日线选股器：多树并行集成 → 优质分 + 投机形态标注（双输出）+ 历史回测验证。

pub mod config;
pub mod combine;

use crate::data::aux_table::AuxTable;
use crate::data::bar::Bar;
use crate::eval::llm::LlmEvaluator;
use crate::tree::loader::Tree;
use crate::tree::schema::Stance;
use crate::Result;
use chrono::{NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::backtest::portfolio::{build_timeline, is_fresh, select_top};
use crate::screen::combine::{combine, CombineOutput, MergeParams};
use crate::screen::config::{load_screen_config, ScreenConfig};

/// 单棵树命中理由。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenReason {
    pub tree: String,
    pub leaf: String,
    pub score: f64,
}

/// 单股选股记录（双输出：tags 标注 + combined_score 排名）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenRow {
    pub symbol: String,
    pub rank: usize,
    pub quality_score: f64,
    pub speculative_score: f64,
    pub combined_score: f64,
    pub tags: Vec<String>,
    pub selected: bool,
    pub reasons: Vec<ScreenReason>,
}

/// 选股结果（as-of 某根 K）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenResult {
    pub as_of: NaiveDateTime,
    pub n_universe: usize,
    pub top: usize,
    pub rows: Vec<ScreenRow>,
}

/// as-of 选股运行配置。
pub struct ScreenRunConfig {
    pub config_path: PathBuf,
    pub universe_path: PathBuf,
    pub as_of: Option<NaiveDate>,
    pub top: Option<usize>,
    pub window: usize,
    pub out_path: Option<PathBuf>,
}

fn dir(s: Stance) -> f64 {
    match s {
        Stance::Long => 1.0,
        Stance::Short => -1.0,
        Stance::Flat => 0.0,
    }
}

/// 硬模式：返回 (得分, 叶名)；不新鲜 → None。用于 as-of 的可解释路径。
#[allow(clippy::too_many_arguments)]
async fn score_and_leaf(
    primary: &[Bar],
    context: &[Bar],
    aux: &BTreeMap<String, AuxTable>,
    tree: &Tree,
    llm: &LlmEvaluator,
    t: NaiveDateTime,
    window: usize,
) -> Result<Option<(f64, String)>> {
    if !is_fresh(primary, t) {
        return Ok(None);
    }
    let ctx = crate::features::context::build_context(primary, context, &[], aux, t, window);
    let tr = crate::engine::traversal::traverse(tree, &ctx, llm).await?;
    let score = tree.leaves.get(&tr.leaf).map_or(0.0, |l| l.weight_at(&ctx) * dir(l.stance));
    Ok(Some((score, tr.leaf.clone())))
}

/// 加载配置声明的所有树：(name, Tree)。
fn load_trees(paths: &[PathBuf]) -> Result<Vec<(String, Tree)>> {
    let mut out = Vec::with_capacity(paths.len());
    for p in paths {
        let t = crate::tree::loader::load_tree_file(p)?;
        out.push((t.meta.name.clone(), t));
    }
    Ok(out)
}

/// 选取 as-of 时间戳：给定日期 → ≤该日期的最大时间线点；否则 → 末点。
fn pick_as_of(timeline: &[NaiveDateTime], as_of: Option<NaiveDate>) -> Result<NaiveDateTime> {
    if timeline.is_empty() {
        return Err(crate::Error::Data("screen: empty timeline".into()));
    }
    match as_of {
        None => Ok(*timeline.last().unwrap()),
        Some(d) => timeline
            .iter()
            .rev()
            .find(|t| t.date() <= d)
            .copied()
            .ok_or_else(|| crate::Error::Data(format!("screen: no bar on/before {d}"))),
    }
}

/// as-of 选股：并行跑树集成 → 合并 → 排名 → ScreenResult。
pub async fn run_screen(cfg: &ScreenRunConfig, llm: &LlmEvaluator) -> Result<ScreenResult> {
    let sc: ScreenConfig = load_screen_config(&cfg.config_path)?;
    let quality = load_trees(&sc.quality_trees)?;
    let mut setups: BTreeMap<String, Vec<(String, Tree)>> = BTreeMap::new();
    for (tag, paths) in &sc.setup_trees {
        setups.insert(tag.clone(), load_trees(paths)?);
    }

    let universe = crate::data::universe::read_universe_csv(&cfg.universe_path)?;
    let mut primaries: Vec<Vec<Bar>> = Vec::with_capacity(universe.len());
    let mut contexts: Vec<Vec<Bar>> = Vec::with_capacity(universe.len());
    for e in &universe {
        primaries.push(crate::data::reader::read_bars_csv(&e.primary)?);
        contexts.push(crate::data::reader::read_bars_csv(&e.context)?);
    }

    let timeline = build_timeline(&primaries);
    let t = pick_as_of(&timeline, cfg.as_of)?;
    let aux: BTreeMap<String, AuxTable> = BTreeMap::new();
    let mp = MergeParams {
        theta_fire: sc.merge.theta_fire,
        vote_frac: sc.merge.vote_frac,
        q_floor: sc.merge.q_floor,
    };
    let top = cfg.top.unwrap_or(sc.merge.top);

    let mut rows: Vec<ScreenRow> = Vec::new();
    for (i, e) in universe.iter().enumerate() {
        if !is_fresh(&primaries[i], t) {
            continue;
        }
        let mut reasons: Vec<ScreenReason> = Vec::new();
        let mut q_scores: Vec<f64> = Vec::new();
        for (name, tree) in &quality {
            if let Some((s, leaf)) = score_and_leaf(&primaries[i], &contexts[i], &aux, tree, llm, t, cfg.window).await? {
                q_scores.push(s);
                reasons.push(ScreenReason { tree: name.clone(), leaf, score: s });
            }
        }
        let mut setup_scores: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        let mut fired_reasons: Vec<ScreenReason> = Vec::new();
        for (tag, trees) in &setups {
            let mut v = Vec::new();
            for (name, tree) in trees {
                if let Some((s, leaf)) = score_and_leaf(&primaries[i], &contexts[i], &aux, tree, llm, t, cfg.window).await? {
                    v.push(s);
                    if s >= mp.theta_fire {
                        fired_reasons.push(ScreenReason { tree: name.clone(), leaf, score: s });
                    }
                }
            }
            setup_scores.insert(tag.clone(), v);
        }
        reasons.extend(fired_reasons);

        let out: CombineOutput = combine(&q_scores, &setup_scores, &mp);
        rows.push(ScreenRow {
            symbol: e.symbol.clone(),
            rank: 0,
            quality_score: out.quality_score,
            speculative_score: out.speculative_score,
            combined_score: out.combined_score,
            tags: out.tags,
            selected: false,
            reasons,
        });
    }

    rows.sort_by(|a, b| {
        b.combined_score
            .partial_cmp(&a.combined_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.symbol.cmp(&b.symbol))
    });
    for (idx, r) in rows.iter_mut().enumerate() {
        r.rank = idx + 1;
    }
    let scores: Vec<(String, f64)> = rows.iter().map(|r| (r.symbol.clone(), r.combined_score)).collect();
    let chosen: std::collections::BTreeSet<String> =
        select_top(&scores, top).into_iter().map(|(s, _)| s).collect();
    for r in rows.iter_mut() {
        r.selected = chosen.contains(&r.symbol);
    }

    let result = ScreenResult { as_of: t, n_universe: universe.len(), top, rows };
    if let Some(p) = &cfg.out_path {
        let json = serde_json::to_string_pretty(&result)?;
        std::fs::write(p, json)?;
    }
    Ok(result)
}

/// 打印选股摘要（选出的标的 + 标签 + 分数）。
pub fn print_screen(r: &ScreenResult) {
    println!("=== rquant SCREEN @ {} （universe {}，top {}）===", r.as_of, r.n_universe, r.top);
    println!("{:<10} {:>4} {:>7} {:>7} {:>7}  标签", "标的", "排名", "优质", "投机", "综合");
    for row in r.rows.iter().filter(|x| x.selected) {
        println!(
            "{:<10} {:>4} {:>7.3} {:>7.3} {:>7.3}  {}",
            row.symbol, row.rank, row.quality_score, row.speculative_score, row.combined_score,
            row.tags.join("/")
        );
    }
    let n_sel = r.rows.iter().filter(|x| x.selected).count();
    println!("入选 {n_sel} 只（共 {} 只评估）", r.rows.len());
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use std::io::Write;

    fn daily(y: i32, m: u32, d: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(0, 0, 0).unwrap()
    }

    const QUALITY_SIMPLE: &str = r#"
meta: { name: q_simple, forward_window: 1, stances: [long, flat] }
root: g
nodes:
  g:
    type: quant
    branches:
      - when: "close > sma(close, 3)"
        goto: leaf_long
        label: up
    default: { goto: leaf_flat, label: flat }
leaves:
  leaf_long: { stance: long, weight: 1.0 }
  leaf_flat: { stance: flat }
"#;
    const MOM_SIMPLE: &str = r#"
meta: { name: m_simple, forward_window: 1, stances: [long, flat] }
root: g
nodes:
  g:
    type: quant
    branches:
      - when: "close > ref(close, 2)"
        goto: leaf_long
        label: up
    default: { goto: leaf_flat, label: flat }
leaves:
  leaf_long: { stance: long, weight: 1.0 }
  leaf_flat: { stance: flat }
"#;

    fn write_tmp(suffix: &str, content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::Builder::new().suffix(suffix).tempfile().unwrap();
        write!(f, "{content}").unwrap();
        f.flush().unwrap();
        f
    }

    fn write_bars(rising: bool) -> tempfile::NamedTempFile {
        let mut f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        writeln!(f, "time,open,high,low,close,volume").unwrap();
        let mut price = 100.0;
        for d in 1..=10u32 {
            writeln!(f, "{},{p},{p},{p},{p},1000", daily(2024, 1, d).format("%Y-%m-%d %H:%M:%S"), p = price).unwrap();
            price *= if rising { 1.02 } else { 0.98 };
        }
        f.flush().unwrap();
        f
    }

    #[tokio::test]
    async fn screen_selects_rising_symbol_with_tag() {
        let q = write_tmp(".yaml", QUALITY_SIMPLE);
        let m = write_tmp(".yaml", MOM_SIMPLE);
        let cfg_yaml = format!(
            "quality_trees: [{}]\nsetup_trees:\n  动量延续: [{}]\nmerge: {{ q_floor: 0.5, top: 1 }}\n",
            q.path().to_str().unwrap().replace('\\', "/"),
            m.path().to_str().unwrap().replace('\\', "/"),
        );
        let cfg_f = write_tmp(".yaml", &cfg_yaml);

        let f_up = write_bars(true);
        let f_dn = write_bars(false);
        let mut univ = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        writeln!(univ, "symbol,primary\nUP,{}\nDN,{}",
            f_up.path().to_str().unwrap(), f_dn.path().to_str().unwrap()).unwrap();
        univ.flush().unwrap();

        let run = ScreenRunConfig {
            config_path: cfg_f.path().to_path_buf(),
            universe_path: univ.path().to_path_buf(),
            as_of: None,
            top: None,
            window: 10,
            out_path: None,
        };
        let res = run_screen(&run, &LlmEvaluator::Disabled).await.unwrap();
        assert_eq!(res.n_universe, 2);
        let up = res.rows.iter().find(|r| r.symbol == "UP").unwrap();
        let dn = res.rows.iter().find(|r| r.symbol == "DN").unwrap();
        assert!(up.selected, "rising symbol should be selected");
        assert!(up.tags.contains(&"动量延续".to_string()));
        assert!(!dn.selected, "falling symbol should not be selected");
        assert_eq!(up.rank, 1);
    }
}
