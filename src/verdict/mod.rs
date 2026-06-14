//! WFO 五门槛策略级自动裁决（设计 2026-06-14-eval-gate-verdict-design.md §7）。
//! 纯函数：吃 N 个 (symbol, OptimizeReport) → Verdict。无 IO。

use crate::optimize::OptimizeReport;
use serde::{Deserialize, Serialize};

pub mod gates;

/// 五门槛阈值（::default() = 文档方法学编码；Phase-1 不暴露 CLI）。
#[derive(Debug, Clone)]
pub struct GateThresholds {
    pub os_positive_symbol_frac: f64,    // ① ≥60% 标的有 OS 正折
    pub min_degradation: f64,            // ② 退化比下限
    pub degradation_symbol_frac: f64,    // ② 健康标的占比下限
    pub drift_stable_unique_frac: f64,   // ③ 参数 n_unique ≤ ⌈frac×OS折数⌉ 为稳
    pub drift_stable_symbol_frac: f64,   // ③ 稳标的占比下限
    pub drift_consensus_frac: f64,       // ③ 跨标的众数共识下限
    pub interior_symbol_frac: f64,       // ④ 内点标的占比下限
    pub max_single_symbol_os_share: f64, // ⑤ 单标的正 OS 份额上限
}

impl Default for GateThresholds {
    fn default() -> Self {
        Self {
            os_positive_symbol_frac: 0.6,
            min_degradation: 0.5,
            degradation_symbol_frac: 0.6,
            drift_stable_unique_frac: 0.5,
            drift_stable_symbol_frac: 0.6,
            drift_consensus_frac: 0.6,
            interior_symbol_frac: 0.6,
            max_single_symbol_os_share: 0.5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GateStatus {
    Pass,
    Fail,
    Indeterminate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateOutcome {
    pub gate: String,
    pub status: GateStatus,
    pub value: f64,
    pub threshold: f64,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Verdict {
    pub strategy: String,
    pub n_symbols: usize,
    pub certified: bool,
    pub gates: Vec<GateOutcome>,
    pub failed_gates: Vec<String>,
}

/// 入口：五门槛全 Pass → certified。Indeterminate ≠ Pass（保守）。
pub fn certify(reports: &[(String, OptimizeReport)], strategy: &str, th: &GateThresholds) -> Verdict {
    let gates = vec![
        gates::gate_os_breadth(reports, th),
        gates::gate_degradation(reports, th),
        gates::gate_param_drift(reports, th),
        gates::gate_interior(reports, th),
        gates::gate_not_single(reports, th),
    ];
    let certified = gates.iter().all(|g| g.status == GateStatus::Pass);
    let failed_gates = gates
        .iter()
        .filter(|g| g.status != GateStatus::Pass)
        .map(|g| g.gate.clone())
        .collect();
    Verdict {
        strategy: strategy.to_string(),
        n_symbols: reports.len(),
        certified,
        gates,
        failed_gates,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optimize::OptimizeReport;

    fn os_report(primary: &str, os: &[f64]) -> (String, OptimizeReport) {
        // 复用 gates::tests 的构造思路，这里独立构造仅 OS 折
        let fold_results = os.iter().enumerate().map(|(i, v)| crate::optimize::FoldResult {
            fold: i + 2,
            is_from: chrono::NaiveDate::from_ymd_opt(2025,1,1).unwrap().and_hms_opt(0,0,0).unwrap(),
            is_to: chrono::NaiveDate::from_ymd_opt(2025,1,1).unwrap().and_hms_opt(0,0,0).unwrap(),
            os_from: chrono::NaiveDate::from_ymd_opt(2025,1,1).unwrap().and_hms_opt(0,0,0).unwrap(),
            os_to: chrono::NaiveDate::from_ymd_opt(2025,1,1).unwrap().and_hms_opt(0,0,0).unwrap(),
            best_params: None, is_objective: Some(1.0), os_objective: Some(*v), degradation: None,
        }).collect();
        (primary.into(), OptimizeReport {
            mode: "sim".into(), objective_name: "sharpe".into(), folds: os.len()+1, n_combos: 1,
            fold_results, os_mean_objective: None, full_sample_best: None, drift: vec![],
            is_top5: vec![], axes: vec![], primary: primary.into(),
        })
    }

    #[test]
    fn tree4_regression_lock_os_counts_and_not_certified() {
        // 树4 真实 10 标的 3 OS 折（来自 tmps/wfo_ma_*.json）
        let reps = vec![
            os_report("sh600030", &[1.031, 0.993, -2.281]),
            os_report("sh600036", &[0.555, 0.770, -1.694]),
            os_report("sh600276", &[-2.303, 0.935, 0.0]),
            os_report("sh600519", &[-0.656, -0.637, -2.653]),
            os_report("sh600900", &[-0.211, -0.052, -1.318]),
            os_report("sh601088", &[-1.894, 0.654, -1.186]),
            os_report("sh601318", &[-0.393, 0.728, -0.110]),
            os_report("sz000333", &[-1.427, 0.549, -0.459]),
            os_report("sz000858", &[-0.428, -0.937, 0.0]),
            os_report("sz300750", &[-0.639, 1.964, -0.610]),
        ];
        // 直接锁"正 OS 折总数 = 9"（手工曾误数为 10）
        let total_pos: usize = reps.iter()
            .map(|(_, r)| r.fold_results.iter().filter(|f| f.os_objective.unwrap_or(0.0) > 0.0).count())
            .sum();
        assert_eq!(total_pos, 9, "树4 OS 正折总数必须是 9（非手工误数的 10）");

        let v = certify(&reps, "ma_stack", &GateThresholds::default());
        // 门槛① 广度 = 7/10 标的有正折 → Pass
        let t1 = v.gates.iter().find(|g| g.gate == "T1_os_breadth").unwrap();
        assert_eq!(t1.status, GateStatus::Pass);
        assert!((t1.value - 0.7).abs() < 1e-9, "广度必须是 7/10");
        // 门槛④ axes 全空 → Fail（保守，提示重跑 --auto-extend）
        let t4 = v.gates.iter().find(|g| g.gate == "T4_interior").unwrap();
        assert_eq!(t4.status, GateStatus::Fail);
        // 整体未认证
        assert!(!v.certified, "树4 必须未认证");
    }

    #[test]
    fn thresholds_default_encodes_methodology() {
        let t = GateThresholds::default();
        assert_eq!(t.os_positive_symbol_frac, 0.6);
        assert_eq!(t.min_degradation, 0.5);
        assert_eq!(t.degradation_symbol_frac, 0.6);
        assert_eq!(t.drift_stable_unique_frac, 0.5);
        assert_eq!(t.drift_stable_symbol_frac, 0.6);
        assert_eq!(t.drift_consensus_frac, 0.6);
        assert_eq!(t.interior_symbol_frac, 0.6);
        assert_eq!(t.max_single_symbol_os_share, 0.5);
    }

    #[test]
    fn gate_status_serializes_lowercase() {
        assert_eq!(serde_json::to_string(&GateStatus::Pass).unwrap(), "\"pass\"");
        assert_eq!(serde_json::to_string(&GateStatus::Indeterminate).unwrap(), "\"indeterminate\"");
    }
}
