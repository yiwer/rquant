# rquant E1+E2（params/factors + 叶子 weight/horizon + 时间标识符）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 树顶层 `params:`/`factors:` 命名块（加载期 AST 内联替换 + 未知名左移到加载错）、叶子 `weight∈(0,1]`/`horizon`、DSL `hour/minute/dow`；旧树零改动兼容，w=1/horizon=全局 退化逐字一致。

**Architecture:** 在 master(HEAD `3468328`)上扩展。`dsl/ast.rs` 加 `substitute`；loader 构建 env（params→Number、factors 按 serde_yaml::Mapping 文档序编译）并对全部 when/strength 替换+未知 Ident 检查；`eval.rs` Ident 臂特判时间名；runner/score_soft 按叶取 horizon/weight（position r 用分布内 max horizon）。

**Tech Stack:** Rust 2024 + 既有（serde_yaml Mapping 保序）。

> 设计依据：`docs/superpowers/specs/2026-06-10-rquant-e1e2-factors-weights-design.md`。提交信息用英文。**所有"以实际代码为准"处：先读目标文件再写。**

---

## 文件结构
```
改动: src/dsl/eval.rs      # T1 Ident 臂时间特判
改动: src/dsl/ast.rs       # T2 substitute + 测试
改动: src/tree/schema.rs   # T2 TreeSpec.params/factors；T3 LeafSpec.weight/horizon
改动: src/tree/loader.rs   # T2 env 构建/命名校验/未知 Ident 检查；T3 Leaf 校验与默认
改动: src/backtest/runner.rs  # T3 eval_point 按叶 horizon/weight
改动: src/backtest/soft.rs    # T3 score_soft 按叶 + max_h + 去 fw 参（调用点涟漪）
改动: docs/tree-yaml-schema.md docs/dsl-reference.md examples/ tests/e2e.rs README.md  # T4
```

---

## Task 1: DSL 时间标识符

**Files:**
- Modify: `src/dsl/eval.rs`
- Test: 同文件

- [ ] **Step 1: 失败测试（`mod tests`，`ctx_from_closes` 基准时间 2024-01-02 09:45 周二；单根 bar 时 t=09:45）**

```rust
    #[test]
    fn time_identifiers_hour_minute_dow() {
        let ctx = ctx_from_closes(&[1.0]); // t = 2024-01-02 09:45（周二）
        let f = |src: &str| eval(&parse_str(src).unwrap(), &ctx).unwrap();
        assert_eq!(f("hour == 9"), Value::Bool(true));
        assert_eq!(f("minute == 45"), Value::Bool(true));
        assert_eq!(f("dow == 2"), Value::Bool(true));
        assert_eq!(f("dow <= 5"), Value::Bool(true));
        // fuzzy 路径可用（比较经 as_scalar）
        assert!((eval_fuzzy(&parse_str("hour >= 9").unwrap(), &ctx, 0.02).unwrap() - 0.5).abs() < 0.5);
    }
```
> 若 `ctx_from_closes` 的基准时间与上不符，以实际为准调整断言值（先读测试助手）。

- [ ] **Step 2: 验证失败**

Run: `cargo test --lib dsl::eval::tests::time_identifiers_hour_minute_dow`
Expected: FAIL（hour 当 unknown identifier 报错）。

- [ ] **Step 3: 实现**

`src/dsl/eval.rs` 顶部加 `use chrono::{Datelike, Timelike};`。把 `eval` 中 `Expr::Ident(name) => Ok(Value::Series(resolve_series(name, ctx)?)),` 改为：
```rust
        Expr::Ident(name) => match name.as_str() {
            "hour" => Ok(Value::Scalar(f64::from(ctx.t.hour()))),
            "minute" => Ok(Value::Scalar(f64::from(ctx.t.minute()))),
            "dow" => Ok(Value::Scalar(f64::from(ctx.t.weekday().number_from_monday()))),
            _ => Ok(Value::Series(resolve_series(name, ctx)?)),
        },
```

- [ ] **Step 4: 验证通过 + Commit**

Run: `cargo test --lib dsl::eval` → 全 PASS。
```bash
git add src/dsl/eval.rs
git commit -m "feat(dsl): hour/minute/dow scalar time identifiers" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: substitute + params/factors（加载期内联）

**Files:**
- Modify: `src/dsl/ast.rs`（substitute + 测试）、`src/tree/schema.rs`、`src/tree/loader.rs`（env/校验 + 测试）

- [ ] **Step 1: `src/dsl/ast.rs` 加 substitute + 失败测试**

（先读 ast.rs 确认 Expr 变体与 derive；若 `BinaryOp`/`UnaryOp` 非 Copy 用 `.clone()`。）
```rust
use std::collections::HashMap;

