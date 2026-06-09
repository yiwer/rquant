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
pub struct BranchSpec {
    pub when: String,
    #[serde(default)]
    pub strength: Option<String>,
    pub goto: String,
    pub label: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum NodeSpec {
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
        default: String,
    },
}

#[derive(Debug, Deserialize)]
pub struct LeafSpec {
    pub stance: Stance,
}

#[derive(Debug, Deserialize)]
pub struct TreeSpec {
    pub meta: Meta,
    pub root: String,
    pub nodes: HashMap<String, NodeSpec>,
    pub leaves: HashMap<String, LeafSpec>,
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
