use crate::dsl::ast::Expr;
use crate::dsl::parser::parse_str;
use crate::tree::schema::{Meta, NodeSpec, Stance, Target, TreeSpec};
use crate::{Error, Result};
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct Branch {
    pub when: Expr,
    pub when_src: String,
    pub goto: String,
    pub label: String,
}

#[derive(Debug, Clone)]
pub enum Node {
    Quant {
        branches: Vec<Branch>,
        default: Target,
    },
    Llm {
        inputs: Vec<String>,
        prompt: String,
        labels: HashMap<String, String>,
        default: String,
    },
}

#[derive(Debug, Clone)]
pub struct Leaf {
    pub stance: Stance,
}

#[derive(Debug, Clone)]
pub struct Tree {
    pub meta: Meta,
    pub root: String,
    pub nodes: HashMap<String, Node>,
    pub leaves: HashMap<String, Leaf>,
}

pub fn load_tree_file(path: &Path) -> Result<Tree> {
    let src = std::fs::read_to_string(path)?;
    load_tree_str(&src)
}

pub fn load_tree_str(src: &str) -> Result<Tree> {
    let spec: TreeSpec = serde_yaml::from_str(src)?;
    let stances: HashSet<Stance> = spec.meta.stances.iter().copied().collect();

    let mut leaves = HashMap::new();
    for (id, l) in &spec.leaves {
        if !stances.contains(&l.stance) {
            return Err(Error::Tree(format!(
                "leaf '{id}' stance {:?} not in meta.stances",
                l.stance
            )));
        }
        leaves.insert(id.clone(), Leaf { stance: l.stance });
    }

    let mut nodes = HashMap::new();
    for (id, ns) in &spec.nodes {
        match ns {
            NodeSpec::Quant { branches, default } => {
                let mut compiled = Vec::new();
                for b in branches {
                    let expr = parse_str(&b.when).map_err(|e| {
                        Error::Tree(format!("node '{id}' branch '{}': {e}", b.label))
                    })?;
                    compiled.push(Branch {
                        when: expr,
                        when_src: b.when.clone(),
                        goto: b.goto.clone(),
                        label: b.label.clone(),
                    });
                }
                nodes.insert(
                    id.clone(),
                    Node::Quant {
                        branches: compiled,
                        default: default.clone(),
                    },
                );
            }
            NodeSpec::Llm {
                inputs,
                prompt,
                labels,
                default,
            } => {
                nodes.insert(
                    id.clone(),
                    Node::Llm {
                        inputs: inputs.clone(),
                        prompt: prompt.clone(),
                        labels: labels.clone(),
                        default: default.clone(),
                    },
                );
            }
        }
    }

    let tree = Tree {
        meta: spec.meta.clone(),
        root: spec.root.clone(),
        nodes,
        leaves,
    };
    validate(&tree)?;
    Ok(tree)
}

fn node_targets(node: &Node) -> Vec<String> {
    match node {
        Node::Quant { branches, default } => {
            let mut v: Vec<String> = branches.iter().map(|b| b.goto.clone()).collect();
            v.push(default.goto.clone());
            v
        }
        Node::Llm { labels, default, .. } => {
            let mut v: Vec<String> = labels.values().cloned().collect();
            v.push(default.clone());
            v
        }
    }
}

fn validate(tree: &Tree) -> Result<()> {
    let exists = |id: &str| tree.nodes.contains_key(id) || tree.leaves.contains_key(id);

    if !tree.nodes.contains_key(&tree.root) {
        return Err(Error::Tree(format!("root '{}' is not a node", tree.root)));
    }

    for (id, node) in &tree.nodes {
        for tgt in node_targets(node) {
            if !exists(&tgt) {
                return Err(Error::Tree(format!(
                    "node '{id}' points to unknown target '{tgt}'"
                )));
            }
        }
    }

    // reachability from root
    let mut seen = HashSet::new();
    let mut stack = vec![tree.root.clone()];
    while let Some(cur) = stack.pop() {
        if !seen.insert(cur.clone()) {
            continue;
        }
        if let Some(node) = tree.nodes.get(&cur) {
            for tgt in node_targets(node) {
                stack.push(tgt);
            }
        }
    }
    for id in tree.nodes.keys() {
        if !seen.contains(id) {
            return Err(Error::Tree(format!("node '{id}' unreachable from root")));
        }
    }

    // DAG check
    let mut color: HashMap<String, u8> = HashMap::new();
    dfs_cycle(&tree.root, tree, &mut color)?;
    Ok(())
}

fn dfs_cycle(cur: &str, tree: &Tree, color: &mut HashMap<String, u8>) -> Result<()> {
    color.insert(cur.to_string(), 1); // 1 = in stack
    if let Some(node) = tree.nodes.get(cur) {
        for tgt in node_targets(node) {
            match color.get(&tgt).copied().unwrap_or(0) {
                1 => return Err(Error::Tree(format!("cycle detected at '{tgt}'"))),
                0 => dfs_cycle(&tgt, tree, color)?,
                _ => {}
            }
        }
    }
    color.insert(cur.to_string(), 2); // 2 = done
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
root: a
nodes:
  a:
    type: quant
    branches:
      - when: "close > sma(close,3)"
        goto: leaf_l
        label: up
    default: { goto: leaf_f, label: none }
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;

    #[test]
    fn loads_valid_tree() {
        let tree = load_tree_str(VALID).unwrap();
        assert_eq!(tree.root, "a");
        assert_eq!(tree.nodes.len(), 1);
        assert_eq!(tree.leaves.len(), 2);
    }

    #[test]
    fn rejects_unknown_target() {
        let bad = VALID.replace("goto: leaf_l", "goto: nope");
        assert!(load_tree_str(&bad).is_err());
    }

    #[test]
    fn rejects_leaf_stance_not_in_meta() {
        let bad = VALID.replace("leaf_l: { stance: long }", "leaf_l: { stance: short }");
        assert!(load_tree_str(&bad).is_err());
    }

    #[test]
    fn rejects_bad_dsl_at_load() {
        let bad = VALID.replace(r#"when: "close > sma(close,3)""#, r#"when: "close >""#);
        assert!(load_tree_str(&bad).is_err());
    }

    #[test]
    fn rejects_cycle() {
        let cyc = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "close > 1", goto: b, label: x } ]
    default: { goto: leaf_f, label: none }
  b:
    type: quant
    branches: [ { when: "close > 1", goto: a, label: y } ]
    default: { goto: leaf_f, label: none }
leaves:
  leaf_f: { stance: flat }
"#;
        assert!(load_tree_str(cyc).is_err());
    }
}