/// 把表达式中的 Ident(name) 按 env 替换为对应子树（深拷贝）；params/factors 加载期内联用。
pub fn substitute(expr: &Expr, env: &HashMap<String, Expr>) -> Expr {
    match expr {
        Expr::Ident(name) => env.get(name).cloned().unwrap_or_else(|| expr.clone()),
        Expr::Number(_) => expr.clone(),
        Expr::Unary(op, e) => Expr::Unary(*op, Box::new(substitute(e, env))),
        Expr::Binary(op, l, r) => Expr::Binary(*op, Box::new(substitute(l, env)), Box::new(substitute(r, env))),
        Expr::Call(name, args) => Expr::Call(name.clone(), args.iter().map(|a| substitute(a, env)).collect()),
        Expr::Index(e, k) => Expr::Index(Box::new(substitute(e, env)), *k),
    }
}
```
测试（ast.rs 若无 `mod tests` 则新建；用 `crate::dsl::parser::parse_str` 构造）：
```rust
    #[test]
    fn substitute_params_and_nested() {
        use crate::dsl::parser::parse_str;
        let mut env = HashMap::new();
        env.insert("n".to_string(), Expr::Number(20.0));
        let e = substitute(&parse_str("sma(close, n) > n").unwrap(), &env);
        // n 全部替换为 20，close 保留
        let rendered = format!("{e:?}");
        assert!(!rendered.contains("Ident(\"n\")"));
        assert!(rendered.contains("Ident(\"close\")"));
        assert!(rendered.contains("Number(20.0)"));
    }
```

- [ ] **Step 2: RED → GREEN（substitute）**

Run: `cargo test --lib dsl::ast` → 编译失败 → 实现后 PASS。

- [ ] **Step 3: schema + loader**

(a) `src/tree/schema.rs` `TreeSpec` 加（保持现有 pub(crate) 风格）：
```rust
    #[serde(default)]
    pub(crate) params: std::collections::HashMap<String, f64>,
    #[serde(default)]
    pub(crate) factors: serde_yaml::Mapping,
```
(b) `src/tree/loader.rs` 加（`load_tree_str` 内、编译 nodes 之前构建 env；新增辅助函数）：
```rust
const RESERVED_IDENTS: [&str; 8] = ["close", "open", "high", "low", "volume", "hour", "minute", "dow"];
const RESERVED_FNS: [&str; 16] = [
    "sma", "ema", "wma", "rsi", "atr", "slope", "highest", "lowest", "crossover", "crossunder",
    "macd_line", "macd_signal", "macd_hist", "std", "sigmoid", "auto",
];

fn check_name(name: &str, env: &HashMap<String, Expr>) -> Result<()> {
    if RESERVED_IDENTS.contains(&name) || RESERVED_FNS.contains(&name) {
        return Err(Error::Tree(format!("name '{name}' shadows a built-in identifier/function")));
    }
    if env.contains_key(name) {
        return Err(Error::Tree(format!("duplicate name '{name}' in params/factors")));
    }
    Ok(())
}

/// 替换后残余 Ident 必须是内置标识符或 ctx. 前缀（把"未定义名"左移到加载错）。
fn check_no_unknown_idents(expr: &Expr, where_: &str) -> Result<()> {
    match expr {
        Expr::Ident(name) => {
            if RESERVED_IDENTS.contains(&name.as_str()) || name.starts_with("ctx.") {
                Ok(())
            } else {
                Err(Error::Tree(format!("{where_}: unknown identifier '{name}'")))
            }
        }
        Expr::Number(_) => Ok(()),
        Expr::Unary(_, e) | Expr::Index(e, _) => check_no_unknown_idents(e, where_),
        Expr::Binary(_, l, r) => {
            check_no_unknown_idents(l, where_)?;
            check_no_unknown_idents(r, where_)
        }
        Expr::Call(_, args) => args.iter().try_for_each(|a| check_no_unknown_idents(a, where_)),
    }
}
```
env 构建（`load_tree_str` 内）：
```rust
    let mut env: HashMap<String, Expr> = HashMap::new();
    for (k, v) in &spec.params {
        check_name(k, &env)?;
        env.insert(k.clone(), Expr::Number(*v));
    }
    for (k, v) in &spec.factors {
        let name = k.as_str().ok_or_else(|| Error::Tree("factor name must be a string".into()))?;
        let src = v.as_str().ok_or_else(|| Error::Tree(format!("factor '{name}' expr must be a string")))?;
        check_name(name, &env)?;
        let e = parse_str(src).map_err(|e| Error::Tree(format!("factor '{name}': {e}")))?;
        let e = substitute(&e, &env);
        check_no_unknown_idents(&e, &format!("factor '{name}'"))?;
        env.insert(name.to_string(), e);
    }
