# rquant 软量化谓词（多路）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 给量化分支加可选 `strength` 标量 DSL 表达式，软遍历下量化节点产出多路分支分布（首真泄漏），`traverse_soft` 边模型升级为多路；硬模式与现有软-LLM 行为零改动。

**Architecture:** 在 master(HEAD `82eea19`)上扩展。schema/loader 加 `strength`（编译期）；`eval/quant.rs` 加 `quant_branch_dist`（首真泄漏 + clamp01）；`dsl` 加 `sigmoid` + 暴露 `eval_scalar`；`engine/soft.rs` 边模型从二元改多路 `Vec<(goto,weight)>`。硬 `eval_quant`/`traverse` 不动。

**Tech Stack:** Rust 2024 + 既有。

> 设计依据：`docs/superpowers/specs/2026-06-09-rquant-soft-quant-predicates-design.md`。提交信息用英文。

---

## 文件结构
```
改动: src/tree/schema.rs      # BranchSpec + strength: Option<String>
改动: src/tree/loader.rs      # Branch + strength: Option<Expr>；编译 strength
改动: src/eval/quant.rs       # + quant_branch_dist（首真泄漏）；br() 测试助手 +strength
改动: src/dsl/eval.rs         # + pub eval_scalar；eval_call + "sigmoid"
改动: src/engine/soft.rs      # 边模型多路化（quant 用 quant_branch_dist、llm 2 元）
改动: examples/ + README.md   # strength 示例树 + 文档
改动: tests/e2e.rs            # strength 量化树软模式 e2e
```

---

## Task 1: schema/loader — branch `strength`

**Files:**
- Modify: `src/tree/schema.rs`（`BranchSpec` + `strength`）
- Modify: `src/tree/loader.rs`（`Branch` + `strength`，编译）
- Modify: `src/eval/quant.rs`（`br()` 测试助手补 `strength: None`）
- Test: `src/tree/loader.rs`

> **编译耦合**：给 `Branch` 加字段会破坏所有 `Branch{}` 字面量——共两处：`loader.rs` 的 `compiled.push` 与 `eval/quant.rs` 的 `br()` 助手。本任务一并改。

- [ ] **Step 1: `src/tree/loader.rs` 的 `mod tests` 加失败测试**

```rust
    #[test]
    fn loads_branch_strength() {
        let src = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "close > 1", strength: "sigmoid(close - 1)", goto: leaf_l, label: up } ]
    default: { goto: leaf_f, label: flat }
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;
        let tree = load_tree_str(src).unwrap();
        let n = tree.nodes.get("a").unwrap();
        match n {
            crate::tree::loader::Node::Quant { branches, .. } => assert!(branches[0].strength.is_some()),
            _ => panic!("expected quant"),
        }
    }

    #[test]
    fn bad_strength_expr_errors() {
        let src = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "close > 1", strength: "sigmoid(", goto: leaf_l, label: up } ]
    default: { goto: leaf_f, label: flat }
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;
        assert!(load_tree_str(src).is_err());
    }
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test --lib tree::loader::tests::loads_branch_strength`
Expected: 编译失败（`BranchSpec`/`Branch` 无 `strength`）。

- [ ] **Step 3: 加字段 + 编译**

(a) `src/tree/schema.rs` `BranchSpec` 加字段：
```rust
#[derive(Debug, Deserialize)]
pub struct BranchSpec {
    pub when: String,
    #[serde(default)]
    pub strength: Option<String>,
    pub goto: String,
    pub label: String,
}
```
(b) `src/tree/loader.rs` `Branch` 加字段：
```rust
#[derive(Debug, Clone)]
pub struct Branch {
    pub when: Expr,
    pub when_src: String,
    pub strength: Option<Expr>,
    pub goto: String,
    pub label: String,
}
```
(c) `src/tree/loader.rs` 的 Quant 分支编译循环（`for b in branches { ... compiled.push(...) }`）改为：
```rust
                for b in branches {
                    let expr = parse_str(&b.when).map_err(|e| {
                        Error::Tree(format!("node '{id}' branch '{}': {e}", b.label))
                    })?;
                    let strength = match &b.strength {
                        Some(src) => Some(parse_str(src).map_err(|e| {
                            Error::Tree(format!("node '{id}' branch '{}' strength: {e}", b.label))
                        })?),
                        None => None,
                    };
                    compiled.push(Branch {
                        when: expr,
                        when_src: b.when.clone(),
                        strength,
                        goto: b.goto.clone(),
                        label: b.label.clone(),
                    });
                }
```
(d) `src/eval/quant.rs` 的 `br()` 测试助手补字段：
```rust
    fn br(when: &str, goto: &str, label: &str) -> Branch {
        Branch { when: parse_str(when).unwrap(), when_src: when.into(), strength: None, goto: goto.into(), label: label.into() }
    }
```

