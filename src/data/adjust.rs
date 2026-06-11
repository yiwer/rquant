use crate::data::bar::Bar;
use crate::{Error, Result};
use chrono::NaiveDate;
use std::collections::BTreeMap;

/// 按日期交集对齐：factor(d) = qfq_close(d) / raw_close(d)。
/// 空交集 / raw close ≤ 0 / 因子非有限或 ≤ 0 → Error（拒绝静默错数据）。
pub fn adjust_factors(raw_daily: &[Bar], qfq_daily: &[Bar]) -> Result<BTreeMap<NaiveDate, f64>> {
    let raw: BTreeMap<NaiveDate, f64> = raw_daily.iter().map(|b| (b.time.date(), b.close)).collect();
    let mut out = BTreeMap::new();
    for b in qfq_daily {
        let d = b.time.date();
        if let Some(rc) = raw.get(&d) {
            if *rc <= 0.0 {
                return Err(Error::Data(format!("adjust: raw close <= 0 on {d}")));
            }
            let f = b.close / rc;
            if !f.is_finite() || f <= 0.0 {
                return Err(Error::Data(format!("adjust: bad factor {f} on {d}")));
            }
            out.insert(d, f);
        }
    }
    if out.is_empty() {
        return Err(Error::Data("adjust: no overlapping dates between raw and qfq daily".into()));
    }
    Ok(out)
}

/// 逐 bar 乘当日因子（OHLC；volume 不动）。缺因子日回退最近前值
/// （复权因子是阶梯函数，前值语义正确）；早于因子表起点 → Error。
pub fn apply_factors(bars: &[Bar], factors: &BTreeMap<NaiveDate, f64>) -> Result<Vec<Bar>> {
    let mut out = Vec::with_capacity(bars.len());
    for b in bars {
        let d = b.time.date();
        let f = factors
            .range(..=d)
            .next_back()
            .map(|(_, f)| *f)
            .ok_or_else(|| Error::Data(format!("adjust: bar date {d} earlier than factor table start")))?;
        out.push(Bar {
            time: b.time,
            open: b.open * f,
            high: b.high * f,
            low: b.low * f,
            close: b.close * f,
            volume: b.volume,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(d: (i32, u32, u32), h: u32, mi: u32, px: f64) -> Bar {
        let time = NaiveDate::from_ymd_opt(d.0, d.1, d.2).unwrap().and_hms_opt(h, mi, 0).unwrap();
        Bar { time, open: px, high: px, low: px, close: px, volume: 100.0 }
    }

    #[test]
    fn golden_factor_step_and_propagation() {
        // 除息发生在 d2 开盘前：raw d1=10/d2=10；qfq d1=9.5/d2=10 → 因子 {d1:0.95, d2:1.0}
        let raw = vec![bar((2025, 7, 4), 15, 0, 10.0), bar((2025, 7, 7), 15, 0, 10.0)];
        let qfq = vec![bar((2025, 7, 4), 15, 0, 9.5), bar((2025, 7, 7), 15, 0, 10.0)];
        let f = adjust_factors(&raw, &qfq).unwrap();
        assert_eq!(f.len(), 2);
        assert!((f[&NaiveDate::from_ymd_opt(2025, 7, 4).unwrap()] - 0.95).abs() < 1e-12);
        assert!((f[&NaiveDate::from_ymd_opt(2025, 7, 7).unwrap()] - 1.0).abs() < 1e-12);
        // 分钟传播：d1 close 10.2 → ×0.95；d2 不变；周末日(7-5/7-6)回退 d1 因子
        let mins = vec![
            bar((2025, 7, 4), 14, 30, 10.2),
            bar((2025, 7, 5), 10, 0, 10.1), // 假想缺因子日 → 前值 0.95
            bar((2025, 7, 7), 10, 0, 9.5),
        ];
        let adj = apply_factors(&mins, &f).unwrap();
        assert!((adj[0].close - 10.2 * 0.95).abs() < 1e-12);
        assert!((adj[1].close - 10.1 * 0.95).abs() < 1e-12);
        assert!((adj[2].close - 9.5).abs() < 1e-12);
        assert_eq!(adj[0].volume, 100.0); // volume 不动
        // 隔夜跳空消除：raw 跳空 (9.5/10.0−1)=−5% → 调整后 d1 末 close 10.0×0.95=9.5 vs d2 9.5 → 0%
        let d1_last = 10.0 * 0.95;
        assert!((adj[2].open / d1_last - 1.0).abs() < 1e-12);
    }

    #[test]
    fn errors_on_no_overlap_and_pre_start_bar() {
        let raw = vec![bar((2025, 7, 4), 15, 0, 10.0)];
        let qfq = vec![bar((2025, 8, 4), 15, 0, 9.5)];
        assert!(adjust_factors(&raw, &qfq).is_err()); // 无交集
        let qfq_same = vec![bar((2025, 7, 4), 15, 0, 9.5)];
        let f = adjust_factors(&raw, &qfq_same).unwrap();
        assert!(apply_factors(&[bar((2025, 7, 1), 10, 0, 10.0)], &f).is_err()); // 早于起点
    }

    #[test]
    fn errors_on_nonpositive_factor() {
        let raw = vec![bar((2025, 7, 4), 15, 0, 0.0)];
        let qfq = vec![bar((2025, 7, 4), 15, 0, 9.5)];
        assert!(adjust_factors(&raw, &qfq).is_err());
    }
}
