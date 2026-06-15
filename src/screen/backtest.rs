//! 选股器历史回测：镜像 portfolio 主循环，把单树打分换成多树合并选股。
//! 复用 portfolio 的 timeline/last_close_at/select_top/accrue/turnover_between + risk_metrics。

use crate::data::aux_table::AuxTable;
use crate::data::bar::Bar;
use crate::eval::llm::LlmEvaluator;
use crate::tree::loader::Tree;
use crate::Result;
use chrono::{NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::backtest::portfolio::{
    accrue, build_timeline, last_close_at, score_symbol, select_top, turnover_between,
};
use crate::screen::combine::{combine, MergeParams};
use crate::screen::config::load_screen_config;

/// 调仓快照。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenHolding {
    pub t: NaiveDateTime,
    pub nav: f64,
    pub benchmark_nav: f64,
    /// (symbol, combined_score)
    pub selected: Vec<(String, f64)>,
}

/// 回测报告（核心字段；归因/regime/质量分层在后续任务补）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenBacktestReport {
    pub n_rebalances: usize,
    pub top: usize,
    pub rebalance: usize,
    pub total_return: f64,
    pub benchmark_return: f64,
    pub excess_return: f64,
    pub max_drawdown: f64,
    pub turnover: f64,
    pub avg_members: f64,
    pub holdings: Vec<ScreenHolding>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub risk: Option<crate::report::risk::RiskMetrics>,
    #[serde(default)]
    pub tag_attribution: Vec<TagAttribution>,
    #[serde(default)]
    pub regime_slices: Vec<RegimeSlice>,
    #[serde(default)]
    pub quality_layers: Vec<QualityLayer>,
}

/// 标签归因（SCR-6 填充）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TagAttribution {
    pub tag: String,
    pub n_picks: usize,
    pub hit_rate: f64,
    pub mean_fwd_return: f64,
}

/// regime 切片（SCR-7 填充）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RegimeSlice {
    pub label: String,
    pub from: String,
    pub to: String,
    pub picks_return: f64,
    pub benchmark_return: f64,
    pub excess: f64,
}

/// 优质分分层（SCR-8 填充）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QualityLayer {
    pub layer: usize,
    pub n: usize,
    pub mean_quality: f64,
    pub mean_fwd_return: f64,
}

/// 回测运行配置。
pub struct ScreenBacktestConfig {
    pub config_path: PathBuf,
    pub universe_path: PathBuf,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub rebalance: usize,
    pub top: Option<usize>,
    pub warmup: usize,
    pub window: usize,
    pub cost_bps: f64,
    pub soft: bool,
    pub out_path: Option<PathBuf>,
}

/// 单标的、单调仓点的多树合并结果（内部 helper）。
/// `quality` 供 SCR-8 质量分层消费，`tags` 已在 SCR-6 归因中消费。
struct SymbolEval {
    combined: f64,
    #[allow(dead_code)] // SCR-8 will consume quality
    quality: f64,
    tags: Vec<String>,
}

#[allow(clippy::too_many_arguments)]
async fn eval_symbol(
    primary: &[Bar],
    context: &[Bar],
    aux: &BTreeMap<String, AuxTable>,
    quality: &[Tree],
    setups: &BTreeMap<String, Vec<Tree>>,
    llm: &LlmEvaluator,
    soft: bool,
    t: NaiveDateTime,
    window: usize,
    mp: &MergeParams,
) -> Result<Option<SymbolEval>> {
    let mut q_scores: Vec<f64> = Vec::new();
    let mut any = false;
    for tree in quality {
        if let Some(s) = score_symbol(primary, context, aux, tree, llm, soft, t, window).await? {
            q_scores.push(s);
            any = true;
        }
    }
    if !any {
        return Ok(None);
    }
    let mut setup_scores: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for (tag, trees) in setups {
        let mut v = Vec::new();
        for tree in trees {
            if let Some(s) = score_symbol(primary, context, aux, tree, llm, soft, t, window).await? {
                v.push(s);
            }
        }
        setup_scores.insert(tag.clone(), v);
    }
    let out = combine(&q_scores, &setup_scores, mp);
    Ok(Some(SymbolEval { combined: out.combined_score, quality: out.quality_score, tags: out.tags }))
}