```
（import：`use crate::dsl::ast::{substitute, Expr};` 与现有合并。）
所有 `when` 编译后接 `let expr = substitute(&expr, &env); check_no_unknown_idents(&expr, &format!("node '{id}' branch '{}'", b.label))?;`；`Strength::Expr` 同样替换+检查（`parse_strength` 返回后处理，或给 `parse_strength` 传 env——以实际代码结构选最小改动）。

- [ ] **Step 4: loader 失败测试 → GREEN**

```rust
    #[test]
    fn params_and_factors_inline_and_validate() {
        let ok = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
params: { th: 2.0 }
factors:
  mom: "close - th"
  momp: "mom > 0"
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "momp and mom > th", goto: leaf_l, label: up } ]
    default: { goto: leaf_f, label: flat }
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;
        assert!(load_tree_str(ok).is_ok());
        // 反向引用（mom 引用后定义的 momp）→ 加载错
        let bad_order = ok.replace("mom: \"close - th\"", "mom: \"close - momp\"");
        assert!(load_tree_str(&bad_order).is_err());
        // 与函数/内置名冲突
        assert!(load_tree_str(&ok.replace("mom:", "sma:")).is_err());
        assert!(load_tree_str(&ok.replace("th: 2.0", "close: 2.0")).is_err());
        // when 引用未定义名 → 加载错（左移）
        let unknown = ok.replace("momp and mom > th", "nope > 0");
        assert!(load_tree_str(&unknown).is_err());
    }
```
> 注意 `bad_order` 的 replace 会让 momp 仍引用 mom——确保替换后字符串语义正确；若 replace 方案绕，直接写第二个完整 YAML 字面量。

Run: `cargo test --lib tree::loader` → 全 PASS。`cargo test` 全量绿（既有树都不带新块，零影响）。

- [ ] **Step 5: Commit**

```bash
git add src/dsl/ast.rs src/tree/schema.rs src/tree/loader.rs
git commit -m "feat(tree,dsl): params/factors named blocks with load-time AST inlining and unknown-ident left-shift" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: 叶子 weight/horizon + 打分接线

**Files:**
- Modify: `src/tree/schema.rs`、`src/tree/loader.rs`、`src/backtest/runner.rs`、`src/backtest/soft.rs`
- Test: loader + soft 同文件

- [ ] **Step 1: schema/loader**

(a) `LeafSpec` 加 `#[serde(default)] pub(crate) weight: Option<f64>,` 与 `#[serde(default)] pub(crate) horizon: Option<usize>,`。
(b) runtime `Leaf` 改为：
```rust
#[derive(Debug, Clone)]
pub struct Leaf {
    pub stance: Stance,
    /// 仓位大小 ∈ (0,1]，默认 1.0
    pub weight: f64,
    /// 该叶前瞻评分窗口，默认 meta.forward_window
    pub horizon: usize,
}
```
(c) loader 构建处校验：
```rust
        let weight = l.weight.unwrap_or(1.0);
        if !(weight > 0.0 && weight <= 1.0) {
            return Err(Error::Tree(format!("leaf '{id}' weight must be in (0,1], got {weight}")));
        }
        let horizon = l.horizon.unwrap_or(spec.meta.forward_window);
        if horizon == 0 {
            return Err(Error::Tree(format!("leaf '{id}' horizon must be >= 1")));
        }
        leaves.insert(id.clone(), Leaf { stance: l.stance, weight, horizon });
```
loader 测试：weight 0.5/horizon 8 读取正确；weight 0.0 与 1.5、horizon 0 → Err；缺省 = 1.0/forward_window。

- [ ] **Step 2: 硬打分（`src/backtest/runner.rs` `eval_point`）**

