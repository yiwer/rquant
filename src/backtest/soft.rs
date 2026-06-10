use crate::backtest::costs::CostModel;
use crate::backtest::forward_return::forward_return;
use crate::backtest::metrics::{signal_stat, SignalStat};
use crate::backtest::runner::BacktestConfig;
use crate::data::bar::Bar;
use crate::data::news::{read_news_csv, NewsRecord};
use crate::data::reader::read_bars_csv;
use crate::engine::soft::{traverse_soft, SoftTrace};
use crate::eval::llm::LlmEvaluator;
use crate::features::context::build_context;
use crate::report::{write_soft_report, SoftReport};
use crate::tree::loader::{load_tree_file, Tree};
use crate::tree::schema::Stance;
use crate::Result;
use chrono::NaiveDateTime;
use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy)]
pub struct SoftScore {
    pub expected_net: f64,
    pub engaged: f64,
    pub t1_executable: bool,
}

/// 按叶子分布求期望净收益；任一叶子前瞻越界(None) → 整点 None。
pub fn score_soft(
    soft: &SoftTrace,
    tree: &Tree,
    primary: &[Bar],
    i: usize,
    fw: usize,
    costs: &CostModel,
) -> Option<SoftScore> {
    let mut expected_net = 0.0;
    let mut engaged = 0.0;
    let mut t1 = false;
    for (leaf_id, &p) in &soft.leaf_probs {
        let stance = tree.leaves.get(leaf_id)?.stance;
        let fr = forward_return(primary, i, fw, stance, costs)?;
        expected_net += p * fr.net;
        if !matches!(stance, Stance::Flat) {
            engaged += p;
        }
        t1 |= fr.t1_executable;
    }
    Some(SoftScore { expected_net, engaged, t1_executable: t1 })
}

#[derive(Debug, Serialize)]
pub struct SoftMetrics {
    pub total_decisions: usize,
    pub scored: usize,
    pub engaged: SignalStat,
    pub buy_and_hold: f64,
    pub overlap_warning: String,
}

/// 软模式逐点 trace 记录：决策点时间、叶子分布、期望净收益（未计分点为 None）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoftStepRecord {
    pub t: NaiveDateTime,
    pub leaf_probs: BTreeMap<String, f64>,
    pub expected_net: Option<f64>,
}

/// 聚合软度量：engaged = 在 engaged>0 的已评分点上对 expected_net 做 SignalStat。
/// `primary` 应传评估窗口段（warmup 之后），buy_and_hold 同口径。
pub fn soft_metrics(items: &[Option<SoftScore>], primary: &[Bar]) -> SoftMetrics {
    let total = items.len();
    let mut scored = 0;
    let mut engaged_nets: Vec<f64> = Vec::new();
    for s in items.iter().flatten() {
        scored += 1;
        if s.engaged > 0.0 {
            engaged_nets.push(s.expected_net);
        }
    }
    let buy_and_hold = if primary.len() >= 2 {
        primary.last().unwrap().close / primary[0].open - 1.0
    } else {
        0.0
    };
    SoftMetrics {
        total_decisions: total,
        scored,
        engaged: signal_stat(&engaged_nets),
        buy_and_hold,
        overlap_warning: "前瞻窗口重叠 → 样本自相关，t 值偏乐观，勿据此鼓吹显著性".into(),
    }
}

#[allow(clippy::too_many_arguments)]
async fn eval_point_soft(
    i: usize,
    primary: &[Bar],
    context: &[Bar],
    news: &[NewsRecord],
    tree: &Tree,
    costs: &CostModel,
    fw: usize,
    window: usize,
    llm: &LlmEvaluator,
) -> Result<Option<SoftScore>> {
    let t = primary[i].time;
    let ctx = build_context(primary, context, news, t, window);
    let soft = traverse_soft(tree, &ctx, llm).await?;
    Ok(score_soft(&soft, tree, primary, i, fw, costs))
}