fn load_trees(paths: &[PathBuf]) -> Result<Vec<Tree>> {
    paths.iter().map(|p| crate::tree::loader::load_tree_file(p)).collect()
}

/// 端到端选股回测。
pub async fn run_screen_backtest(
    cfg: &ScreenBacktestConfig,
    llm: &LlmEvaluator,
) -> Result<ScreenBacktestReport> {
    if cfg.rebalance == 0 {
        return Err(crate::Error::Data("rebalance must be >= 1".into()));
    }
    let sc = load_screen_config(&cfg.config_path)?;
    let regimes = sc.regimes.clone();
    let quality = load_trees(&sc.quality_trees)?;
    let mut setups: BTreeMap<String, Vec<Tree>> = BTreeMap::new();
    for (tag, paths) in &sc.setup_trees {
        setups.insert(tag.clone(), load_trees(paths)?);
    }
    let mp = MergeParams {
        theta_fire: sc.merge.theta_fire,
        vote_frac: sc.merge.vote_frac,
        q_floor: sc.merge.q_floor,
    };
    let top = cfg.top.unwrap_or(sc.merge.top);

    let universe = crate::data::universe::read_universe_csv(&cfg.universe_path)?;
    let mut primaries: Vec<Vec<Bar>> = Vec::with_capacity(universe.len());
    let mut contexts: Vec<Vec<Bar>> = Vec::with_capacity(universe.len());
    for e in &universe {
        primaries.push(crate::data::reader::read_bars_csv(&e.primary)?);
        contexts.push(crate::data::reader::read_bars_csv(&e.context)?);
    }
    let aux: BTreeMap<String, AuxTable> = BTreeMap::new();

    let full = build_timeline(&primaries);
    let timeline: Vec<NaiveDateTime> = full
        .into_iter()
        .filter(|t| cfg.from.is_none_or(|f| t.date() >= f) && cfg.to.is_none_or(|to| t.date() <= to))
        .collect();
    let n = timeline.len();
    let rb_indices: Vec<usize> = (cfg.warmup..n).step_by(cfg.rebalance).collect();
    if rb_indices.len() < 2 {
        return Err(crate::Error::Data("timeline too short for warmup/rebalance".into()));
    }
    let mut segments: Vec<(usize, usize)> = Vec::new();
    for w in rb_indices.windows(2) {
        segments.push((w[0], w[1]));
    }
    let last_rb = *rb_indices.last().unwrap();
    if last_rb != n - 1 {
        segments.push((last_rb, n - 1));
    }

    let rate = cfg.cost_bps / 2.0 / 10_000.0;
    let mut nav = 1.0_f64;
    let mut bnav = 1.0_f64;
    let mut peak = 1.0_f64;
    let mut max_dd = 0.0_f64;
    let mut total_turnover = 0.0_f64;
    let mut total_members = 0usize;
    let mut holdings: Vec<ScreenHolding> = Vec::new();
    let mut w_old: BTreeMap<String, f64> = BTreeMap::new();
    // 标签归因累加器：tag -> picks 段收益列表
    let mut tag_rets: BTreeMap<String, Vec<f64>> = BTreeMap::new();

    for (rb_idx, end_idx) in &segments {
        let t_rb = timeline[*rb_idx];
        let t_end = timeline[*end_idx];

        let mut evals: BTreeMap<String, SymbolEval> = BTreeMap::new();
        let mut scores: Vec<(String, f64)> = Vec::new();
        for (i, e) in universe.iter().enumerate() {
            if let Some(ev) = eval_symbol(
                &primaries[i], &contexts[i], &aux, &quality, &setups, llm, cfg.soft, t_rb, cfg.window, &mp,
            ).await? {
                scores.push((e.symbol.clone(), ev.combined));
                evals.insert(e.symbol.clone(), ev);
            }
        }
        let selected = select_top(&scores, top);
        total_members += selected.len();
        let w_new: BTreeMap<String, f64> = if !selected.is_empty() {
            let eq = 1.0 / selected.len() as f64;
            selected.iter().map(|(s, _)| (s.clone(), eq)).collect()
        } else {
            BTreeMap::new()
        };

        let tv = turnover_between(&w_old, &w_new);
        nav *= 1.0 - rate * tv;
        total_turnover += tv;

        holdings.push(ScreenHolding { t: t_rb, nav, benchmark_nav: bnav, selected: selected.clone() });
        peak = peak.max(nav);
        max_dd = max_dd.max(1.0 - nav / peak);

        let px_start: BTreeMap<String, f64> = universe.iter().enumerate()
            .filter_map(|(i, e)| last_close_at(&primaries[i], t_rb).map(|p| (e.symbol.clone(), p)))
            .collect();
        let px_end: BTreeMap<String, f64> = universe.iter().enumerate()
            .filter_map(|(i, e)| last_close_at(&primaries[i], t_end).map(|p| (e.symbol.clone(), p)))
            .collect();

        for (sym, _) in &selected {
            let seg_ret = match (px_start.get(sym), px_end.get(sym)) {
                (Some(a), Some(b)) if *a > 0.0 => b / a - 1.0,
                _ => 0.0,
            };
            if let Some(ev) = evals.get(sym) {
                for tag in &ev.tags {
                    tag_rets.entry(tag.clone()).or_default().push(seg_ret);
                }
            }
        }

        let r = accrue(&w_new, &px_start, &px_end);
        nav *= 1.0 + r;

        let bw: BTreeMap<String, f64> = {
            let syms: Vec<String> = px_start.keys().cloned().collect();
            let neq = syms.len();
            if neq > 0 { let eq = 1.0 / neq as f64; syms.into_iter().map(|s| (s, eq)).collect() } else { BTreeMap::new() }
        };
        let br = accrue(&bw, &px_start, &px_end);
        bnav *= 1.0 + br;

        peak = peak.max(nav);
        max_dd = max_dd.max(1.0 - nav / peak);
        w_old = w_new;
    }

    let n_rebalances = holdings.len();
    let total_return = nav - 1.0;
    let benchmark_return = bnav - 1.0;
    let nav_series: Vec<(NaiveDateTime, f64)> = holdings.iter().map(|h| (h.t, h.nav)).collect();
    let risk = crate::report::risk::risk_metrics(&nav_series, max_dd);

    let tag_attribution: Vec<TagAttribution> = tag_rets.iter().map(|(tag, rets)| {
        let n = rets.len();
        let hit = rets.iter().filter(|r| **r > 0.0).count();
        let mean = if n > 0 { rets.iter().sum::<f64>() / n as f64 } else { 0.0 };
        TagAttribution {
            tag: tag.clone(),
            n_picks: n,
            hit_rate: if n > 0 { hit as f64 / n as f64 } else { 0.0 },
            mean_fwd_return: mean,
        }
    }).collect();

    let regime_slices: Vec<RegimeSlice> = regimes.iter().filter_map(|rw| {
        let inside: Vec<&ScreenHolding> = holdings.iter()
            .filter(|h| h.t.date() >= rw.from && h.t.date() <= rw.to)
            .collect();
        if inside.len() < 2 {
            return None; // 不足以算区间收益
        }
        let p0 = inside.first().unwrap();
        let p1 = inside.last().unwrap();
        let picks = if p0.nav > 0.0 { p1.nav / p0.nav - 1.0 } else { 0.0 };
        let bench = if p0.benchmark_nav > 0.0 { p1.benchmark_nav / p0.benchmark_nav - 1.0 } else { 0.0 };
        Some(RegimeSlice {
            label: rw.label.clone(),
            from: rw.from.to_string(),
            to: rw.to.to_string(),
            picks_return: picks,
            benchmark_return: bench,
            excess: picks - bench,
        })
    }).collect();

    let report = ScreenBacktestReport {
        n_rebalances,
        top,
        rebalance: cfg.rebalance,
        total_return,
        benchmark_return,
        excess_return: total_return - benchmark_return,
        max_drawdown: max_dd,
        turnover: total_turnover,
        avg_members: if n_rebalances > 0 { total_members as f64 / n_rebalances as f64 } else { 0.0 },
        holdings,
        risk,
        tag_attribution,
        regime_slices,
        quality_layers: Vec::new(),
    };

    if let Some(p) = &cfg.out_path {
        let json = serde_json::to_string_pretty(&report)?;
        std::fs::write(p, json)?;
    }
    Ok(report)
}

