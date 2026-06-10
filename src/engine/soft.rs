use crate::eval::llm::{LlmEvaluator, LlmNode};
use crate::eval::quant::quant_branch_dist;
use crate::features::context::Context;
use crate::tree::loader::{Node, Tree};
use crate::{Error, Result};
use chrono::NaiveDateTime;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, Serialize)]
pub struct SoftTrace {
    pub t: NaiveDateTime,
    pub leaf_probs: BTreeMap<String, f64>,
}

/// 置信度加权软遍历：质量按多路边 Vec<(goto, weight)>（Σweight=1）沿 DAG 传播 → 叶子分布。
/// 两阶段：①async 收边（每可达节点评一次；仅当某子边 weight>0 才把该子节点压栈）②sync 记忆化求叶子分布。
pub async fn traverse_soft(tree: &Tree, ctx: &Context, llm: &LlmEvaluator) -> Result<SoftTrace> {
    // 阶段一：收集 node -> Vec<(goto, weight)>（Σweight=1）
    let mut edges: HashMap<String, Vec<(String, f64)>> = HashMap::new();
    let mut stack: Vec<String> = vec![tree.root.clone()];
    while let Some(id) = stack.pop() {
        if tree.leaves.contains_key(&id) || edges.contains_key(&id) {
            continue;
        }
        let node = tree
            .nodes
            .get(&id)
            .ok_or_else(|| Error::Engine(format!("dangling node '{id}'")))?;
        let dist: Vec<(String, f64)> = match node {
            Node::Quant { branches, default } => quant_branch_dist(branches, default, ctx)?,
            Node::Llm { inputs, prompt, labels, default } => {
                let ln = LlmNode { inputs, prompt, labels, default };
                let (dist, _rationale) = llm.eval_llm_dist(&id, &ln, ctx).await?;
                dist
            }
        };
        for (g, w) in &dist {
            if *w > 0.0 && tree.nodes.contains_key(g) {
                stack.push(g.clone());
            }
        }
        edges.insert(id, dist);
    }
    // 阶段二：记忆化求叶子分布
    let mut memo: HashMap<String, BTreeMap<String, f64>> = HashMap::new();
    let mut leaf_probs = leaf_dist(&tree.root, &edges, tree, &mut memo);
    leaf_probs.retain(|_, p| *p > 0.0);
    debug_assert!(
        (leaf_probs.values().sum::<f64>() - 1.0).abs() < 1e-9,
        "soft leaf_probs must sum to 1.0"
    );
    Ok(SoftTrace { t: ctx.t, leaf_probs })
}

fn leaf_dist(
    id: &str,
    edges: &HashMap<String, Vec<(String, f64)>>,
    tree: &Tree,
    memo: &mut HashMap<String, BTreeMap<String, f64>>,
) -> BTreeMap<String, f64> {
    if tree.leaves.contains_key(id) {
        return BTreeMap::from([(id.to_string(), 1.0)]);
    }
    if let Some(m) = memo.get(id) {
        return m.clone();
    }
    let dist = edges[id].clone();
    let mut out: BTreeMap<String, f64> = BTreeMap::new();
    for (g, w) in dist {
        if w > 0.0 {
            for (leaf, p) in leaf_dist(&g, edges, tree, memo) {
                *out.entry(leaf).or_insert(0.0) += p * w;
            }
        }
    }
    memo.insert(id.to_string(), out.clone());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::bar::{Bar, Window};
    use crate::eval::llm::{LlmEvaluator, StubLlm};
    use crate::features::context::Context;
    use crate::tree::loader::load_tree_str;
    use chrono::NaiveDate;
    use std::collections::HashMap;

    fn ctx(closes: &[f64]) -> Context {
        let base = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        let bars: Vec<Bar> = closes.iter().enumerate().map(|(i, &c)| Bar {
            time: base + chrono::Duration::minutes(i as i64 * 15), open: c, high: c, low: c, close: c, volume: 1.0,
        }).collect();
        let t = bars.last().unwrap().time;
        Context { t, primary: Window { bars: bars.clone() }, context: Window { bars }, news: None, aux: std::collections::BTreeMap::new() }
    }

    const QUANT_TREE: &str = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "close > sma(close,3)", goto: leaf_l, label: up } ]
    default: { goto: leaf_f, label: flat }
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;

    const LLM_TREE: &str = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
root: a
nodes:
  a:
    type: llm
    prompt: "x"
    labels: { yes: leaf_l }
    default: leaf_f
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;

    const LLM_MERGE_TREE: &str = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
root: a
nodes:
  a:
    type: llm
    prompt: "x"
    labels: { yes: leaf_x }
    default: leaf_x
leaves:
  leaf_x: { stance: long }