把 `let fr = forward_return(primary, i, fw, trace.stance, costs);` 替换为：
```rust
    let fr = match tree.leaves.get(&trace.leaf) {
        Some(l) => forward_return(primary, i, l.horizon, trace.stance, costs).map(|f| ForwardResult {
            gross: f.gross * l.weight,
            net: f.net * l.weight,
            t1_executable: f.t1_executable,
        }),
        None => forward_return(primary, i, fw, trace.stance, costs), // 防御（validate 保证不可达）
    };
```
（`ForwardResult` 已在 runner 导入；若无补 `use crate::backtest::forward_return::ForwardResult;`。）

- [ ] **Step 3: 软打分（`src/backtest/soft.rs` `score_soft`）**

签名去掉 `fw` 参（不再使用全局窗口）：`pub fn score_soft(soft: &SoftTrace, tree: &Tree, primary: &[Bar], i: usize, costs: &CostModel) -> Option<SoftScore>`。循环改为：
```rust
    let mut max_h = 0usize;
    for (leaf_id, &p) in &soft.leaf_probs {
        let leaf = tree.leaves.get(leaf_id)?;
        let fr = forward_return(primary, i, leaf.horizon, leaf.stance, costs)?;
        let w = leaf.weight;
        expected_net += p * w * fr.net;
        exposure += p * w * match leaf.stance {
            Stance::Long => 1.0,
            Stance::Short => -1.0,
            Stance::Flat => 0.0,
        };
        if !matches!(leaf.stance, Stance::Flat) {
            engaged += p * w;
        }
        t1 |= fr.t1_executable;
        max_h = max_h.max(leaf.horizon);
    }
    // 净仓位口径：r 取分布内最大 horizon（最长腿；max_h 必属已过边界检查的集合 → 必 Some）
    let r = forward_return(primary, i, max_h, Stance::Long, costs)?.gross;
```
涟漪：`eval_point_soft` 去掉 `fw` 形参并更新对 `score_soft` 的调用；`run_soft` 中 `eval_point_soft(...)` 调用去掉 fw 实参（`fw` 变量仍用于 `SoftReport.forward_window`）；`backtest/soft.rs` 测试里所有 `score_soft(..., 2, &costs)` 形如 `(soft,&tree,&primary, i, fw, costs)` 的调用去掉 fw 实参。

- [ ] **Step 4: 测试（`src/backtest/soft.rs` `mod tests`）**

(a) 既有全部 soft 测试在去 fw 实参后**数值零变化**（叶子默认 horizon=meta.forward_window=2 与原 fw=2 一致）——验收标准。
(b) 新增已知值：
```rust
    #[test]
    fn leaf_weight_scales_soft_score() {
        const TREE_W: &str = r#"
meta: { name: t, forward_window: 2, stances: [long, flat] }
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "close > 0", goto: leaf_l, label: up } ]
    default: { goto: leaf_f, label: flat }
leaves:
  leaf_l: { stance: long, weight: 0.5 }
  leaf_f: { stance: flat }
"#;
        let tree = load_tree_str(TREE_W).unwrap();
        let primary = vec![
            bar("2024-01-02 14:45:00", 9.0, 9.5),
            bar("2024-01-02 15:00:00", 10.0, 10.2),
            bar("2024-01-03 09:45:00", 10.2, 11.0),
        ];
        let costs = CostModel { round_trip_bps: 10.0 };
        let mut lp = BTreeMap::new();
        lp.insert("leaf_l".to_string(), 1.0);
        let soft = SoftTrace { t: primary[0].time, leaf_probs: lp };
        let s = score_soft(&soft, &tree, &primary, 0, &costs).unwrap();
        // net_long = 0.099；w=0.5 → expected 0.0495；exposure/engaged = 0.5
        assert!((s.expected_net - 0.0495).abs() < 1e-9);
        assert!((s.exposure - 0.5).abs() < 1e-9);
        assert!((s.engaged - 0.5).abs() < 1e-9);
    }

    #[test]
    fn leaf_horizon_overrides_global_window() {
        const TREE_H: &str = r#"
meta: { name: t, forward_window: 16, stances: [long, flat] }
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "close > 0", goto: leaf_l, label: up } ]
    default: { goto: leaf_f, label: flat }
leaves:
  leaf_l: { stance: long, horizon: 2 }
  leaf_f: { stance: flat, horizon: 2 }
"#;
        let tree = load_tree_str(TREE_H).unwrap();
        let primary = vec![
            bar("2024-01-02 14:45:00", 9.0, 9.5),
            bar("2024-01-02 15:00:00", 10.0, 10.2),
            bar("2024-01-03 09:45:00", 10.2, 11.0),
        ];
        let costs = CostModel { round_trip_bps: 10.0 };
        let mut lp = BTreeMap::new();
        lp.insert("leaf_l".to_string(), 1.0);
        let soft = SoftTrace { t: primary[0].time, leaf_probs: lp };
        // 全局 fw=16 在 3 根 bar 下必越界；leaf horizon=2 仍可计分
        assert!(score_soft(&soft, &tree, &primary, 0, &costs).is_some());
    }
```

