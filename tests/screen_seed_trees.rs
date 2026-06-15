//! 种子树须能加载且通过加载期 lint（构造正确性闸）。
use std::path::Path;

#[test]
fn all_seed_trees_load() {
    let paths = [
        "examples/trees/screen/quality_v1.yaml",
        "examples/trees/screen/momentum_v1.yaml",
        "examples/trees/screen/breakout_v1.yaml",
        "examples/trees/screen/pullback_v1.yaml",
    ];
    for p in paths {
        let tree = rquant::tree::loader::load_tree_file(Path::new(p))
            .unwrap_or_else(|e| panic!("seed tree {p} failed to load/lint: {e}"));
        assert!(!tree.meta.name.is_empty(), "tree {p} has empty name");
        assert!(!tree.leaves.is_empty(), "tree {p} has no leaves");
    }
}