- [ ] **Step 4: 运行验证通过**

Run: `cargo test --lib tree::loader`
Expected: 既有 + 2 个新测试 PASS。
Run: `cargo build`
Expected: 通过（含 eval/quant.rs 的 br 修正）。

- [ ] **Step 5: Commit**

```bash
git add src/tree/schema.rs src/tree/loader.rs src/eval/quant.rs
git commit -m "feat(tree): optional per-branch strength expr (compiled at load)" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: quant_branch_dist + sigmoid

**Files:**
- Modify: `src/dsl/eval.rs`（`pub fn eval_scalar` + `"sigmoid"`）
- Modify: `src/eval/quant.rs`（`quant_branch_dist` + 测试）
- Test: 同文件

- [ ] **Step 1: `src/dsl/eval.rs` 的 `mod tests` 加 sigmoid 失败测试**

```rust
    #[test]
    fn sigmoid_eval() {
        let ctx = ctx_from_closes(&[1.0]);
        match eval(&parse_str("sigmoid(0)").unwrap(), &ctx).unwrap() {
            Value::Scalar(x) => assert!((x - 0.5).abs() < 1e-9),
            o => panic!("{o:?}"),
        }
        match eval(&parse_str("sigmoid(100)").unwrap(), &ctx).unwrap() {
            Value::Scalar(x) => assert!(x > 0.99),
            o => panic!("{o:?}"),
        }
    }
```

- [ ] **Step 2: `src/eval/quant.rs` 的 `mod tests` 加 quant_branch_dist 失败测试**

```rust
    fn br_s(when: &str, goto: &str, label: &str, strength: &str) -> Branch {
        Branch { when: parse_str(when).unwrap(), when_src: when.into(), strength: Some(parse_str(strength).unwrap()), goto: goto.into(), label: label.into() }
    }

    #[test]
    fn dist_single_no_strength_is_hard() {
        let branches = vec![br("close > 1", "g", "up")];
        let default = Target { goto: "d".into(), label: "none".into() };
        let dist = quant_branch_dist(&branches, &default, &ctx(&[1.0, 2.0, 3.0])).unwrap();
        assert_eq!(dist, vec![("g".to_string(), 1.0)]);
    }

    #[test]
    fn dist_single_strength_leaks_to_default() {
        let branches = vec![br_s("close > 1", "g", "up", "0.7")];
        let default = Target { goto: "d".into(), label: "none".into() };
        let dist = quant_branch_dist(&branches, &default, &ctx(&[1.0, 2.0, 3.0])).unwrap();
        assert_eq!(dist.len(), 2);
        assert_eq!(dist[0].0, "g");
        assert!((dist[0].1 - 0.7).abs() < 1e-9);
        assert_eq!(dist[1].0, "d");
        assert!((dist[1].1 - 0.3).abs() < 1e-9);
    }

    #[test]
    fn dist_two_true_branches_leak() {
        let branches = vec![br_s("close > 1", "a", "x", "0.6"), br_s("close > 1", "b", "y", "0.5")];
        let default = Target { goto: "d".into(), label: "none".into() };
        let dist = quant_branch_dist(&branches, &default, &ctx(&[1.0, 2.0, 3.0])).unwrap();
        assert_eq!(dist.len(), 3);
        assert!((dist[0].1 - 0.6).abs() < 1e-9);
        assert!((dist[1].1 - 0.2).abs() < 1e-9); // 0.4 * 0.5
        assert!((dist[2].1 - 0.2).abs() < 1e-9); // remaining 0.4 * 0.5
        let sum: f64 = dist.iter().map(|(_, w)| w).sum();
        assert!((sum - 1.0).abs() < 1e-9);
    }

    #[test]
    fn dist_no_true_branch_is_all_default() {
        let branches = vec![br("close > 100", "a", "x")];
        let default = Target { goto: "d".into(), label: "none".into() };
        let dist = quant_branch_dist(&branches, &default, &ctx(&[1.0, 2.0, 3.0])).unwrap();
        assert_eq!(dist, vec![("d".to_string(), 1.0)]);
    }
```

- [ ] **Step 3: 运行验证失败**

Run: `cargo test --lib dsl::eval::tests::sigmoid_eval`
Expected: 失败（`sigmoid` unknown function）。
Run: `cargo test --lib eval::quant`
Expected: 编译失败（`quant_branch_dist` 未定义）。

- [ ] **Step 4: 实现**

(a) `src/dsl/eval.rs`：在 `eval_bool` 旁加 public `eval_scalar`：
```rust
/// Evaluate and reduce to a single f64 (series → last). For strength expressions.
pub fn eval_scalar(expr: &Expr, ctx: &Context) -> Result<f64> {
    as_scalar(&eval(expr, ctx)?)
}
```
并在 `eval_call` 的 `match name { ... }` 里、`_ => Err(...)` 之前加：
```rust
        "sigmoid" => {
            need(&vals, 1, name)?;
            Ok(Value::Scalar(1.0 / (1.0 + (-as_scalar(&vals[0])?).exp())))
        }
