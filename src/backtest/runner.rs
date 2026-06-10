use crate::backtest::costs::CostModel;
use crate::backtest::forward_return::{forward_return, ForwardResult};
use crate::backtest::metrics::compute_metrics;
use crate::data::aux_table::AuxTable;
use crate::data::bar::Bar;
use crate::data::news::NewsRecord;
use crate::engine::trace::Trace;
use crate::eval::llm::LlmEvaluator;
use crate::features::context::build_context;
use crate::report::Report;
use crate::tree::loader::Tree;
use crate::tree::schema::Stance;
use crate::Result;
use futures::stream::{self, StreamExt};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// 回测运行时配置，硬/软模式共用。
#[derive(Debug, Clone)]
pub struct BacktestConfig {
    /// YAML 决策树路径。
    pub tree_path: PathBuf,
    /// 主行情 CSV（用于前瞻收益计算）。
    pub primary_path: PathBuf,
    /// 上下文行情 CSV（DSL 表达式的 context 序列）。
    pub context_path: PathBuf,
    /// 可选新闻 CSV 路径（LLM 节点 inputs 引用）。
    pub news_path: Option<PathBuf>,
    /// 报告 JSON 输出路径。
    pub out_path: PathBuf,
    /// 可选 trace JSONL 输出路径。
    pub traces_path: Option<PathBuf>,
    /// 单边交易成本（基点），实际按 round-trip 扣除。
    pub cost_bps: f64,
    /// 跳过前 N bar 不评估（数据预热期，不计入度量）。
    pub warmup: usize,
    /// 每次评估时回看的上下文窗口长度（bar 数）。
    pub window: usize,
    /// LLM 调用的异步并发数。
    pub concurrency: usize,
    /// 可选节假日 CSV 路径，用于数据缺口检测。
    pub holidays_path: Option<PathBuf>,
    /// Walk-forward 折叠数；`>= 2` 时启用，否则不计算。
    pub folds: usize,
}

#[allow(clippy::too_many_arguments)]
async fn eval_point(
    i: usize,
    primary: &[Bar],
    context: &[Bar],
    news: &[NewsRecord],
    aux: &BTreeMap<String, AuxTable>,
    tree: &Tree,
    costs: &CostModel,
    fw: usize,
    window: usize,
    llm: &LlmEvaluator,
) -> Result<(Trace, Option<ForwardResult>)> {
    let t = primary[i].time;
    let ctx = build_context(primary, context, news, aux, t, window);
    let trace = crate::engine::traversal::traverse(tree, &ctx, llm).await?;
    let fr = match tree.leaves.get(&trace.leaf) {
        Some(l) => forward_return(primary, i, l.horizon, trace.stance, costs).map(|f| ForwardResult {
            gross: f.gross * l.weight,
            net: f.net * l.weight,
            t1_executable: f.t1_executable,
        }),
        None => forward_return(primary, i, fw, trace.stance, costs), // 防御（validate 保证不可达）
    };
    Ok((trace, fr))
}

/// 端到端（异步、有序并发）：加载→逐点遍历→前瞻收益→度量→写报告。
pub async fn run(cfg: &BacktestConfig, llm: &LlmEvaluator) -> Result<Report> {
    let tree = crate::tree::loader::load_tree_file(&cfg.tree_path)?;
    let primary = crate::data::reader::read_bars_csv(&cfg.primary_path)?;
    let context = crate::data::reader::read_bars_csv(&cfg.context_path)?;
    let news: Vec<NewsRecord> = match &cfg.news_path {
        Some(p) => crate::data::news::read_news_csv(p)?,
        None => Vec::new(),
    };
    let holidays = match &cfg.holidays_path {
        Some(p) => crate::data::calendar::read_holidays(p)?,
        None => std::collections::HashSet::new(),
    };
    let calendar = crate::data::calendar::AShareCalendar::new(holidays);
    let gaps = crate::backtest::gaps::detect_gaps(&primary, &calendar);
    if !gaps.is_empty() {
        eprintln!(
            "[rquant] data gaps on primary: {} missing trading day(s), {} partial day(s) (see report.gaps)",
            gaps.missing_trading_days.len(),
            gaps.partial_days.len()
        );
        if cfg.holidays_path.is_none() {
            eprintln!("  note: no --holidays provided; A-share holidays may be reported as missing trading days");
        }
    }
    let costs = CostModel { round_trip_bps: cfg.cost_bps };
    let fw = tree.meta.forward_window;
    let start = cfg.warmup.min(primary.len());
    let aux_tables: BTreeMap<String, AuxTable> = BTreeMap::new(); // Task 4 will wire real loading

    let results: Vec<(Trace, Option<ForwardResult>)> = stream::iter(start..primary.len())
        .map(|i| eval_point(i, &primary, &context, &news, &aux_tables, &tree, &costs, fw, cfg.window, llm))
        .buffered(cfg.concurrency.max(1))
        .collect::<Vec<Result<(Trace, Option<ForwardResult>)>>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>>>()?;

    let traces: Vec<Trace> = results.iter().map(|(t, _)| t.clone()).collect();
    // buy&hold 基准跨与信号相同的"过预热"窗口（不含 warmup 前缀），同口径对比
    let metrics = compute_metrics(&results, &primary[start..]);
    let walk_forward = if cfg.folds >= 2 {
        let nets: Vec<Option<f64>> = results
            .iter()
            .map(|(tr, fr)| match fr {
                Some(f) if tr.stance != Stance::Flat => Some(f.net),
                _ => None,
            })
            .collect();
        Some(crate::backtest::walkforward::walk_forward(&nets, &primary[start..], cfg.folds))
    } else {
        None
    };
    let report = Report {
        tree_name: tree.meta.name.clone(),
        forward_window: fw,
        cost_bps: cfg.cost_bps,
        metrics,
        gaps,
        walk_forward,
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
