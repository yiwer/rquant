//! 手动 run 的纪律闸(spec §5.1)。纯函数,时钟由调用方注入——可测。
use crate::dto::GateDto;
use chrono::{Datelike, NaiveDateTime, NaiveTime, Weekday};

pub fn classify_run_window(now: NaiveDateTime) -> GateDto {
    let wd = now.weekday();
    // 注:A 股法定节假日未检查;节假日 allow → fetch 返回陈旧 bar,replay 幂等无副作用。
    let weekday = !matches!(wd, Weekday::Sat | Weekday::Sun);
    let t = now.time();
    let hm = |h, m| NaiveTime::from_hms_opt(h, m, 0).expect("valid literal time");
    // [09:30,15:00) 右开:15:00 整点 allow——收盘集合竞价 bar 在 15:00 后数秒内定型,
    // 且 UI 默认 DRY,误触面极窄;常规通路是 15:35 schtask(落在 warn 窗)。
    if weekday && t >= hm(9, 30) && t < hm(15, 0) {
        return GateDto {
            gate: "dry_only".into(),
            message: Some("盘中:sina 末根为 forming bar,commit 会以未定型价格记账——仅允许 DRY".into()),
        };
    }
    if weekday && t >= hm(15, 30) && t < hm(15, 40) {
        return GateDto {
            gate: "warn".into(),
            message: Some("与 15:35 schtask 窗口重叠:并发 commit 有竞态风险(幂等可兜底),确认后继续".into()),
        };
    }
    GateDto { gate: "allow".into(), message: None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()
    }

    #[test]
    fn gate_table() {
        let cases = [
            ("2026-06-12 09:29", "allow"),
            ("2026-06-12 09:30", "dry_only"),
            ("2026-06-12 11:00", "dry_only"),
            ("2026-06-12 14:59", "dry_only"),
            ("2026-06-12 15:00", "allow"),
            ("2026-06-12 15:29", "allow"),
            ("2026-06-12 15:30", "warn"),
            ("2026-06-12 15:39", "warn"),
            ("2026-06-12 15:40", "allow"),
            ("2026-06-13 11:00", "allow"), // 周六盘中时刻也放行(无成形 bar 风险)
        ];
        for (when, want) in cases {
            let g = classify_run_window(t(when));
            assert_eq!(g.gate, want, "at {}", when);
        }
    }

    #[test]
    fn messages_explain_why() {
        let g = classify_run_window(t("2026-06-12 11:00"));
        assert!(g.message.unwrap().contains("forming"));
        let g = classify_run_window(t("2026-06-12 15:35"));
        assert!(g.message.unwrap().contains("schtask"));
    }
}
