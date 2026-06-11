//! 报告生成：硬回测与软回测结果的序列化、摘要打印及 HTML 渲染。

use crate::backtest::gaps::GapReport;
use crate::backtest::metrics::Metrics;
use crate::backtest::soft::{SoftMetrics, SoftStepRecord};
use crate::backtest::walkforward::WalkForward;
use crate::engine::trace::Trace;
use crate::Result;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;

pub mod curve;
pub mod risk;
pub mod viz;

/// 硬回测完整报告，可序列化为 JSON。
#[derive(Debug, Serialize, Deserialize)]
pub struct Report {
    /// YAML 树的 `meta.name`。
    pub tree_name: String,
    /// 前瞻窗口大小（bar 数），取自树 meta。
    pub forward_window: usize,
    /// 单边成本（bps），实际扣除双边（round-trip）。
    pub cost_bps: f64,
    /// 汇总度量（active / T+1 可执行 / 按叶 / 按 stance）。
    pub metrics: Metrics,
    /// 主行情数据质量报告（缺失交易日、残缺日）。
    pub gaps: GapReport,
    /// Walk-forward 折叠结果；`folds < 2` 时为 `None`，不参与 JSON 序列化。
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub walk_forward: Option<WalkForward>,
}

pub fn write_report(report: &Report, path: &Path) -> Result<()> {
    let json = serde_json::to_string_pretty(report)?;
    std::fs::write(path, json)?;
    Ok(())
}

pub fn write_traces_jsonl(traces: &[Trace], path: &Path) -> Result<()> {
    let mut f = std::fs::File::create(path)?;
    for t in traces {
        let line = serde_json::to_string(t)?;
        writeln!(f, "{line}")?;
    }
    Ok(())
}

pub fn write_soft_traces_jsonl(records: &[SoftStepRecord], path: &Path) -> Result<()> {
    let mut f = std::fs::File::create(path)?;
    for r in records {
        let line = serde_json::to_string(r)?;
        writeln!(f, "{line}")?;
    }
    Ok(())
}

pub fn print_summary(report: &Report) {
    let m = &report.metrics;
    println!("=== rquant backtest: {} ===", report.tree_name);
    println!("forward_window={} cost_bps={}", report.forward_window, report.cost_bps);
    println!("decisions={} scored={}", m.total_decisions, m.scored);
    println!(
        "active  : n={} mean_net={:.4} hit={:.1}%",
        m.active.count,
        m.active.mean_net,
        m.active.hit_rate * 100.0,
    );
    println!("t统计量   : {}", m.active.t_stat.map_or("—".to_string(), |v| format!("{:.2}", v)));
    println!(
        "T+1 exec: n={} mean_net={:.4} hit={:.1}%",
        m.t1_executable.count,
        m.t1_executable.mean_net,
        m.t1_executable.hit_rate * 100.0
    );
    println!("buy&hold={:.4}", m.buy_and_hold);
    println!(
        "gaps    : {} missing trading day(s), {} partial day(s)",
        report.gaps.missing_trading_days.len(),
        report.gaps.partial_days.len()
    );
    if let Some(wf) = &report.walk_forward {
        for (i, f) in wf.folds.iter().enumerate() {
            println!(
                "wf {}/{} [{} → {}]: n={} mean={:.4} hit={:.1}% | bh={:.4}",
                i + 1, wf.folds.len(), f.from, f.to, f.stat.count, f.stat.mean_net, f.stat.hit_rate * 100.0, f.buy_and_hold
            );
        }
        println!("wf summary: positive {}/{}, worst mean={:.4}", wf.positive_folds, wf.folds.len(), wf.worst_mean_net);
    }
    println!("[warn] {}", m.overlap_warning);
}

/// 软遍历回测完整报告，可序列化为 JSON。
#[derive(Debug, Serialize, Deserialize)]
pub struct SoftReport {
    /// YAML 树的 `meta.name`。
    pub tree_name: String,
    /// 前瞻窗口大小（bar 数），取自树 meta。
    pub forward_window: usize,
    /// 单边成本（bps），实际扣除双边（round-trip）。
    pub cost_bps: f64,
    /// 软模式汇总度量（engaged 期望净收益 / 净仓位口径）。
    pub soft: SoftMetrics,
    /// Walk-forward 折叠结果；`folds < 2` 时为 `None`，不参与 JSON 序列化。
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub walk_forward: Option<WalkForward>,
}

pub fn write_soft_report(report: &SoftReport, path: &Path) -> Result<()> {
    let json = serde_json::to_string_pretty(report)?;
    std::fs::write(path, json)?;
    Ok(())
}

