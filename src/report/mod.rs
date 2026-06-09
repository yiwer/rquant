use crate::backtest::gaps::GapReport;
use crate::backtest::metrics::Metrics;
use crate::engine::trace::Trace;
use crate::Result;
use serde::Serialize;
use std::io::Write;
use std::path::Path;

#[derive(Debug, Serialize)]
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
}
