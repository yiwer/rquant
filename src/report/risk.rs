use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

/// 风险指标（公式约定见 spec 2026-06-11-rquant-f4-risk-metrics-design.md §3）。
/// Option=None 表示样本不足/除零——拒绝假数字。VaR 族恒可算。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RiskMetrics {
    pub ann_return: Option<f64>,
    pub ann_vol: Option<f64>,
    pub sharpe: Option<f64>,
    pub sortino: Option<f64>,
    pub calmar: Option<f64>,
    pub var95: f64,
    pub cvar95: f64,
}

const EPS: f64 = 1e-12;
const MIN_SPAN_DAYS: f64 = 30.0;

/// 净值点列 → 风险指标。len<2 或任一 nav≤0 → None。
/// 年化基准由时间戳推断：bpy = n_rets / 首末跨度年数；跨度 < 30 天 → 年化族 None。
pub fn risk_metrics(nav: &[(NaiveDateTime, f64)], max_drawdown: f64) -> Option<RiskMetrics> {
    if nav.len() < 2 || nav.iter().any(|(_, v)| *v <= 0.0) {
        return None;
    }
    let rets: Vec<f64> = nav.windows(2).map(|w| w[1].1 / w[0].1 - 1.0).collect();
    let n = rets.len() as f64;
    // VaR95/CVaR95：升序，idx = max(⌈0.05n⌉−1, 0)
    let mut sorted = rets.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = ((0.05 * n).ceil() as usize).max(1) - 1;
    let var95 = sorted[idx];
    let cvar95 = sorted[..=idx].iter().sum::<f64>() / (idx + 1) as f64;
    // 年化族
    let span_secs = (nav[nav.len() - 1].0 - nav[0].0).num_seconds() as f64;
    let span_days = span_secs / 86_400.0;
    let span_years = span_secs / (365.25 * 86_400.0);
    let (ann_return, ann_vol, sharpe, sortino, calmar) =
        if span_days < MIN_SPAN_DAYS || span_years <= 0.0 {
            (None, None, None, None, None)
        } else {
            let bpy = n / span_years;
            let ar = (nav[nav.len() - 1].1 / nav[0].1).powf(1.0 / span_years) - 1.0;
            let mean = rets.iter().sum::<f64>() / n;
            let av = if rets.len() >= 2 {
                let var = rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (n - 1.0);
                Some(var.sqrt() * bpy.sqrt())
            } else {
                None
            };
            let sharpe = av.filter(|v| *v > EPS).map(|v| ar / v);
            let downside =
                (rets.iter().map(|r| r.min(0.0).powi(2)).sum::<f64>() / n).sqrt() * bpy.sqrt();
            let sortino = (downside > EPS).then(|| ar / downside);
            let calmar = (max_drawdown > EPS).then(|| ar / max_drawdown);
            (Some(ar), av, sharpe, sortino, calmar)
        };
    Some(RiskMetrics { ann_return, ann_vol, sharpe, sortino, calmar, var95, cvar95 })
}

