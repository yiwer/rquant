use crate::data::bar::{Bar, Window};
use crate::data::news::{NewsRecord, NewsView};
use chrono::NaiveDateTime;

/// 决策时点上下文：节点能看到的全部信息（绝不含未来）。
#[derive(Debug, Clone)]
pub struct Context {
    pub t: NaiveDateTime,
    pub primary: Window,
    pub context: Window,
    pub news: Option<NewsView>,
}

fn trailing_visible(bars: &[Bar], t: NaiveDateTime, window: usize) -> Vec<Bar> {
    let visible_end = bars.partition_point(|b| b.time <= t);
    let start = visible_end.saturating_sub(window);
    bars[start..visible_end].to_vec()
}

/// 构建 t 时刻的 Context：小/大周期各取最近 window 根可见 bar；
/// news 非空时取 time<=t 的最近 5 条（同 partition_point 闸门），空切片则 news=None。
pub fn build_context(
    primary: &[Bar],
    context: &[Bar],
    news: &[NewsRecord],
    t: NaiveDateTime,
    window: usize,
) -> Context {
    let news_view = if news.is_empty() {
        None
    } else {
        let end = news.partition_point(|n| n.time <= t);
        let start = end.saturating_sub(5);
        Some(NewsView { recent: news[start..end].to_vec() })
    };
    Context {
        t,
        primary: Window { bars: trailing_visible(primary, t, window) },
        context: Window { bars: trailing_visible(context, t, window) },
        news: news_view,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::bar::Bar;
    use crate::data::news::NewsRecord;
    use chrono::NaiveDate;

    fn bar_at(min_from_open: i64, price: f64) -> Bar {
        let base = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
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
        let ctx = build_context(&primary, &[], &[], t, 3);
        assert_eq!(ctx.primary.bars.len(), 3);
        assert_eq!(ctx.primary.bars.last().unwrap().close, 5.0);
        assert!(ctx.news.is_none());
    }

    #[test]
    fn no_future_bar_leaks_property() {
        let primary = series(50);
        for i in 0..primary.len() {
            let t = primary[i].time;
            let ctx = build_context(&primary, &primary, &[], t, 100);
            for b in &ctx.primary.bars {
                assert!(b.time <= t, "future primary bar leaked at i={i}");
            }
            for b in &ctx.context.bars {
                assert!(b.time <= t, "future context bar leaked at i={i}");
            }
        }
    }

    #[test]
    fn news_respects_lookahead() {
        let news = vec![
            NewsRecord { time: bar_at(0, 0.0).time, score: 0.5, headline: "n0".into() },
            NewsRecord { time: bar_at(150, 0.0).time, score: -0.5, headline: "n1".into() },
        ];
        let primary = series(20);
        let t = primary[3].time;
        let ctx = build_context(&primary, &[], &news, t, 100);
        let v = ctx.news.unwrap();
        for r in &v.recent {
            assert!(r.time <= t, "future news leaked");
        }
        assert_eq!(v.recent.len(), 1);
    }
}