/// 打印回测摘要。
pub fn print_screen_backtest(r: &ScreenBacktestReport) {
    println!("=== rquant SCREEN BACKTEST （top {}，rebalance {}）===", r.top, r.rebalance);
    println!("调仓次数    : {}", r.n_rebalances);
    println!("总收益率    : {:.4}", r.total_return);
    println!("基准收益率  : {:.4}", r.benchmark_return);
    println!("超额收益    : {:.4}", r.excess_return);
    println!("最大回撤    : {:.4}", r.max_drawdown);
    println!("换手率      : {:.4}", r.turnover);
    println!("平均成员数  : {:.2}", r.avg_members);
    if let Some(rk) = &r.risk {
        let f = |v: Option<f64>| v.map_or("—".to_string(), |x| format!("{x:.2}"));
        println!("Sharpe      : {}", f(rk.sharpe));
        println!("Calmar      : {}", f(rk.calmar));
    }
    for ta in &r.tag_attribution {
        println!("标签 {:<10} picks={:<4} 胜率={:.2} 均前瞻收益={:+.4}", ta.tag, ta.n_picks, ta.hit_rate, ta.mean_fwd_return);
    }
    for rs in &r.regime_slices {
        println!("regime {:<10} [{}~{}] 组合={:+.4} 基准={:+.4} 超额={:+.4}", rs.label, rs.from, rs.to, rs.picks_return, rs.benchmark_return, rs.excess);
    }
    for ql in &r.quality_layers {
        println!("优质层 Q{} n={:<4} 均优质={:.3} 均前瞻收益={:+.4}", ql.layer, ql.n, ql.mean_quality, ql.mean_fwd_return);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use std::io::Write;

    fn daily(d: u32) -> NaiveDateTime {
        let base = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        (base + chrono::Duration::days(d as i64)).and_hms_opt(0, 0, 0).unwrap()
    }

    const Q_SIMPLE: &str = r#"
meta: { name: q, forward_window: 1, stances: [long, flat] }
root: g
nodes:
  g: { type: quant, branches: [ { when: "close > sma(close, 3)", goto: l, label: up } ], default: { goto: f, label: flat } }
leaves: { l: { stance: long, weight: 1.0 }, f: { stance: flat } }
"#;
    const M_SIMPLE: &str = r#"
meta: { name: m, forward_window: 1, stances: [long, flat] }
root: g
nodes:
  g: { type: quant, branches: [ { when: "close > ref(close, 2)", goto: l, label: up } ], default: { goto: f, label: flat } }
leaves: { l: { stance: long, weight: 1.0 }, f: { stance: flat } }
"#;

    fn wf(suffix: &str, content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::Builder::new().suffix(suffix).tempfile().unwrap();
        write!(f, "{content}").unwrap();
        f.flush().unwrap();
        f
    }

    fn bars(pct: f64) -> tempfile::NamedTempFile {
        let mut f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        writeln!(f, "time,open,high,low,close,volume").unwrap();
        let mut price = 100.0;
        for d in 0..30u32 {
            writeln!(f, "{},{p},{p},{p},{p},1000", daily(d).format("%Y-%m-%d %H:%M:%S"), p = price).unwrap();
            price *= 1.0 + pct;
        }
        f.flush().unwrap();
        f
    }

    #[tokio::test]
    async fn backtest_tag_attribution_populated() {
        let q = wf(".yaml", Q_SIMPLE);
        let m = wf(".yaml", M_SIMPLE);
        let cfg_yaml = format!(
            "quality_trees: [{}]\nsetup_trees:\n  动量延续: [{}]\nmerge: {{ q_floor: 0.5, top: 1 }}\n",
            q.path().to_str().unwrap().replace('\\', "/"),
            m.path().to_str().unwrap().replace('\\', "/"),
        );
        let cfg_f = wf(".yaml", &cfg_yaml);
        let up = bars(0.01);
        let dn = bars(-0.01);
        let mut univ = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        writeln!(univ, "symbol,primary\nUP,{}\nDN,{}", up.path().to_str().unwrap(), dn.path().to_str().unwrap()).unwrap();
        univ.flush().unwrap();
        let cfg = ScreenBacktestConfig {
            config_path: cfg_f.path().to_path_buf(), universe_path: univ.path().to_path_buf(),
            from: None, to: None, rebalance: 4, top: None, warmup: 5, window: 10, cost_bps: 10.0, soft: false, out_path: None,
        };
        let r = run_screen_backtest(&cfg, &LlmEvaluator::Disabled).await.unwrap();
        let mom = r.tag_attribution.iter().find(|a| a.tag == "动量延续").expect("动量延续 attribution present");
        assert!(mom.n_picks >= 2, "should have picks tagged 动量延续");
        assert!(mom.mean_fwd_return > 0.0, "rising picks → positive forward return");
        assert!(mom.hit_rate > 0.5);
    }

    #[tokio::test]
    async fn backtest_regime_slices_populated() {
        let q = wf(".yaml", Q_SIMPLE);
        let m = wf(".yaml", M_SIMPLE);
        let cfg_yaml = format!(
            "quality_trees: [{}]\nsetup_trees:\n  动量延续: [{}]\nmerge: {{ q_floor: 0.5, top: 1 }}\nregimes:\n  - {{ label: full, from: 2024-01-01, to: 2024-02-01 }}\n",
            q.path().to_str().unwrap().replace('\\', "/"),
            m.path().to_str().unwrap().replace('\\', "/"),
        );
        let cfg_f = wf(".yaml", &cfg_yaml);
        let up = bars(0.01);
        let dn = bars(-0.01);
        let mut univ = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        writeln!(univ, "symbol,primary\nUP,{}\nDN,{}", up.path().to_str().unwrap(), dn.path().to_str().unwrap()).unwrap();
        univ.flush().unwrap();
        let cfg = ScreenBacktestConfig {
            config_path: cfg_f.path().to_path_buf(), universe_path: univ.path().to_path_buf(),
            from: None, to: None, rebalance: 4, top: None, warmup: 5, window: 10, cost_bps: 10.0, soft: false, out_path: None,
        };
        let r = run_screen_backtest(&cfg, &LlmEvaluator::Disabled).await.unwrap();
        let slice = r.regime_slices.iter().find(|s| s.label == "full").expect("regime slice present");
        assert!((slice.excess - (slice.picks_return - slice.benchmark_return)).abs() < 1e-9);
        assert!(slice.picks_return > slice.benchmark_return);
    }

    #[tokio::test]
    async fn backtest_picks_beat_benchmark() {
        let q = wf(".yaml", Q_SIMPLE);
        let m = wf(".yaml", M_SIMPLE);
        let cfg_yaml = format!(
            "quality_trees: [{}]\nsetup_trees:\n  动量延续: [{}]\nmerge: {{ q_floor: 0.5, top: 1 }}\n",
            q.path().to_str().unwrap().replace('\\', "/"),
            m.path().to_str().unwrap().replace('\\', "/"),
        );
        let cfg_f = wf(".yaml", &cfg_yaml);
        let up = bars(0.01);
        let dn = bars(-0.01);
        let mut univ = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        writeln!(univ, "symbol,primary\nUP,{}\nDN,{}", up.path().to_str().unwrap(), dn.path().to_str().unwrap()).unwrap();
        univ.flush().unwrap();

        let cfg = ScreenBacktestConfig {
            config_path: cfg_f.path().to_path_buf(),
            universe_path: univ.path().to_path_buf(),
            from: None, to: None,
            rebalance: 4, top: None, warmup: 5, window: 10, cost_bps: 10.0, soft: false,
            out_path: None,
        };
        let r = run_screen_backtest(&cfg, &LlmEvaluator::Disabled).await.unwrap();
        assert!(r.n_rebalances >= 2);
        assert!(r.total_return > r.benchmark_return, "picks {} should beat benchmark {}", r.total_return, r.benchmark_return);
        for h in &r.holdings {
            if !h.selected.is_empty() {
                assert_eq!(h.selected[0].0, "UP");
            }
        }
    }
}
