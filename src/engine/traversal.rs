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
            Node::Llm { inputs, prompt, labels, default } => {
                let ln = LlmNode { inputs, prompt, labels, default };
                llm.eval_llm(&current, &ln, ctx).await?
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
}
