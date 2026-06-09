use chrono::{Datelike, NaiveDate, NaiveDateTime, NaiveTime, Weekday};
use std::collections::HashSet;

/// A股交易日历：工作日且非节假日为交易日；时段 09:30–11:30、13:00–15:00。
/// bar 收盘时刻落在 (start, end] 内视为在交易时段（首根 15m bar 收于 09:45，末根收于 15:00）。
pub struct AShareCalendar {
    holidays: HashSet<NaiveDate>,
}

impl AShareCalendar {
    pub fn new(holidays: HashSet<NaiveDate>) -> Self {
        Self { holidays }
    }

    pub fn is_trading_day(&self, d: NaiveDate) -> bool {
        !matches!(d.weekday(), Weekday::Sat | Weekday::Sun) && !self.holidays.contains(&d)
    }

    pub fn in_session(&self, dt: NaiveDateTime) -> bool {
        if !self.is_trading_day(dt.date()) {
            return false;
        }
        let t = dt.time();
        let am_start = NaiveTime::from_hms_opt(9, 30, 0).unwrap();
        let am_end = NaiveTime::from_hms_opt(11, 30, 0).unwrap();
        let pm_start = NaiveTime::from_hms_opt(13, 0, 0).unwrap();
        let pm_end = NaiveTime::from_hms_opt(15, 0, 0).unwrap();
        (t > am_start && t <= am_end) || (t > pm_start && t <= pm_end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use std::collections::HashSet;

    fn cal() -> AShareCalendar {
        let mut h = HashSet::new();
        h.insert(NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()); // 元旦
        AShareCalendar::new(h)
    }

    #[test]
    fn weekend_and_holiday_are_not_trading_days() {
        let c = cal();
        assert!(!c.is_trading_day(NaiveDate::from_ymd_opt(2024, 1, 6).unwrap())); // 周六
        assert!(!c.is_trading_day(NaiveDate::from_ymd_opt(2024, 1, 1).unwrap())); // 节假日
        assert!(c.is_trading_day(NaiveDate::from_ymd_opt(2024, 1, 2).unwrap()));  // 周二
    }

    #[test]
    fn session_boundaries() {
        let c = cal();
        let d = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
        assert!(c.in_session(d.and_hms_opt(9, 45, 0).unwrap()));
        assert!(c.in_session(d.and_hms_opt(11, 30, 0).unwrap()));
        assert!(!c.in_session(d.and_hms_opt(12, 0, 0).unwrap()));
        assert!(c.in_session(d.and_hms_opt(13, 15, 0).unwrap()));
        assert!(c.in_session(d.and_hms_opt(15, 0, 0).unwrap()));
        assert!(!c.in_session(d.and_hms_opt(15, 15, 0).unwrap()));
    }
}
