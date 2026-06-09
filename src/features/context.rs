use crate::data::bar::Bar;
use crate::data::bar::Window;
use chrono::NaiveDateTime;

/// 决策时点上下文：节点能看到的全部信息（绝不含未来）。
#[derive(Debug, Clone)]
pub struct Context {
    pub t: NaiveDateTime,
    pub primary: Window,
    pub context: Window,
}

/// 取 bars 中 time <= t 的最后 window 根（要求 bars 已按时间升序）。
/// 用 partition_point 二分，O(log n)。这是防未来函数的唯一闸门。
fn trailing_visible(bars: &[Bar], t: NaiveDateTime, window: usize) -> Vec<Bar> {
    let visible_end = bars.partition_point(|b| b.time <= t);
    let start = visible_end.saturating_sub(window);
    bars[start..visible_end].to_vec()
}

/// 构建 t 时刻的 Context：小周期与大周期各取最近 window 根可见 bar。
pub fn build_context(
    primary: &[Bar],
    context: &[Bar],
    t: NaiveDateTime,
    window: usize,
) -> Context {
    Context {
        t,
        primary: Window { bars: trailing_visible(primary, t, window) },
        context: Window { bars: trailing_visible(context, t, window) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::bar::Bar;
    use chrono::NaiveDate;

    fn bar_at(min_from_open: i64, price: f64) -> Bar {
        let base = NaiveDate::from_ymd_opt(2024, 1, 2)
            .unwrap()
            .and_hms_opt(9, 45, 0)
            .unwrap();
        let time = base + chrono::Duration::minutes(min_from_open);
        Bar { time, open: price, high: price, low: price, close: price, volume: 1.0 }
    }

    fn series(n: usize) -> Vec<Bar> {
        (0..n).map(|i| bar_at(i as i64 * 15, i as f64)).collect()
    }

    #[test]
    fn window_takes_trailing_visible_bars() {
        let primary = series(10);
        let t = primary[5].time;
        let ctx = build_context(&primary, &[], t, 3);
        assert_eq!(ctx.primary.bars.len(), 3);
        assert_eq!(ctx.primary.bars.last().unwrap().close, 5.0);
    }

    #[test]
    fn no_future_bar_leaks_property() {
        let primary = series(50);
        for i in 0..primary.len() {
            let t = primary[i].time;
            let ctx = build_context(&primary, &primary, t, 100);
            for b in &ctx.primary.bars {
                assert!(b.time <= t, "future primary bar leaked at i={i}");
            }
            for b in &ctx.context.bars {
                assert!(b.time <= t, "future context bar leaked at i={i}");
            }
        }
    }
}