- [ ] **Step 5: 全量验证 + Commit**

Run: `cargo test` → 全绿；`cargo clippy --all-targets`（平铺）→ 无告警。
```bash
git add src/tree/schema.rs src/tree/loader.rs src/backtest/runner.rs src/backtest/soft.rs
git commit -m "feat(backtest,tree): per-leaf weight and horizon in hard/soft scoring (max-h position basis)" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: 文档同步 + example + e2e

**Files:**
- Modify: `docs/tree-yaml-schema.md`、`docs/dsl-reference.md`、`README.md`、`tests/e2e.rs`；Create: `examples/factor_tree.yaml`

- [ ] **Step 1: `examples/factor_tree.yaml`**

```yaml
meta:
  name: factor_demo
  forward_window: 16
  stances: [long, flat]

params: { ma_n: 20, mom_n: 5 }

factors:
  mom: "slope(ema(close, ma_n), mom_n)"
  above: "close > sma(close, ma_n)"

root: entry

nodes:
  entry:
    type: quant
    branches:
      - when: "above and mom > 0 and hour < 14"
        strength: "sigmoid(mom * 50)"
        goto: leaf_half
        label: trend_morning
    default: { goto: leaf_flat, label: none }

leaves:
  leaf_half: { stance: long, weight: 0.5, horizon: 32 }
  leaf_flat: { stance: flat }
```

- [ ] **Step 2: e2e（`tests/e2e.rs`）**

新增 `factor_tree_full_chain`：复用 `gen_primary_csv`/`gen_context_csv` 上升趋势 fixture，tree 用行内 YAML（同 factor_tree 但 `ma_n: 5`、`horizon: 4`、去 hour 条件以免 fixture 时间不匹配——或保留 `hour < 23` 恒真），`run`（硬）+ `run_soft` 各跑一遍，断言 `m.scored > 0` 且软模式 `engaged.count > 0`（weight 0.5 经全链路不炸）。

- [ ] **Step 3: 文档**

(a) `docs/tree-yaml-schema.md`：顶层结构加 `params`/`factors` 块（语法、有序引用规则、命名限制、加载期内联与未知名报错）；leaves 表加 `weight`/`horizon` 行（默认值/范围/语义——weight 进打分、horizon 覆盖全局；软模式 position 用分布内最大 horizon 的说明）。
(b) `docs/dsl-reference.md`：标识符表加 `hour`/`minute`/`dow`（含 dow=1 周一）；新节"命名因子与参数"（引用即内联展开、重复求值代价说明）。
(c) `README.md`：树构建小节补一句指向 factor_tree 示例。

- [ ] **Step 4: 验证 + Commit**

Run: `cargo test` → 全绿；`cargo clippy --all-targets` → 无告警；`cargo run -q -- backtest --tree examples/factor_tree.yaml --help` 不适用——改为用 loader 单测保证 example 可加载：在 `tree/loader.rs` 测试加 `loads_factor_tree_example`（`include_str!("../../examples/factor_tree.yaml")` → load ok）。
```bash
git add examples/factor_tree.yaml tests/e2e.rs docs/tree-yaml-schema.md docs/dsl-reference.md README.md src/tree/loader.rs
git commit -m "docs+test: factor/params example tree, e2e full chain, schema/DSL reference updates" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## 附录 A：Spec 覆盖对照（自检）
| Spec § | 实现于 |
|---|---|
| §3.1 substitute | Task 2 |
| §3.2 params/factors（保序/命名校验/未知名左移/strength 替换）| Task 2 |
| §3.3 weight/horizon（校验/硬/软/max_h position）| Task 3 |
| §3.4 hour/minute/dow | Task 1 |
| §5 测试（替换/loader/时间/打分已知值/退化/e2e/example 可加载）| Task 1-4 |
| 文档同步 | Task 4 |

## 附录 B：明确不在范围（YAGNI）
- factor 求值缓存；weight>1；params 扫描器；date/bar_index；按 factor 出报告。
