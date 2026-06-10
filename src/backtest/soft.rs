use crate::backtest::costs::CostModel;
use crate::backtest::forward_return::forward_return;
use crate::backtest::metrics::{signal_stat, SignalStat, OVERLAP_WARNING};
use crate::backtest::runner::BacktestConfig;
use crate::data::aux_table::AuxTable;
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
    pub exposure: f64,
    pub position_net: f64,
    pub t1_executable: bool,
}

/// 按叶子分布求期望净收益；任一叶子前瞻越界(None) → 整点 None。
pub fn score_soft(
    soft: &SoftTrace,
    tree: &Tree,
    primary: &[Bar],
    i: usize,
    costs: &CostModel,
) -> Option<SoftScore> {
    let mut expected_net = 0.0;
    let mut engaged = 0.0;
    let mut exposure = 0.0;
    let mut t1 = false;
    let mut max_h = 0usize;
    for (leaf_id, &p) in &soft.leaf_probs {
        let leaf = tree.leaves.get(leaf_id)?;
        let fr = forward_return(primary, i, leaf.horizon, leaf.stance, costs)?;
        let w = leaf.weight;
        expected_net += p * w * fr.net;
        exposure += p * w * match leaf.stance {
            Stance::Long => 1.0,
            Stance::Short => -1.0,
            Stance::Flat => 0.0,
        };
        if !matches!(leaf.stance, Stance::Flat) {
            engaged += p * w;
        }
        t1 |= fr.t1_executable;
        max_h = max_h.max(leaf.horizon);
    }
    // 净仓位口径：r 取分布内最大 horizon（最长腿；max_h 必属已过边界检查的集合 → 必 Some）
    let r = forward_return(primary, i, max_h, Stance::Long, costs)?.gross;
    let position_net = if exposure == 0.0 {
        0.0
    } else {
        exposure * r - (costs.round_trip_bps / 10_000.0) * exposure.abs()
    };
    Some(SoftScore { expected_net, engaged, exposure, position_net, t1_executable: t1 })
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SoftMetrics {
    pub total_decisions: usize,
    pub scored: usize,
    pub engaged: SignalStat,
    pub position: SignalStat,
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
    let mut position_nets: Vec<f64> = Vec::new();
    for s in items.iter().flatten() {
        scored += 1;
        if s.engaged > 0.0 {
            engaged_nets.push(s.expected_net);
        }
        if s.exposure.abs() > 0.0 {
            position_nets.push(s.position_net);
        }
    }
    let buy_and_hold = if primary.len() >= 2 {
        primary.last().unwrap().close / primary.first().unwrap().open - 1.0
    } else {
        0.0
    };
    SoftMetrics {
        total_decisions: total,
        scored,
        engaged: signal_stat(&engaged_nets),
        position: signal_stat(&position_nets),
        buy_and_hold,
        overlap_warning: OVERLAP_WARNING.into(),
    }
}

#[allow(clippy::too_many_arguments)]
async fn eval_point_soft(
    i: usize,
    primary: &[Bar],
    context: &[Bar],
    news: &[NewsRecord],
    aux: &BTreeMap<String, AuxTable>,
    tree: &Tree,
    costs: &CostModel,
    window: usize,
    llm: &LlmEvaluator,
) -> Result<(SoftTrace, Option<SoftScore>)> {
    let t = primary[i].time;
    let ctx = build_context(primary, context, news, aux, t, window);
    let soft = traverse_soft(tree, &ctx, llm).await?;
    let score = score_soft(&soft, tree, primary, i, costs);
    Ok((soft, score))
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
    let costs = CostModel { round_trip_bps: cfg.cost_bps };
    let fw = tree.meta.forward_window;
    let start = cfg.warmup.min(primary.len());
    let aux_tables: BTreeMap<String, AuxTable> = BTreeMap::new(); // Task 4 will wire real loading
    let results: Vec<(SoftTrace, Option<SoftScore>)> = stream::iter(start..primary.len())
        .map(|i| eval_point_soft(i, &primary, &context, &news, &aux_tables, &tree, &costs, cfg.window, llm))
        .buffered(cfg.concurrency.max(1))
        .collect::<Vec<Result<(SoftTrace, Option<SoftScore>)>>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>>>()?;
    let scores: Vec<Option<SoftScore>> = results.iter().map(|(_, s)| *s).collect();
    let metrics = soft_metrics(&scores, &primary[start..]);
    let walk_forward = if cfg.folds >= 2 {
        let nets: Vec<Option<f64>> = results
            .iter()
            .map(|(_, s)| match s {
                Some(x) if x.engaged > 0.0 => Some(x.expected_net),
                _ => None,
            })
            .collect();
        Some(crate::backtest::walkforward::walk_forward(&nets, &primary[start..], cfg.folds))
    } else {
        None
    };
    if let Some(tp) = &cfg.traces_path {
        let records: Vec<SoftStepRecord> = results
            .iter()
            .map(|(tr, s)| SoftStepRecord {
                t: tr.t,
                leaf_probs: tr.leaf_probs.clone(),
                expected_net: s.map(|x| x.expected_net),
            })
            .collect();
        crate::report::write_soft_traces_jsonl(&records, tp)?;
    }
    let report = SoftReport {
        tree_name: tree.meta.name.clone(),
        forward_window: fw,
        cost_bps: cfg.cost_bps,
        soft: metrics,
        walk_forward,
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
        let s = score_soft(&soft, &tree, &primary, 0, &costs).unwrap();
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
        assert!(score_soft(&soft, &tree, &primary, 1, &costs).is_none());
    }

    #[test]
    fn soft_metrics_aggregates_engaged() {
        let items = vec![
            Some(SoftScore { expected_net: 0.04, engaged: 0.5, exposure: 0.5, position_net: 0.04, t1_executable: true }),
            Some(SoftScore { expected_net: -0.02, engaged: 0.3, exposure: -0.3, position_net: -0.02, t1_executable: false }),
            Some(SoftScore { expected_net: 0.0, engaged: 0.0, exposure: 0.0, position_net: 0.0, t1_executable: false }),
            None,
        ];
        let m = soft_metrics(&items, &[]);
        assert_eq!(m.total_decisions, 4);
        assert_eq!(m.scored, 3);
        assert_eq!(m.engaged.count, 2);
        assert_eq!(m.position.count, 2); // 仅 |exposure|>0 两点
    }

    #[test]
    fn position_equals_expected_for_long_flat() {
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
        let s = score_soft(&soft, &tree, &primary, 0, &costs).unwrap();
        // long/flat 下净仓位 ≡ 逐腿期望（成本线性）
        assert!((s.position_net - s.expected_net).abs() < 1e-12);
        assert!((s.exposure - 0.5).abs() < 1e-9);
    }

    #[test]
    fn position_nets_out_hedged_legs() {
        // 树要含 short：行内构造三叶树
        const TREE_LS: &str = r#"
meta: { name: t, forward_window: 2, stances: [long, flat, short] }
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "close > 0", goto: leaf_l, label: up } ]
    default: { goto: leaf_f, label: flat }
leaves:
  leaf_l: { stance: long }
  leaf_s: { stance: short }
  leaf_f: { stance: flat }
"#;
        let tree = load_tree_str(TREE_LS).unwrap();
        let primary = vec![
            bar("2024-01-02 14:45:00", 9.0, 9.5),
            bar("2024-01-02 15:00:00", 10.0, 10.2),
            bar("2024-01-03 09:45:00", 10.2, 11.0),
        ];
        let costs = CostModel { round_trip_bps: 10.0 };
        let mut lp = BTreeMap::new();
        lp.insert("leaf_l".to_string(), 0.6);
        lp.insert("leaf_s".to_string(), 0.4);
        let soft = SoftTrace { t: primary[0].time, leaf_probs: lp };
        let s = score_soft(&soft, &tree, &primary, 0, &costs).unwrap();
        // r = 11/10 - 1 = 0.1, rate = 0.001
        // E = 0.6 - 0.4 = 0.2；position_net = 0.2*0.1 - 0.001*0.2 = 0.0198
        assert!((s.exposure - 0.2).abs() < 1e-9);
        assert!((s.position_net - 0.0198).abs() < 1e-9);
        // 逐腿：0.6*(0.1-0.001) + 0.4*(-0.1-0.001) = 0.0594 - 0.0404 = 0.019
        assert!((s.expected_net - 0.019).abs() < 1e-9);
    }

    #[test]
    fn all_flat_has_zero_exposure_and_position() {
        let tree = load_tree_str(TREE).unwrap();
        let primary = vec![
            bar("2024-01-02 14:45:00", 9.0, 9.5),
            bar("2024-01-02 15:00:00", 10.0, 10.2),
            bar("2024-01-03 09:45:00", 10.2, 11.0),
        ];
        let costs = CostModel { round_trip_bps: 10.0 };
        let mut lp = BTreeMap::new();
        lp.insert("leaf_f".to_string(), 1.0);
        let soft = SoftTrace { t: primary[0].time, leaf_probs: lp };
        let s = score_soft(&soft, &tree, &primary, 0, &costs).unwrap();
        assert_eq!(s.exposure, 0.0);
        assert_eq!(s.position_net, 0.0);
    }

    #[test]
    fn leaf_weight_scales_soft_score() {
        const TREE_W: &str = r#"
meta: { name: t, forward_window: 2, stances: [long, flat] }
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "close > 0", goto: leaf_l, label: up } ]
    default: { goto: leaf_f, label: flat }
