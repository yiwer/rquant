use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stance {
    Long,
    Flat,
    Short,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Meta {
    pub name: String,
    pub forward_window: usize,
    pub stances: Vec<Stance>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Target {
    pub goto: String,
    pub label: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct BranchSpec {
    pub(crate) when: String,
    #[serde(default)]
    pub(crate) strength: Option<String>,
    pub(crate) goto: String,
    pub(crate) label: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub(crate) enum NodeSpec {
    Quant {
        branches: Vec<BranchSpec>,
        default: Target,
    },
    Llm {
        #[serde(default)]
        inputs: Vec<String>,
        #[serde(default)]
        prompt: String,
        #[serde(default)]
        labels: HashMap<String, String>,
        #[serde(default)]
        judge: Option<String>,
        #[serde(default)]
        map: HashMap<String, String>,
        default: String,
    },
}

/// 顶层命名判定：prompt+inputs+允许的 label 集合；llm 节点经 judge: 引用并各自映射落点。
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct JudgeSpec {
    #[serde(default)]
    pub(crate) inputs: Vec<String>,
    pub(crate) prompt: String,
    pub(crate) labels: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LeafSpec {
    pub(crate) stance: Stance,
    #[serde(default)]
    pub(crate) weight: Option<serde_yaml::Value>,
    #[serde(default)]
    pub(crate) horizon: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RiskSpec {
    #[serde(default)]
    pub(crate) stop_loss: Option<f64>,
    #[serde(default)]
    pub(crate) take_profit: Option<f64>,
    #[serde(default)]
    pub(crate) max_hold_bars: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct TreeSpec {
    pub(crate) meta: Meta,
    #[serde(default)]
    pub(crate) params: HashMap<String, f64>,
    #[serde(default)]
    pub(crate) factors: serde_yaml::Mapping,
    #[serde(default)]
    pub(crate) risk: Option<RiskSpec>,
    #[serde(default)]
    pub(crate) judges: HashMap<String, JudgeSpec>,
    pub(crate) root: String,
    pub(crate) nodes: HashMap<String, NodeSpec>,
    pub(crate) leaves: HashMap<String, LeafSpec>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const YAML: &str = r#"
meta:
  name: t
  forward_window: 16
  stances: [long, flat]
root: a
nodes:
  a:
    type: quant
    branches:
      - when: "close > 1"
        goto: leaf_l
        label: up
    default: { goto: leaf_f, label: none }
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;

    #[test]
    fn deserializes_tree_spec() {
        let spec: TreeSpec = serde_yaml::from_str(YAML).unwrap();
        assert_eq!(spec.meta.forward_window, 16);
        assert_eq!(spec.root, "a");
        assert!(matches!(spec.nodes.get("a").unwrap(), NodeSpec::Quant { .. }));
        assert_eq!(spec.leaves.get("leaf_l").unwrap().stance, Stance::Long);
    }
}
