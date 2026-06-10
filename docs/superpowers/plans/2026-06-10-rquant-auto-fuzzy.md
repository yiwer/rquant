# rquant 自动模糊 DSL（strength: "auto(scale)"）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 分支 `strength: "auto"` / `"auto(0.05)"` 时，软模式强度 = `when` AST 的模糊真值（比较→相对尺度 sigmoid；and=min/or=max/not=1−x），免手写公式；硬模式零改动。

**Architecture:** 在 master(HEAD `ab1cd3c`)上扩展。T1 纯增量 `dsl::eval_fuzzy`；T2 耦合切换 `Strength::Expr|Auto` 枚举（loader 解析 + quant_branch_dist Auto 臂 + br_s 涟漪）；T3 soft YAML 测试 + README 边界说明。

**Tech Stack:** Rust 2024 + 既有。

> 设计依据：`docs/superpowers/specs/2026-06-10-rquant-auto-fuzzy-design.md`。提交信息用英文。

---

## 文件结构
```
改动: src/dsl/eval.rs      # + eval_fuzzy（and/or/not/比较软化）+ 测试
改动: src/tree/loader.rs   # + pub enum Strength；Branch.strength: Option<Strength>；解析 auto + 测试
改动: src/eval/quant.rs    # quant_branch_dist 匹配 Strength；br_s 涟漪；Auto dist 测试
改动: src/engine/soft.rs   # YAML auto 软测试
改动: README.md
```

---

## Task 1: dsl::eval_fuzzy（纯增量）

**Files:**
- Modify: `src/dsl/eval.rs`
- Test: 同文件

- [ ] **Step 1: `mod tests` 加失败测试**

（测试模块已有 `ctx_from_closes` 与 `use crate::dsl::parser::parse_str;`。**注**：`not` 的具体词法以 `src/dsl/lexer.rs` 为准——若 DSL 用 `!` 而非 `not` 关键字，把测试里的 `"not (10 > 10)"` 改成实际语法；`UnaryOp::Not` 在 AST 层是确定存在的。）
```rust
    #[test]
    fn fuzzy_comparison_and_combinators() {
        let ctx = ctx_from_closes(&[1.0]);
        let f = |src: &str| eval_fuzzy(&parse_str(src).unwrap(), &ctx, 0.02).unwrap();
        // 相等 → 0.5
        assert!((f("10 > 10") - 0.5).abs() < 1e-9);
        // above → >0.5 且单调；below → <0.5
        assert!(f("10.2 > 10") > 0.5);
        assert!(f("12 > 10") > f("10.2 > 10"));
        assert!(f("9.8 > 10") < 0.5);
        // 镜像
        assert!((f("10 < 10") - 0.5).abs() < 1e-9);
        assert!(f("9.8 < 10") > 0.5);
        // and=min / or=max / not=1-x
        let a = f("12 > 10");
        assert!((f("12 > 10 and 10 > 10") - 0.5).abs() < 1e-9);
        assert!((f("9.8 > 10 or 12 > 10") - a).abs() < 1e-9);
        assert!((f("not (10 > 10)") - 0.5).abs() < 1e-9);
        // == 保持硬
        assert!((f("10 == 10") - 1.0).abs() < 1e-9);
        assert!((f("10 == 11") - 0.0).abs() < 1e-9);
        // 双方≈0 → 0.5（无信息）
        assert!((f("0 > 0") - 0.5).abs() < 1e-9);
        // 非布尔 → Err
        assert!(eval_fuzzy(&parse_str("close").unwrap(), &ctx, 0.02).is_err());
    }
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test --lib dsl::eval::tests::fuzzy_comparison_and_combinators`
Expected: 编译失败（`eval_fuzzy` 未定义）。

- [ ] **Step 3: 实现（`eval_scalar` 附近）**

```rust
/// 模糊求值布尔表达式 → [0,1] 真值（软量化 strength: "auto" 用）。
/// 比较：sigmoid((lhs-rhs)/denom)，denom = scale·max(|lhs|,|rhs|)；denom≈0 → 0.5。
/// and=min、or=max、not=1-x（Gödel）；==/!= 保持硬；非布尔节点 → Err。
pub fn eval_fuzzy(expr: &Expr, ctx: &Context, scale: f64) -> Result<f64> {
    match expr {
        Expr::Binary(op, l, r) => match op {
            BinaryOp::And => Ok(eval_fuzzy(l, ctx, scale)?.min(eval_fuzzy(r, ctx, scale)?)),
            BinaryOp::Or => Ok(eval_fuzzy(l, ctx, scale)?.max(eval_fuzzy(r, ctx, scale)?)),
            BinaryOp::Gt | BinaryOp::Ge => fuzzy_cmp(l, r, ctx, scale, 1.0),
            BinaryOp::Lt | BinaryOp::Le => fuzzy_cmp(l, r, ctx, scale, -1.0),
            BinaryOp::Eq | BinaryOp::Ne => Ok(if as_bool(&eval(expr, ctx)?)? { 1.0 } else { 0.0 }),
            _ => Err(Error::Eval("fuzzy: expected boolean expression".into())),
        },
        Expr::Unary(UnaryOp::Not, e) => Ok(1.0 - eval_fuzzy(e, ctx, scale)?),
        _ => Err(Error::Eval("fuzzy: expected boolean expression".into())),
    }
}

fn fuzzy_cmp(l: &Expr, r: &Expr, ctx: &Context, scale: f64, sign: f64) -> Result<f64> {
    let lv = as_scalar(&eval(l, ctx)?)?;
    let rv = as_scalar(&eval(r, ctx)?)?;
    let margin = (lv - rv) * sign;
    let denom = scale * lv.abs().max(rv.abs());
    if denom <= 1e-12 {
        return Ok(0.5);
    }
    Ok(1.0 / (1.0 + (-margin / denom).exp()))
}
```

