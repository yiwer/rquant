//! 日线选股器：多树并行集成 → 优质分 + 投机形态标注（双输出）+ 历史回测验证。

pub mod config;
pub mod combine;
pub mod backtest;

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

use crate::backtest::portfolio::{build_timeline, is_fresh, select_top, select_top_per_sector};
use crate::screen::combine::{combine, CombineOutput, MergeParams};
use crate::screen::config::{load_screen_config, ScreenConfig};

/// 读 symbol→行业 映射 CSV（首列 symbol、次列 industry；跳表头/空行；忽略其余列）。
/// 供行业中性选股（per_sector）。如 data/baostock/sector_membership.csv。
pub fn load_sector_map(path: &std::path::Path) -> Result<std::collections::HashMap<String, String>> {
    let text = std::fs::read_to_string(path)?;
    let mut m = std::collections::HashMap::new();
    for (i, line) in text.lines().enumerate() {
        if i == 0 || line.trim().is_empty() {
            continue;
        }
        let mut it = line.split(',');
        if let (Some(sym), Some(ind)) = (it.next(), it.next()) {
            let (sym, ind) = (sym.trim(), ind.trim());
            if !sym.is_empty() && !ind.is_empty() {
                m.insert(sym.to_string(), ind.to_string());
            }
        }
    }
    Ok(m)
}

