//! gm 尾盘取数定时任务 DTO——配置 + 状态;派生 ts-rs 供 ui 生成 TS 类型。
//! 配置落盘 data/gm/tail.config.json(每实例自配);路径经 Workspace 解析,无写死。
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub(crate) const RANKS: [&str; 4] = ["liquidity", "intraday", "range_pos", "vwap_gap"];

/// 尾盘漏斗 + 排程配置。launcher 读它构造 Python tail --funnel 参数。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct GmTailConfig {
    /// 触发时刻 "HH:MM"(24h)。默认 14:46——等 14:45 bar 落定。
    pub schedule_time: String,
    /// 粗排键:liquidity|intraday|range_pos|vwap_gap。
    pub rank: String,
    /// 短名单取前 N。
    pub top: u32,
    /// 日线候选集文件(""=不用;相对仓库根或绝对)。
    pub pool: String,
    /// 门槛:今日成交额下限(元)。
    pub min_amount: f64,
    /// 门槛:最低价。
    pub min_price: f64,
    /// 门槛:剔除涨停封板(无卖盘)。
    pub drop_limit_up: bool,
}

impl Default for GmTailConfig {
    fn default() -> Self {
        GmTailConfig {
            schedule_time: "14:46".into(),
            rank: "liquidity".into(),
            top: 300,
            pool: String::new(),
            min_amount: 30_000_000.0,
            min_price: 2.0,
            drop_limit_up: false,
        }
    }
}

impl GmTailConfig {
    /// 校验+夹紧:非法 rank→liquidity；time 非 HH:MM→14:46；top∈[1,5115]；金额/价非负。
    pub fn sanitized(mut self) -> Self {
        if !RANKS.contains(&self.rank.as_str()) {
            self.rank = "liquidity".into();
        }
        if !valid_hhmm(&self.schedule_time) {
            self.schedule_time = "14:46".into();
        }
        self.top = self.top.clamp(1, 5115);
        if !(self.min_amount >= 0.0) {
            self.min_amount = 0.0;
        }
        if !(self.min_price >= 0.0) {
            self.min_price = 0.0;
        }
        self
    }
}

/// 严格 "HH:MM" 24h 校验（schtasks /ST 要这个格式）。
pub fn valid_hhmm(s: &str) -> bool {
    let b = s.as_bytes();
    s.len() == 5
        && b[2] == b':'
        && s[..2].parse::<u32>().map(|h| h < 24).unwrap_or(false)
        && s[3..].parse::<u32>().map(|m| m < 60).unwrap_or(false)
}

/// 驾驶舱状态:任务是否装、schtasks 查询、配置、token 是否就绪、产物计数、日志尾。
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct GmTailStatusDto {
    pub installed: bool,
    pub schtask: Option<crate::dto::SchtaskDto>,
    pub config: GmTailConfig,
    /// data/gm/.token 存在且非空。
    pub token_present: bool,
    /// data/gm/k15m/*.csv 数量。
    pub k15m_count: u32,
    /// 最新 snapshot_*.csv 文件名。
    pub last_snapshot: Option<String>,
    /// tail.log 末尾若干行。
    pub log_tail: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_roundtrips_through_json() {
        let c = GmTailConfig::default();
        let j = serde_json::to_string(&c).unwrap();
        let back: GmTailConfig = serde_json::from_str(&j).unwrap();
        assert_eq!(c, back);
        assert_eq!(c.schedule_time, "14:46");
        assert_eq!(c.top, 300);
    }

    #[test]
    fn sanitize_fixes_bad_fields() {
        let c = GmTailConfig {
            schedule_time: "99:99".into(),
            rank: "bogus".into(),
            top: 99_999,
            pool: String::new(),
            min_amount: -1.0,
            min_price: -2.0,
            drop_limit_up: false,
        }
        .sanitized();
        assert_eq!(c.rank, "liquidity");
        assert_eq!(c.schedule_time, "14:46");
        assert_eq!(c.top, 5115);
        assert_eq!(c.min_amount, 0.0);
        assert_eq!(c.min_price, 0.0);
    }

    #[test]
    fn sanitize_keeps_valid_fields() {
        let c = GmTailConfig {
            schedule_time: "14:30".into(),
            rank: "intraday".into(),
            top: 200,
            pool: "data/gm/daily_pool.txt".into(),
            min_amount: 5e7,
            min_price: 3.0,
            drop_limit_up: true,
        }
        .sanitized();
        assert_eq!(c.rank, "intraday");
        assert_eq!(c.schedule_time, "14:30");
        assert_eq!(c.top, 200);
        assert!(c.drop_limit_up);
    }

    #[test]
    fn hhmm_validation() {
        for ok in ["14:46", "00:00", "23:59", "09:05"] {
            assert!(valid_hhmm(ok), "{ok}");
        }
        for bad in ["24:00", "14:60", "9:00", "14-46", "14:6", "abcde", ""] {
            assert!(!valid_hhmm(bad), "{bad}");
        }
    }
}
