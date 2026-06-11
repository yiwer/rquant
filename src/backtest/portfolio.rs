//! 横截面组合层：时间线/新鲜度/打分/select_top/accrue/turnover + 组合循环。

use crate::data::bar::Bar;
use crate::eval::llm::LlmEvaluator;
use crate::Result;
use crate::Error;
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::PathBuf;

// ─────────────────────────────────────────────────────────────────────────────
// 纯函数：时间线 / 新鲜度 / 打分 / select_top
// ─────────────────────────────────────────────────────────────────────────────

/// 全标的 bar 时间有序并集。
pub fn build_timeline(all: &[Vec<Bar>]) -> Vec<NaiveDateTime> {
    let mut set = BTreeSet::new();
    for bars in all {
        for b in bars {
            set.insert(b.time);
        }
    }
    set.into_iter().collect()
}

/// t 时刻最后已知收盘价（time ≤ t）。
pub fn last_close_at(bars: &[Bar], t: NaiveDateTime) -> Option<f64> {
    let cut = bars.partition_point(|b| b.time <= t);
    if cut == 0 { None } else { Some(bars[cut - 1].close) }
}

/// 新鲜：恰有 bar 在 t（停牌标的当期出局）。
pub fn is_fresh(bars: &[Bar], t: NaiveDateTime) -> bool {
    bars.binary_search_by_key(&t, |b| b.time).is_ok()
}

/// score>0 取前 n：score 降序、并列 symbol 升序（确定性）。
pub fn select_top(scores: &[(String, f64)], n: usize) -> Vec<(String, f64)> {
    let mut pos: Vec<(String, f64)> = scores.iter().filter(|(_, s)| *s > 0.0).cloned().collect();
    pos.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)));
    pos.truncate(n);
    pos
}

// ─────────────────────────────────────────────────────────────────────────────
// 纯函数：记账（accrue / turnover_between）
// ─────────────────────────────────────────────────────────────────────────────

/// 区间收益：Σ w·(px_end/px_start − 1)；缺价成员贡献 0（防御；spec 保证持有成员价存在）。
pub fn accrue(
    weights: &BTreeMap<String, f64>,
    px_start: &BTreeMap<String, f64>,
    px_end: &BTreeMap<String, f64>,
) -> f64 {
    weights.iter().map(|(s, w)| {
        match (px_start.get(s), px_end.get(s)) {
            (Some(a), Some(b)) if *a > 0.0 => w * (b / a - 1.0),
            _ => 0.0,
        }
    }).sum()
}

/// 换手：Σ_union |w_new − w_old|。
pub fn turnover_between(old: &BTreeMap<String, f64>, new: &BTreeMap<String, f64>) -> f64 {
    let keys: BTreeSet<&String> = old.keys().chain(new.keys()).collect();
    keys.into_iter()
        .map(|k| {
            (new.get(k).copied().unwrap_or(0.0)
                - old.get(k).copied().unwrap_or(0.0))
            .abs()
        })
        .sum()
}

// ─────────────────────────────────────────────────────────────────────────────
// 打分（依赖 tree/llm，async）
// ─────────────────────────────────────────────────────────────────────────────

/// 单标的在 t 的横截面分数：不新鲜 → None；硬=叶 dir×weight；软=E=Σp·w·dir。
#[allow(clippy::too_many_arguments)]
pub async fn score_symbol(
    primary: &[Bar],
    context: &[Bar],
    aux: &BTreeMap<String, crate::data::aux_table::AuxTable>,
    tree: &crate::tree::loader::Tree,
    llm: &LlmEvaluator,
    soft: bool,
    t: NaiveDateTime,
    window: usize,
) -> crate::Result<Option<f64>> {
    if !is_fresh(primary, t) {
        return Ok(None);
    }
    let ctx = crate::features::context::build_context(primary, context, &[], aux, t, window);
    let dir = |s: crate::tree::schema::Stance| match s {
        crate::tree::schema::Stance::Long => 1.0,
        crate::tree::schema::Stance::Short => -1.0,
        crate::tree::schema::Stance::Flat => 0.0,
    };
    let score = if soft {
        let st = crate::engine::soft::traverse_soft(tree, &ctx, llm).await?;
        st.leaf_probs.iter().map(|(id, p)| {
            tree.leaves.get(id).map_or(0.0, |l| p * l.weight * dir(l.stance))
        }).sum()
    } else {
        let tr = crate::engine::traversal::traverse(tree, &ctx, llm).await?;
        tree.leaves.get(&tr.leaf).map_or(0.0, |l| l.weight * dir(l.stance))
    };
    Ok(Some(score))
}