```

(b) `src/eval/quant.rs`：顶部 `use` 加 `use crate::dsl::eval::eval_scalar;`（与现有 `use crate::dsl::eval::eval_bool;` 并列，可合并为 `use crate::dsl::eval::{eval_bool, eval_scalar};`）。在 `eval_quant` 之后加：
```rust
/// 软模式量化分支分布：按声明顺序对 when-true 分支做"首真泄漏"，
/// 权重 w_i = remaining·clamp01(strength_i)（无 strength → 1.0），残余给 default。Σ weights ≡ 1。
pub fn quant_branch_dist(branches: &[Branch], default: &Target, ctx: &Context) -> Result<Vec<(String, f64)>> {
    let mut out: Vec<(String, f64)> = Vec::new();
    let mut remaining = 1.0_f64;
    for b in branches {
        if eval_bool(&b.when, ctx)? {
            let raw = match &b.strength {
                Some(e) => eval_scalar(e, ctx)?,
                None => 1.0,
            };
            let s = if raw.is_nan() { 0.0 } else { raw.clamp(0.0, 1.0) };
            let w = remaining * s;
            if w > 0.0 {
                out.push((b.goto.clone(), w));
            }
            remaining *= 1.0 - s;
            if remaining <= 1e-12 {
                break;
            }
        }
    }
    if remaining > 1e-12 {
        out.push((default.goto.clone(), remaining));
    }
    Ok(out)
}
```

- [ ] **Step 5: 运行验证通过**

Run: `cargo test --lib dsl::eval::tests::sigmoid_eval`
Expected: PASS。
Run: `cargo test --lib eval::quant`
Expected: 既有 2 个 + 4 个新 dist 测试 PASS。

- [ ] **Step 6: Commit**

```bash
git add src/dsl/eval.rs src/eval/quant.rs
git commit -m "feat(eval): quant_branch_dist (first-true leakage) + sigmoid DSL fn" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: traverse_soft 多路化

**Files:**
- Modify: `src/engine/soft.rs`（边模型 `Vec<(String,f64)>`；quant 用 `quant_branch_dist`、llm 2 元）
- Test: 同文件（既有 5 测试不变 + 1 个 strength 新测试）

- [ ] **Step 1: 加 strength 软测试（`src/engine/soft.rs` 的 `mod tests`）**

```rust
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
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test --lib engine::soft::tests::quant_strength_splits_soft`
Expected: 失败（量化仍硬 c=1 → leaf_l 1.0、无 leaf_f；新断言不成立）。

- [ ] **Step 3: 多路化 `traverse_soft` + `leaf_dist`**

(a) `src/engine/soft.rs` 顶部 `use`：把 `use crate::eval::quant::eval_quant;` 改为 `use crate::eval::quant::quant_branch_dist;`。
(b) 把 `traverse_soft` 的边类型与阶段一改为：
```rust
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
                let d = llm.eval_llm(&id, &ln, ctx).await?;
                let c = d.confidence;
                let mut v: Vec<(String, f64)> = Vec::new();
                if c > 0.0 {
                    v.push((d.goto.clone(), c));
                }
                if 1.0 - c > 0.0 {
                    v.push((default.clone(), 1.0 - c));
                }
                v
            }
        };
        for (g, w) in &dist {
            if *w > 0.0 && tree.nodes.contains_key(g) {
                stack.push(g.clone());
            }
        }
        edges.insert(id, dist);
    }
    let mut memo: HashMap<String, BTreeMap<String, f64>> = HashMap::new();
    let mut leaf_probs = leaf_dist(&tree.root, &edges, tree, &mut memo);
    leaf_probs.retain(|_, p| *p > 0.0);
    debug_assert!(
        (leaf_probs.values().sum::<f64>() - 1.0).abs() < 1e-9,
        "soft leaf_probs must sum to 1.0"
    );
    Ok(SoftTrace { t: ctx.t, leaf_probs })
}
```
(c) 把 `leaf_dist` 改为按多路边求和：
```rust
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
```

- [ ] **Step 4: 运行验证通过**

Run: `cargo test --lib engine::soft`
Expected: 既有 5 个（quant_hard_path/llm_split/merged/quant_default/llm_disabled）+ 新 `quant_strength_splits_soft` = 6 PASS。**既有 5 个必须仍过**（无 strength → 退化硬首真；LLM 2 元不变）。
Run: `cargo test`
Expected: 全量全绿。
Run: `cargo clippy --all-targets`
Expected: 无告警（平铺执行，勿用 `2>&1`）。

