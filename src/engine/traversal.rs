use crate::engine::trace::{StepRecord, Trace};
use crate::eval::llm::{LlmEvaluator, LlmNode};
use crate::eval::quant::eval_quant;
use crate::features::context::Context;
use crate::tree::loader::{Node, Tree};
use crate::{Error, Result};

/// 从 root 走树到叶子。量化节点同步求值；LLM 节点 await 评估器（Disabled 时走 default）。
pub async fn traverse(tree: &Tree, ctx: &Context, llm: &LlmEvaluator) -> Result<Trace> {
    let mut path: Vec<StepRecord> = Vec::new();
    let mut current = tree.root.clone();
    let max_steps = tree.nodes.len() + 1;
    for _ in 0..=max_steps {
        if let Some(leaf) = tree.leaves.get(&current) {
            return Ok(Trace { t: ctx.t, path, leaf: current.clone(), stance: leaf.stance });
        }
        let node = tree
            .nodes
            .get(&current)
            .ok_or_else(|| Error::Engine(format!("dangling node '{current}'")))?;
        let decision = match node {
            Node::Quant { branches, default } => eval_quant(branches, default, ctx)?,
            Node::Llm { inputs, prompt, labels, default, scope } => {
                let ln = LlmNode { inputs, prompt, labels, default };
                llm.eval_llm(scope.as_deref().unwrap_or(&current), &ln, ctx).await?
            }
        };
        path.push(StepRecord {
            node_id: current.clone(),
            label: decision.label.clone(),
            confidence: decision.confidence,
            rationale: decision.rationale.clone(),
        });
        current = decision.goto;
    }
    Err(Error::Engine("traversal exceeded max steps (cycle?)".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::bar::{Bar, Window};
    use crate::eval::llm::{LlmEvaluator, StubLlm};
    use crate::features::context::Context;
    use crate::tree::loader::load_tree_str;
    use crate::tree::schema::Stance;
    use chrono::NaiveDate;
    use std::collections::HashMap;

    fn ctx(closes: &[f64]) -> Context {
        let base = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        let bars: Vec<Bar> = closes.iter().enumerate().map(|(i, &c)| Bar {
            time: base + chrono::Duration::minutes(i as i64 * 15), open: c, high: c, low: c, close: c, volume: 1.0,
        }).collect();
        let t = bars.last().unwrap().time;
        Context { t, primary: Window { bars: bars.clone() }, context: Window { bars }, news: None, aux: std::collections::BTreeMap::new(), sim: crate::features::context::SimState::default(), eval_cache: Default::default() }
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

    #[tokio::test]
    async fn quant_uptrend_reaches_long_leaf() {
        let tree = load_tree_str(QUANT_TREE).unwrap();
        let tr = traverse(&tree, &ctx(&[1.0, 2.0, 3.0, 4.0, 5.0]), &LlmEvaluator::Disabled).await.unwrap();
        assert_eq!(tr.leaf, "leaf_l");
        assert!(matches!(tr.stance, Stance::Long));
        assert_eq!(tr.path.len(), 1);
        assert_eq!(tr.path[0].node_id, "a");
    }

    #[tokio::test]
    async fn llm_node_disabled_takes_default() {
        let tree = load_tree_str(LLM_TREE).unwrap();
        let tr = traverse(&tree, &ctx(&[1.0, 2.0, 3.0]), &LlmEvaluator::Disabled).await.unwrap();
        assert_eq!(tr.leaf, "leaf_f");
        assert!(matches!(tr.stance, Stance::Flat));
        assert!(tr.path[0].rationale.contains("LLM disabled"));
    }

    #[tokio::test]
    async fn llm_node_stub_takes_label() {
        let tree = load_tree_str(LLM_TREE).unwrap();
        let ev = LlmEvaluator::Stub(StubLlm { answers: HashMap::from([("a".to_string(), "yes".to_string())]) });
        let tr = traverse(&tree, &ctx(&[1.0, 2.0, 3.0]), &ev).await.unwrap();
        assert_eq!(tr.leaf, "leaf_l");
        assert!(matches!(tr.stance, Stance::Long));
    }

    const SHARED_JUDGE_TREE: &str = r#"
meta: { name: t, forward_window: 3, stances: [long, flat, short] }
judges:
  veto:
    prompt: "veto?"
    labels: [bad, ok]
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "close > 100", goto: g_hi, label: hi } ]
    default: { goto: g_lo, label: lo }
  g_hi:
    type: llm
    judge: veto
    map: { ok: leaf_l }
    default: leaf_f
  g_lo:
    type: llm
    judge: veto
    map: { ok: leaf_s }
    default: leaf_f
leaves:
  leaf_l: { stance: long }
  leaf_s: { stance: short }
  leaf_f: { stance: flat }
"#;

    #[tokio::test]
    async fn factor_tree_equivalent_to_inline_tree() {
        let factored = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
factors:
  f: "sma(close, 3)"
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "close > f and f > 0", goto: leaf_l, label: up } ]
    default: { goto: leaf_f, label: flat }
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;
        let inline = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "close > sma(close, 3) and sma(close, 3) > 0", goto: leaf_l, label: up } ]
    default: { goto: leaf_f, label: flat }
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;
        let (tf, ti) = (load_tree_str(factored).unwrap(), load_tree_str(inline).unwrap());
        for closes in [&[1.0, 2.0, 3.0, 4.0, 5.0][..], &[5.0, 4.0, 3.0][..], &[1.0][..]] {
            let c = ctx(closes);
            let a = traverse(&tf, &c, &LlmEvaluator::Disabled).await.unwrap();
            let b = traverse(&ti, &c, &LlmEvaluator::Disabled).await.unwrap();
            assert_eq!(a.leaf, b.leaf, "closes={closes:?}");
        }
    }

    #[tokio::test]
    async fn shared_judge_nodes_resolve_via_judge_scope() {
        let tree = load_tree_str(SHARED_JUDGE_TREE).unwrap();
        // stub 以 scope（judge:veto）为键——一个答案同时驱动两个调用点，证明判定已与落点解耦
        let ev = LlmEvaluator::Stub(StubLlm {
            answers: HashMap::from([("judge:veto".to_string(), "ok".to_string())]),
        });
        // close=1 → 走 g_lo → ok → leaf_s
        let tr = traverse(&tree, &ctx(&[1.0, 1.0, 1.0]), &ev).await.unwrap();
        assert_eq!(tr.leaf, "leaf_s");
        // close=200 → 走 g_hi → 同一判定 → leaf_l（不同落点）
        let tr2 = traverse(&tree, &ctx(&[200.0, 200.0, 200.0]), &ev).await.unwrap();
        assert_eq!(tr2.leaf, "leaf_l");
    }
}
