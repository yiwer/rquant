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
pub fn certify(reports: &[(String, OptimizeReport)], strategy: &str, _th: &GateThresholds) -> Verdict {
    let gates: Vec<GateOutcome> = vec![]; // Task 3-7 起替换为 5 个 gate 调用
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