/// t 统计量 = mean/(sample_std/√n)；n<2 或 std≈0 → None。
pub fn t_stat(values: &[f64]) -> Option<f64> {
    if values.len() < 2 {
        return None;
    }
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    let var = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1.0);
    let std = var.sqrt();
    (std > EPS).then(|| mean / (std / n.sqrt()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use chrono::{Duration, NaiveDate};

    fn t0() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2025, 1, 1).unwrap().and_hms_opt(12, 0, 0).unwrap()
    }

    /// 构造 nav 序列：首点 t0，末点 t0+span_secs，中间点 +1s 递增（span 只看首末）。
    fn nav_with_rets(rets: &[f64], span_secs: i64) -> Vec<(NaiveDateTime, f64)> {
        let mut nav = vec![(t0(), 1.0)];
        let mut v = 1.0;
        for (i, r) in rets.iter().enumerate() {
            v *= 1.0 + r;
            let t = if i == rets.len() - 1 {
                t0() + Duration::seconds(span_secs)
            } else {
                t0() + Duration::seconds(i as i64 + 1)
            };
            nav.push((t, v));
        }
        nav
    }

    const YEAR_SECS: i64 = (365.25 * 86_400.0) as i64; // 31_557_600

    #[test]
    fn golden_constant_return_one_year() {
        // 恒定 r=0.1% × 252 步，跨度恰一年 → CAGR=1.001^252−1；vol≈0 → sharpe None
        let m = risk_metrics(&nav_with_rets(&[0.001; 252], YEAR_SECS), 0.05).unwrap();
        assert_relative_eq!(m.ann_return.unwrap(), 1.001f64.powi(252) - 1.0, epsilon = 1e-9);
        assert!(m.ann_vol.unwrap() < 1e-12);
        assert!(m.sharpe.is_none()); // vol≈0 拒绝假 Sharpe
        assert!(m.sortino.is_none()); // 无负收益
        assert_relative_eq!(m.calmar.unwrap(), m.ann_return.unwrap() / 0.05, epsilon = 1e-9);
        assert_relative_eq!(m.var95, 0.001, epsilon = 1e-12); // 恒定收益分位=自身
    }

    #[test]
    fn golden_alternating_vol_sortino_annualization() {
        // +1%/−0.5% × 10 对（n=20），跨度恰 1/4 年 → bpy=80（钉年化接线）
        let rets: Vec<f64> = (0..20).map(|i| if i % 2 == 0 { 0.01 } else { -0.005 }).collect();
        let m = risk_metrics(&nav_with_rets(&rets, YEAR_SECS / 4), 0.05).unwrap();
        let span_years = 0.25;
        let bpy = 20.0 / span_years;
        let ar = (1.01f64 * 0.995).powi(10).powf(1.0 / span_years) - 1.0;
        assert_relative_eq!(m.ann_return.unwrap(), ar, epsilon = 1e-9);
        // sample std: mean=0.0025, 偏差 ±0.0075 → std=0.0075·√(20/19)
        let std = 0.0075 * (20.0f64 / 19.0).sqrt();
        assert_relative_eq!(m.ann_vol.unwrap(), std * bpy.sqrt(), epsilon = 1e-9);
        assert_relative_eq!(m.sharpe.unwrap(), ar / (std * bpy.sqrt()), epsilon = 1e-9);
        // downside = √(10·0.005²/20) = 0.005·√0.5（全量 n 分母约定，spec §3）
        let downside = 0.005 * 0.5f64.sqrt();
        assert_relative_eq!(m.sortino.unwrap(), ar / (downside * bpy.sqrt()), epsilon = 1e-9);
        // VaR：n=20 → idx=⌈1⌉−1=0 → 最差收益 −0.005
        assert_relative_eq!(m.var95, -0.005, epsilon = 1e-12);
        assert_relative_eq!(m.cvar95, -0.005, epsilon = 1e-12);
    }

    #[test]
    fn short_span_gives_none_annualized_but_var_present() {
        let m = risk_metrics(&nav_with_rets(&[0.01, -0.02, 0.005], 20 * 86_400), 0.05).unwrap();
        assert!(m.ann_return.is_none() && m.ann_vol.is_none() && m.sharpe.is_none());
        assert!(m.sortino.is_none() && m.calmar.is_none());
        assert_relative_eq!(m.var95, -0.02, epsilon = 1e-12);
    }

    #[test]
    fn degenerate_inputs_give_overall_none() {
        assert!(risk_metrics(&[(t0(), 1.0)], 0.05).is_none()); // 单点
        assert!(risk_metrics(&[(t0(), 1.0), (t0() + Duration::seconds(1), 0.0)], 0.05).is_none()); // nav≤0
    }

    #[test]
    fn t_stat_closed_form() {
        // [1,2,3]: mean=2, std=1 → t = 2/(1/√3) = 2√3
        assert_relative_eq!(t_stat(&[1.0, 2.0, 3.0]).unwrap(), 2.0 * 3.0f64.sqrt(), epsilon = 1e-12);
        assert!(t_stat(&[1.0]).is_none());
        assert!(t_stat(&[2.0, 2.0, 2.0]).is_none()); // std≈0
    }
}
