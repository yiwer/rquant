//! 数据质量分析（设计 2026-06-14-data-expansion-design.md §5）。纯函数，无 IO。
use crate::backtest::gaps::detect_gaps;
use crate::data::bar::Bar;
use crate::data::calendar::AShareCalendar;
use chrono::NaiveDateTime;

/// 一条序列的质量画像。
#[derive(Debug, Clone)]
pub struct QualityReport {
    pub n_bars: usize,
    pub first: NaiveDateTime,
    pub last: NaiveDateTime,
    /// 时间严格递增（无重复、无逆序）。
    pub strictly_increasing: bool,
    /// 最大 |相邻收盘收益|。
    pub max_abs_daily_return: f64,
    /// |收益| > 阈值的可疑跳空（时刻, 收益）。
    pub suspicious_jumps: Vec<(NaiveDateTime, f64)>,
    /// 对日历的意外缺交易日数（detect_gaps；无 --holidays 时含市场假日，信息性）。
    pub calendar_gaps: usize,
}

/// 分析一段（已按时间排序的）bar 序列。空序列返回零值画像。
pub fn analyze(bars: &[Bar], calendar: &AShareCalendar, jump_threshold: f64) -> QualityReport {
    if bars.is_empty() {
        let zero = NaiveDateTime::default();
        return QualityReport {
            n_bars: 0, first: zero, last: zero, strictly_increasing: true,
            max_abs_daily_return: 0.0, suspicious_jumps: Vec::new(), calendar_gaps: 0,
        };
    }
    let mut strictly_increasing = true;
    let mut max_abs = 0.0_f64;
    let mut jumps = Vec::new();
    for w in bars.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        if b.time <= a.time {
            strictly_increasing = false;
        }
        if a.close != 0.0 {
            let ret = b.close / a.close - 1.0;
            if ret.abs() > max_abs {
                max_abs = ret.abs();
            }
            if ret.abs() > jump_threshold {
                jumps.push((b.time, ret));
            }
        }
    }
    let gaps = detect_gaps(bars, calendar);
    QualityReport {
        n_bars: bars.len(),
        first: bars[0].time,
        last: bars[bars.len() - 1].time,
        strictly_increasing,
        max_abs_daily_return: max_abs,
        suspicious_jumps: jumps,
        calendar_gaps: gaps.missing_trading_days.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use std::collections::HashSet;

    fn day(y: i32, m: u32, d: u32) -> chrono::NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(15, 0, 0).unwrap()
    }
    fn bar(t: chrono::NaiveDateTime, close: f64) -> Bar {
        Bar { time: t, open: close, high: close, low: close, close, volume: 100.0 }
    }
    fn empty_cal() -> AShareCalendar { AShareCalendar::new(HashSet::new()) }

    #[test]
    fn clean_series_all_clear() {
        // 连续四个交易日（2024-01-02 二 ~ 01-05 五），收盘平滑
        let bars = vec![
            bar(day(2024,1,2), 10.0), bar(day(2024,1,3), 10.1),
            bar(day(2024,1,4), 10.2), bar(day(2024,1,5), 10.3),
        ];
        let q = analyze(&bars, &empty_cal(), 0.21);
        assert!(q.strictly_increasing);
        assert!(q.suspicious_jumps.is_empty());
        assert_eq!(q.calendar_gaps, 0);
        assert_eq!(q.n_bars, 4);
        assert_eq!(q.first, day(2024,1,2));
        assert_eq!(q.last, day(2024,1,5));
    }

    #[test]
    fn out_of_order_flagged_non_monotonic() {
        let bars = vec![bar(day(2024,1,3), 10.0), bar(day(2024,1,2), 10.1)];
        let q = analyze(&bars, &empty_cal(), 0.21);
        assert!(!q.strictly_increasing);
    }

    #[test]
    fn gross_jump_flagged() {
        // +30% 跳（超 ±21%）→ 可疑
        let bars = vec![bar(day(2024,1,2), 10.0), bar(day(2024,1,3), 13.0)];
        let q = analyze(&bars, &empty_cal(), 0.21);
        assert_eq!(q.suspicious_jumps.len(), 1);
        assert_eq!(q.suspicious_jumps[0].0, day(2024,1,3));
        assert!((q.max_abs_daily_return - 0.30).abs() < 1e-9);
    }

    #[test]
    fn legit_limit_move_not_flagged() {
        // +10%（主板涨停）< 0.21 → 不报
        let bars = vec![bar(day(2024,1,2), 10.0), bar(day(2024,1,3), 11.0)];
        let q = analyze(&bars, &empty_cal(), 0.21);
        assert!(q.suspicious_jumps.is_empty());
    }

    #[test]
    fn missing_trading_day_counted_as_gap() {
        // 缺 2024-01-03（周三，空日历视其为交易日）→ detect_gaps 计 1
        let bars = vec![bar(day(2024,1,2), 10.0), bar(day(2024,1,4), 10.1)];
        let q = analyze(&bars, &empty_cal(), 0.21);
        assert_eq!(q.calendar_gaps, 1);
    }
}
