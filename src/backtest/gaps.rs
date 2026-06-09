use crate::data::bar::Bar;
use crate::data::calendar::AShareCalendar;
use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartialDay {
    pub date: NaiveDate,
    pub bars: usize,
    pub expected: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GapReport {
    pub missing_trading_days: Vec<NaiveDate>,
    pub partial_days: Vec<PartialDay>,
}

impl GapReport {
    pub fn is_empty(&self) -> bool {
        self.missing_trading_days.is_empty() && self.partial_days.is_empty()
    }
}

/// 检测 primary 序列缺口：缺失交易日（日历交易日无 bar）+ 残日
/// （bar 数 < 数据自校准的 full_day，排除首/末日）。纯函数，不报错。
pub fn detect_gaps(bars: &[Bar], calendar: &AShareCalendar) -> GapReport {
    let mut report = GapReport::default();
    if bars.is_empty() {
        return report;
    }
    let mut counts: BTreeMap<NaiveDate, usize> = BTreeMap::new();
    for b in bars {
        *counts.entry(b.time.date()).or_insert(0) += 1;
    }
    let full_day = counts.values().copied().max().unwrap_or(0);
    let first = *counts.keys().next().unwrap();
    let last = *counts.keys().next_back().unwrap();

    let mut d = first;
    while d <= last {
        if calendar.is_trading_day(d) && !counts.contains_key(&d) {
            report.missing_trading_days.push(d);
        }
        d += Duration::days(1);
    }

    for (&date, &c) in &counts {
        if date != first && date != last && c < full_day {
            report.partial_days.push(PartialDay { date, bars: c, expected: full_day });
        }
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use std::collections::HashSet;

    fn day_bars(y: i32, m: u32, d: u32, n: u32) -> Vec<Bar> {
        let base = NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(9, 45, 0).unwrap();
        (0..n)
            .map(|i| Bar {
                time: base + chrono::Duration::minutes(i as i64 * 15),
                open: 1.0, high: 1.0, low: 1.0, close: 1.0, volume: 1.0,
            })
            .collect()
    }
    fn cal(holidays: &[(i32, u32, u32)]) -> crate::data::calendar::AShareCalendar {
        let h: HashSet<NaiveDate> = holidays
            .iter()
            .map(|&(y, m, d)| NaiveDate::from_ymd_opt(y, m, d).unwrap())
            .collect();
        crate::data::calendar::AShareCalendar::new(h)
    }

    #[test]
    fn no_gaps_when_complete() {
        let mut bars = day_bars(2024, 1, 2, 4);
        bars.extend(day_bars(2024, 1, 3, 4));
        bars.extend(day_bars(2024, 1, 4, 4));
        assert!(detect_gaps(&bars, &cal(&[])).is_empty());
    }

    #[test]
    fn flags_missing_trading_day() {
        let mut bars = day_bars(2024, 1, 2, 4);
        bars.extend(day_bars(2024, 1, 4, 4));
        let r = detect_gaps(&bars, &cal(&[]));
        assert_eq!(r.missing_trading_days, vec![NaiveDate::from_ymd_opt(2024, 1, 3).unwrap()]);
    }

    #[test]
    fn holiday_not_flagged_missing() {
        let mut bars = day_bars(2024, 1, 2, 4);
        bars.extend(day_bars(2024, 1, 4, 4));
        let r = detect_gaps(&bars, &cal(&[(2024, 1, 3)]));
        assert!(r.missing_trading_days.is_empty());
    }

    #[test]
    fn flags_partial_interior_day() {
        let mut bars = day_bars(2024, 1, 2, 4);
        bars.extend(day_bars(2024, 1, 3, 2));
        bars.extend(day_bars(2024, 1, 4, 4));
        let r = detect_gaps(&bars, &cal(&[]));
        assert_eq!(r.partial_days.len(), 1);
        assert_eq!(r.partial_days[0].date, NaiveDate::from_ymd_opt(2024, 1, 3).unwrap());
        assert_eq!(r.partial_days[0].bars, 2);
        assert_eq!(r.partial_days[0].expected, 4);
    }

    #[test]
    fn boundary_partial_days_not_flagged() {
        let mut bars = day_bars(2024, 1, 2, 2);
        bars.extend(day_bars(2024, 1, 3, 4));
        bars.extend(day_bars(2024, 1, 4, 2));
        assert!(detect_gaps(&bars, &cal(&[])).partial_days.is_empty());
    }
}
