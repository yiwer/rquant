use crate::eval::llm::{LlmEvaluator, LlmNode};
use crate::eval::quant::eval_quant;
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

/// 置信度加权软遍历：质量按 (选中支: c, 残余 1-c → default) 沿 DAG 传播 → 叶子分布。
/// 两阶段：①async 收边（每可达节点评一次，weight>0 才探索）②sync 记忆化求叶子分布。
pub async fn traverse_soft(tree: &Tree, ctx: &Context, llm: &LlmEvaluator) -> Result<SoftTrace> {
    // 阶段一：收集 node -> (chosen_goto, c, default_goto)
    let mut edges: HashMap<String, (String, f64, String)> = HashMap::new();
    let mut stack: Vec<String> = vec![tree.root.clone()];
    while let Some(id) = stack.pop() {
        if tree.leaves.contains_key(&id) || edges.contains_key(&id) {
            continue;
        }
        let node = tree
            .nodes
            .get(&id)
            .ok_or_else(|| Error::Engine(format!("dangling node '{id}'")))?;
        let (decision, default_goto) = match node {
            Node::Quant { branches, default } => (eval_quant(branches, default, ctx)?, default.goto.clone()),
            Node::Llm { inputs, prompt, labels, default } => {
                let ln = LlmNode { inputs, prompt, labels, default };
                (llm.eval_llm(&id, &ln, ctx).await?, default.clone())
            }
        };
        let chosen = decision.goto.clone();
        let c = decision.confidence;
        // 仅探索 weight>0 的分支（避免评估 0 质量子树 / 多余 LLM 调用）
        if c > 0.0 && tree.nodes.contains_key(&chosen) {
            stack.push(chosen.clone());
        }
        if 1.0 - c > 0.0 && tree.nodes.contains_key(&default_goto) {
            stack.push(default_goto.clone());
        }
        edges.insert(id, (chosen, c, default_goto));
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
    edges: &HashMap<String, (String, f64, String)>,
    tree: &Tree,
    memo: &mut HashMap<String, BTreeMap<String, f64>>,
) -> BTreeMap<String, f64> {
    if tree.leaves.contains_key(id) {
        return BTreeMap::from([(id.to_string(), 1.0)]);
    }
    if let Some(m) = memo.get(id) {
        return m.clone();
    }
    let (chosen, c, default_goto) = edges[id].clone();
    let mut out: BTreeMap<String, f64> = BTreeMap::new();
    if c > 0.0 {
        for (leaf, p) in leaf_dist(&chosen, edges, tree, memo) {
            *out.entry(leaf).or_insert(0.0) += p * c;
        }
    }
    if 1.0 - c > 0.0 {
        for (leaf, p) in leaf_dist(&default_goto, edges, tree, memo) {
            *out.entry(leaf).or_insert(0.0) += p * (1.0 - c);
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
        Context { t, primary: Window { bars: bars.clone() }, context: Window { bars }, news: None }
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
}
