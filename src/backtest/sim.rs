use chrono::{NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};

/// 平仓回合记录。reason: tree/stop/tp/max_hold/end。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundTrip {
    pub entry_t: NaiveDateTime,
    pub exit_t: NaiveDateTime,
    pub entry_px: f64,
    pub exit_px: f64,
    pub max_abs_pos: f64,
    pub trip_return: f64,
    pub bars_held: usize,
    pub reason: String,
}

#[derive(Debug)]
struct OpenTrip {
    entry_t: NaiveDateTime,
    entry_px: f64,
    open_nav: f64,
    max_abs_pos: f64,
}

/// 模拟账户（spec §3 为记账权威）。
#[derive(Debug)]
pub struct SimAccount {
    pub pos: f64,
    pub entry_price: f64,
    pub bars_held: usize,
    pub nav: f64,
    pub peak_nav: f64,
    pub max_drawdown: f64,
    pub turnover: f64,
    pub last_increase_date: Option<NaiveDate>,
    trip: Option<OpenTrip>,
}

impl Default for SimAccount {
    fn default() -> Self {
        Self {
            pos: 0.0,
            entry_price: f64::NAN,
            bars_held: 0,
            nav: 1.0,
            peak_nav: 1.0,
            max_drawdown: 0.0,
            turnover: 0.0,
            last_increase_date: None,
            trip: None,
        }
    }
}

const EPS: f64 = 1e-12;

impl SimAccount {
    fn close_trip(
        &mut self,
        exit_t: NaiveDateTime,
        exit_px: f64,
        reason: &str,
    ) -> Option<RoundTrip> {
        let trip = self.trip.take()?;
        Some(RoundTrip {
            entry_t: trip.entry_t,
            exit_t,
            entry_px: trip.entry_px,
            exit_px,
            max_abs_pos: trip.max_abs_pos,
            trip_return: self.nav / trip.open_nav - 1.0,
            bars_held: self.bars_held,
            reason: reason.to_string(),
        })
    }
}

/// 一步执行+记账：决策于上根 bar 收盘的 target，在本 bar（prev_close→open→close）执行。
/// 返回本步平掉的回合（翻向时为旧回合）。T+1：同自然日加过仓 → 减仓/翻向顺延（本步不交易）。
pub fn sim_step(
    acc: &mut SimAccount,
    prev_close: f64,
    open: f64,
    close: f64,
    exec_t: NaiveDateTime,
    target: f64,
    rate: f64,
    reason: &str,
) -> Option<RoundTrip> {
    let mut target = target.clamp(-1.0, 1.0);
    let reduces = acc.pos.abs() > EPS
        && (target.abs() < acc.pos.abs() - EPS || target * acc.pos < -EPS);
    if reduces && acc.last_increase_date == Some(exec_t.date()) {
        target = acc.pos; // T+1 顺延
    }
    // 段1：旧仓 prev_close→open
    acc.nav *= 1.0 + acc.pos * (open / prev_close - 1.0);
    let delta = target - acc.pos;
    let mut closed = None;
    if delta.abs() > EPS {
        acc.nav *= 1.0 - rate * delta.abs();
        acc.turnover += delta.abs();
        let old = acc.pos;
        let flat_or_flip = old.abs() > EPS && (target.abs() <= EPS || target * old < -EPS);
        if flat_or_flip {
            closed = acc.close_trip(exec_t, open, reason);
            acc.entry_price = f64::NAN;
            acc.bars_held = 0;
        }
        if target.abs() > EPS {
            if old.abs() <= EPS || target * old < -EPS {
                // 自 flat 开仓 / 翻向开新
                acc.trip = Some(OpenTrip {
                    entry_t: exec_t,
                    entry_px: open,
                    open_nav: acc.nav,
                    max_abs_pos: target.abs(),
                });
                acc.entry_price = open;
                acc.bars_held = 0;
                acc.last_increase_date = Some(exec_t.date());
            } else if target.abs() > old.abs() + EPS {
                // 加仓：加权均价
                acc.entry_price = (acc.entry_price * old.abs()
                    + open * (target.abs() - old.abs()))
                    / target.abs();
                acc.last_increase_date = Some(exec_t.date());
            }
            // 部分减仓：entry 不变
        }
        acc.pos = target;
    }
    // 段2：新仓 open→close
    acc.nav *= 1.0 + acc.pos * (close / open - 1.0);
    if acc.pos.abs() > EPS {
        acc.bars_held += 1; // 开仓执行 bar 收盘即为 1（spec §3.5）
        if let Some(trip) = acc.trip.as_mut() {
            trip.max_abs_pos = trip.max_abs_pos.max(acc.pos.abs());
        }
    }
    acc.peak_nav = acc.peak_nav.max(acc.nav);
    acc.max_drawdown = acc.max_drawdown.max(1.0 - acc.nav / acc.peak_nav);
    closed
}

