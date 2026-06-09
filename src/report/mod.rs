use crate::backtest::gaps::GapReport;
use crate::backtest::metrics::Metrics;
use crate::backtest::soft::SoftMetrics;
use crate::engine::trace::Trace;
use crate::Result;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
pub struct Report {
    pub tree_name: String,
    pub forward_window: usize,
    pub cost_bps: f64,
    pub metrics: Metrics,
    pub gaps: GapReport,
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

pub fn print_summary(report: &Report) {
    let m = &report.metrics;
    println!("=== rquant backtest: {} ===", report.tree_name);
    println!("forward_window={} cost_bps={}", report.forward_window, report.cost_bps);
    println!("decisions={} scored={}", m.total_decisions, m.scored);
    println!(
        "active  : n={} mean_net={:.4} hit={:.1}% t={:.2}",
        m.active.count,
        m.active.mean_net,
        m.active.hit_rate * 100.0,
        m.active.t_stat
    );
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
    println!("[warn] {}", m.overlap_warning);
}

#[derive(Debug, Serialize)]
pub struct SoftReport {
    pub tree_name: String,
    pub forward_window: usize,
    pub cost_bps: f64,
    pub soft: SoftMetrics,
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
        "engaged : n={} mean_expected_net={:.4} hit={:.1}% t={:.2}",
        m.engaged.count, m.engaged.mean_net, m.engaged.hit_rate * 100.0, m.engaged.t_stat
    );
    println!("buy&hold={:.4}", m.buy_and_hold);
    println!("[warn] {}", m.overlap_warning);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backtest::metrics::compute_metrics;

    #[test]
    fn report_serializes_to_json() {
        let metrics = compute_metrics(&[], &[]);
        let report = Report { tree_name: "t".into(), forward_window: 16, cost_bps: 10.0, metrics, gaps: GapReport::default() };
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
        let report = Report { tree_name: "rt".into(), forward_window: 8, cost_bps: 5.0, metrics, gaps: GapReport::default() };
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
}
