use crate::data::aux_table::AuxTable;
use crate::data::bar::{Bar, Window};
use crate::data::news::{NewsRecord, NewsView};
use chrono::NaiveDateTime;
use std::collections::BTreeMap;

/// --sim 模式注入的持仓状态；打分模式恒为默认（pos=0/entry=NaN/…）。
#[derive(Debug, Clone)]
pub struct SimState {
    pub pos: f64,
    pub entry_price: f64,
    pub bars_held: usize,
    pub unreal_pnl: f64,
    /// 入场以来所见 high 最大值，空仓 NaN（弃权纪律同 entry_price）。
    pub max_price_since_entry: f64,
    /// 入场以来所见 low 最小值，空仓 NaN（弃权纪律同 entry_price）。
    pub min_price_since_entry: f64,
}

impl Default for SimState {
    fn default() -> Self {
        Self {
            pos: 0.0,
            entry_price: f64::NAN,
            bars_held: 0,
            unreal_pnl: 0.0,
            max_price_since_entry: f64::NAN,
            min_price_since_entry: f64::NAN,
        }
    }
}

/// 已按 time≤t 截断的 aux 列视图。
#[derive(Debug, Clone)]
pub struct AuxView {
    pub cols: BTreeMap<String, Vec<f64>>,
}

/// 决策时点上下文：节点能看到的全部信息（绝不含未来）。
#[derive(Debug, Clone)]
pub struct Context {
    pub t: NaiveDateTime,
    pub primary: Window,
    pub context: Window,
    pub news: Option<NewsView>,
    pub aux: BTreeMap<String, AuxView>,
    pub sim: SimState,
}

fn trailing_visible(bars: &[Bar], t: NaiveDateTime, window: usize) -> Vec<Bar> {
    let visible_end = bars.partition_point(|b| b.time <= t);
    let start = visible_end.saturating_sub(window);
    bars[start..visible_end].to_vec()
}

/// 构建 t 时刻的 Context：小/大周期各取最近 window 根可见 bar；
/// news 非空时取 time<=t 的最近 5 条（同 partition_point 闸门），空切片则 news=None。
/// aux 每张表按 time<=t 截断为 AuxView（partition_point 闸门）。
pub fn build_context(
    primary: &[Bar],
    context: &[Bar],
    news: &[NewsRecord],
    aux: &BTreeMap<String, AuxTable>,
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
    let aux_views = aux
        .iter()
        .map(|(name, table)| {
            let cut = table.times.partition_point(|x| *x <= t);
            let cols = table.cols.iter().map(|(c, v)| (c.clone(), v[..cut].to_vec())).collect();
            (name.clone(), AuxView { cols })
        })
        .collect();
    Context {
        t,
        primary: Window { bars: trailing_visible(primary, t, window) },
        context: Window { bars: trailing_visible(context, t, window) },
        news: news_view,
        aux: aux_views,
        sim: SimState::default(),
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
        let ctx = build_context(&primary, &[], &[], &BTreeMap::new(), t, 3);
        assert_eq!(ctx.primary.bars.len(), 3);
        assert_eq!(ctx.primary.bars.last().unwrap().close, 5.0);
        assert!(ctx.news.is_none());
    }

    #[test]
    fn no_future_bar_leaks_property() {
        let primary = series(50);
        for i in 0..primary.len() {
            let t = primary[i].time;
            let ctx = build_context(&primary, &primary, &[], &BTreeMap::new(), t, 100);
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
        let ctx = build_context(&primary, &[], &news, &BTreeMap::new(), t, 100);
        let v = ctx.news.unwrap();
        for r in &v.recent {
            assert!(r.time <= t, "future news leaked");
        }
        assert_eq!(v.recent.len(), 1);
    }

    #[test]
    fn aux_tables_gated_by_time() {
        use crate::data::aux_table::AuxTable;
        let base = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        let bars: Vec<Bar> = (0..3).map(|i| Bar {
            time: base + chrono::Duration::minutes(i * 15),
            open: 1.0, high: 1.0, low: 1.0, close: 1.0, volume: 1.0,
        }).collect();
        let mut aux = BTreeMap::new();
        aux.insert("idx".to_string(), AuxTable {
            times: vec![base, base + chrono::Duration::minutes(15), base + chrono::Duration::minutes(45)],
            cols: BTreeMap::from([("v".to_string(), vec![1.0, 2.0, 3.0])]),
        });
        // t = 第 2 根 bar（base+15）：第三行（base+45）必须被闸门挡住
        let ctx = build_context(&bars, &bars, &[], &aux, bars[1].time, 100);
        assert_eq!(ctx.aux["idx"].cols["v"], vec![1.0, 2.0]);
        // t 早于首行 → 空
        let ctx2 = build_context(&bars, &bars, &[], &aux, base - chrono::Duration::minutes(1), 100);
        assert!(ctx2.aux["idx"].cols["v"].is_empty());
    }
}