/// 软遍历回测：与 `run` 同构，每点用 traverse_soft + score_soft，聚合成 SoftReport。
pub async fn run_soft(cfg: &BacktestConfig, llm: &LlmEvaluator) -> Result<SoftReport> {
    let tree = load_tree_file(&cfg.tree_path)?;
    let primary = read_bars_csv(&cfg.primary_path)?;
    let context = read_bars_csv(&cfg.context_path)?;
    let news: Vec<NewsRecord> = match &cfg.news_path {
        Some(p) => read_news_csv(p)?,
        None => Vec::new(),
    };
    // 数据质量告警（与硬模式同口径；软模式不把缺口写进 SoftReport，仅告警）
    let holidays = match &cfg.holidays_path {
        Some(p) => crate::data::calendar::read_holidays(p)?,
        None => std::collections::HashSet::new(),
    };
    let gaps = crate::backtest::gaps::detect_gaps(&primary, &crate::data::calendar::AShareCalendar::new(holidays));
    if !gaps.is_empty() {
        eprintln!(
            "[rquant] data gaps on primary: {} missing trading day(s), {} partial day(s)",
            gaps.missing_trading_days.len(),
            gaps.partial_days.len()
        );
        if cfg.holidays_path.is_none() {
            eprintln!("  note: no --holidays provided; A-share holidays may be reported as missing trading days");
        }
    }
    if cfg.traces_path.is_some() {
        eprintln!("[rquant] note: --traces is not written in --soft mode yet (SoftReport carries expected_net only)");
    }
    let costs = CostModel { round_trip_bps: cfg.cost_bps };
    let fw = tree.meta.forward_window;
    let start = cfg.warmup.min(primary.len());
    let results: Vec<Option<SoftScore>> = stream::iter(start..primary.len())
        .map(|i| eval_point_soft(i, &primary, &context, &news, &tree, &costs, fw, cfg.window, llm))
        .buffered(cfg.concurrency.max(1))
        .collect::<Vec<Result<Option<SoftScore>>>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>>>()?;
    let metrics = soft_metrics(&results, &primary[start..]);
    let report = SoftReport {
        tree_name: tree.meta.name.clone(),
        forward_window: fw,
        cost_bps: cfg.cost_bps,
        soft: metrics,
    };
    write_soft_report(&report, &cfg.out_path)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::soft::SoftTrace;
    use crate::tree::loader::load_tree_str;
    use chrono::NaiveDateTime;
    use std::collections::BTreeMap;

    fn bar(t: &str, open: f64, close: f64) -> Bar {
        Bar {
            time: NaiveDateTime::parse_from_str(t, "%Y-%m-%d %H:%M:%S").unwrap(),
            open, high: open.max(close), low: open.min(close), close, volume: 1.0,
        }
    }
    const TREE: &str = r#"
meta: { name: t, forward_window: 2, stances: [long, flat] }
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "close > 0", goto: leaf_l, label: up } ]
    default: { goto: leaf_f, label: flat }
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;

    #[test]
    fn score_soft_expected_net() {
        let tree = load_tree_str(TREE).unwrap();
        let primary = vec![
            bar("2024-01-02 14:45:00", 9.0, 9.5),
            bar("2024-01-02 15:00:00", 10.0, 10.2),
            bar("2024-01-03 09:45:00", 10.2, 11.0),
        ];
        let costs = CostModel { round_trip_bps: 10.0 };
        let mut lp = BTreeMap::new();
        lp.insert("leaf_l".to_string(), 0.5);
        lp.insert("leaf_f".to_string(), 0.5);
        let soft = SoftTrace { t: primary[0].time, leaf_probs: lp };
        let s = score_soft(&soft, &tree, &primary, 0, 2, &costs).unwrap();
        // long net = 11/10-1-0.001 = 0.099; flat = 0; expected = 0.5*0.099
        assert!((s.expected_net - 0.0495).abs() < 1e-9);
        assert!((s.engaged - 0.5).abs() < 1e-9);
        assert!(s.t1_executable);
    }

    #[test]
    fn score_soft_out_of_range_is_none() {
        let tree = load_tree_str(TREE).unwrap();
        let primary = vec![bar("2024-01-02 14:45:00", 9.0, 9.5), bar("2024-01-02 15:00:00", 10.0, 10.2)];
        let costs = CostModel { round_trip_bps: 10.0 };
        let mut lp = BTreeMap::new();
        lp.insert("leaf_l".to_string(), 1.0);
        let soft = SoftTrace { t: primary[0].time, leaf_probs: lp };
        assert!(score_soft(&soft, &tree, &primary, 1, 2, &costs).is_none());
    }

    #[test]
    fn soft_metrics_aggregates_engaged() {
        let items = vec![
            Some(SoftScore { expected_net: 0.04, engaged: 0.5, t1_executable: true }),
            Some(SoftScore { expected_net: -0.02, engaged: 0.3, t1_executable: false }),
            Some(SoftScore { expected_net: 0.0, engaged: 0.0, t1_executable: false }),
            None,
        ];
        let m = soft_metrics(&items, &[]);
        assert_eq!(m.total_decisions, 4);
        assert_eq!(m.scored, 3);
        assert_eq!(m.engaged.count, 2);
    }

    #[test]
    fn soft_step_record_round_trips() {
        use std::collections::BTreeMap;
        let mut lp = BTreeMap::new();
        lp.insert("leaf_l".to_string(), 0.7);
        lp.insert("leaf_f".to_string(), 0.3);
        let t = chrono::NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        let rec = SoftStepRecord { t, leaf_probs: lp, expected_net: Some(0.05) };
        let json = serde_json::to_string(&rec).unwrap();
        let back: SoftStepRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(back.t, t);
        assert_eq!(back.leaf_probs.len(), 2);
        assert_eq!(back.expected_net, Some(0.05));
        // None 也往返
        let rec2 = SoftStepRecord { t, leaf_probs: BTreeMap::new(), expected_net: None };
        let back2: SoftStepRecord = serde_json::from_str(&serde_json::to_string(&rec2).unwrap()).unwrap();
        assert_eq!(back2.expected_net, None);
    }
}