// ─────────────────────────────────────────────────────────────────────────────
// 报告类型
// ─────────────────────────────────────────────────────────────────────────────

/// 每次调仓后的持仓快照。
#[derive(Debug, Serialize, Deserialize)]
pub struct HoldingsRecord {
    pub t: NaiveDateTime,
    /// 该调仓点扣成本后的净值（随后段 accrue 之前）。
    pub nav: f64,
    pub benchmark_nav: f64,
    pub selected: Vec<(String, f64)>,
}

/// 组合回测汇总报告。
#[derive(Debug, Serialize, Deserialize)]
pub struct PortfolioReport {
    pub tree_name: String,
    pub cost_bps: f64,
    pub top_n: usize,
    pub rebalance: usize,
    pub n_rebalances: usize,
    pub avg_members: f64,
    pub total_return: f64,
    pub max_drawdown: f64,
    pub turnover: f64,
    pub benchmark_return: f64,
    pub holdings: Vec<HoldingsRecord>,
}

/// 组合回测配置。
pub struct PortfolioConfig {
    pub tree_path: PathBuf,
    pub universe_path: PathBuf,
    pub top: usize,
    pub rebalance: usize,
    pub warmup: usize,
    pub window: usize,
    pub cost_bps: f64,
    pub soft: bool,
    pub aux_paths: Vec<(String, PathBuf)>,
    pub out_path: PathBuf,
    pub traces_path: Option<PathBuf>,
}

// ─────────────────────────────────────────────────────────────────────────────
// 组合主循环
// ─────────────────────────────────────────────────────────────────────────────