pub fn print_soft_summary(report: &SoftReport) {
    let m = &report.soft;
    println!("=== rquant SOFT backtest: {} ===", report.tree_name);
    println!("forward_window={} cost_bps={}", report.forward_window, report.cost_bps);
    println!("decisions={} scored={}", m.total_decisions, m.scored);
    println!(
        "engaged : n={} mean_expected_net={:.4} hit={:.1}%",
        m.engaged.count, m.engaged.mean_net, m.engaged.hit_rate * 100.0,
    );
    println!("t统计量   : {}", m.engaged.t_stat.map_or("—".to_string(), |v| format!("{:.2}", v)));
    println!(
        "position: n={} mean_net={:.4} hit={:.1}%",
        m.position.count, m.position.mean_net, m.position.hit_rate * 100.0,
    );
    println!("buy&hold={:.4}", m.buy_and_hold);
    if let Some(wf) = &report.walk_forward {
        for (i, f) in wf.folds.iter().enumerate() {
            println!(
                "wf {}/{} [{} → {}]: n={} mean={:.4} hit={:.1}% | bh={:.4}",
                i + 1, wf.folds.len(), f.from, f.to, f.stat.count, f.stat.mean_net, f.stat.hit_rate * 100.0, f.buy_and_hold
            );
        }
        println!("wf summary: positive {}/{}, worst mean={:.4}", wf.positive_folds, wf.folds.len(), wf.worst_mean_net);
    }
    println!("[warn] {}", m.overlap_warning);
}

/// report 子命令的渲染模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportMode { Hard, Soft, Sim, Portfolio }