- [ ] **Step 4: 运行验证通过**

Run: `cargo test --lib dsl::eval`
Expected: 既有 + 新测试 PASS。

- [ ] **Step 5: Commit**

```bash
git add src/dsl/eval.rs
git commit -m "feat(dsl): eval_fuzzy (relative-scale sigmoid comparisons, Godel combinators)" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: Strength 枚举 + loader 解析 + quant Auto 臂（耦合）

**Files:**
- Modify: `src/tree/loader.rs`、`src/eval/quant.rs`
- Test: 两文件

> `Branch.strength` 类型从 `Option<Expr>` 变 `Option<Strength>`，破坏 loader 编译循环与 `quant.rs`（quant_branch_dist 匹配 + `br_s` 助手）——一次切完。

- [ ] **Step 1: loader 失败测试（`mod tests`）**

```rust
    #[test]
    fn parses_auto_strength_variants() {
        let yaml = |s: &str| format!(r#"
meta: {{ name: t, forward_window: 3, stances: [long, flat] }}
root: a
nodes:
  a:
    type: quant
    branches: [ {{ when: "close > 1", strength: "{s}", goto: leaf_l, label: up }} ]
    default: {{ goto: leaf_f, label: flat }}
leaves:
  leaf_l: {{ stance: long }}
  leaf_f: {{ stance: flat }}
"#);
        let get = |s: &str| -> Option<Strength> {
            let tree = load_tree_str(&yaml(s)).ok()?;
            match tree.nodes.get("a")? {
                Node::Quant { branches, .. } => branches[0].strength.clone(),
                _ => None,
            }
        };
        assert!(matches!(get("auto"), Some(Strength::Auto(s)) if (s - 0.02).abs() < 1e-12));
        assert!(matches!(get("auto(0.05)"), Some(Strength::Auto(s)) if (s - 0.05).abs() < 1e-12));
        assert!(matches!(get("0.7"), Some(Strength::Expr(_))));
        assert!(load_tree_str(&yaml("auto(x)")).is_err());
        assert!(load_tree_str(&yaml("auto(-1)")).is_err());
    }
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test --lib tree::loader::tests::parses_auto_strength_variants`
Expected: 编译失败（`Strength` 未定义）。

- [ ] **Step 3: loader 实现**

(a) `Branch` 定义旁加枚举并改字段：
```rust
/// 分支强度：显式标量表达式，或对 when 做模糊求值的 auto(scale)。
#[derive(Debug, Clone)]
pub enum Strength {
    Expr(Expr),
    Auto(f64),
}

#[derive(Debug, Clone)]
pub struct Branch {
    pub when: Expr,
    pub when_src: String,
    pub strength: Option<Strength>,
    pub goto: String,
    pub label: String,
}
```
(b) 编译循环里 strength 解析改为：
```rust
                    let strength = match &b.strength {
                        Some(src) => Some(parse_strength(src).map_err(|e| {
                            Error::Tree(format!("node '{id}' branch '{}' strength: {e}", b.label))
                        })?),
                        None => None,
                    };
```
并在文件内（`load_tree_str` 之后）加：
```rust
/// "auto" → Auto(0.02)；"auto(<f64>)" → Auto(s)（s>0）；其余按 DSL 表达式编译。
fn parse_strength(src: &str) -> Result<Strength> {
    let s = src.trim();
    if s == "auto" {
        return Ok(Strength::Auto(0.02));
    }
    if let Some(inner) = s.strip_prefix("auto(").and_then(|r| r.strip_suffix(')')) {
        let scale: f64 = inner
            .trim()
            .parse()
            .map_err(|_| Error::Tree(format!("bad auto scale '{inner}'")))?;
        if scale <= 0.0 {
            return Err(Error::Tree(format!("auto scale must be > 0, got {scale}")));
        }
        return Ok(Strength::Auto(scale));
    }
    Ok(Strength::Expr(parse_str(s)?))
}
```
（`parse_str` 的错误类型若非 `Error::Tree`，按现有 strength 编译处的 map_err 习惯包装；以现有代码为准。）

- [ ] **Step 4: quant.rs 适配 + Auto dist 测试**

(a) 顶部 `use crate::tree::loader::Branch;` 扩为 `use crate::tree::loader::{Branch, Strength};`，import 加 `use crate::dsl::eval::eval_fuzzy;`（与 eval_bool/eval_scalar 合并）。
(b) `quant_branch_dist` 的 strength 取值改为：
```rust
            let raw = match &b.strength {
                Some(Strength::Expr(e)) => eval_scalar(e, ctx)?,
                Some(Strength::Auto(scale)) => eval_fuzzy(&b.when, ctx, *scale)?,
                None => 1.0,
            };
```
(c) 测试助手 `br_s` 改为：
```rust
    fn br_s(when: &str, goto: &str, label: &str, strength: &str) -> Branch {
        Branch { when: parse_str(when).unwrap(), when_src: when.into(), strength: Some(Strength::Expr(parse_str(strength).unwrap())), goto: goto.into(), label: label.into() }
    }
```
（`br` 助手 `strength: None` 不变。）
(d) `mod tests` 加 Auto dist 测试：
```rust
    #[test]
    fn dist_auto_strength_uses_fuzzy_when() {
        // close=10.2 vs 阈值 10：margin 2% / scale 0.02 → 权重 ∈ (0.5, 1)
        let branches = vec![Branch {
            when: parse_str("close > 10").unwrap(),
            when_src: "close > 10".into(),
            strength: Some(Strength::Auto(0.02)),
            goto: "g".into(),
            label: "up".into(),
        }];
        let default = Target { goto: "d".into(), label: "none".into() };
        let dist = quant_branch_dist(&branches, &default, &ctx(&[10.0, 10.1, 10.2])).unwrap();
        assert_eq!(dist[0].0, "g");
        assert!(dist[0].1 > 0.5 && dist[0].1 < 1.0);
        let sum: f64 = dist.iter().map(|(_, w)| w).sum();
        assert!((sum - 1.0).abs() < 1e-9);
    }
```

- [ ] **Step 5: 验证**

Run: `cargo test --lib tree::loader`
Expected: 既有 + auto 解析测试 PASS。
Run: `cargo test --lib eval::quant`
Expected: 既有 + Auto dist 测试 PASS（既有 dist/strength 测试经 br_s 适配后不变语义）。
Run: `cargo test`
Expected: 全量全绿。
Run: `cargo clippy --all-targets`
Expected: 无告警（平铺执行，勿用 `2>&1`）。

- [ ] **Step 6: Commit**

```bash
git add src/tree/loader.rs src/eval/quant.rs
git commit -m "feat(tree,eval): Strength enum with auto(scale) fuzzy-when strength" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: soft YAML auto 测试 + README

**Files:**
- Modify: `src/engine/soft.rs`（测试）、`README.md`

- [ ] **Step 1: `engine/soft.rs` 的 `mod tests` 加测试**

```rust
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
```

- [ ] **Step 2: 运行验证**

Run: `cargo test --lib engine::soft`
Expected: 既有 + 新测试 PASS（T2 已实现，本测试应直接绿；它是端到端确认而非 RED——若失败说明 T2 有 bug，停下报告）。
Run: `cargo test`
Expected: 全量全绿。
Run: `cargo clippy --all-targets`
Expected: 无告警。

- [ ] **Step 3: README**（软量化谓词一节补一段）

````markdown
`strength: "auto"`（或 `"auto(0.05)"` 自定尺度）= 对该支 `when` 做模糊求值：
比较 → `sigmoid((lhs−rhs)/(scale·max(|lhs|,|rhs|)))`，`and`=min、`or`=max、`not`=1−x。
适合**量纲相近的双边比较**（如 `close > sma(close,20)`）；对 `x > 0` 型比较相对尺度会饱和趋硬——这类请写显式 `strength` 公式。
````

- [ ] **Step 4: Commit**

```bash
git add src/engine/soft.rs README.md
git commit -m "test(engine): auto-strength soft split; docs: auto fuzzy usage and bounds" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## 附录 A：Spec 覆盖对照（自检）
| Spec § | 实现于 |
|---|---|
| §1.2 eval_fuzzy（比较/组合/硬 Eq/denom≈0/非布尔 Err）| Task 1 |
| §1.3 Strength 枚举 + auto 解析（含坏格式/负 scale 拒绝）| Task 2 |
| §1.4 quant_branch_dist Auto 臂（NaN/clamp 沿用）| Task 2 |
| §3 涟漪（br_s）| Task 2 |
| §5 测试（fuzzy/loader/dist/soft YAML）| Task 1/2/3 |
| §4 README 边界说明 | Task 3 |

## 附录 B：明确不在范围（YAGNI）
- 树级 fuzzy 开关；Eq/Ne 模糊化；其它 t-norm；自动 scale 推断；硬模式任何改动。