/// 端到端横截面组合回测：加载→时间线→调仓点→打分→等权→记账→报告。
pub async fn run_portfolio(cfg: &PortfolioConfig, llm: &LlmEvaluator) -> Result<PortfolioReport> {
    // ── 1. 加载 ─────────────────────────────────────────────────────────────
    let tree = crate::tree::loader::load_tree_file(&cfg.tree_path)?;
    let universe = crate::data::universe::read_universe_csv(&cfg.universe_path)?;

    let mut aux_tables: BTreeMap<String, crate::data::aux_table::AuxTable> = BTreeMap::new();
    for (name, p) in &cfg.aux_paths {
        aux_tables.insert(name.clone(), crate::data::aux_table::read_aux_csv(p)?);
    }

    // 逐标的加载 bars（primary + context 均加载）
    let mut primaries: Vec<Vec<Bar>> = Vec::with_capacity(universe.len());
    let mut contexts: Vec<Vec<Bar>> = Vec::with_capacity(universe.len());
    for entry in &universe {
        primaries.push(crate::data::reader::read_bars_csv(&entry.primary)?);
        contexts.push(crate::data::reader::read_bars_csv(&entry.context)?);
    }

    // ── 2. 时间线 + 调仓点 ──────────────────────────────────────────────────
    let timeline = build_timeline(&primaries);
    let n = timeline.len();

    // 调仓点索引：warmup, warmup+K, warmup+2K, ... (越界即止)
    let k = cfg.rebalance;
    if k == 0 {
        return Err(Error::Data("rebalance must be >= 1".into()));
    }
    let warmup = cfg.warmup;
    let rb_indices: Vec<usize> = (warmup..n).step_by(k).collect();

    if rb_indices.len() < 2 {
        return Err(Error::Data(
            "universe timeline too short for warmup/rebalance".into(),
        ));
    }

    // ── 3. 段序列 ───────────────────────────────────────────────────────────
    // 每段 = (调仓点索引, 段末索引)
    // 相邻调仓对 + 末调仓点→末时间点（若不同）
    let mut segments: Vec<(usize, usize)> = Vec::new();
    for w in rb_indices.windows(2) {
        segments.push((w[0], w[1]));
    }
    let last_rb = *rb_indices.last().unwrap();
    if last_rb != n - 1 {
        segments.push((last_rb, n - 1));
    }

    // ── 4. 主循环 ───────────────────────────────────────────────────────────
    let rate = cfg.cost_bps / 2.0 / 10_000.0;

    let mut nav = 1.0_f64;
    let mut bnav = 1.0_f64;
    let mut peak_nav = 1.0_f64;
    let mut max_drawdown = 0.0_f64;
    let mut total_turnover = 0.0_f64;
    let mut holdings: Vec<HoldingsRecord> = Vec::new();
    let mut total_members: usize = 0;

    let mut w_old: BTreeMap<String, f64> = BTreeMap::new(); // 上期权重
    let mut t1_warned = false;

    for (rb_idx, end_idx) in &segments {
        let t_rb = timeline[*rb_idx];

        // ── 4a. 相邻调仓同自然日警告 ─────────────────────────────────────
        if !holdings.is_empty() {
            let prev_t = holdings.last().unwrap().t;
            if !t1_warned && t_rb.date() == prev_t.date() {
                eprintln!(
                    "[rquant portfolio] T+1 note: adjacent rebalance points {prev_t} and {t_rb} fall on the same calendar day; execution may be infeasible at T+1."
                );
                t1_warned = true;
            }
        }

        // ── 4b. 逐标的打分 ───────────────────────────────────────────────
        let mut scores: Vec<(String, f64)> = Vec::new();
        for (i, entry) in universe.iter().enumerate() {
            if let Some(s) = score_symbol(
                &primaries[i],
                &contexts[i],
                &aux_tables,
                &tree,
                llm,
                cfg.soft,
                t_rb,
                cfg.window,
            ).await? {
                scores.push((entry.symbol.clone(), s));
            }
        }

        // ── 4c. select_top → 等权 ────────────────────────────────────────
        let selected = select_top(&scores, cfg.top);
        let n_sel = selected.len();
        total_members += n_sel;

        let w_new: BTreeMap<String, f64> = if n_sel > 0 {
            let eq = 1.0 / n_sel as f64;
            selected.iter().map(|(s, _)| (s.clone(), eq)).collect()
        } else {
            BTreeMap::new()
        };

        // ── 4d. 换手 + 成本 ─────────────────────────────────────────────
        let tv = turnover_between(&w_old, &w_new);
        nav *= 1.0 - rate * tv;
        total_turnover += tv;

        // ── 4e. 基准权重：所有有 last_close_at(t_rb) 的标的等权 ──────────
        let bw_symbols: Vec<String> = universe.iter().enumerate()
            .filter_map(|(i, e)| {
                if last_close_at(&primaries[i], t_rb).is_some() {
                    Some(e.symbol.clone())
                } else {
                    None
                }
            })
            .collect();
        let n_bw = bw_symbols.len();
        let bw_new: BTreeMap<String, f64> = if n_bw > 0 {
            let eq = 1.0 / n_bw as f64;
            bw_symbols.into_iter().map(|s| (s, eq)).collect()
        } else {
            BTreeMap::new()
        };

        // ── 4f. 记录调仓快照 ─────────────────────────────────────────────
        holdings.push(HoldingsRecord {
            t: t_rb,
            nav,
            benchmark_nav: bnav,
            selected: selected.clone(),
        });

        // 峰值/回撤（调仓后 nav）
        peak_nav = peak_nav.max(nav);
        max_drawdown = max_drawdown.max(1.0 - nav / peak_nav);

        // ── 4g. 段收益 ───────────────────────────────────────────────────
        let t_end = timeline[*end_idx];

        // 价格映射（segment start = 调仓点，end = 段末）
        let px_start: BTreeMap<String, f64> = universe.iter().enumerate()
            .filter_map(|(i, e)| {
                last_close_at(&primaries[i], t_rb).map(|p| (e.symbol.clone(), p))
            })
            .collect();
        let px_end: BTreeMap<String, f64> = universe.iter().enumerate()
            .filter_map(|(i, e)| {
                last_close_at(&primaries[i], t_end).map(|p| (e.symbol.clone(), p))
            })
            .collect();

        // 组合段收益
        let r = accrue(&w_new, &px_start, &px_end);
        nav *= 1.0 + r;

        // 基准段收益（无成本，等权重置）
        let br = accrue(&bw_new, &px_start, &px_end);
        bnav *= 1.0 + br;

        // 峰值/回撤（段末 nav）
        peak_nav = peak_nav.max(nav);
        max_drawdown = max_drawdown.max(1.0 - nav / peak_nav);

        // 滚动权重（基准每期无成本重置，无需追踪 bw_old）
        w_old = w_new;
        drop(bw_new);
    }

    // ── 5. 汇总 ─────────────────────────────────────────────────────────────
    let n_rebalances = holdings.len();
    let avg_members = if n_rebalances > 0 {
        total_members as f64 / n_rebalances as f64
    } else {
        0.0
    };
    let total_return = nav - 1.0;
    let benchmark_return = bnav - 1.0;

    let report = PortfolioReport {
        tree_name: tree.meta.name.clone(),
        cost_bps: cfg.cost_bps,
        top_n: cfg.top,
        rebalance: cfg.rebalance,
        n_rebalances,
        avg_members,
        total_return,
        max_drawdown,
        turnover: total_turnover,
        benchmark_return,
        holdings,
    };

    // ── 6. 写输出 ────────────────────────────────────────────────────────────
    let json = serde_json::to_string_pretty(&report)?;
    std::fs::write(&cfg.out_path, json)?;

    if let Some(tp) = &cfg.traces_path {
        let mut f = std::fs::File::create(tp)?;
        for rec in &report.holdings {
            let line = serde_json::to_string(rec)?;
            writeln!(f, "{line}")?;
        }
    }

    Ok(report)
}