/// 读取回测产物并渲染自包含 HTML（CLI report 子命令的业务实现）。
/// Soft 模式时 primary 被忽略（expected_net 已在 traces 内；给了会 eprintln 提示）。
/// Sim 模式时 primary 被忽略（同理）。
/// Portfolio 模式时 traces/primary 均被忽略（PortfolioReport 自包含）。
pub fn render_report_files(
    report_path: &Path,
    out_path: &Path,
    traces_path: Option<&Path>,
    primary_path: Option<&Path>,
    mode: ReportMode,
) -> Result<()> {
    match mode {
        ReportMode::Soft => {
            let rep: SoftReport = serde_json::from_str(&std::fs::read_to_string(report_path)?)?;
            if primary_path.is_some() {
                eprintln!("[rquant] --primary ignored in --soft report (expected_net is in traces)");
            }
            let (series, avg, stack) = match traces_path {
                Some(tp) => {
                    let content = std::fs::read_to_string(tp)?;
                    let mut recs = Vec::new();
                    for line in content.lines().filter(|l| !l.trim().is_empty()) {
                        recs.push(serde_json::from_str::<crate::backtest::soft::SoftStepRecord>(line)?);
                    }
                    (
                        crate::report::curve::derive_soft_series(&recs),
                        crate::report::curve::avg_leaf_probs(&recs),
                        Some(crate::report::curve::leaf_prob_stack(&recs)),
                    )
                }
                None => (
                    crate::report::curve::EquitySeries {
                        points: vec![],
                        hist: crate::report::curve::Histogram { bins: vec![] },
                        skipped: 0,
                    },
                    vec![],
                    None,
                ),
            };
            let html = crate::report::viz::render_soft_html(&rep, &series, &avg, stack.as_ref());
            std::fs::write(out_path, html)?;
            println!("wrote soft HTML report to {}", out_path.display());
        }
        ReportMode::Hard => {
            let json = std::fs::read_to_string(report_path)?;
            let rep: Report = serde_json::from_str(&json)?;
            let series = match (traces_path, primary_path) {
                (Some(tp), Some(pp)) => {
                    let content = std::fs::read_to_string(tp)?;
                    let mut tr = Vec::new();
                    for line in content.lines().filter(|l| !l.trim().is_empty()) {
                        tr.push(serde_json::from_str::<crate::engine::trace::Trace>(line)?);
                    }
                    let bars = crate::data::reader::read_bars_csv(pp)?;
                    let costs = crate::backtest::costs::CostModel { round_trip_bps: rep.cost_bps };
                    Some(crate::report::curve::derive_series(&tr, &bars, rep.forward_window, &costs))
                }
                (None, None) => None,
                _ => {
                    eprintln!("[rquant] --traces and --primary must be given together to draw the curve; rendering aggregates only");
                    None
                }
            };
            let html = crate::report::viz::render_html(&rep, series.as_ref());
            std::fs::write(out_path, html)?;
            println!("wrote HTML report to {}", out_path.display());
        }
        ReportMode::Sim => {
            let rep: crate::backtest::sim::SimReport =
                serde_json::from_str(&std::fs::read_to_string(report_path)?)?;
            if primary_path.is_some() {
                eprintln!("[rquant] --primary ignored in --sim report (traces carry nav/pos)");
            }
            let steps: Option<Vec<crate::backtest::sim::SimStepRecord>> = match traces_path {
                Some(tp) => {
                    let content = std::fs::read_to_string(tp)?;
                    let mut recs = Vec::new();
                    for line in content.lines().filter(|l| !l.trim().is_empty()) {
                        recs.push(serde_json::from_str::<crate::backtest::sim::SimStepRecord>(line)?);
                    }
                    Some(recs)
                }
                None => None,
            };
            let html = crate::report::viz::render_sim_html(&rep, steps.as_deref());
            std::fs::write(out_path, html)?;
            println!("wrote sim HTML report to {}", out_path.display());
        }
        ReportMode::Portfolio => {
            let rep: crate::backtest::portfolio::PortfolioReport =
                serde_json::from_str(&std::fs::read_to_string(report_path)?)?;
            if traces_path.is_some() {
                eprintln!("[rquant] --traces ignored in --portfolio report (PortfolioReport is self-contained)");
            }
            if primary_path.is_some() {
                eprintln!("[rquant] --primary ignored in --portfolio report");
            }
            let html = crate::report::viz::render_portfolio_html(&rep);
            std::fs::write(out_path, html)?;
            println!("wrote portfolio HTML report to {}", out_path.display());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backtest::metrics::compute_metrics;

    #[test]
    fn report_serializes_to_json() {
        let metrics = compute_metrics(&[], &[]);
        let report = Report { tree_name: "t".into(), forward_window: 16, cost_bps: 10.0, metrics, gaps: GapReport::default(), walk_forward: None };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"tree_name\":\"t\""));
        assert!(json.contains("overlap_warning"));
        assert!(json.contains("missing_trading_days"));
    }

    #[test]
    fn traces_jsonl_writes_one_line_per_trace() {
        use crate::engine::trace::Trace;
        use crate::tree::schema::Stance;
        use chrono::NaiveDate;
        let t = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        let traces = vec![
            Trace { t, path: vec![], leaf: "x".into(), stance: Stance::Flat },
            Trace { t, path: vec![], leaf: "y".into(), stance: Stance::Long },
        ];
        let f = tempfile::NamedTempFile::new().unwrap();
        write_traces_jsonl(&traces, f.path()).unwrap();
        let content = std::fs::read_to_string(f.path()).unwrap();
        assert_eq!(content.lines().count(), 2);
    }

    #[test]
    fn report_round_trips_json() {
        let metrics = compute_metrics(&[], &[]);
        let report = Report { tree_name: "rt".into(), forward_window: 8, cost_bps: 5.0, metrics, gaps: GapReport::default(), walk_forward: None };
        let json = serde_json::to_string(&report).unwrap();
        let back: Report = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tree_name, "rt");
        assert_eq!(back.forward_window, 8);
        assert_eq!(back.metrics.total_decisions, 0);
    }

    #[test]
    fn trace_round_trips_json() {
        use crate::engine::trace::Trace;
        use crate::tree::schema::Stance;
        use chrono::NaiveDate;
        let t = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        let tr = Trace { t, path: vec![], leaf: "x".into(), stance: Stance::Long };
        let json = serde_json::to_string(&tr).unwrap();
        let back: Trace = serde_json::from_str(&json).unwrap();
        assert_eq!(back.leaf, "x");
        assert_eq!(back.stance, Stance::Long);
    }

    #[test]
    fn walk_forward_field_is_optional_and_compatible() {
        let metrics = compute_metrics(&[], &[]);
        let report = Report { tree_name: "wf".into(), forward_window: 8, cost_bps: 5.0, metrics, gaps: GapReport::default(), walk_forward: None };
        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("walk_forward"), "None must not serialize");
        let back: Report = serde_json::from_str(&json).unwrap(); // 旧 JSON（无键）可反序列化
        assert!(back.walk_forward.is_none());
    }

    // M7 — report JSON determinism: writing the same Report twice gives byte-identical output
    #[test]
    fn write_report_is_deterministic() {
        let metrics = compute_metrics(&[], &[]);
        let report = Report {
            tree_name: "det".into(),
            forward_window: 4,
            cost_bps: 10.0,
            metrics,
            gaps: GapReport::default(),
            walk_forward: None,
        };
        let f1 = tempfile::NamedTempFile::new().unwrap();
        let f2 = tempfile::NamedTempFile::new().unwrap();
        write_report(&report, f1.path()).unwrap();
        write_report(&report, f2.path()).unwrap();
        let s1 = std::fs::read_to_string(f1.path()).unwrap();
        let s2 = std::fs::read_to_string(f2.path()).unwrap();
        assert_eq!(s1, s2, "write_report output must be byte-identical on repeated calls");
    }

    #[test]
    fn soft_traces_jsonl_one_line_per_record() {
        use crate::backtest::soft::SoftStepRecord;
        use chrono::NaiveDate;
        use std::collections::BTreeMap;
        let t = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        let mut lp = BTreeMap::new();
        lp.insert("x".to_string(), 1.0);
        let recs = vec![
            SoftStepRecord { t, leaf_probs: lp.clone(), expected_net: Some(0.1) },
            SoftStepRecord { t, leaf_probs: lp, expected_net: None },
        ];
        let f = tempfile::NamedTempFile::new().unwrap();
        write_soft_traces_jsonl(&recs, f.path()).unwrap();
        let content = std::fs::read_to_string(f.path()).unwrap();
        assert_eq!(content.lines().count(), 2);
        let first: SoftStepRecord = serde_json::from_str(content.lines().next().unwrap()).unwrap();
        assert_eq!(first.expected_net, Some(0.1));
    }
}