/// 读 symbol 名单 CSV（首列 symbol，跳表头/空行）→集合。供 --exclude-st 排除高风险股(data/baostock/st_symbols.csv)。
pub fn load_symbol_set(path: &std::path::Path) -> Result<std::collections::HashSet<String>> {
    let text = std::fs::read_to_string(path)?;
    let mut set = std::collections::HashSet::new();
    for (i, line) in text.lines().enumerate() {
        if i == 0 || line.trim().is_empty() {
            continue;
        }
        if let Some(sym) = line.split(',').next() {
            let sym = sym.trim();
            if !sym.is_empty() {
                set.insert(sym.to_string());
            }
        }
    }
    Ok(set)
}

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
    /// Point-in-time membership CSV (date,symbol); when set, restricts candidates to members
    /// effective at the as-of time. None → no restriction.
    pub membership_path: Option<PathBuf>,
    /// symbol→行业 CSV；配 config.merge.per_sector=Some(k) 时行业中性选股（每行业 top-k）。None → 全局。
    pub sectors_path: Option<PathBuf>,
    /// ST/*ST 名单 CSV（symbol 列）；Some 时从候选剔除这些高风险股(选股前剔除→top-N 自动回补非ST)。None → 不剔除。
    pub st_symbols_path: Option<PathBuf>,
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
    fundamentals: Option<&crate::data::fundamentals::FundamentalSeries>,
) -> Result<Option<(f64, String)>> {
    if !is_fresh(primary, t) {
        return Ok(None);
    }
    let ctx = crate::features::context::build_context(primary, context, &[], aux, fundamentals, t, window);
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
    let sectors = cfg.sectors_path.as_ref().map(|p| load_sector_map(p)).transpose()?.unwrap_or_default();
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
    let mut funds: Vec<Option<crate::data::fundamentals::FundamentalSeries>> = Vec::with_capacity(universe.len());
    for e in &universe {
        funds.push(e.fundamentals.as_ref().map(|p| crate::data::fundamentals::load_fundamentals_csv(p)).transpose()?);
    }

    let timeline = build_timeline(&primaries);
    let t = pick_as_of(&timeline, cfg.as_of)?;
    let membership = cfg.membership_path.as_ref()
        .map(|p| crate::data::membership::Membership::load_csv(p)).transpose()?;
    let st_set = cfg.st_symbols_path.as_ref().map(|p| load_symbol_set(p)).transpose()?;
    let aux: BTreeMap<String, AuxTable> = BTreeMap::new();
    let mp = MergeParams {
        theta_fire: sc.merge.theta_fire,
        vote_frac: sc.merge.vote_frac,
        q_floor: sc.merge.q_floor,
        lambda: sc.merge.lambda,
        tilt_setups: sc.merge.tilt_setups.clone(),
    };
    let top = cfg.top.unwrap_or(sc.merge.top);

    let mut rows: Vec<ScreenRow> = Vec::new();
    for (i, e) in universe.iter().enumerate() {
        if !is_fresh(&primaries[i], t) {
            continue;
        }
        if let Some(m) = &membership {
            match m.effective_at(t) {
                Some(set) if set.contains(&e.symbol) => {}
                _ => continue, // 非当期成员（或早于首期）→ 跳过
            }
        }
        if let Some(st) = &st_set {
            if st.contains(&e.symbol) {
                continue; // ST/*ST 高风险股 → 选股前剔除（top-N 自动回补非 ST）
            }
        }
        let mut reasons: Vec<ScreenReason> = Vec::new();
        let mut q_scores: Vec<f64> = Vec::new();
        for (name, tree) in &quality {
            if let Some((s, leaf)) = score_and_leaf(&primaries[i], &contexts[i], &aux, tree, llm, t, cfg.window, funds[i].as_ref()).await? {
                q_scores.push(s);
                reasons.push(ScreenReason { tree: name.clone(), leaf, score: s });
            }
        }
        let mut setup_scores: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        let mut fired_reasons: Vec<ScreenReason> = Vec::new();
        for (tag, trees) in &setups {
            let mut v = Vec::new();
            for (name, tree) in trees {
                if let Some((s, leaf)) = score_and_leaf(&primaries[i], &contexts[i], &aux, tree, llm, t, cfg.window, funds[i].as_ref()).await? {
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
    let picked = match sc.merge.per_sector {
        Some(k) => select_top_per_sector(&scores, &sectors, k), // 行业中性：每行业 top-k
        None => select_top(&scores, top),
    };
    let chosen: std::collections::BTreeSet<String> = picked.into_iter().map(|(s, _)| s).collect();
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
            membership_path: None,
            sectors_path: None,
            st_symbols_path: None,
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

    #[tokio::test]
    async fn screen_exclude_st_drops_listed_symbols() {
        let q = write_tmp(".yaml", QUALITY_SIMPLE);
        let m = write_tmp(".yaml", MOM_SIMPLE);
        let cfg_yaml = format!(
            "quality_trees: [{}]\nsetup_trees:\n  动量延续: [{}]\nmerge: {{ q_floor: 0.5, top: 1 }}\n",
            q.path().to_str().unwrap().replace('\\', "/"),
            m.path().to_str().unwrap().replace('\\', "/"),
        );
        let cfg_f = write_tmp(".yaml", &cfg_yaml);
        // 两只同样上行（均合格）的标的：无 ST 时 top-1 取 UP（同分按 symbol 升序）；
        // UP 被 ST 剔除后，唯一合格候选 UP2 回补 top-1 槽。
        let f_up = write_bars(true);
        let f_up2 = write_bars(true);
        let mut univ = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        writeln!(univ, "symbol,primary\nUP,{}\nUP2,{}",
            f_up.path().to_str().unwrap(), f_up2.path().to_str().unwrap()).unwrap();
        univ.flush().unwrap();
        let st = write_tmp(".csv", "symbol,name\nUP,*ST示例\n");
        let run = ScreenRunConfig {
            config_path: cfg_f.path().to_path_buf(),
            universe_path: univ.path().to_path_buf(),
            as_of: None, top: None, window: 10, out_path: None,
            membership_path: None, sectors_path: None,
            st_symbols_path: Some(st.path().to_path_buf()),
        };
        let res = run_screen(&run, &LlmEvaluator::Disabled).await.unwrap();
        assert_eq!(res.n_universe, 2, "UP 仍计入 universe 总数（加载后才剔除）");
        assert!(res.rows.iter().all(|r| r.symbol != "UP"), "ST symbol UP must be excluded from rows");
        let up2 = res.rows.iter().find(|r| r.symbol == "UP2").unwrap();
        assert!(up2.selected, "non-ST UP2 should backfill the top-1 slot after UP excluded");
    }
}