// ─────────────────────────────────────────────────────────────────────────────
// 摘要打印（风格同 print_sim_summary）
// ─────────────────────────────────────────────────────────────────────────────

/// 打印 PortfolioReport 摘要。
pub fn print_portfolio_summary(report: &PortfolioReport) {
    println!("=== rquant PORTFOLIO: {} ===", report.tree_name);
    println!("cost_bps={}  top_n={}  rebalance={}", report.cost_bps, report.top_n, report.rebalance);
    println!("总收益率    : {:.4}", report.total_return);
    println!("基准收益率  : {:.4}", report.benchmark_return);
    println!("超额收益    : {:.4}", report.total_return - report.benchmark_return);
    println!("最大回撤    : {:.4}", report.max_drawdown);
    println!("换手率      : {:.4}", report.turnover);
    println!("调仓次数    : {}", report.n_rebalances);
    println!("平均成员数  : {:.2}", report.avg_members);
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::bar::Bar;
    use chrono::NaiveDate;

    fn dt(day: u32, hour: u32, min: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2024, 1, day).unwrap().and_hms_opt(hour, min, 0).unwrap()
    }

    fn bar_at(t: NaiveDateTime, close: f64) -> Bar {
        Bar { time: t, open: close, high: close, low: close, close, volume: 1.0 }
    }

    // ── build_timeline ────────────────────────────────────────────────────────

    #[test]
    fn timeline_union_sort_dedup() {
        let t1 = dt(2, 9, 30);
        let t2 = dt(2, 10, 0);
        let t3 = dt(2, 10, 30);
        let t4 = dt(2, 11, 0);

        let series_a = vec![bar_at(t1, 1.0), bar_at(t3, 3.0)];
        let series_b = vec![bar_at(t2, 2.0), bar_at(t3, 3.5), bar_at(t4, 4.0)];

        let tl = build_timeline(&[series_a, series_b]);
        assert_eq!(tl, vec![t1, t2, t3, t4]);
    }

    #[test]
    fn timeline_empty_input() {
        let tl = build_timeline(&[]);
        assert!(tl.is_empty());
    }

    #[test]
    fn timeline_single_series() {
        let t1 = dt(2, 9, 30);
        let t2 = dt(2, 10, 0);
        let bars = vec![bar_at(t1, 1.0), bar_at(t2, 2.0)];
        let tl = build_timeline(&[bars]);
        assert_eq!(tl, vec![t1, t2]);
    }

    // ── last_close_at ─────────────────────────────────────────────────────────

    #[test]
    fn last_close_at_exact_hit() {
        let t1 = dt(2, 9, 30);
        let t2 = dt(2, 10, 0);
        let t3 = dt(2, 10, 30);
        let bars = vec![bar_at(t1, 10.0), bar_at(t2, 20.0), bar_at(t3, 30.0)];
        assert_eq!(last_close_at(&bars, t2), Some(20.0));
        assert_eq!(last_close_at(&bars, t3), Some(30.0));
    }

    #[test]
    fn last_close_at_before_first_returns_none() {
        let t1 = dt(2, 10, 0);
        let bars = vec![bar_at(t1, 10.0)];
        let before = dt(2, 9, 0);
        assert_eq!(last_close_at(&bars, before), None);
    }

    #[test]
    fn last_close_at_between_takes_previous() {
        let t1 = dt(2, 9, 30);
        let t3 = dt(2, 10, 30);
        let bars = vec![bar_at(t1, 10.0), bar_at(t3, 30.0)];
        let t2 = dt(2, 10, 0);
        assert_eq!(last_close_at(&bars, t2), Some(10.0));
    }

    // ── is_fresh ──────────────────────────────────────────────────────────────

    #[test]
    fn is_fresh_exact_hit() {
        let t1 = dt(2, 9, 30);
        let t2 = dt(2, 10, 0);
        let bars = vec![bar_at(t1, 1.0), bar_at(t2, 2.0)];
        assert!(is_fresh(&bars, t1));
        assert!(is_fresh(&bars, t2));
    }

    #[test]
    fn is_fresh_miss() {
        let t1 = dt(2, 9, 30);
        let t3 = dt(2, 10, 30);
        let bars = vec![bar_at(t1, 1.0), bar_at(t3, 3.0)];
        let t2 = dt(2, 10, 0);
        assert!(!is_fresh(&bars, t2));
    }

    // ── select_top ────────────────────────────────────────────────────────────

    #[test]
    fn select_top_filters_nonpositive() {
        let scores = vec![
            ("a".to_string(), -0.5),
            ("b".to_string(), 0.0),
            ("c".to_string(), 0.9),
        ];
        let top = select_top(&scores, 3);
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].0, "c");
    }

    #[test]
    fn select_top_desc_order_tie_symbol_asc() {
        let scores = vec![
            ("b".to_string(), 0.5),
            ("a".to_string(), 0.5),
            ("c".to_string(), 0.9),
        ];
        let top = select_top(&scores, 2);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].0, "c");
        assert!((top[0].1 - 0.9).abs() < 1e-12);
        assert_eq!(top[1].0, "a");
        assert!((top[1].1 - 0.5).abs() < 1e-12);
    }

    #[test]
    fn select_top_fewer_than_n_returns_all() {
        let scores = vec![("a".to_string(), 0.3), ("b".to_string(), 0.7)];
        let top = select_top(&scores, 10);
        assert_eq!(top.len(), 2);
    }

    #[test]
    fn select_top_empty_scores() {
        let top = select_top(&[], 5);
        assert!(top.is_empty());
    }

    // ── accrue / turnover_between ─────────────────────────────────────────────

    #[test]
    fn golden_two_period_walk() {
        let m = |pairs: &[(&str, f64)]| -> BTreeMap<String, f64> {
            pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
        };
        // t0：建仓 {A:0.5, B:0.5}，换手 1.0
        let w0 = m(&[("A", 0.5), ("B", 0.5)]);
        assert!((turnover_between(&BTreeMap::new(), &w0) - 1.0).abs() < 1e-12);
        let mut nav = 1.0 * (1.0 - 0.001 * 1.0);
        // 期1：A 10→11、B 20→19
        let r1 = accrue(&w0, &m(&[("A", 10.0), ("B", 20.0)]), &m(&[("A", 11.0), ("B", 19.0)]));
        assert!((r1 - (0.5 * 0.10 + 0.5 * (-0.05))).abs() < 1e-12);
        nav *= 1.0 + r1;
        // t1：换成 {A:0.5, C:0.5}，换手 = B 出 0.5 + C 进 0.5 = 1.0
        let w1 = m(&[("A", 0.5), ("C", 0.5)]);
        assert!((turnover_between(&w0, &w1) - 1.0).abs() < 1e-12);
        nav *= 1.0 - 0.001 * 1.0;
        // 期2：A 11→11、C 5→5.5
        let r2 = accrue(&w1, &m(&[("A", 11.0), ("C", 5.0)]), &m(&[("A", 11.0), ("C", 5.5)]));
        nav *= 1.0 + r2;
        assert!((nav - 0.999 * 1.025 * 0.999 * 1.05).abs() < 1e-12);
        // 停牌成员：px_end 缺失 → 贡献 0
        let r3 = accrue(&w1, &m(&[("A", 11.0), ("C", 5.0)]), &m(&[("A", 11.0)]));
        assert!((r3 - 0.0).abs() < 1e-12);
    }

    // ── score_symbol (tokio async) ────────────────────────────────────────────

    const SIMPLE_TREE_YAML: &str = r#"
