use crate::backtest::costs::CostModel;
use crate::backtest::forward_return::{forward_return, ForwardResult};
use crate::backtest::metrics::compute_metrics;
use crate::engine::trace::Trace;
use crate::features::context::build_context;
use crate::report::Report;
use crate::Result;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct BacktestConfig {
    pub tree_path: PathBuf,
    pub primary_path: PathBuf,
    pub context_path: PathBuf,
    pub out_path: PathBuf,
    pub traces_path: Option<PathBuf>,
    pub cost_bps: f64,
    pub warmup: usize,
    pub window: usize,
}

/// 端到端：加载树+数据 → 逐时点遍历 → 前瞻收益 → 度量 → 写报告。返回 Report。
pub fn run(cfg: &BacktestConfig) -> Result<Report> {
    let tree = crate::tree::loader::load_tree_file(&cfg.tree_path)?;
    let primary = crate::data::reader::read_bars_csv(&cfg.primary_path)?;
    let context = crate::data::reader::read_bars_csv(&cfg.context_path)?;
    let costs = CostModel { round_trip_bps: cfg.cost_bps };

    let mut items: Vec<(Trace, Option<ForwardResult>)> = Vec::new();
    let mut traces: Vec<Trace> = Vec::new();
    let start = cfg.warmup.min(primary.len());
    for i in start..primary.len() {
        let t = primary[i].time;
        let ctx = build_context(&primary, &context, t, cfg.window);
        let trace = crate::engine::traversal::traverse(&tree, &ctx)?;
        let fr = forward_return(&primary, i, tree.meta.forward_window, trace.stance, &costs);
        traces.push(trace.clone());
        items.push((trace, fr));
    }

    let metrics = compute_metrics(&items, &primary);
    let report = Report {
        tree_name: tree.meta.name.clone(),
        forward_window: tree.meta.forward_window,
        cost_bps: cfg.cost_bps,
        metrics,
    };
    crate::report::write_report(&report, &cfg.out_path)?;
    if let Some(tp) = &cfg.traces_path {
        crate::report::write_traces_jsonl(&traces, tp)?;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[test]
    fn example_tree_loads_and_validates() {
        let tree = crate::tree::loader::load_tree_file(Path::new("examples/trend_tree.yaml")).unwrap();
        assert_eq!(tree.root, "trend");
        assert_eq!(tree.nodes.len(), 3);
        assert_eq!(tree.leaves.len(), 3);
    }
}