/// 期末清算：仍持仓 → 按末收盘计成本平仓（reason="end"）。
pub fn finalize(
    acc: &mut SimAccount,
    last_t: NaiveDateTime,
    last_close: f64,
    rate: f64,
) -> Option<RoundTrip> {
    if acc.pos.abs() <= EPS {
        return None;
    }
    acc.nav *= 1.0 - rate * acc.pos.abs();
    acc.turnover += acc.pos.abs();
    let closed = acc.close_trip(last_t, last_close, "end");
    acc.pos = 0.0;
    acc.entry_price = f64::NAN;
    acc.bars_held = 0;
    acc.peak_nav = acc.peak_nav.max(acc.nav);
    acc.max_drawdown = acc.max_drawdown.max(1.0 - acc.nav / acc.peak_nav);
    closed
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
    }

    #[test]
    fn golden_walk_enter_hold_exit() {
        // 注意：执行时间须跨自然日（入场日 T+1 禁止当日平仓——纯记账路径用三天展开）
        // bars: b0 c=10 | b1 o=10 c=10.2 | b2 o=10.4 c=10.6 | b3 o=10.8 c=10.6
        // rate=0.001。i0: target 1 → exec b1；i1: hold → b2 无交易；i2: target 0 → exec b3 平仓。
        let mut acc = SimAccount::default();
        let rt1 = sim_step(
            &mut acc,
            10.0,
            10.0,
            10.2,
            t("2024-01-02 10:00:00"),
            1.0,
            0.001,
            "tree",
        );
        assert!(rt1.is_none());
        assert_relative_eq!(acc.nav, 0.999 * (10.2 / 10.0), epsilon = 1e-12);
        assert_relative_eq!(acc.entry_price, 10.0);
        assert_eq!(acc.bars_held, 1);
        let rt2 = sim_step(
            &mut acc,
            10.2,
            10.4,
            10.6,
            t("2024-01-03 10:00:00"),
            1.0,
            0.001,
            "tree",
        );
        assert!(rt2.is_none());
        assert_relative_eq!(acc.nav, 0.999 * (10.6 / 10.0), epsilon = 1e-12); // 连续持仓 = 链式收益
        assert_eq!(acc.bars_held, 2);
        let rt3 = sim_step(
            &mut acc,
            10.6,
            10.8,
            10.6,
            t("2024-01-04 10:00:00"),
            0.0,
            0.001,
            "tree",
        )
        .unwrap();
        // 平仓后 nav = 0.999*(10.8/10.0)*0.999；段2 pos=0 不变
        assert_relative_eq!(
            acc.nav,
            0.999 * (10.8 / 10.0) * 0.999,
            epsilon = 1e-12
        );
        assert_eq!(acc.pos, 0.0);
        assert!(acc.entry_price.is_nan());
        assert_eq!(rt3.exit_px, 10.8);
        assert_eq!(rt3.bars_held, 2);
        assert_eq!(rt3.reason, "tree");
        // trip_return 以回合 open_nav（入场成本后、入场 bar 段2 前）为基：
        // open_nav = 0.999；close 时 nav = 0.999×(10.8/10)×0.999 → trip_return = (10.8/10)×0.999 − 1
        assert_relative_eq!(
            rt3.trip_return,
            (10.8 / 10.0) * 0.999 - 1.0,
            epsilon = 1e-12
        );
        assert_relative_eq!(acc.turnover, 2.0);
    }

    #[test]
    fn t1_defers_same_day_reduction() {
        let mut acc = SimAccount::default();
        // 同一自然日：开仓后立刻请求平仓 → 顺延；次日可平
        sim_step(
            &mut acc,
            10.0,
            10.0,
            10.0,
            t("2024-01-02 10:00:00"),
            1.0,
            0.0,
            "tree",
        );
        let r = sim_step(
            &mut acc,
            10.0,
            10.0,
            10.0,
            t("2024-01-02 10:15:00"),
            0.0,
            0.0,
            "tree",
        );
        assert!(r.is_none());
        assert_eq!(acc.pos, 1.0); // 被顺延
        let r2 = sim_step(
            &mut acc,
            10.0,
            10.0,
            10.0,
            t("2024-01-03 09:45:00"),
            0.0,
            0.0,
            "tree",
        );
        assert!(r2.is_some());
        assert_eq!(acc.pos, 0.0);
    }

    #[test]
    fn flip_closes_old_and_opens_new() {
        let mut acc = SimAccount::default();
        sim_step(
            &mut acc,
            10.0,
            10.0,
            10.0,
            t("2024-01-02 10:00:00"),
            1.0,
            0.0,
            "tree",
        );
        let closed = sim_step(
            &mut acc,
            10.0,
            10.0,
            10.0,
            t("2024-01-03 10:00:00"),
            -0.5,
            0.001,
            "tree",
        )
        .unwrap();
        assert_eq!(closed.exit_px, 10.0);
        assert_eq!(acc.pos, -0.5);
        assert_relative_eq!(acc.entry_price, 10.0);
        assert_eq!(acc.bars_held, 1); // 新回合从 1 起
        assert_relative_eq!(acc.turnover, 1.0 + 1.5); // |Δ|=1.5 一次计
    }

    #[test]
    fn add_position_weighted_entry_and_partial_reduce_keeps_entry() {
        let mut acc = SimAccount::default();
        sim_step(
            &mut acc,
            10.0,
            10.0,
            10.0,
            t("2024-01-02 10:00:00"),
            0.5,
            0.0,
            "tree",
        );
        sim_step(
            &mut acc,
            10.0,
            12.0,
            12.0,
            t("2024-01-03 10:00:00"),
            1.0,
            0.0,
            "tree",
        );
        assert_relative_eq!(acc.entry_price, (10.0 * 0.5 + 12.0 * 0.5) / 1.0); // 11.0
        sim_step(
            &mut acc,
            12.0,
            12.0,
            12.0,
            t("2024-01-04 10:00:00"),
            0.4,
            0.0,
            "tree",
        );
        assert_relative_eq!(acc.entry_price, 11.0); // 部分减仓 entry 不变
        assert_eq!(acc.pos, 0.4);
    }

    #[test]
    fn finalize_liquidates_with_cost() {
        let mut acc = SimAccount::default();
        sim_step(
            &mut acc,
            10.0,
            10.0,
            11.0,
            t("2024-01-02 10:00:00"),
            1.0,
            0.001,
            "tree",
        );
        let nav_before = acc.nav;
        let rt = finalize(&mut acc, t("2024-01-02 10:15:00"), 11.0, 0.001).unwrap();
        assert_relative_eq!(acc.nav, nav_before * 0.999, epsilon = 1e-12);
        assert_eq!(rt.reason, "end");
        assert_eq!(acc.pos, 0.0);
    }
}