meta: { name: test, forward_window: 3, stances: [long, flat] }
root: root_node
nodes:
  root_node:
    type: quant
    branches:
      - when: "close > 0"
        goto: leaf_long
        label: positive
    default: { goto: leaf_flat, label: none }
leaves:
  leaf_long: { stance: long, weight: 1.0 }
  leaf_flat: { stance: flat }
"#;

    #[tokio::test]
    async fn score_symbol_fresh_returns_some_score() {
        use crate::eval::llm::LlmEvaluator;
        use crate::tree::loader::load_tree_str;

        let tree = load_tree_str(SIMPLE_TREE_YAML).unwrap();
        let llm = LlmEvaluator::Disabled;
        let t = dt(2, 10, 0);
        let primary = vec![bar_at(dt(2, 9, 30), 1.0), bar_at(t, 2.0)];
        let result = score_symbol(&primary, &primary, &BTreeMap::new(), &tree, &llm, false, t, 10)
            .await.unwrap();
        assert!(result.is_some());
        let score = result.unwrap();
        assert!((score - 1.0).abs() < 1e-12, "expected score 1.0, got {score}");
    }

    #[tokio::test]
    async fn score_symbol_stale_returns_none() {
        use crate::eval::llm::LlmEvaluator;
        use crate::tree::loader::load_tree_str;

        let tree = load_tree_str(SIMPLE_TREE_YAML).unwrap();
        let llm = LlmEvaluator::Disabled;
        let t = dt(2, 10, 0);
        let primary = vec![bar_at(dt(2, 9, 30), 1.0), bar_at(dt(2, 9, 45), 2.0)];
        let result = score_symbol(&primary, &primary, &BTreeMap::new(), &tree, &llm, false, t, 10)
            .await.unwrap();
        assert!(result.is_none());
    }

    // ── run_portfolio 集成测试 ────────────────────────────────────────────────

    /// 合成3标的：A每bar+1%、B横盘、C每bar-1%；多日网格；
    /// 树 close > sma(close,3) → leaf_long else flat；
    /// top=1, rebalance=4, warmup=6, cost_bps=10。
    /// 断言：每期selected==["A"]；total_return > benchmark_return；
    ///       n_rebalances>=2；traces行数==n_rebalances。
    #[tokio::test]
    async fn integration_portfolio_selects_best_symbol() {
        use crate::eval::llm::LlmEvaluator;
        use std::io::Write as _;

        // 树：close > sma(close,3) → long else flat
        const MOMENTUM_TREE: &str = r#"
meta: { name: momentum, forward_window: 1, stances: [long, flat] }
root: gate
nodes:
  gate:
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

        // 生成时间网格：4天×4根bar/天 = 16根，跨4自然日
        // 时间：2024-01-02 ~ 2024-01-05，每天 09:30/10:00/10:30/11:00
        let days: Vec<u32> = vec![2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
        let hours_mins: Vec<(u32, u32)> = vec![(9, 30), (10, 0), (10, 30), (11, 0)];
        let mut timestamps: Vec<NaiveDateTime> = Vec::new();
        for &d in &days {
            for &(h, m) in &hours_mins {
                timestamps.push(dt(d, h, m));
            }
        }
        // 40 bars total: warmup=6, rebalance=4 → rb indices: 6,10,14,18,22,26,30,34,38
        // → ≥2 rebalance points satisfied

        // A: start 100, +1% per bar
        // B: start 100, flat
        // C: start 100, -1% per bar
        let write_bars = |start: f64, pct: f64| -> tempfile::NamedTempFile {
            let mut f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
            writeln!(f, "time,open,high,low,close,volume").unwrap();
            let mut price = start;
            for ts in &timestamps {
                writeln!(
                    f,
                    "{},{},{},{},{},1000",
                    ts.format("%Y-%m-%d %H:%M:%S"),
                    price, price, price, price
                ).unwrap();
                price *= 1.0 + pct;
            }
            f.flush().unwrap();
            f
        };

        let f_a = write_bars(100.0, 0.01);
        let f_b = write_bars(100.0, 0.0);
        let f_c = write_bars(100.0, -0.01);

        // universe CSV
        let mut univ_f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        writeln!(
            univ_f,
            "symbol,primary\nA,{}\nB,{}\nC,{}",
            f_a.path().to_str().unwrap(),
            f_b.path().to_str().unwrap(),
            f_c.path().to_str().unwrap()
        ).unwrap();
        univ_f.flush().unwrap();

        // tree file
        let mut tree_f = tempfile::Builder::new().suffix(".yaml").tempfile().unwrap();
        write!(tree_f, "{MOMENTUM_TREE}").unwrap();
        tree_f.flush().unwrap();

        // output files
        let out_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        let traces_f = tempfile::Builder::new().suffix(".jsonl").tempfile().unwrap();

        let cfg = PortfolioConfig {
            tree_path: tree_f.path().to_path_buf(),
            universe_path: univ_f.path().to_path_buf(),
            top: 1,
            rebalance: 4,
            warmup: 6,
            window: 10,
            cost_bps: 10.0,
            soft: false,
            aux_paths: Vec::new(),
            out_path: out_f.path().to_path_buf(),
            traces_path: Some(traces_f.path().to_path_buf()),
        };

        let report = run_portfolio(&cfg, &LlmEvaluator::Disabled)
            .await
            .expect("run_portfolio should succeed");

        // Every rebalance should select only "A"
        for rec in &report.holdings {
            assert_eq!(
                rec.selected.len(), 1,
                "expected 1 selected symbol, got {}: {:?}", rec.selected.len(), rec.selected
            );
            assert_eq!(
                rec.selected[0].0, "A",
                "expected symbol A, got {}", rec.selected[0].0
            );
        }

        // Portfolio should outperform benchmark
        assert!(
            report.total_return > report.benchmark_return,
            "total_return ({}) should beat benchmark_return ({})",
            report.total_return, report.benchmark_return
        );

        // At least 2 rebalances
        assert!(
            report.n_rebalances >= 2,
            "expected n_rebalances >= 2, got {}", report.n_rebalances
        );

        // Traces line count == n_rebalances
        let traces_content = std::fs::read_to_string(traces_f.path()).unwrap();
        let trace_lines = traces_content.lines().filter(|l| !l.trim().is_empty()).count();
        assert_eq!(
            trace_lines, report.n_rebalances,
            "traces lines ({trace_lines}) should equal n_rebalances ({})", report.n_rebalances
        );
    }
}