"#;

    #[tokio::test]
    async fn quant_hard_path_is_single_leaf() {
        let tree = load_tree_str(QUANT_TREE).unwrap();
        let st = traverse_soft(&tree, &ctx(&[1.0, 2.0, 3.0, 4.0, 5.0]), &LlmEvaluator::Disabled).await.unwrap();
        assert_eq!(st.leaf_probs.len(), 1);
        assert!((st.leaf_probs["leaf_l"] - 1.0).abs() < 1e-9);
    }

    #[tokio::test]
    async fn llm_node_splits_by_confidence() {
        let tree = load_tree_str(LLM_TREE).unwrap();
        let ev = LlmEvaluator::Stub(StubLlm { answers: HashMap::from([("a".to_string(), "yes".to_string())]) });
        let st = traverse_soft(&tree, &ctx(&[1.0, 2.0, 3.0]), &ev).await.unwrap();
        assert!((st.leaf_probs["leaf_l"] - 0.9).abs() < 1e-9); // stub confidence 0.9
        assert!((st.leaf_probs["leaf_f"] - 0.1).abs() < 1e-9);
        let sum: f64 = st.leaf_probs.values().sum();
        assert!((sum - 1.0).abs() < 1e-9);
    }

    #[tokio::test]
    async fn merged_branches_sum_probability() {
        let tree = load_tree_str(LLM_MERGE_TREE).unwrap();
        let ev = LlmEvaluator::Stub(StubLlm { answers: HashMap::from([("a".to_string(), "yes".to_string())]) });
        let st = traverse_soft(&tree, &ctx(&[1.0]), &ev).await.unwrap();
        assert_eq!(st.leaf_probs.len(), 1);
        assert!((st.leaf_probs["leaf_x"] - 1.0).abs() < 1e-9);
    }

    #[tokio::test]
    async fn quant_default_routes_to_default() {
        // 下跌 → close < sma → 无分支命中 → default(c=0.5, chosen==default) → 全给 leaf_f
        let tree = load_tree_str(QUANT_TREE).unwrap();
        let st = traverse_soft(&tree, &ctx(&[5.0, 4.0, 3.0, 2.0, 1.0]), &LlmEvaluator::Disabled).await.unwrap();
        assert_eq!(st.leaf_probs.len(), 1);
        assert!((st.leaf_probs["leaf_f"] - 1.0).abs() < 1e-9);
    }

    #[tokio::test]
    async fn llm_disabled_routes_to_default() {
        // LLM 不可用 → c=0.0 → 全部质量走 default(leaf_f)
        let tree = load_tree_str(LLM_TREE).unwrap();
        let st = traverse_soft(&tree, &ctx(&[1.0, 2.0, 3.0]), &LlmEvaluator::Disabled).await.unwrap();
        assert_eq!(st.leaf_probs.len(), 1);
        assert!((st.leaf_probs["leaf_f"] - 1.0).abs() < 1e-9);
    }

    const QUANT_SOFT_TREE: &str = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "close > sma(close,3)", strength: "0.7", goto: leaf_l, label: up } ]
    default: { goto: leaf_f, label: flat }
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;

    #[tokio::test]
    async fn quant_strength_splits_soft() {
        let tree = load_tree_str(QUANT_SOFT_TREE).unwrap();
        let st = traverse_soft(&tree, &ctx(&[1.0, 2.0, 3.0, 4.0, 5.0]), &LlmEvaluator::Disabled).await.unwrap();
        assert!((st.leaf_probs["leaf_l"] - 0.7).abs() < 1e-9);
        assert!((st.leaf_probs["leaf_f"] - 0.3).abs() < 1e-9);
    }

    const LLM3_TREE: &str = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
root: a
nodes:
  a:
    type: llm
    prompt: "x"
    labels: { up: leaf_x, dn: leaf_y }
    default: leaf_f
leaves:
  leaf_x: { stance: long }
  leaf_y: { stance: flat }
  leaf_f: { stance: flat }
"#;

    #[tokio::test]
    async fn llm_multi_label_distribution_splits_three_ways() {
        let tree = load_tree_str(LLM3_TREE).unwrap();
        let ev = LlmEvaluator::Stub(StubLlm { answers: HashMap::from([("a".to_string(), "up:0.5,dn:0.3".to_string())]) });
        let st = traverse_soft(&tree, &ctx(&[1.0, 2.0, 3.0]), &ev).await.unwrap();
        assert!((st.leaf_probs["leaf_x"] - 0.5).abs() < 1e-9);
        assert!((st.leaf_probs["leaf_y"] - 0.3).abs() < 1e-9);
        assert!((st.leaf_probs["leaf_f"] - 0.2).abs() < 1e-9);
        let sum: f64 = st.leaf_probs.values().sum();
        assert!((sum - 1.0).abs() < 1e-9);
    }

    const QUANT_AUTO_TREE: &str = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "close > sma(close,3)", strength: "auto", goto: leaf_l, label: up } ]
    default: { goto: leaf_f, label: flat }
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;

    #[tokio::test]
    async fn quant_auto_strength_splits_soft() {
        let tree = load_tree_str(QUANT_AUTO_TREE).unwrap();
        let st = traverse_soft(&tree, &ctx(&[1.0, 2.0, 3.0, 4.0, 5.0]), &LlmEvaluator::Disabled).await.unwrap();
        // close=5 vs sma=4：margin 25%，scale 2% → 接近 1 但 <1；两叶都有质量、Σ=1
        let l = st.leaf_probs["leaf_l"];
        assert!(l > 0.5 && l < 1.0, "auto strength should soft-split, got {l}");
        assert!(st.leaf_probs.contains_key("leaf_f"));
        let sum: f64 = st.leaf_probs.values().sum();
        assert!((sum - 1.0).abs() < 1e-9);
    }
}