leaves:
  leaf_l: { stance: long, weight: 0.5 }
  leaf_f: { stance: flat }
"#;
        let tree = load_tree_str(TREE_W).unwrap();
        let primary = vec![
            bar("2024-01-02 14:45:00", 9.0, 9.5),
            bar("2024-01-02 15:00:00", 10.0, 10.2),
            bar("2024-01-03 09:45:00", 10.2, 11.0),
        ];
        let costs = CostModel { round_trip_bps: 10.0 };
        let mut lp = BTreeMap::new();
        lp.insert("leaf_l".to_string(), 1.0);
        let soft = SoftTrace { t: primary[0].time, leaf_probs: lp };
        let s = score_soft(&soft, &tree, &primary, 0, &costs).unwrap();
        // net_long = 0.099；w=0.5 → expected 0.0495；exposure/engaged = 0.5
        assert!((s.expected_net - 0.0495).abs() < 1e-9);
        assert!((s.exposure - 0.5).abs() < 1e-9);
        assert!((s.engaged - 0.5).abs() < 1e-9);
    }

    #[test]
    fn leaf_horizon_overrides_global_window() {
        const TREE_H: &str = r#"
meta: { name: t, forward_window: 16, stances: [long, flat] }
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "close > 0", goto: leaf_l, label: up } ]
    default: { goto: leaf_f, label: flat }
leaves:
  leaf_l: { stance: long, horizon: 2 }
  leaf_f: { stance: flat, horizon: 2 }
"#;
        let tree = load_tree_str(TREE_H).unwrap();
        let primary = vec![
            bar("2024-01-02 14:45:00", 9.0, 9.5),
            bar("2024-01-02 15:00:00", 10.0, 10.2),
            bar("2024-01-03 09:45:00", 10.2, 11.0),
        ];
        let costs = CostModel { round_trip_bps: 10.0 };
        let mut lp = BTreeMap::new();
        lp.insert("leaf_l".to_string(), 1.0);
        let soft = SoftTrace { t: primary[0].time, leaf_probs: lp };
        // 全局 fw=16 在 3 根 bar 下必越界；leaf horizon=2 仍可计分
        assert!(score_soft(&soft, &tree, &primary, 0, &costs).is_some());
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
