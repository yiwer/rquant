use crate::backtest::forward_return::ForwardResult;
use crate::data::bar::Bar;
use crate::engine::trace::Trace;
use crate::tree::schema::Stance;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(crate) const OVERLAP_WARNING: &str = "前瞻窗口重叠 → 样本自相关，t 值偏乐观，勿据此鼓吹显著性";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalStat {
    pub count: usize,
    pub mean_net: f64,
    pub hit_rate: f64,
    pub std: f64,
    pub t_stat: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Metrics {
    pub total_decisions: usize,
    pub scored: usize,
    pub active: SignalStat,
    pub t1_executable: SignalStat,
    pub by_leaf: BTreeMap<String, SignalStat>,
    pub by_stance: BTreeMap<String, SignalStat>,
    pub node_label_counts: BTreeMap<String, usize>,
    pub buy_and_hold: f64,
    pub overlap_warning: String,
}

pub(crate) fn signal_stat(nets: &[f64]) -> SignalStat {
    let count = nets.len();
    if count == 0 {
        return SignalStat { count: 0, mean_net: 0.0, hit_rate: 0.0, std: 0.0, t_stat: 0.0 };
    }
    let mean = nets.iter().sum::<f64>() / count as f64;
    let var = nets.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / count as f64;
    let std = var.sqrt();
    let wins = nets.iter().filter(|x| **x > 0.0).count();
    let hit_rate = wins as f64 / count as f64;
    let t_stat = if std == 0.0 { 0.0 } else { mean / (std / (count as f64).sqrt()) };
    SignalStat { count, mean_net: mean, hit_rate, std, t_stat }
}

/// 聚合度量。`primary` 应传**评估窗口**那段（如 warmup 之后），
/// `buy_and_hold` 即按该段首开盘 → 末收盘计算，与信号同口径。
pub fn compute_metrics(items: &[(Trace, Option<ForwardResult>)], primary: &[Bar]) -> Metrics {
    let total = items.len();
    let mut active_nets: Vec<f64> = Vec::new();
    let mut t1_nets: Vec<f64> = Vec::new();
    let mut by_leaf: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut by_stance: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut node_label_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut scored = 0;

    for (trace, fr) in items {
        for step in &trace.path {
            *node_label_counts
                .entry(format!("{}::{}", step.node_id, step.label))
                .or_insert(0) += 1;
        }
        if let Some(fr) = fr {
            scored += 1;
            let stance_name = format!("{:?}", trace.stance).to_lowercase();
            by_leaf.entry(trace.leaf.clone()).or_default().push(fr.net);
            by_stance.entry(stance_name).or_default().push(fr.net);
            if !matches!(trace.stance, Stance::Flat) {
                active_nets.push(fr.net);
                if fr.t1_executable {
                    t1_nets.push(fr.net);
                }
            }
        }
    }

    let buy_and_hold = if primary.len() >= 2 {
        primary.last().unwrap().close / primary.first().unwrap().open - 1.0
    } else {
        0.0
    };

    Metrics {
        total_decisions: total,
        scored,
        active: signal_stat(&active_nets),
        t1_executable: signal_stat(&t1_nets),
        by_leaf: by_leaf.iter().map(|(k, v)| (k.clone(), signal_stat(v))).collect(),
        by_stance: by_stance.iter().map(|(k, v)| (k.clone(), signal_stat(v))).collect(),
        node_label_counts,
        buy_and_hold,
        overlap_warning: OVERLAP_WARNING.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backtest::forward_return::ForwardResult;
    use crate::engine::trace::{StepRecord, Trace};
    use crate::tree::schema::Stance;
    use chrono::NaiveDate;

    fn trace(leaf: &str, stance: Stance) -> Trace {
        let t = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        Trace {
            t,
            path: vec![StepRecord { node_id: "a".into(), label: "up".into(), confidence: 1.0, rationale: "".into() }],
            leaf: leaf.into(),
            stance,
        }
    }

    #[test]
    fn aggregates_active_leaf_and_node_stats() {
        let items = vec![
            (trace("leaf_l", Stance::Long), Some(ForwardResult { gross: 0.05, net: 0.04, t1_executable: true })),
            (trace("leaf_l", Stance::Long), Some(ForwardResult { gross: -0.02, net: -0.03, t1_executable: false })),
            (trace("leaf_f", Stance::Flat), Some(ForwardResult { gross: 0.0, net: 0.0, t1_executable: false })),
        ];
        let primary = vec![];
        let m = compute_metrics(&items, &primary);
        assert_eq!(m.total_decisions, 3);
        assert_eq!(m.scored, 3);
        assert_eq!(m.active.count, 2);
        assert_eq!(m.t1_executable.count, 1);
        assert_eq!(m.by_leaf.get("leaf_l").unwrap().count, 2);
        assert_eq!(*m.node_label_counts.get("a::up").unwrap(), 3);
    }
}
