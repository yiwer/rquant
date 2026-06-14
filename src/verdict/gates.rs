//! 五门槛纯函数（设计 §7.2）。
#![allow(unused)] // Task 3-7 逐个实现后移除
use super::{GateOutcome, GateStatus, GateThresholds};
use crate::optimize::OptimizeReport;

/// 该标的是否有 ≥1 个 OS 正折。
fn symbol_has_positive_os(r: &OptimizeReport) -> bool {
    r.fold_results.iter().any(|f| f.os_objective.is_some_and(|v| v > 0.0))
}

fn median_sorted(xs: &mut [f64]) -> f64 {
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = xs.len();
    if n % 2 == 1 { xs[n / 2] } else { (xs[n / 2 - 1] + xs[n / 2]) / 2.0 }
}

/// 标的非空 per-fold degradation 的中位数（无有效折 → None）。
fn symbol_degradation_median(r: &OptimizeReport) -> Option<f64> {
    let mut ds: Vec<f64> = r.fold_results.iter().filter_map(|f| f.degradation).collect();
    if ds.is_empty() { None } else { Some(median_sorted(&mut ds)) }
}

/// ② 退化比：中位退化 >min 的"健康"标的占可判定标的的比例 ≥ 阈值。
pub fn gate_degradation(reports: &[(String, OptimizeReport)], th: &GateThresholds) -> GateOutcome {
    let n = reports.len();
    let mut determinate = 0usize;
    let mut healthy = 0usize;
    for (_, r) in reports {
        if let Some(med) = symbol_degradation_median(r) {
            determinate += 1;
            if med > th.min_degradation { healthy += 1; }
        }
    }
    // 多数标的无有效退化折 → 无法判定（保守）
    if determinate < n.div_ceil(2) || determinate == 0 {
        return GateOutcome {
            gate: "T2_degradation".into(),
            status: GateStatus::Indeterminate,
            value: 0.0,
            threshold: th.degradation_symbol_frac,
            note: format!("only {determinate}/{n} symbols have valid degradation folds"),
        };
    }
    let value = healthy as f64 / determinate as f64;
    let status = if value >= th.degradation_symbol_frac { GateStatus::Pass } else { GateStatus::Fail };
    GateOutcome {
        gate: "T2_degradation".into(),
        status,
        value,
        threshold: th.degradation_symbol_frac,
        note: format!("{healthy}/{determinate} determinate symbols have median degradation > {}", th.min_degradation),
    }
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

    #[test]
    fn t2_degradation_pass_when_majority_healthy() {
        // 7/10 标的中位退化比 >0.5 → 0.7 ≥ 0.6 Pass
        let reps: Vec<_> = (0..10)
            .map(|i| {
                let deg = if i < 7 { 0.8 } else { 0.2 };
                let folds = vec![(Some(1.0), Some(1.0), Some(deg)), (Some(1.0), Some(1.0), Some(deg))];
                (format!("s{i}"), mk_report(&format!("s{i}"), &folds, vec![], None, vec![]))
            })
            .collect();
        let g = gate_degradation(&reps, &GateThresholds::default());
        assert_eq!(g.status, GateStatus::Pass);
    }

    #[test]
    fn t2_degradation_indeterminate_when_too_few_valid() {
        // 多数标的全 None degradation → Indeterminate
        let reps: Vec<_> = (0..10)
            .map(|i| {
                let folds = vec![(Some(1.0), Some(1.0), None), (Some(1.0), Some(1.0), None)];
                (format!("s{i}"), mk_report(&format!("s{i}"), &folds, vec![], None, vec![]))
            })
            .collect();
        let g = gate_degradation(&reps, &GateThresholds::default());
        assert_eq!(g.status, GateStatus::Indeterminate);
    }
}