- [ ] **Step 5: Commit**

```bash
git add src/engine/soft.rs
git commit -m "feat(engine): multi-way soft edges; quant nodes participate via strength" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: example + README + e2e

**Files:**
- Create: `examples/strength_tree.yaml`
- Modify: `README.md`、`tests/e2e.rs`

- [ ] **Step 1: 创建 `examples/strength_tree.yaml`**

```yaml
meta:
  name: strength_demo
  forward_window: 16
  stances: [long, flat]
root: trend
nodes:
  trend:
    type: quant
    branches:
      - when: "close > sma(close,20)"
        strength: "sigmoid((close - sma(close,20)) / (0.02 * sma(close,20)))"
        goto: leaf_long
        label: above_ma
    default: { goto: leaf_flat, label: below_ma }
leaves:
  leaf_long: { stance: long }
  leaf_flat: { stance: flat }
```

- [ ] **Step 2: `tests/e2e.rs` 加 strength 软 e2e**

> 复用 `end_to_end_uptrend_yields_positive_long_edge` 的 fixture（`gen_primary_csv`/`gen_context_csv` + `BacktestConfig`），但 tree YAML 用一个**带 strength 的量化树**（如上 strength_tree，或行内常量），调 `run_soft`，断言软量化生效。

```rust
#[tokio::test]
async fn soft_quant_strength_engages() {
    // 行内 strength 量化树（quant 节点 + strength + leaf_long/leaf_flat），
    // 复用 gen_primary_csv()/gen_context_csv() 上升趋势数据与 BacktestConfig 构造，
    // 用 LlmEvaluator::Disabled（本树无 LLM 节点）。然后：
    let report = rquant::backtest::soft::run_soft(&cfg, &rquant::eval::llm::LlmEvaluator::Disabled).await.unwrap();
    let m = &report.soft;
    assert!(m.scored > 0);
    assert!(m.engaged.count > 0, "strength-weighted quant should put mass on long");
}
```
> 把注释展开为真实代码：tree YAML 用带 strength 的量化树（warmup 内 sma 有效后 close>sma 命中、strength=sigmoid(...)>0），写入 tree_f；其余 fixture 照搬 `end_to_end_uptrend_yields_positive_long_edge`（primary_f/context_f/out_f/cfg）。

- [ ] **Step 3: 运行验证**

Run: `cargo test --test e2e soft_quant_strength_engages`
Expected: PASS（engaged.count>0）。
Run: `cargo test`
Expected: 全量全绿。
Run: `cargo clippy --all-targets`
Expected: 无告警。

- [ ] **Step 4: README 加说明**（软遍历一节内或其后）

````markdown
### 软量化谓词（`strength`）

软模式下，量化分支可选 `strength`（标量 DSL 表达式，clamp[0,1]）表达"命中强度"。
节点按 `when` 选真分支，按 `strength` 做**首真泄漏**：`w_i = remaining·strength_i`，残余给 `default`。
不写 `strength` 则 `strength=1` —— 软模式退化为硬首真（渐进采用）。

```yaml
branches:
  - when: "close > sma(close,20)"
    strength: "sigmoid((close - sma(close,20)) / (0.02 * sma(close,20)))"  # 高于均线 2% 处≈0.88
    goto: leaf_long
```

`sigmoid(x)=1/(1+e^-x)` 是内置 DSL 函数；尺度（`margin/scale`）由作者按指标量纲选定。
（见 `examples/strength_tree.yaml`。）
````

- [ ] **Step 5: Commit**

```bash
git add examples/strength_tree.yaml README.md tests/e2e.rs
git commit -m "docs+test: strength example tree, README section, soft-quant e2e" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## 附录 A：Spec 覆盖对照（自检）
| Spec § | 实现于 |
|---|---|
| §4.1 schema/loader strength | Task 1 |
| §4.2 quant_branch_dist（首真泄漏 + clamp01）| Task 2 |
| §4.3 traverse_soft 多路化（quant 多路 / llm 2 元）| Task 3 |
| §4.4 sigmoid DSL | Task 2 |
| §6 硬模式不变 + 无 strength 退化硬 | Task 3（既有 5 软测试 + 硬测试不变）|
| §7 测试（dist 四例 / sigmoid / 多路 / 退化 / e2e）| Task 1/2/3/4 |
| §5 错误处理（strength 编译/求值错、NaN→0、守恒）| Task 1/2 |

## 附录 B：明确不在范围（YAGNI）
- 自动模糊 DSL（比较/布尔自动软化）；strength 影响硬模式或选支；概率校准。
- `quant_branch_dist` 不合并同 goto（`leaf_dist` 求和已正确处理重复）；如需展示层去重再说。
