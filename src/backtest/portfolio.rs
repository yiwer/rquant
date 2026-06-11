//! 横截面组合层：时间线/新鲜度/打分/select_top。

use crate::data::bar::Bar;
use chrono::NaiveDateTime;
use std::collections::BTreeSet;

/// 全标的 bar 时间有序并集。
pub fn build_timeline(all: &[Vec<Bar>]) -> Vec<NaiveDateTime> {
    let mut set = BTreeSet::new();
    for bars in all {
        for b in bars {
            set.insert(b.time);
        }
    }
    set.into_iter().collect()
}

/// t 时刻最后已知收盘价（time ≤ t）。
pub fn last_close_at(bars: &[Bar], t: NaiveDateTime) -> Option<f64> {
    let cut = bars.partition_point(|b| b.time <= t);
    if cut == 0 { None } else { Some(bars[cut - 1].close) }
}

/// 新鲜：恰有 bar 在 t（停牌标的当期出局）。
pub fn is_fresh(bars: &[Bar], t: NaiveDateTime) -> bool {
    bars.binary_search_by_key(&t, |b| b.time).is_ok()
}

/// score>0 取前 n：score 降序、并列 symbol 升序（确定性）。
pub fn select_top(scores: &[(String, f64)], n: usize) -> Vec<(String, f64)> {
    let mut pos: Vec<(String, f64)> = scores.iter().filter(|(_, s)| *s > 0.0).cloned().collect();
    pos.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)));
    pos.truncate(n);
    pos
}

/// 单标的在 t 的横截面分数：不新鲜 → None；硬=叶 dir×weight；软=E=Σp·w·dir。
#[allow(clippy::too_many_arguments)]
pub async fn score_symbol(
    primary: &[Bar],
    context: &[Bar],
    aux: &std::collections::BTreeMap<String, crate::data::aux_table::AuxTable>,
    tree: &crate::tree::loader::Tree,
    llm: &crate::eval::llm::LlmEvaluator,
    soft: bool,
    t: NaiveDateTime,
    window: usize,
) -> crate::Result<Option<f64>> {
    if !is_fresh(primary, t) {
        return Ok(None);
    }
    let ctx = crate::features::context::build_context(primary, context, &[], aux, t, window);
    let dir = |s: crate::tree::schema::Stance| match s {
        crate::tree::schema::Stance::Long => 1.0,
        crate::tree::schema::Stance::Short => -1.0,
        crate::tree::schema::Stance::Flat => 0.0,
    };
    let score = if soft {
        let st = crate::engine::soft::traverse_soft(tree, &ctx, llm).await?;
        st.leaf_probs.iter().map(|(id, p)| {
            tree.leaves.get(id).map_or(0.0, |l| p * l.weight * dir(l.stance))
        }).sum()
    } else {
        let tr = crate::engine::traversal::traverse(tree, &ctx, llm).await?;
        tree.leaves.get(&tr.leaf).map_or(0.0, |l| l.weight * dir(l.stance))
    };
    Ok(Some(score))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::bar::Bar;
    use chrono::NaiveDate;

    fn dt(day: u32, hour: u32, min: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2024, 1, day).unwrap().and_hms_opt(hour, min, 0).unwrap()
    }

    fn bar_at(t: NaiveDateTime, close: f64) -> Bar {
        Bar { time: t, open: close, high: close, low: close, close, volume: 1.0 }
    }

    // ── build_timeline ────────────────────────────────────────────────────────

    #[test]
    fn timeline_union_sort_dedup() {
        // Two series with staggered/overlapping timestamps
        let t1 = dt(2, 9, 30);
        let t2 = dt(2, 10, 0);
        let t3 = dt(2, 10, 30);
        let t4 = dt(2, 11, 0);

        let series_a = vec![bar_at(t1, 1.0), bar_at(t3, 3.0)];
        let series_b = vec![bar_at(t2, 2.0), bar_at(t3, 3.5), bar_at(t4, 4.0)];

        let tl = build_timeline(&[series_a, series_b]);

        // Should be sorted and deduped
        assert_eq!(tl, vec![t1, t2, t3, t4]);
    }

    #[test]
    fn timeline_empty_input() {
        let tl = build_timeline(&[]);
        assert!(tl.is_empty());
    }

    #[test]
    fn timeline_single_series() {
        let t1 = dt(2, 9, 30);
        let t2 = dt(2, 10, 0);
        let bars = vec![bar_at(t1, 1.0), bar_at(t2, 2.0)];
        let tl = build_timeline(&[bars]);
        assert_eq!(tl, vec![t1, t2]);
    }

    // ── last_close_at ─────────────────────────────────────────────────────────

    #[test]
    fn last_close_at_exact_hit() {
        let t1 = dt(2, 9, 30);
        let t2 = dt(2, 10, 0);
        let t3 = dt(2, 10, 30);
        let bars = vec![
            bar_at(t1, 10.0),
            bar_at(t2, 20.0),
            bar_at(t3, 30.0),
        ];
        assert_eq!(last_close_at(&bars, t2), Some(20.0));
        assert_eq!(last_close_at(&bars, t3), Some(30.0));
    }

    #[test]
    fn last_close_at_before_first_returns_none() {
        let t1 = dt(2, 10, 0);
        let bars = vec![bar_at(t1, 10.0)];
        let before = dt(2, 9, 0);
        assert_eq!(last_close_at(&bars, before), None);
    }

    #[test]
    fn last_close_at_between_takes_previous() {
        let t1 = dt(2, 9, 30);
        let t3 = dt(2, 10, 30);
        let bars = vec![bar_at(t1, 10.0), bar_at(t3, 30.0)];
        // t2 is between t1 and t3 — should return close at t1
        let t2 = dt(2, 10, 0);
        assert_eq!(last_close_at(&bars, t2), Some(10.0));
    }

    // ── is_fresh ──────────────────────────────────────────────────────────────

    #[test]
    fn is_fresh_exact_hit() {
        let t1 = dt(2, 9, 30);
        let t2 = dt(2, 10, 0);
        let bars = vec![bar_at(t1, 1.0), bar_at(t2, 2.0)];
        assert!(is_fresh(&bars, t1));
        assert!(is_fresh(&bars, t2));
    }

    #[test]
    fn is_fresh_miss() {
        let t1 = dt(2, 9, 30);
        let t3 = dt(2, 10, 30);
        let bars = vec![bar_at(t1, 1.0), bar_at(t3, 3.0)];
        // t2 has no bar → not fresh
        let t2 = dt(2, 10, 0);
        assert!(!is_fresh(&bars, t2));
    }

    // ── select_top ────────────────────────────────────────────────────────────

    #[test]
    fn select_top_filters_nonpositive() {
        let scores = vec![
            ("a".to_string(), -0.5),
            ("b".to_string(), 0.0),
            ("c".to_string(), 0.9),
        ];
        let top = select_top(&scores, 3);
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].0, "c");
    }

    #[test]
    fn select_top_desc_order_tie_symbol_asc() {
        // [("b",0.5),("a",0.5),("c",0.9)], n=2 → [("c",0.9),("a",0.5)]
        let scores = vec![
            ("b".to_string(), 0.5),
            ("a".to_string(), 0.5),
            ("c".to_string(), 0.9),
        ];
        let top = select_top(&scores, 2);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].0, "c");
        assert!((top[0].1 - 0.9).abs() < 1e-12);
        assert_eq!(top[1].0, "a");
        assert!((top[1].1 - 0.5).abs() < 1e-12);
    }

    #[test]
    fn select_top_fewer_than_n_returns_all() {
        let scores = vec![
            ("a".to_string(), 0.3),
            ("b".to_string(), 0.7),
        ];
        let top = select_top(&scores, 10);
        assert_eq!(top.len(), 2);
    }

    #[test]
    fn select_top_empty_scores() {
        let top = select_top(&[], 5);
        assert!(top.is_empty());
    }

    // ── score_symbol (tokio async) ────────────────────────────────────────────

    /// Tree: close > 0 → leaf_long (weight 1, stance Long); default → leaf_flat (stance Flat).
    /// Hard score for a bar with close=1.0: Long * 1.0 = 1.0.
    const SIMPLE_TREE_YAML: &str = r#"
