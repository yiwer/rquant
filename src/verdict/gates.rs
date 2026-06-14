//! 五门槛纯函数（设计 §7.2）。
#![allow(unused)] // Task 3-7 逐个实现后移除
use super::{GateOutcome, GateStatus, GateThresholds};
use crate::optimize::OptimizeReport;

/// 该标的是否有 ≥1 个 OS 正折。
fn symbol_has_positive_os(r: &OptimizeReport) -> bool {
    r.fold_results.iter().any(|f| f.os_objective.is_some_and(|v| v > 0.0))
}

/// ① OS 广度：有 OS 正折的标的占比 ≥ 阈值。
pub fn gate_os_breadth(reports: &[(String, OptimizeReport)], th: &GateThresholds) -> GateOutcome {
    let n = reports.len();
    let positive = reports.iter().filter(|(_, r)| symbol_has_positive_os(r)).count();
    let value = if n == 0 { 0.0 } else { positive as f64 / n as f64 };
    let status = if n == 0 {
        GateStatus::Indeterminate
    } else if value >= th.os_positive_symbol_frac {
        GateStatus::Pass
    } else {
        GateStatus::Fail
    };
    GateOutcome {
        gate: "T1_os_breadth".into(),
        status,
        value,
        threshold: th.os_positive_symbol_frac,
        note: format!("{positive}/{n} symbols have >=1 positive OS fold"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optimize::{AxisOutcome, ComboScore, FoldResult, OptimizeReport, ParamDrift};
    use chrono::NaiveDate;
    use std::collections::BTreeMap;

    fn dt() -> chrono::NaiveDateTime {
        NaiveDate::from_ymd_opt(2025, 1, 1).unwrap().and_hms_opt(0, 0, 0).unwrap()
    }

    /// 构造一份只关心 os/is/degradation 与 drift/axes 的报告。
    pub(super) fn mk_report(
        primary: &str,
        folds_os_is_deg: &[(Option<f64>, Option<f64>, Option<f64>)],
        drift: Vec<ParamDrift>,
        full_best: Option<ComboScore>,
        axes: Vec<AxisOutcome>,
    ) -> OptimizeReport {
        let fold_results = folds_os_is_deg
            .iter()
            .enumerate()
            .map(|(i, (os, is, deg))| FoldResult {
                fold: i + 2,
                is_from: dt(), is_to: dt(), os_from: dt(), os_to: dt(),
                best_params: None,
                is_objective: *is,
                os_objective: *os,
                degradation: *deg,
            })
            .collect();
        OptimizeReport {
            mode: "sim".into(), objective_name: "sharpe".into(),
            folds: folds_os_is_deg.len() + 1, n_combos: 1,
            fold_results, os_mean_objective: None, full_sample_best: full_best,
            drift, is_top5: vec![], axes, primary: primary.into(),
        }
    }

    fn os_only(primary: &str, os: &[f64]) -> (String, OptimizeReport) {
        let folds: Vec<_> = os.iter().map(|v| (Some(*v), Some(1.0), None)).collect();
        (primary.into(), mk_report(primary, &folds, vec![], None, vec![]))
    }

    #[test]
    fn t1_breadth_pass_when_60pct_symbols_have_positive_os() {
        // 6/10 标的有 ≥1 正 OS 折 → 0.6 ≥ 0.6 Pass
        let reps: Vec<_> = (0..10)
            .map(|i| {
                let os = if i < 6 { vec![1.0, -1.0] } else { vec![-1.0, -1.0] };
                os_only(&format!("s{i}"), &os)
            })
            .collect();
        let g = gate_os_breadth(&reps, &GateThresholds::default());
        assert_eq!(g.status, GateStatus::Pass);
        assert!((g.value - 0.6).abs() < 1e-9);
    }

    #[test]
    fn t1_breadth_fail_when_below_threshold() {
        let reps: Vec<_> = (0..10)
            .map(|i| {
                let os = if i < 5 { vec![1.0] } else { vec![-1.0] };
                os_only(&format!("s{i}"), &os)
            })
            .collect();
        let g = gate_os_breadth(&reps, &GateThresholds::default());
        assert_eq!(g.status, GateStatus::Fail);
        assert!((g.value - 0.5).abs() < 1e-9);
    }
}