meta: { name: test, forward_window: 3, stances: [long, flat] }
root: root_node
nodes:
  root_node:
    type: quant
    branches:
      - when: "close > 0"
        goto: leaf_long
        label: positive
    default: { goto: leaf_flat, label: none }
leaves:
  leaf_long: { stance: long, weight: 1.0 }
  leaf_flat: { stance: flat }
"#;

    #[tokio::test]
    async fn score_symbol_fresh_returns_some_score() {
        use crate::eval::llm::LlmEvaluator;
        use crate::tree::loader::load_tree_str;
        use std::collections::BTreeMap;

        let tree = load_tree_str(SIMPLE_TREE_YAML).unwrap();
        let llm = LlmEvaluator::Disabled;

        let t = dt(2, 10, 0);
        // Fresh symbol: has a bar exactly at t with close > 0
        let primary = vec![
            bar_at(dt(2, 9, 30), 1.0),
            bar_at(t, 2.0),
        ];

        let result = score_symbol(
            &primary,
            &primary,
            &BTreeMap::new(),
            &tree,
            &llm,
            false,
            t,
            10,
        ).await.unwrap();

        assert!(result.is_some(), "fresh symbol should return Some(score)");
        let score = result.unwrap();
        // close=2.0 > 0 → leaf_long → weight=1.0, dir=1.0 → score=1.0
        assert!((score - 1.0).abs() < 1e-12, "expected score 1.0, got {score}");
    }

    #[tokio::test]
    async fn score_symbol_stale_returns_none() {
        use crate::eval::llm::LlmEvaluator;
        use crate::tree::loader::load_tree_str;
        use std::collections::BTreeMap;

        let tree = load_tree_str(SIMPLE_TREE_YAML).unwrap();
        let llm = LlmEvaluator::Disabled;

        let t = dt(2, 10, 0);
        // Stale symbol: bars staggered so last bar is NOT at t
        let primary = vec![
            bar_at(dt(2, 9, 30), 1.0),
            bar_at(dt(2, 9, 45), 2.0), // most recent bar is BEFORE t, not AT t
        ];

        let result = score_symbol(
            &primary,
            &primary,
            &BTreeMap::new(),
            &tree,
            &llm,
            false,
            t,
            10,
        ).await.unwrap();

        assert!(result.is_none(), "stale symbol should return None");
    }
}
