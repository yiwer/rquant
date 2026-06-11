# Schema 表达力补强（G 系列）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) 语法跟踪进度。

**Goal:** 按策略师补强报告，补齐树 YAML schema 的六项表达力缺口：计数原语（count/barssince）与数学函数（abs/min/max）、持仓极值状态量、weight 表达式（金字塔加减仓）、LLM 判定复用（judges）、因子运行时 memoize、aux 时间对齐语义文档化。

**Architecture:** 全部改动落在既有九层架构内：DSL 层（eval.rs 新增布尔序列求值与三个数学函数）、特征层（SimState 扩两个极值字段 + Context 挂 eval 缓存）、树层（LeafSpec.weight 多态、judges 块、因子包 Cached 槽）、模拟层（sim_step 维护极值）、评估器层（LLM 缓存 scope）。不改 Trace/SoftTrace 序列化格式——weight 表达式在各打分点就地求值（ctx 均在作用域内）。

**Tech Stack:** Rust（serde_yaml / chrono / tokio / approx），无新依赖。

---

## 报告项 → 阶段映射与设计决策

| 报告项 | 现状核对 | 方案 | 阶段 |
|---|---|---|---|
| 1. ref + count/barssince | `ref(series,k)` **已实现**（`src/dsl/eval.rs:214`，2026-06-11 窗口语义确认）；缺口收窄为 count/barssince | 新增私有 `eval_bool_series`（比较逐位求值、NaN 逐位弃权、尾对齐），`count(cond,n)`/`barssince(cond)` 返回 Scalar | Phase 1 |
| 7. abs/max/min | 不存在 | 标量函数三连；min/max 显式 NaN 传播（Rust `f64::max(NaN,x)=x` 会吃掉弃权，必须拦截） | Phase 1 |
| 2. 持仓极值 | SimState 只有 pos/entry_price/bars_held/unreal_pnl | `max_price_since_entry`/`min_price_since_entry` 状态量（空仓 NaN 弃权）；MFE/MAE 留给 DSL 自行推导 | Phase 2 |
| 3. 金字塔加减仓 | weight 是静态 f64 | **选 weight 表达式路线**（报告两条路之一）：`weight: "min(1, pos + 0.25)"`，引用 pos 即获得相对调仓语义；units 状态量方案被否——需要新增状态机与调仓指令集，复杂度远高于复用既有 target 覆盖语义 | Phase 3 |
| 4. LLM 判定复用 | 缓存键含 node_id（`client.rs:30`），labels 渲染进 prompt → 跨节点必然 miss | **选调用点覆盖 labels 路线**：顶层 `judges:` 块定义判定（prompt+inputs+label 列表），llm 节点 `judge: <名> + map: {label: goto}` 引用；共享 judge 的节点渲染出相同 prompt 且缓存 scope = `judge:<名>` → 每 bar 只打一次 LLM。被否的 `llm.gate1 == veto` 路线需要跨节点求值状态与求值顺序保证，侵入遍历器 | Phase 4 |
| 5. 因子 memoize | `substitute` 加载期内联展开，多处引用独立求值（`dsl-reference.md` "重复求值代价"） | AST 新增 `Expr::Cached(u32, Box<Expr>)`，loader 给每个因子体包一个缓存槽；`Context` 挂 `RefCell<HashMap<u32, Value>>`，同一决策点首算后命中。语义不变（纯函数），N 处引用 → 1 次求值 | Phase 5 |
| 6. aux 时间对齐 | 代码闸门 `time ≤ t` 正确（`context.rs:67`），但行时间戳纪律没写进文档 | 纯文档：as-of join 语义 + 时间戳=「数值完全确定时刻」纪律 + 高周期重采样必须打在周期末 | Phase 6 |

**依赖关系：** Phase 3 用到 Phase 1 的 `min`/`max`（金字塔 weight 表达式），其余阶段相互独立，可乱序执行。

**统一验证命令：** `cargo test`（全量）；逐任务用 `cargo test <filter>` 过滤。每阶段结束跑一次 `cargo clippy --all-targets -- -D warnings` 防回归。

---

## Phase 1 — DSL 计数原语与数学函数

**File Structure：** 全部落在既有文件，无新文件。
- `src/dsl/eval.rs` — `eval_call` 新增 abs/min/max 调度与 count/barssince 拦截；新增私有 `tail_align`、`eval_bool_series`、`series_cross`
- `src/tree/loader.rs` — `RESERVED_FNS` 追加 5 个名字
- `docs/dsl-reference.md` — 函数表 + 「事件计数」示例节

### Task 1: abs / min / max 标量函数

**Files:**
- Modify: `src/dsl/eval.rs`（`eval_call` 的 `"sigmoid"` 臂之后、`_ =>` 之前）
- Modify: `src/tree/loader.rs:10-28`（RESERVED_FNS）

- [ ] **Step 1: 写失败测试**（`src/dsl/eval.rs` tests 模块末尾）

```rust
    #[test]
    fn abs_min_max_eval() {
        let ctx = ctx_from_closes(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        let f = |src: &str| eval(&parse_str(src).unwrap(), &ctx).unwrap();
        assert_eq!(f("abs(0 - 3) == 3"), Value::Bool(true));
        assert_eq!(f("abs(close - 10) == 5"), Value::Bool(true));
        assert_eq!(f("max(2, 3) == 3"), Value::Bool(true));
        assert_eq!(f("min(2, 3) == 2"), Value::Bool(true));
        // 序列参数经 as_scalar 归约取末元素
        assert_eq!(f("max(close, 4.5) == 5"), Value::Bool(true));
        // NaN 传播：预热期 sma 为 NaN → max/min 必须返回 NaN（弃权），不得吃掉 NaN
        assert_eq!(f("max(sma(close, 10), 1) > 0"), Value::Bool(false));
        assert_eq!(f("min(sma(close, 10), 1) < 99"), Value::Bool(false));
        // 错参数量
        assert!(eval(&parse_str("abs(1, 2)").unwrap(), &ctx).is_err());
        assert!(eval(&parse_str("max(1)").unwrap(), &ctx).is_err());
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --lib dsl::eval::tests::abs_min_max_eval`
Expected: FAIL，报 `unknown function: abs`

- [ ] **Step 3: 最小实现**（`eval_call` 中 `"sigmoid"` 臂后插入）

```rust
        "abs" => {
            need(&vals, 1, name)?;
            Ok(Value::Scalar(as_scalar(&vals[0])?.abs()))
        }
        "max" | "min" => {
            need(&vals, 2, name)?;
            let (a, b) = (as_scalar(&vals[0])?, as_scalar(&vals[1])?);
            // f64::max(NaN, x) 返回 x，会吞掉预热弃权 → 显式传播 NaN
            let v = if a.is_nan() || b.is_nan() {
                f64::NAN
            } else if name == "max" {
                a.max(b)
            } else {
                a.min(b)
            };
            Ok(Value::Scalar(v))
        }
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --lib dsl::eval::tests::abs_min_max_eval`
Expected: PASS

- [ ] **Step 5: 保留字 + 加载期测试**（`src/tree/loader.rs`）

RESERVED_FNS 改为（注意数组长度标注同步 17→20）：

```rust
const RESERVED_FNS: [&str; 20] = [
    "sma", "ema", "wma", "rsi", "atr", "slope", "ref", "highest", "lowest",
    "crossover", "crossunder", "macd_line", "macd_signal", "macd_hist",
    "std", "sigmoid", "auto", "abs", "max", "min",
];
```

loader.rs tests 模块加测试：

```rust
    #[test]
    fn math_fns_are_reserved() {
        let yaml = |factor: &str| format!(r#"
meta: {{ name: t, forward_window: 3, stances: [long, flat] }}
factors:
  {factor}: "close - 1"
root: a
nodes:
  a:
    type: quant
    branches: [ {{ when: "{factor} > 0", goto: leaf_l, label: up }} ]
    default: {{ goto: leaf_f, label: flat }}
leaves:
  leaf_l: {{ stance: long }}
  leaf_f: {{ stance: flat }}
"#);
        assert!(load_tree_str(&yaml("abs")).is_err());
        assert!(load_tree_str(&yaml("max")).is_err());
        assert!(load_tree_str(&yaml("min")).is_err());
    }
```

Run: `cargo test --lib tree::loader::tests::math_fns_are_reserved`
Expected: PASS

- [ ] **Step 6: 提交**

```bash
git add src/dsl/eval.rs src/tree/loader.rs
git commit -m "feat(dsl): abs/min/max scalar fns with explicit NaN propagation"
```

### Task 2: 布尔序列求值 + count(cond, n)

**语义定义（写代码前先固定）：**
- `count(cond, n)`：对 cond 做**逐位**布尔求值得到 `Vec<bool>`，统计**末 n 位**中 true 的个数，返回 Scalar。布尔序列长度 < n 或 n=0 → NaN（弃权，与 highest 窗口不足同纪律）。
- 逐位求值规则：比较两侧先 `eval` 成 Value，**尾对齐**（两序列取右端公共长度；标量广播）；任一侧该位为 NaN → 该位 false（NaN 弃权逐位生效）；`and`=逐位与、`or`=逐位或、`not`=逐位取反；其余表达式形态 → Err。
- cond 是**原始 AST** 传入（不能先经 `eval_call` 的统一参数求值——那会把比较归约成单个 Bool），因此在 `eval_call` 函数体**最顶部**拦截。

**Files:**
- Modify: `src/dsl/eval.rs`
- Modify: `src/tree/loader.rs`（RESERVED_FNS +"count"，长度 20→21）

- [ ] **Step 1: 写失败测试**

```rust
    #[test]
    fn count_over_bool_series() {
        let ctx = ctx_from_closes(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        let f = |src: &str| eval(&parse_str(src).unwrap(), &ctx).unwrap();
        // 末 3 位 [3>3,4>3,5>3] = [F,T,T] → 2
        assert_eq!(f("count(close > 3, 3) == 2"), Value::Bool(true));
        // 预热 NaN 逐位弃权：sma(close,3)=[N,N,2,3,4]，close>sma 逐位 [F,F,T,T,T] → 3
        assert_eq!(f("count(close > sma(close,3), 5) == 3"), Value::Bool(true));
        // and 逐位组合：close>2 且 close<5 → [F,F,T,T,F] 末 5 位 → 2
        assert_eq!(f("count(close > 2 and close < 5, 5) == 2"), Value::Bool(true));
        // not 逐位
        assert_eq!(f("count(not (close > 3), 5) == 3"), Value::Bool(true));
        // 尾对齐：ref(close,1)=[1,2,3,4] 与标量 2 广播 → [F,F,T,T]，n=4 → 2
        assert_eq!(f("count(ref(close,1) > 2, 4) == 2"), Value::Bool(true));
        // 序列不足 n → NaN 弃权（所有比较 false）
        assert_eq!(f("count(close > 0, 99) > 0"), Value::Bool(false));
        assert_eq!(f("count(close > 0, 99) == count(close > 0, 99)"), Value::Bool(false));
        // 条件不是布尔表达式 → Err
        assert!(eval(&parse_str("count(close, 3)").unwrap(), &ctx).is_err());
        // 错参数量 → Err
        assert!(eval(&parse_str("count(close > 0)").unwrap(), &ctx).is_err());
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --lib dsl::eval::tests::count_over_bool_series`
Expected: FAIL，报 `unknown function: count`

- [ ] **Step 3: 实现**（`src/dsl/eval.rs`，放在 `eval_call` 之前）

```rust
/// 把两个 Value 调成等长数值序列对（尾对齐）：双 Series 取右端公共长度；Scalar 广播；Bool → Err。
fn tail_align(a: &Value, b: &Value) -> Result<(Vec<f64>, Vec<f64>)> {
    match (a, b) {
        (Value::Series(x), Value::Series(y)) => {
            let m = x.len().min(y.len());
            Ok((x[x.len() - m..].to_vec(), y[y.len() - m..].to_vec()))
        }
        (Value::Series(x), Value::Scalar(s)) => Ok((x.clone(), vec![*s; x.len()])),
        (Value::Scalar(s), Value::Series(y)) => Ok((vec![*s; y.len()], y.clone())),
        (Value::Scalar(p), Value::Scalar(q)) => Ok((vec![*p], vec![*q])),
        _ => Err(Error::Eval("expected numeric operands in condition".into())),
    }
}

/// 布尔序列求值（count/barssince 的条件臂）：比较 → 逐位（任一侧 NaN → 该位 false），
/// and/or/not → 逐位组合（尾对齐到公共长度），其余表达式形态 → Err。
fn eval_bool_series(expr: &Expr, ctx: &Context) -> Result<Vec<bool>> {
    match expr {
        Expr::Binary(op, l, r) => match op {
            BinaryOp::And | BinaryOp::Or => {
                let a = eval_bool_series(l, ctx)?;
                let b = eval_bool_series(r, ctx)?;
                let m = a.len().min(b.len());
                let (a, b) = (&a[a.len() - m..], &b[b.len() - m..]);
                Ok(a.iter()
                    .zip(b)
                    .map(|(&x, &y)| if matches!(op, BinaryOp::And) { x && y } else { x || y })
                    .collect())
            }
            BinaryOp::Gt | BinaryOp::Lt | BinaryOp::Ge | BinaryOp::Le | BinaryOp::Eq | BinaryOp::Ne => {
                let (a, b) = tail_align(&eval(l, ctx)?, &eval(r, ctx)?)?;
                Ok(a.iter()
                    .zip(&b)
                    .map(|(&x, &y)| {
                        if x.is_nan() || y.is_nan() {
                            return false;
                        }
                        match op {
                            BinaryOp::Gt => x > y,
                            BinaryOp::Lt => x < y,
                            BinaryOp::Ge => x >= y,
                            BinaryOp::Le => x <= y,
                            BinaryOp::Eq => x == y,
                            BinaryOp::Ne => x != y,
                            _ => unreachable!(),
                        }
                    })
                    .collect())
            }
            _ => Err(Error::Eval(
                "count/barssince: condition must be a comparison or boolean combination".into(),
            )),
        },
        Expr::Unary(UnaryOp::Not, e) => Ok(eval_bool_series(e, ctx)?.into_iter().map(|x| !x).collect()),
        _ => Err(Error::Eval(
            "count/barssince: condition must be a comparison or boolean combination".into(),
        )),
    }
}
```

`eval_call` 函数体**最顶部**（`let vals` 之前）插入拦截：

```rust
    // count/barssince 的条件参数必须按原始 AST 逐位求值，不能走统一参数求值（那会归约成单 Bool）
    match name {
        "count" => {
            if args.len() != 2 {
                return Err(Error::Eval(format!("count expects 2 args, got {}", args.len())));
            }
            let cond = eval_bool_series(&args[0], ctx)?;
            let n = as_usize(&eval(&args[1], ctx)?)?;
            if n == 0 || cond.len() < n {
                return Ok(Value::Scalar(f64::NAN)); // 窗口不足 → 弃权
            }
            return Ok(Value::Scalar(cond[cond.len() - n..].iter().filter(|&&b| b).count() as f64));
        }
        _ => {}
    }
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --lib dsl::eval::tests::count_over_bool_series`
Expected: PASS

- [ ] **Step 5: 模糊路径回归测试**（count 返回 Scalar，`fuzzy_cmp` 经 `as_scalar` 自动可用）

```rust
    #[test]
    fn count_works_in_fuzzy_strength() {
        let ctx = ctx_from_closes(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        // count(close>3,3)=2，两侧相等 → 模糊真值 0.5
        let v = eval_fuzzy(&parse_str("count(close > 3, 3) >= 2").unwrap(), &ctx, 0.02).unwrap();
        assert!((v - 0.5).abs() < 1e-9);
    }
```

Run: `cargo test --lib dsl::eval::tests::count_works_in_fuzzy_strength`
Expected: PASS

- [ ] **Step 6: 保留字**：`RESERVED_FNS` 追加 `"count"`（长度 20→21），`math_fns_are_reserved` 测试中追加 `assert!(load_tree_str(&yaml("count")).is_err());`

Run: `cargo test --lib tree::loader`
Expected: PASS

- [ ] **Step 7: 提交**

```bash
git add src/dsl/eval.rs src/tree/loader.rs
git commit -m "feat(dsl): count(cond,n) with element-wise bool-series evaluation"
```

### Task 3: barssince(cond)

**语义：** 距离 cond 最近一次为 true 过去了几根 bar（当前 bar 为 0）。可见窗口内从未 true → NaN 弃权。

**Files:**
- Modify: `src/dsl/eval.rs`（`eval_call` 顶部拦截块）
- Modify: `src/tree/loader.rs`（RESERVED_FNS +"barssince"，21→22）

- [ ] **Step 1: 写失败测试**

```rust
    #[test]
    fn barssince_last_true_distance() {
        let ctx = ctx_from_closes(&[1.0, 5.0, 2.0, 3.0, 4.0]);
        let f = |src: &str| eval(&parse_str(src).unwrap(), &ctx).unwrap();
        // close>4 仅在 idx1（5.0）为 true → 距末位 3 根
        assert_eq!(f("barssince(close > 4) == 3"), Value::Bool(true));
        // 当前 bar 即 true → 0
        assert_eq!(f("barssince(close > 3.5) == 0"), Value::Bool(true));
        // 从未 true → NaN 弃权
        assert_eq!(f("barssince(close > 99) >= 0"), Value::Bool(false));
        // 非布尔条件 / 错参数量 → Err
        assert!(eval(&parse_str("barssince(close)").unwrap(), &ctx).is_err());
        assert!(eval(&parse_str("barssince(close > 0, 1)").unwrap(), &ctx).is_err());
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --lib dsl::eval::tests::barssince_last_true_distance`
Expected: FAIL，报 `unknown function: barssince`

- [ ] **Step 3: 实现**（拦截块 `"count"` 臂后加）

```rust
        "barssince" => {
            if args.len() != 1 {
                return Err(Error::Eval(format!("barssince expects 1 arg, got {}", args.len())));
            }
            let cond = eval_bool_series(&args[0], ctx)?;
            return Ok(Value::Scalar(match cond.iter().rposition(|&b| b) {
                Some(j) => (cond.len() - 1 - j) as f64,
                None => f64::NAN, // 可见窗口内从未触发 → 弃权
            }));
        }
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --lib dsl::eval::tests::barssince_last_true_distance`
Expected: PASS

- [ ] **Step 5: 保留字**：`RESERVED_FNS` 追加 `"barssince"`（22），reserved 测试追加一行。

- [ ] **Step 6: 提交**

```bash
git add src/dsl/eval.rs src/tree/loader.rs
git commit -m "feat(dsl): barssince(cond) bars-since-last-true with abstention"
```

### Task 4: crossover/crossunder 进入条件序列

**语义：** `count(crossover(a,b), n)` 统计窗口内上穿**事件**次数（Brooks H2/L2 计数的基石）。逐位定义：位 j 为 true 当且仅当 `a[j-1]<=b[j-1] && a[j]>b[j]`（crossunder 镜像），j=0 或任一参与值 NaN → false。

**Files:**
- Modify: `src/dsl/eval.rs`（`eval_bool_series` 新增 `Expr::Call` 臂 + 辅助函数）

- [ ] **Step 1: 写失败测试**

```rust
    #[test]
    fn count_crossover_events() {
        // closes 围绕 2.5 来回穿越：上穿发生在 idx 1、3、5
        let ctx = ctx_from_closes(&[1.0, 3.0, 2.0, 3.0, 2.0, 3.0]);
        let f = |src: &str| eval(&parse_str(src).unwrap(), &ctx).unwrap();
        assert_eq!(f("count(crossover(close, 2.5), 6) == 3"), Value::Bool(true));
        assert_eq!(f("count(crossunder(close, 2.5), 6) == 2"), Value::Bool(true));
        // 与逐位 and 组合
        assert_eq!(f("count(crossover(close, 2.5) and close > 0, 6) == 3"), Value::Bool(true));
        // barssince + crossover：最近一次上穿在 idx5（当前 bar）→ 0
        assert_eq!(f("barssince(crossover(close, 2.5)) == 0"), Value::Bool(true));
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --lib dsl::eval::tests::count_crossover_events`
Expected: FAIL，报 `condition must be a comparison or boolean combination`

- [ ] **Step 3: 实现**：`eval_bool_series` 的 match 中、`Expr::Unary` 臂之后加：

```rust
        Expr::Call(name, args) if name == "crossover" || name == "crossunder" => {
            if args.len() != 2 {
                return Err(Error::Eval(format!("{name} expects 2 args, got {}", args.len())));
            }
            let (a, b) = tail_align(&eval(&args[0], ctx)?, &eval(&args[1], ctx)?)?;
            let over = name == "crossover";
            Ok((0..a.len())
                .map(|j| {
                    if j == 0 {
                        return false;
                    }
                    let (p0, q0, p1, q1) = (a[j - 1], b[j - 1], a[j], b[j]);
                    if p0.is_nan() || q0.is_nan() || p1.is_nan() || q1.is_nan() {
                        return false;
                    }
                    if over { p0 <= q0 && p1 > q1 } else { p0 >= q0 && p1 < q1 }
                })
                .collect())
        }
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --lib dsl::eval::tests::count_crossover_events`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add src/dsl/eval.rs
git commit -m "feat(dsl): crossover/crossunder as element-wise event series in count/barssince"
```

### Task 5: Phase 1 文档

**Files:**
- Modify: `docs/dsl-reference.md`

- [ ] **Step 1: 标量函数表追加 5 行**（`sigmoid` 行后）：

```markdown
| `abs(x)` | x: Scalar | — | 绝对值 | `abs(close - entry_price)` |
| `max(a, b)` | a, b: Scalar | 任一 NaN → NaN | 较大值；**显式 NaN 传播**（不吃弃权） | `max(pos, 0.25)` |
| `min(a, b)` | a, b: Scalar | 任一 NaN → NaN | 较小值；NaN 传播同 max | `min(1, pos + 0.25)` |
| `count(cond, n)` | cond: 布尔表达式, n: int≥1 | 序列 < n → NaN | 末 n 位中 cond 为 true 的个数；cond **逐位**求值（见下节） | `count(close > ema(close,20), 10)` |
| `barssince(cond)` | cond: 布尔表达式 | 从未 true → NaN | 距最近一次 cond=true 的 bar 数（当前 bar=0） | `barssince(crossover(close, sma(close,20)))` |
```

- [ ] **Step 2: 「模糊求值语义」节之前插入新节**：

```markdown
---

## 事件计数与逐位条件（`count` / `barssince`）

`count`/`barssince` 的条件参数不走「Series → 取末元素」归约，而是**逐位**求值成布尔序列：

- 比较（`> < >= <= == !=`）：两侧序列**尾对齐**（取右端公共长度；标量广播），逐位比较；任一侧该位 NaN → 该位 false（NaN 弃权逐位生效）。
- `and` / `or` / `not`：逐位组合。
- `crossover(a, b)` / `crossunder(a, b)`：逐位事件序列——位 j 为 true 当且仅当前一位未越线且本位越线；首位与含 NaN 位恒 false。
- 其余表达式形态（裸序列、算术结果）作为条件 → 求值报错。

窗口纪律：布尔序列长度 < n（或 `barssince` 从未触发）→ 返回 NaN，外层比较自动弃权走 default。

### 价格行为惯用法

```yaml
# 趋势强度：最近 10 根中至少 8 根收于 EMA20 上方
when: "count(close > ema(close,20), 10) >= 8"
# H2 计数近似：20 根内第 2 次上穿 EMA8
when: "count(crossover(close, ema(close,8)), 20) == 2"
# 突破后回踩不破：距突破 ≤5 根且未跌破前低
when: "barssince(close > highest(ref(high,1), 20)) <= 5 and low > lowest(ref(low,1), 10)"
# inside bar（无需 count，普通索引即可）
when: "high < high[-1] and low > low[-1]"
```
```

- [ ] **Step 3: 验证 + 提交**

Run: `cargo test --lib dsl`
Expected: 全 PASS

```bash
git add docs/dsl-reference.md
git commit -m "docs(dsl): count/barssince/abs/min/max reference and price-action idioms"
```

---

## Phase 2 — 持仓极值状态量 max/min_price_since_entry

**File Structure：**
- `src/features/context.rs` — SimState 扩 2 字段
- `src/backtest/sim.rs` — SimAccount 扩 2 字段；`sim_step` 签名加 `high`/`low`；极值维护；run_sim 注入
- `src/dsl/eval.rs` — Ident 臂暴露 2 个标识符
- `src/tree/loader.rs` — RESERVED_IDENTS 12→14
- `docs/dsl-reference.md` — 标识符表

**语义定义：** 入场以来（含入场执行 bar）所见 `high` 的最大值 / `low` 的最小值。空仓为 NaN（比较弃权，纪律同 `entry_price`）。翻向时重置为新回合起点。MFE/MAE 不另设标识符，DSL 自行推导：`(max_price_since_entry / entry_price - 1)`。

### Task 6: SimState/SimAccount 字段 + sim_step 极值维护

- [ ] **Step 1: 写失败测试**（`src/backtest/sim.rs` tests 模块）

```rust
    #[test]
    fn extremes_track_high_low_since_entry() {
        let mut acc = SimAccount::default();
        assert!(acc.max_price_since_entry.is_nan());
        // 入场执行 bar：high=10.5 low=9.9
        sim_step(&mut acc, 10.0, 10.0, 10.5, 9.9, 10.2, t("2024-01-02 10:00:00"), 1.0, 0.0, "tree");
        assert_relative_eq!(acc.max_price_since_entry, 10.5);
        assert_relative_eq!(acc.min_price_since_entry, 9.9);
        // 持仓 bar：high=11.0 创新高，low=10.2 不创新低
        sim_step(&mut acc, 10.2, 10.4, 11.0, 10.2, 10.8, t("2024-01-03 10:00:00"), 1.0, 0.0, "tree");
        assert_relative_eq!(acc.max_price_since_entry, 11.0);
        assert_relative_eq!(acc.min_price_since_entry, 9.9);
        // 平仓 → 极值重置 NaN
        sim_step(&mut acc, 10.8, 10.9, 10.9, 10.5, 10.6, t("2024-01-04 10:00:00"), 0.0, 0.0, "tree");
        assert!(acc.max_price_since_entry.is_nan());
        assert!(acc.min_price_since_entry.is_nan());
    }

    #[test]
    fn extremes_reset_on_flip() {
        let mut acc = SimAccount::default();
        sim_step(&mut acc, 10.0, 10.0, 10.5, 9.9, 10.0, t("2024-01-02 10:00:00"), 1.0, 0.0, "tree");
        // 翻向：新回合极值只来自当前执行 bar（10.2/9.8），不继承旧回合的 10.5/9.9
        sim_step(&mut acc, 10.0, 10.0, 10.2, 9.8, 10.0, t("2024-01-03 10:00:00"), -1.0, 0.0, "tree");
        assert_relative_eq!(acc.max_price_since_entry, 10.2);
        assert_relative_eq!(acc.min_price_since_entry, 9.8);
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --lib backtest::sim::tests::extremes`
Expected: 编译错（`sim_step` 参数个数 / 字段不存在）——TDD 下编译失败即红灯

- [ ] **Step 3: 实现**

`src/features/context.rs` SimState：

```rust
pub struct SimState {
    pub pos: f64,
    pub entry_price: f64,
    pub bars_held: usize,
    pub unreal_pnl: f64,
    pub max_price_since_entry: f64,
    pub min_price_since_entry: f64,
}

impl Default for SimState {
    fn default() -> Self {
        Self {
            pos: 0.0,
            entry_price: f64::NAN,
            bars_held: 0,
            unreal_pnl: 0.0,
            max_price_since_entry: f64::NAN,
            min_price_since_entry: f64::NAN,
        }
    }
}
```

`src/backtest/sim.rs` SimAccount 加字段（Default 中初始化 `f64::NAN`）：

```rust
    pub max_price_since_entry: f64,
    pub min_price_since_entry: f64,
```

`sim_step` 签名（`#[allow(clippy::too_many_arguments)]` 已有）：

```rust
pub fn sim_step(
    acc: &mut SimAccount,
    prev_close: f64,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    exec_t: NaiveDateTime,
    target: f64,
    rate: f64,
    reason: &str,
) -> Option<RoundTrip> {
```

开新回合臂（`acc.last_increase_date = Some(exec_t.date());` 所在的「自 flat 开仓 / 翻向开新」分支）追加重置，使末尾统一块从本 bar 重新初始化：

```rust
                acc.max_price_since_entry = f64::NAN;
                acc.min_price_since_entry = f64::NAN;
```

函数末尾（`acc.peak_nav = ...` 之前）追加极值维护：

```rust
    // 持仓极值（含执行 bar 本身的 high/low）；空仓重置 NaN
    if acc.pos.abs() > EPS {
        if acc.max_price_since_entry.is_nan() {
            acc.max_price_since_entry = high;
            acc.min_price_since_entry = low;
        } else {
            acc.max_price_since_entry = acc.max_price_since_entry.max(high);
            acc.min_price_since_entry = acc.min_price_since_entry.min(low);
        }
    } else {
        acc.max_price_since_entry = f64::NAN;
        acc.min_price_since_entry = f64::NAN;
    }
```

`finalize` 中 `acc.bars_held = 0;` 后追加：

```rust
    acc.max_price_since_entry = f64::NAN;
    acc.min_price_since_entry = f64::NAN;
```

- [ ] **Step 4: 机械更新既有调用点（保持全 crate 编译）**
  1. `run_sim` 主循环：`sim_step` 调用传 `primary[i + 1].high, primary[i + 1].low`（插在 `open_next` 与 `close_next` 之间）。
  2. sim.rs tests 中全部 `sim_step(...)` 调用在 `open` 后插入 `high, low` 两个实参——无 high/low 断言的旧测试取 `high = open.max(close)`、`low = open.min(close)`。
  3. SimState 字面量构造点补字段（否则编译失败）：
     - `run_sim` 注入处（sim.rs:283 一带）**本任务先填占位** `max_price_since_entry: f64::NAN, min_price_since_entry: f64::NAN,`（真正接到 acc 是 Task 8 的红灯靶子）；
     - `src/dsl/eval.rs` 测试 `sim_state_identifiers` 的字面量末尾加 `..SimState::default()`（保留原 4 个显式字段）。

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test --lib backtest::sim`
Expected: 全 PASS（含旧 golden）

- [ ] **Step 6: 提交**

```bash
git add src/features/context.rs src/backtest/sim.rs
git commit -m "feat(sim): track max/min price since entry in SimAccount and sim_step"
```

### Task 7: DSL 暴露 + 保留字

- [ ] **Step 1: 写失败测试**（`src/dsl/eval.rs` tests，扩展既有 `sim_state_identifiers`）

```rust
    #[test]
    fn position_extreme_identifiers() {
        let mut ctx = ctx_from_closes(&[10.4]);
        let f = |src: &str, c: &Context| eval(&parse_str(src).unwrap(), c).unwrap();
        // 空仓默认 NaN → 弃权
        assert_eq!(f("max_price_since_entry > 0", &ctx), Value::Bool(false));
        assert_eq!(f("min_price_since_entry > 0", &ctx), Value::Bool(false));
        // 注入后可见：Chandelier 形态条件可表达
        ctx.sim = crate::features::context::SimState {
            pos: 1.0,
            entry_price: 10.0,
            bars_held: 3,
            unreal_pnl: 0.04,
            max_price_since_entry: 11.0,
            min_price_since_entry: 9.9,
        };
        assert_eq!(f("max_price_since_entry == 11", &ctx), Value::Bool(true));
        assert_eq!(f("close < max_price_since_entry - 0.5", &ctx), Value::Bool(true));
        // MFE 推导：(11/10 - 1) = 0.1
        assert_eq!(f("max_price_since_entry / entry_price - 1 > 0.09", &ctx), Value::Bool(true));
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --lib dsl::eval::tests::position_extreme_identifiers`
Expected: FAIL，报 `unknown identifier: max_price_since_entry`

- [ ] **Step 3: 实现**：`eval` 的 `Ident` 臂 `"unreal_pnl"` 后加：

```rust
            "max_price_since_entry" => Ok(Value::Scalar(ctx.sim.max_price_since_entry)),
            "min_price_since_entry" => Ok(Value::Scalar(ctx.sim.min_price_since_entry)),
```

`src/tree/loader.rs` RESERVED_IDENTS（12→14）：

```rust
const RESERVED_IDENTS: [&str; 14] = [
    "close", "open", "high", "low", "volume", "hour", "minute", "dow",
    "pos", "entry_price", "bars_held", "unreal_pnl",
    "max_price_since_entry", "min_price_since_entry",
];
```

loader 既有测试 `sim_identifiers_are_reserved` 追加两行：

```rust
        assert!(load_tree_str(&yaml("max_price_since_entry")).is_err());
        assert!(load_tree_str(&yaml("min_price_since_entry")).is_err());
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --lib dsl::eval::tests::position_extreme_identifiers tree::loader::tests::sim_identifiers_are_reserved`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add src/dsl/eval.rs src/tree/loader.rs
git commit -m "feat(dsl): expose max/min_price_since_entry sim identifiers"
```

### Task 8: run_sim 注入 + Chandelier 集成测试 + 文档

- [ ] **Step 1: 写失败测试**（`src/backtest/sim.rs` tests）

```rust
    /// Chandelier 式跟踪止损树：回撤超 2% 即离场。
    const CHANDELIER_TREE: &str = r#"
meta: { name: chandelier, forward_window: 1, stances: [long, flat] }
root: gate
nodes:
  gate:
    type: quant
    branches:
      - when: "pos == 0 and close > 0"
        goto: leaf_long
        label: enter
      - when: "pos > 0 and close < max_price_since_entry * 0.98"
        goto: leaf_flat
        label: chandelier_exit
      - when: "pos > 0"
        goto: leaf_long
        label: hold
    default: { goto: leaf_flat, label: idle }
leaves:
  leaf_long: { stance: long }
  leaf_flat: { stance: flat }
"#;

    /// 冲高后回撤：b1 执行入场（high 10.6），b2 收 10.3 < 10.6*0.98=10.388 → 决策离场，b3 执行。
    fn write_chandelier_bars_csv() -> tempfile::NamedTempFile {
        use std::io::Write as _;
        let csv = "\
time,open,high,low,close,volume
2024-01-02 09:45:00,10.0,10.1,9.9,10.0,1000
2024-01-02 10:00:00,10.0,10.6,9.9,10.5,1000
2024-01-03 09:45:00,10.5,10.55,10.2,10.3,1000
2024-01-03 10:00:00,10.3,10.35,10.1,10.2,1000
";
        let mut f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        write!(f, "{csv}").unwrap();
        f.flush().unwrap();
        f
    }

    #[tokio::test]
    async fn run_sim_chandelier_exit_fires() {
        let tree_f = write_tree_yaml(CHANDELIER_TREE);
        let bars_f = write_chandelier_bars_csv();
        let out_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        let cfg = make_cfg(&tree_f, &bars_f, &out_f, None);
        let report = run_sim(&cfg, &LlmEvaluator::Disabled, false).await.unwrap();
        assert_eq!(report.n_round_trips, 1);
        // 树内 chandelier 分支驱动的离场，reason 是 "tree"（风控块离场才是 stop/tp）
        assert_eq!(report.trades[0].reason, "tree");
        assert_relative_eq!(report.trades[0].exit_px, 10.3); // b3 开盘执行
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --lib backtest::sim::tests::run_sim_chandelier_exit_fires`
Expected: FAIL——Task 6 的注入占位恒为 NaN → chandelier 分支恒弃权 → 持仓拖到期末清算，`trades[0].reason` 是 `"end"` 而非 `"tree"`（断言失败）

- [ ] **Step 3: 实现**：`run_sim` 的 SimState 注入（sim.rs:283 一带）补两字段：

```rust
        ctx.sim = SimState {
            pos: acc.pos,
            entry_price: acc.entry_price,
            bars_held: acc.bars_held,
            unreal_pnl,
            max_price_since_entry: acc.max_price_since_entry,
            min_price_since_entry: acc.min_price_since_entry,
        };
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --lib backtest::sim`
Expected: 全 PASS

- [ ] **Step 5: 文档**：`docs/dsl-reference.md` 标识符表 `unreal_pnl` 行后加两行：

```markdown
| `max_price_since_entry` | `Context.sim`（sim 模式） | 入场以来（含入场执行 bar）最高 `high`（标量）；空仓/非 sim 为 NaN → 比较弃权。Chandelier：`close < max_price_since_entry - 3*atr(22)` |
| `min_price_since_entry` | `Context.sim`（sim 模式） | 入场以来最低 `low`（标量）；空仓 NaN。MFE/MAE 自行推导：`max_price_since_entry/entry_price - 1` |
```

- [ ] **Step 6: 提交**

```bash
git add src/backtest/sim.rs docs/dsl-reference.md
git commit -m "feat(sim): inject position extremes into SimState; chandelier exit integration test"
```

---

## Phase 3 — weight 表达式（金字塔加减仓）

**File Structure：**
- `src/tree/schema.rs` — `LeafSpec.weight: Option<serde_yaml::Value>`（数值或字符串）
- `src/tree/loader.rs` — `Weight` 枚举（Const/Expr）、`Leaf::weight_at(&Context)`
- 消费点：`src/backtest/runner.rs:65-72`、`src/backtest/soft.rs`（score_soft 加 ctx 参数）、`src/backtest/sim.rs`（tree_target）、`src/backtest/portfolio.rs`（score_symbol）
- `docs/tree-yaml-schema.md` — weight 字段说明 + 金字塔示例

**语义定义：**
- `weight: 0.5`（数值）→ 行为完全不变，加载期校验 (0,1]。
- `weight: "min(1, pos + 0.25)"`（字符串）→ 加载期 parse + params/factors 内联 + 未知标识符左移报错；**决策时**对当时 ctx 求值，NaN → 0（弃权 = 不持仓）、结果 clamp 到 [0,1]。
- 相对调仓语义来自引用 `pos`：target 仍是「覆盖」语义，但 `pos + 0.25` 即增量、`pos` 即维持、`pos - 0.25` 即部分减仓。
- 打分模式（非 sim）里 sim 状态恒为默认（pos=0），weight 表达式照常求值——文档明确这点。

### Task 9: schema + loader + weight_at

- [ ] **Step 1: 写失败测试**（`src/tree/loader.rs` tests）

```rust
    /// 构建最小 Context 供 weight_at 测试（loader tests 此前不依赖 Context）。
    fn mini_ctx() -> crate::features::context::Context {
        use crate::data::bar::{Bar, Window};
        let t = chrono::NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        let bars = vec![Bar { time: t, open: 10.0, high: 10.0, low: 10.0, close: 10.0, volume: 1.0 }];
        crate::features::context::Context {
            t,
            primary: Window { bars: bars.clone() },
            context: Window { bars },
            news: None,
            aux: std::collections::BTreeMap::new(),
            sim: crate::features::context::SimState::default(),
        }
    }

    #[test]
    fn leaf_weight_expression_loads_and_evaluates() {
        let ok = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
params: { unit: 0.25 }
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "close > 0", goto: leaf_l, label: up } ]
    default: { goto: leaf_f, label: flat }
leaves:
  leaf_l: { stance: long, weight: "min(1, pos + unit)" }
  leaf_f: { stance: flat }
"#;
        let tree = load_tree_str(ok).unwrap();
        let l = tree.leaves.get("leaf_l").unwrap();
        assert!(matches!(l.weight, Weight::Expr(_)));
        // 非 sim ctx：pos=0 → 0.25
        let mut ctx = mini_ctx();
        assert!((l.weight_at(&ctx) - 0.25).abs() < 1e-12);
        // sim 注入 pos=0.5 → 0.75
        ctx.sim.pos = 0.5;
        assert!((l.weight_at(&ctx) - 0.75).abs() < 1e-12);
        // clamp：pos=0.9 → min(1, 1.15) = 1.0
        ctx.sim.pos = 0.9;
        assert!((l.weight_at(&ctx) - 1.0).abs() < 1e-12);
        // NaN → 0（弃权）：表达式引用空仓 entry_price
        let nan_w = ok.replace("min(1, pos + unit)", "entry_price / 100");
        let tree2 = load_tree_str(&nan_w).unwrap();
        assert!((tree2.leaves["leaf_l"].weight_at(&mini_ctx()) - 0.0).abs() < 1e-12);
        // 数值路径不变：Const + 旧校验
        let tree3 = load_tree_str(&ok.replace(r#""min(1, pos + unit)""#, "0.5")).unwrap();
        assert!(matches!(tree3.leaves["leaf_l"].weight, Weight::Const(w) if (w - 0.5).abs() < 1e-12));
        // 未知标识符 / 坏语法 → 加载错
        assert!(load_tree_str(&ok.replace("pos + unit", "nope + 1")).is_err());
        assert!(load_tree_str(&ok.replace("min(1, pos + unit)", "min(1,")).is_err());
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --lib tree::loader::tests::leaf_weight_expression_loads_and_evaluates`
Expected: 编译错（`Weight` 不存在）

- [ ] **Step 3: 实现**

`src/tree/schema.rs` LeafSpec：

```rust
#[derive(Debug, Deserialize)]
pub(crate) struct LeafSpec {
    pub(crate) stance: Stance,
    #[serde(default)]
    pub(crate) weight: Option<serde_yaml::Value>,
    #[serde(default)]
    pub(crate) horizon: Option<usize>,
}
```

`src/tree/loader.rs`：`Leaf` 上方加枚举与方法；`Leaf.weight` 类型 `f64` → `Weight`：

```rust
/// 叶子权重：常量（加载期校验 (0,1]）或 DSL 表达式（决策时求值，NaN→0、clamp [0,1]）。
#[derive(Debug, Clone)]
pub enum Weight {
    Const(f64),
    Expr(Expr),
}

impl Leaf {
    /// 解析叶子权重：常量直接返回；表达式按 ctx 求值。
    /// NaN/求值失败 → 0.0（弃权 = 不持仓），有限值 clamp 到 [0,1]。
    pub fn weight_at(&self, ctx: &crate::features::context::Context) -> f64 {
        match &self.weight {
            Weight::Const(w) => *w,
            Weight::Expr(e) => match crate::dsl::eval::eval_scalar(e, ctx) {
                Ok(v) if v.is_finite() => v.clamp(0.0, 1.0),
                _ => 0.0,
            },
        }
    }
}
```

leaves 加载段（loader.rs:196-201 替换；注意：leaves 循环必须在 env 构建**之后**，当前文件顺序已满足）：

```rust
        let weight = match &l.weight {
            None => Weight::Const(1.0),
            Some(serde_yaml::Value::Number(n)) => {
                let w = n
                    .as_f64()
                    .ok_or_else(|| Error::Tree(format!("leaf '{id}' weight must be a number")))?;
                if !(w > 0.0 && w <= 1.0) {
                    return Err(Error::Tree(format!(
                        "leaf '{id}' weight must be in (0,1], got {w}"
                    )));
                }
                Weight::Const(w)
            }
            Some(serde_yaml::Value::String(s)) => {
                let e = parse_str(s).map_err(|e| Error::Tree(format!("leaf '{id}' weight: {e}")))?;
                let e = substitute(&e, &env);
                check_no_unknown_idents(&e, &format!("leaf '{id}' weight"))?;
                Weight::Expr(e)
            }
            Some(_) => {
                return Err(Error::Tree(format!(
                    "leaf '{id}' weight must be a number or a DSL expression string"
                )))
            }
        };
```

既有 loader 测试 `leaf_weight_and_horizon_validated_and_defaulted` 中 `l.weight` 的数值断言改为 match `Weight::Const`。

- [ ] **Step 4: 跑测试**（此时消费点还在用 `l.weight` 当 f64 → 编译错，先让 loader 模块绿）：本任务结束时**整个 crate 必须编译**，故 Step 4 与 Task 10 的消费点切换在同一次提交完成——若想小步提交，可临时给消费点写 `match &l.weight { Weight::Const(w) => *w, Weight::Expr(_) => 0.0 }`，但下个任务立即替换为 weight_at，没有必要。直接进 Task 10 Step 1。

### Task 10: 消费点切换（runner/soft/sim/portfolio）

- [ ] **Step 1: 逐点替换**

`src/backtest/runner.rs` eval_point（:65-72）：

```rust
    let fr = match tree.leaves.get(&trace.leaf) {
        Some(l) => {
            let w = l.weight_at(&ctx);
            forward_return(primary, i, l.horizon, trace.stance, costs).map(|f| ForwardResult {
                gross: f.gross * w,
                net: f.net * w,
                t1_executable: f.t1_executable,
            })
        }
        None => forward_return(primary, i, fw, trace.stance, costs), // 防御（validate 保证不可达）
    };
```

`src/backtest/soft.rs` score_soft 签名加 ctx（含文档注释同步），`let w = leaf.weight;` → `let w = leaf.weight_at(ctx);`：

```rust
pub fn score_soft(
    soft: &SoftTrace,
    tree: &Tree,
    primary: &[Bar],
    i: usize,
    costs: &CostModel,
    ctx: &crate::features::context::Context,
) -> Option<SoftScore> {
```

调用点 `eval_point_soft`（soft.rs:133）：`score_soft(&soft, tree, primary, i, costs, &ctx)`；tests 中 7 处 `score_soft(...)` 调用补 `, &ctx)`——测试里用与 traverse 相同的 ctx 构造器。

`src/backtest/sim.rs` tree_target 两处：

```rust
                e += p * leaf.weight_at(ctx) * stance_dir(leaf.stance);
```
```rust
        let target = tree.leaves.get(&trace.leaf).map_or(0.0, |l| {
            stance_dir(l.stance) * l.weight_at(ctx)
        });
```

`src/backtest/portfolio.rs` score_symbol 两处：

```rust
            tree.leaves.get(id).map_or(0.0, |l| p * l.weight_at(&ctx) * dir(l.stance))
```
```rust
        tree.leaves.get(&tr.leaf).map_or(0.0, |l| l.weight_at(&ctx) * dir(l.stance))
```

- [ ] **Step 2: 全量编译 + 测试**

Run: `cargo test`
Expected: 全 PASS（行为零变化：所有既有树的 weight 都是数值 → Const 路径）

- [ ] **Step 3: 提交**

```bash
git add src/tree/schema.rs src/tree/loader.rs src/backtest/runner.rs src/backtest/soft.rs src/backtest/sim.rs src/backtest/portfolio.rs
git commit -m "feat(tree): leaf weight as DSL expression, resolved per-decision via weight_at"
```

### Task 11: 金字塔集成测试 + 文档

- [ ] **Step 1: 写失败测试**（`src/backtest/sim.rs` tests）

```rust
    /// Turtle 式金字塔：首仓 0.5，浮盈 1% 加到满仓；hold 用 weight:"pos" 维持现仓。
    const PYRAMID_TREE: &str = r#"
meta: { name: pyramid, forward_window: 1, stances: [long, flat] }
root: gate
nodes:
  gate:
    type: quant
    branches:
      - when: "pos == 0 and close > 0"
        goto: leaf_enter
        label: enter
      - when: "pos > 0 and pos < 1 and close > entry_price * 1.01"
        goto: leaf_add
        label: add_unit
      - when: "pos > 0"
        goto: leaf_hold
        label: hold
    default: { goto: leaf_flat, label: idle }
leaves:
  leaf_enter: { stance: long, weight: 0.5 }
  leaf_add:   { stance: long, weight: "min(1, pos + 0.5)" }
  leaf_hold:  { stance: long, weight: "pos" }
  leaf_flat:  { stance: flat }
"#;

    /// 5 bar 跨 5 日：b0 决策入场→b1 执行 0.5；b1 持平→hold；b2 涨 2%→加仓→b3 执行 1.0。
    fn write_pyramid_bars_csv() -> tempfile::NamedTempFile {
        use std::io::Write as _;
        let csv = "\
time,open,high,low,close,volume
2024-01-02 10:00:00,10.0,10.1,9.9,10.0,1000
2024-01-03 10:00:00,10.0,10.1,9.9,10.0,1000
2024-01-04 10:00:00,10.2,10.3,10.1,10.2,1000
2024-01-05 10:00:00,10.2,10.4,10.1,10.3,1000
2024-01-08 10:00:00,10.3,10.5,10.2,10.4,1000
";
        let mut f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        write!(f, "{csv}").unwrap();
        f.flush().unwrap();
        f
    }

    #[tokio::test]
    async fn run_sim_pyramid_adds_units() {
        let tree_f = write_tree_yaml(PYRAMID_TREE);
        let bars_f = write_pyramid_bars_csv();
        let out_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        let traces_f = tempfile::Builder::new().suffix(".jsonl").tempfile().unwrap();
        let cfg = make_cfg(&tree_f, &bars_f, &out_f, Some(&traces_f));
        let report = run_sim(&cfg, &LlmEvaluator::Disabled, false).await.unwrap();
        // 4 个决策点 target 阶梯：入场 0.5 → 维持 0.5 → 加仓 1.0 → 维持 1.0
        let targets: Vec<f64> = std::fs::read_to_string(traces_f.path()).unwrap()
            .lines().filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str::<SimStepRecord>(l).unwrap().target)
            .collect();
        assert_eq!(targets.len(), 4);
        assert!((targets[0] - 0.5).abs() < 1e-9, "enter 0.5, got {}", targets[0]);
        assert!((targets[1] - 0.5).abs() < 1e-9, "hold 0.5, got {}", targets[1]);
        assert!((targets[2] - 1.0).abs() < 1e-9, "add to 1.0, got {}", targets[2]);
        assert!((targets[3] - 1.0).abs() < 1e-9, "hold 1.0, got {}", targets[3]);
        // 加权均价：(10.0*0.5 + 10.2*0.5) / 1.0 = 10.1（期末清算回合可见）
        assert_eq!(report.n_round_trips, 1);
        assert_relative_eq!(report.trades[0].entry_px, 10.0); // 回合记录的是首次入场价
    }
```

（决策核对：b1 决策时 pos=0.5、entry=10.0、close=10.0 < 10.1 → hold(0.5)；b2 决策 close=10.2 > 10.1 → add → b3 开盘 10.2 执行，entry=(10×0.5+10.2×0.5)/1=10.1；b3 决策 pos=1 → hold(1.0)。）

- [ ] **Step 2: 跑测试确认通过/失败**

Run: `cargo test --lib backtest::sim::tests::run_sim_pyramid_adds_units`
Expected: PASS（Task 9/10 已实现完毕——本测试是端到端验收；若 FAIL 按输出修正阶梯推演）

- [ ] **Step 3: 文档**：`docs/tree-yaml-schema.md` 的 leaf 字段说明处更新 weight 条目：

```markdown
- `weight`：仓位大小。两种形式——
  - **数值**：∈(0,1]，加载期校验（既有行为）。
  - **表达式字符串**：决策时对当时 Context 求值；NaN→0（弃权=不持仓），结果 clamp 到 [0,1]。可引用 params/factors 与 sim 状态量。引用 `pos` 即获得**相对调仓**语义：`"min(1, pos + 0.25)"` 加一单位、`"pos"` 维持现仓、`"max(0, pos - 0.25)"` 减一单位（结果 0 等价于平仓）。
  - 注意：打分模式（非 `--sim`）下 sim 状态量恒为默认（pos=0 等），weight 表达式按默认值求值。

### 金字塔加仓示例（Turtle 风格）

​```yaml
params: { unit: 0.25 }
nodes:
  gate:
    type: quant
    branches:
      - { when: "pos == 0 and close > highest(ref(high,1), 20)", goto: leaf_enter, label: enter }
      - { when: "pos > 0 and pos < 1 and close > entry_price + 0.5 * atr(20)", goto: leaf_add, label: add_unit }
      - { when: "pos > 0", goto: leaf_hold, label: hold }
    default: { goto: leaf_flat, label: idle }
leaves:
  leaf_enter: { stance: long, weight: 0.25 }
  leaf_add:   { stance: long, weight: "min(1, pos + unit)" }   # 每次加 1 单位，至多 4 单位
  leaf_hold:  { stance: long, weight: "pos" }                  # 维持现仓
  leaf_flat:  { stance: flat }
​```

> 注：加仓后 `entry_price` 是加权均价，`entry_price + 0.5*atr` 的触发锚点会随加仓上移——与原版 Turtle「按上一加仓价」略有差异，需要精确锚点时用 `max_price_since_entry` 表达。
```

同步更新 `docs/dsl-reference.md` 中提及 weight 的段落（若有）。

- [ ] **Step 4: 提交**

```bash
git add src/backtest/sim.rs docs/tree-yaml-schema.md docs/dsl-reference.md
git commit -m "feat(tree): pyramid position ladder via weight expressions; integration test + docs"
```

---

## Phase 4 — LLM 判定复用（judges 块）

**File Structure：**
- `src/tree/schema.rs` — `JudgeSpec`、`TreeSpec.judges`、`NodeSpec::Llm` 加 `judge`/`map`
- `src/tree/loader.rs` — judge 解析/物化/校验；`Node::Llm` 加 `scope: Option<String>`
- `src/engine/traversal.rs` + `src/engine/soft.rs` — eval_llm 传 scope
- `docs/tree-yaml-schema.md`、`docs/llm-protocol.md` — judges 语法与缓存语义

**设计要点（为什么这样就能复用）：** `render_user` 渲染的 labels 列表来自节点 labels 映射的**键集**（排序后）。两个节点引用同一 judge 时，物化出的键集相同（= judge.labels），inputs/prompt 也相同 → 渲染串逐字节相同；再把缓存键里的 `node_id` 换成 `judge:<名>`（`FileCache::key` 的第 4 个参数，`client.rs:30`）→ 同 bar 第二个节点直接命中文件缓存，LLM 调用成本 ×N → ×1。落点差异只存在于各节点自己的 label→goto 映射，不进 prompt、不进缓存键。

### Task 12: schema + loader 物化与校验

- [ ] **Step 1: 写失败测试**（`src/tree/loader.rs` tests）

```rust
    const JUDGE_TREE: &str = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
judges:
  news_veto:
    inputs: [news_score]
    prompt: "消息面是否一票否决？"
    labels: [veto, pass]
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "close > 1", goto: g1, label: up } ]
    default: { goto: g2, label: dn }
  g1:
    type: llm
    judge: news_veto
    map: { veto: leaf_f, pass: leaf_l }
    default: leaf_f
  g2:
    type: llm
    judge: news_veto
    map: { veto: leaf_f }
    default: leaf_l
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;

    #[test]
    fn judge_nodes_materialize_labels_and_scope() {
        let tree = load_tree_str(JUDGE_TREE).unwrap();
        let (g1, g2) = (tree.nodes.get("g1").unwrap(), tree.nodes.get("g2").unwrap());
        match (g1, g2) {
            (
                Node::Llm { labels: l1, inputs: i1, prompt: p1, scope: s1, .. },
                Node::Llm { labels: l2, prompt: p2, scope: s2, .. },
            ) => {
                // 物化：judge 的每个 label 都有落点；g2 未映射的 pass → 其 default(leaf_l)
                assert_eq!(l1["veto"], "leaf_f");
                assert_eq!(l1["pass"], "leaf_l");
                assert_eq!(l2["pass"], "leaf_l");
                // 键集一致（渲染串一致的前提）+ prompt/inputs 来自 judge + scope 一致
                let mut k1: Vec<_> = l1.keys().collect();
                let mut k2: Vec<_> = l2.keys().collect();
                k1.sort(); k2.sort();
                assert_eq!(k1, k2);
                assert_eq!(p1, "消息面是否一票否决？");
                assert_eq!(p2, p1);
                assert_eq!(i1, &vec!["news_score".to_string()]);
                assert_eq!(s1.as_deref(), Some("judge:news_veto"));
                assert_eq!(s1, s2);
            }
            _ => panic!("expected llm nodes"),
        }
    }

    #[test]
    fn judge_validation_rejects_bad_forms() {
        // 未知 judge
        assert!(load_tree_str(&JUDGE_TREE.replace("judge: news_veto", "judge: nope")).is_err());
        // judge 形式不得再带 prompt
        assert!(load_tree_str(&JUDGE_TREE.replace(
            "    judge: news_veto\n    map: { veto: leaf_f, pass: leaf_l }",
            "    judge: news_veto\n    prompt: \"x\"\n    map: { veto: leaf_f, pass: leaf_l }"
        )).is_err());
        // map 键不在 judge labels 内
        assert!(load_tree_str(&JUDGE_TREE.replace("map: { veto: leaf_f, pass: leaf_l }", "map: { nope: leaf_f }")).is_err());
        // map 不带 judge
        let inline_with_map = JUDGE_TREE.replace("    judge: news_veto\n    map: { veto: leaf_f, pass: leaf_l }", "    prompt: \"q\"\n    labels: { veto: leaf_f }\n    map: { veto: leaf_f }");
        assert!(load_tree_str(&inline_with_map).is_err());
        // judge labels 为空
        assert!(load_tree_str(&JUDGE_TREE.replace("labels: [veto, pass]", "labels: []")).is_err());
        // 内联形式（无 judge）完全不受影响：删除 judges 块并改回内联 → OK
        let inline = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
root: g1
nodes:
  g1:
    type: llm
    prompt: "q"
    labels: { yes: leaf_l }
    default: leaf_f
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;
        let t = load_tree_str(inline).unwrap();
        match t.nodes.get("g1").unwrap() {
            Node::Llm { scope, .. } => assert!(scope.is_none()),
            _ => panic!(),
        }
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --lib tree::loader::tests::judge`
Expected: 编译错（schema 无 judges 字段 / Node::Llm 无 scope）

- [ ] **Step 3: 实现**

`src/tree/schema.rs`：

```rust
/// 顶层命名判定：prompt+inputs+允许的 label 集合；llm 节点经 judge: 引用并各自映射落点。
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct JudgeSpec {
    #[serde(default)]
    pub(crate) inputs: Vec<String>,
    pub(crate) prompt: String,
    pub(crate) labels: Vec<String>,
}
```

`NodeSpec::Llm` 加两个可选字段：

```rust
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
```

`TreeSpec` 加字段：

```rust
    #[serde(default)]
    pub(crate) judges: HashMap<String, JudgeSpec>,
```

`src/tree/loader.rs`：`Node::Llm` 加 `scope: Option<String>`（文档注释：「缓存/求值作用域：judge 节点为 `judge:<名>`，内联节点 None=节点 id」）。NodeSpec::Llm 加载臂替换为：

```rust
            NodeSpec::Llm { inputs, prompt, labels, judge, map, default } => {
                let node = match judge {
                    Some(jname) => {
                        if !prompt.is_empty() || !labels.is_empty() || !inputs.is_empty() {
                            return Err(Error::Tree(format!(
                                "llm node '{id}': judge form must not also set prompt/labels/inputs"
                            )));
                        }
                        let j = spec.judges.get(jname).ok_or_else(|| {
                            Error::Tree(format!("llm node '{id}': unknown judge '{jname}'"))
                        })?;
                        if j.labels.is_empty() {
                            return Err(Error::Tree(format!("judge '{jname}': labels must be non-empty")));
                        }
                        for k in map.keys() {
                            if !j.labels.contains(k) {
                                return Err(Error::Tree(format!(
                                    "llm node '{id}': map key '{k}' not in judge '{jname}' labels"
                                )));
                            }
                        }
                        // 物化 label→goto：judge 的每个 label 都有落点（未映射 → 本节点 default）。
                        // 物化后键集 = judge.labels，与共享同一 judge 的其它节点逐字节同渲染。
                        let labels: HashMap<String, String> = j
                            .labels
                            .iter()
                            .map(|l| (l.clone(), map.get(l).cloned().unwrap_or_else(|| default.clone())))
                            .collect();
                        Node::Llm {
                            inputs: j.inputs.clone(),
                            prompt: j.prompt.clone(),
                            labels,
                            default: default.clone(),
                            scope: Some(format!("judge:{jname}")),
                        }
                    }
                    None => {
                        if !map.is_empty() {
                            return Err(Error::Tree(format!("llm node '{id}': 'map' requires 'judge'")));
                        }
                        Node::Llm {
                            inputs: inputs.clone(),
                            prompt: prompt.clone(),
                            labels: labels.clone(),
                            default: default.clone(),
                            scope: None,
                        }
                    }
                };
                nodes.insert(id.clone(), node);
            }
```

（`node_targets` 与 validate 不需改：物化后的 labels.values() 已含全部落点。）

- [ ] **Step 4: 修编译**：`traversal.rs:23`、`engine/soft.rs:32` 的 `Node::Llm { ... }` 解构模式补 `scope`（下个任务使用，本步先 `scope: _` 占位让编译通过）。

Run: `cargo test --lib tree::loader`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add src/tree/schema.rs src/tree/loader.rs src/engine/traversal.rs src/engine/soft.rs
git commit -m "feat(tree): top-level judges block; llm nodes reference via judge+map"
```

### Task 13: scope 贯通遍历器 + 复用验证 + 文档

- [ ] **Step 1: 写失败测试**（`src/engine/traversal.rs` tests）

```rust
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
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --lib engine::traversal::tests::shared_judge_nodes_resolve_via_judge_scope`
Expected: FAIL——stub 按 node_id（g_lo/g_hi）查不到答案 → 走 default leaf_f

- [ ] **Step 3: 实现**

`src/engine/traversal.rs:23-26`：

```rust
            Node::Llm { inputs, prompt, labels, default, scope } => {
                let ln = LlmNode { inputs, prompt, labels, default };
                llm.eval_llm(scope.as_deref().unwrap_or(&current), &ln, ctx).await?
            }
```

`src/engine/soft.rs:32-36` 同构：

```rust
            Node::Llm { inputs, prompt, labels, default, scope } => {
                let ln = LlmNode { inputs, prompt, labels, default };
                let (dist, _rationale) = llm.eval_llm_dist(scope.as_deref().unwrap_or(&id), &ln, ctx).await?;
                dist
            }
```

（`fetch_probs` 的 `node_id` 参数即缓存键第 4 段（client.rs:30）——scope 传入后，共享 judge 的节点自然命中同一文件缓存条目，无需改 client/cache。）

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --lib engine`
Expected: 全 PASS

- [ ] **Step 5: 软遍历复用测试**（`src/engine/soft.rs` tests）

```rust
    // 与 traversal.rs tests 的 SHARED_JUDGE_TREE 内容相同——tests 模块私有，跨模块引用不可达，按计划纪律重复定义
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
    async fn soft_shared_judge_uses_judge_scope() {
        let tree = load_tree_str(SHARED_JUDGE_TREE).unwrap();
        let ev = LlmEvaluator::Stub(StubLlm {
            answers: HashMap::from([("judge:veto".to_string(), "ok".to_string())]),
        });
        let st = traverse_soft(&tree, &ctx(&[1.0, 1.0, 1.0]), &ev).await.unwrap();
        // stub 普通 label 协议：ok → {ok: 0.9}，残余 0.1 → default
        // close=1 → g_lo：ok→leaf_s 0.9；残余 0.1 → leaf_f
        assert!((st.leaf_probs["leaf_s"] - 0.9).abs() < 1e-9);
        assert!((st.leaf_probs["leaf_f"] - 0.1).abs() < 1e-9);
    }
```

Run: `cargo test --lib engine::soft::tests::soft_shared_judge_uses_judge_scope`
Expected: PASS

- [ ] **Step 6: 文档**

`docs/tree-yaml-schema.md` 新增「judges 块」一节：语法（上方 JUDGE_TREE 形态的示例）、物化语义（未映射 label → 节点 default）、校验清单（未知 judge / judge+prompt 互斥 / map 键 ⊆ judge labels / labels 非空 / map 必须配 judge）、**缓存语义**：共享 judge 的节点渲染串一致且缓存 scope 同为 `judge:<名>` → 每 bar 每 judge 至多一次 LLM 调用；StubLlm 测试时以 `judge:<名>` 为答案键。
`docs/llm-protocol.md` 缓存键说明同步：第 4 段由 node_id 变为「scope（judge 节点为 `judge:<名>`，内联节点仍为节点 id）」。

- [ ] **Step 7: 提交**

```bash
git add src/engine/traversal.rs src/engine/soft.rs docs/tree-yaml-schema.md docs/llm-protocol.md
git commit -m "feat(llm): judge-scoped evaluation; shared judges cost one call per bar"
```

---

## Phase 5 — 因子运行时 memoize（Expr::Cached）

**File Structure：**
- `src/dsl/ast.rs` — `Expr::Cached(u32, Box<Expr>)` + substitute 臂
- `src/features/context.rs` — `Context.eval_cache: RefCell<HashMap<u32, Value>>`
- `src/dsl/eval.rs` — Cached 求值臂（命中即返）
- `src/tree/loader.rs` — 因子体包 Cached 槽 + check_no_unknown_idents 臂
- `docs/dsl-reference.md` — 改写「重复求值代价」节

**安全性论证（计划级确认，无需运行时校验）：** 展开后的因子是 `Context` 的纯函数（eval 无副作用）；Context 每个决策点新建（runner/soft/sim/portfolio/factor 五处 `build_context` 均如此），`--sim` 的 `ctx.sim` 注入发生在任何 eval 之前 → 单决策点内缓存恒有效。`RefCell` 满足 Send（Context 进 tokio buffered 任务），eval 全程同步、借用不跨 await。

### Task 14: AST 变体 + Context 缓存 + eval 臂

- [ ] **Step 1: 写失败测试**（`src/dsl/eval.rs` tests）

```rust
    #[test]
    fn cached_expr_memoizes_per_context() {
        let ctx = ctx_from_closes(&[1.0, 2.0, 3.0]);
        let e = Expr::Cached(7, Box::new(parse_str("sma(close, 2)").unwrap()));
        // 首次求值：真算，并写入缓存槽
        let v1 = eval(&e, &ctx).unwrap();
        assert!(matches!(v1, Value::Series(_)));
        assert!(ctx.eval_cache.borrow().contains_key(&7));
        // 改写缓存槽为哨兵 → 第二次求值必须命中缓存（返回哨兵而非重算）
        ctx.eval_cache.borrow_mut().insert(7, Value::Scalar(42.0));
        assert_eq!(eval(&e, &ctx).unwrap(), Value::Scalar(42.0));
        // 不同槽位互不串扰
        let e2 = Expr::Cached(8, Box::new(parse_str("close").unwrap()));
        assert!(matches!(eval(&e2, &ctx).unwrap(), Value::Series(_)));
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --lib dsl::eval::tests::cached_expr_memoizes_per_context`
Expected: 编译错（无 Cached 变体 / 无 eval_cache 字段）

- [ ] **Step 3: 实现**

`src/dsl/ast.rs` Expr 加变体 + substitute 臂：

```rust
    /// 因子缓存槽：loader 给每个因子体包一层；同一 Context 内首算后命中（dsl/eval.rs）。
    Cached(u32, Box<Expr>),
```

```rust
        Expr::Cached(id, e) => Expr::Cached(*id, Box::new(substitute(e, env))),
```

`src/features/context.rs` Context 加字段（`#[derive(Debug, Clone)]` 不变，RefCell/HashMap 均满足）：

```rust
    /// 因子求值缓存（Expr::Cached 槽位 → 值）；每个决策点随 Context 新建，详见 dsl/eval.rs。
    pub eval_cache: std::cell::RefCell<std::collections::HashMap<u32, crate::dsl::eval::Value>>,
```

`build_context` 构造处加 `eval_cache: Default::default(),`。

`src/dsl/eval.rs` `eval` 的 match 加臂（`Expr::Call` 臂前）：

```rust
        Expr::Cached(id, inner) => {
            if let Some(v) = ctx.eval_cache.borrow().get(id) {
                return Ok(v.clone());
            }
            let v = eval(inner, ctx)?;
            ctx.eval_cache.borrow_mut().insert(*id, v.clone());
            Ok(v)
        }
```

`src/tree/loader.rs` `check_no_unknown_idents` 的 Unary/Index 臂合并 Cached：

```rust
        Expr::Unary(_, e) | Expr::Index(e, _) | Expr::Cached(_, e) => check_no_unknown_idents(e, where_),
```

- [ ] **Step 4: 修全 crate 编译**：测试中所有手写 `Context { ... }` 字面量补 `eval_cache: Default::default(),`——已知位置：eval.rs `ctx_from_closes`、traversal.rs / engine/soft.rs 的 `ctx`、prompt.rs `ctx_with`、llm/mod.rs `ctx`、loader.rs `mini_ctx`（Task 9 新增）；以 Grep `Context \{` 全量核对，勿漏。

Run: `cargo test --lib dsl::eval::tests::cached_expr_memoizes_per_context`
Expected: PASS；`cargo test` 全量 PASS

- [ ] **Step 5: 提交**

```bash
git add src/dsl/ast.rs src/dsl/eval.rs src/features/context.rs src/tree/loader.rs
git commit -m "feat(dsl): Expr::Cached memo slots backed by per-Context eval cache"
```

### Task 15: loader 包裹因子 + 等价性验证 + 文档

- [ ] **Step 1: 写失败测试**（`src/tree/loader.rs` tests）

```rust
    #[test]
    fn factors_are_wrapped_in_shared_cache_slots() {
        let src = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
factors:
  atr_v: "atr(3)"
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "atr_v >= 0 and atr_v < 1000", goto: leaf_l, label: up } ]
    default: { goto: leaf_f, label: flat }
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;
        let tree = load_tree_str(src).unwrap();
        // 同名因子的两处引用共享同一槽位 id（Cached(0, ...) 出现 ≥2 次）
        let rendered = format!("{:?}", tree.nodes.get("a").unwrap());
        assert!(
            rendered.matches("Cached(0,").count() >= 2,
            "expected shared cache slot, got: {rendered}"
        );
    }
```

（`src/engine/traversal.rs` tests）端到端等价性：

```rust
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
        let inline = factored
            .replace("factors:\n  f: \"sma(close, 3)\"\n", "")
            .replace("close > f and f > 0", "close > sma(close, 3) and sma(close, 3) > 0");
        let (tf, ti) = (load_tree_str(factored).unwrap(), load_tree_str(&inline).unwrap());
        for closes in [&[1.0, 2.0, 3.0, 4.0, 5.0][..], &[5.0, 4.0, 3.0][..], &[1.0][..]] {
            let c = ctx(closes);
            let a = traverse(&tf, &c, &LlmEvaluator::Disabled).await.unwrap();
            let b = traverse(&ti, &c, &LlmEvaluator::Disabled).await.unwrap();
            assert_eq!(a.leaf, b.leaf, "closes={closes:?}");
        }
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --lib tree::loader::tests::factors_are_wrapped_in_shared_cache_slots`
Expected: FAIL（因子未包 Cached，`rendered` 中无 "Cached"）；等价性测试此时应 PASS（基线）

- [ ] **Step 3: 实现**：loader factors 循环（loader.rs:173-186）改为：

```rust
    let mut next_cache_slot: u32 = 0;
    for (k, v) in &spec.factors {
        let name = k
            .as_str()
            .ok_or_else(|| Error::Tree("factor name must be a string".into()))?;
        let src_expr = v
            .as_str()
            .ok_or_else(|| Error::Tree(format!("factor '{name}' expr must be a string")))?;
        check_name(name, &env)?;
        let e = parse_str(src_expr)
            .map_err(|e| Error::Tree(format!("factor '{name}': {e}")))?;
        let e = substitute(&e, &env);
        check_no_unknown_idents(&e, &format!("factor '{name}'"))?;
        // 包缓存槽：所有引用处共享同一槽位 → 每个 Context 只真算一次（params 是字面量，不包）
        let e = Expr::Cached(next_cache_slot, Box::new(e));
        next_cache_slot += 1;
        env.insert(name.to_string(), e);
    }
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test`
Expected: 全 PASS（含等价性、既有 examples 加载测试）

- [ ] **Step 5: 文档**：`docs/dsl-reference.md` 「重复求值代价」节改写为：

```markdown
### 因子按决策点 memoize

同一因子在多处引用时共享一个缓存槽（加载期由 loader 包裹 `Cached` 节点）：**每个决策点首个引用处真算一次，其余引用命中缓存**。语义与内联展开完全等价（因子是 Context 的纯函数），高频引用的重型因子（如 `atr(14)`、`ema(close,200)`）不再有重复求值代价。缓存随 Context 新建/销毁，不跨决策点、不跨标的。
```

- [ ] **Step 6: 提交**

```bash
git add src/tree/loader.rs src/engine/traversal.rs docs/dsl-reference.md
git commit -m "feat(tree): wrap factor bodies in shared cache slots; per-bar memoization"
```

---

## Phase 6 — aux 时间对齐语义文档化

**File Structure：** 纯文档，不动代码（闸门语义 `context.rs:67` 与测试 `aux_tables_gated_by_time` 已正确）。
- `docs/dsl-reference.md` — aux 节新增「时间戳纪律」小节
- `docs/cli-reference.md` — `--aux` 参数处加一句指针

### Task 16: as-of join 纪律文档

- [ ] **Step 1: `docs/dsl-reference.md` 的「外部 aux 序列」节、「time≤t 闸门」小节之后插入：**

```markdown
### 时间戳纪律（as-of join，防 lookahead）

闸门是**含端点的 as-of join**：决策点 `t` 可见所有 `time ≤ t` 的行——与 primary bar 的可见性约定一致（时间戳为 `t` 的 bar 在 `t` 时刻其 close 已可见）。因此 aux 行时间戳必须满足同一纪律：

> **行时间 = 该行数值完全确定（可被知晓）的时刻。**

- **高周期重采样**（如用 4h K 线做日内 regime 过滤）：行必须打在**周期收盘时刻**。打在周期开始时刻 = 在该周期进行中就泄露其收盘值，lookahead 直接进来。
- **公告 / 财务 / 舆情**：行时间 = 发布时刻（精确到日内则当日盘中即可见；只精确到日，按「当日收盘后写入」处理 → 次日起可见）。
- **指数日线**：打收盘日时间戳（如 `2024-01-02 15:00:00`），不要打 `00:00:00`——后者会让当日开盘即可见当日收盘价。

这与 `--sim` 的 SimState 注入、`build_context` 的 bar 闸门是同一条防未来函数纪律：**引擎只保证 `time ≤ t` 截断正确，时间戳本身的语义由数据制备方负责**。引擎无法检测打错戳的 aux 表——错误的时间戳产生的回测收益是假的。

| 数据 | 错误打法 | 正确打法 |
|---|---|---|
| 4h bar (10:00–14:00) 的 ema20 | `10:00:00`（周期开始） | `14:00:00`（周期收盘） |
| 1 月 5 日盘后年报 | `2024-01-05 00:00:00` | `2024-01-05 17:00:00`（或次日 00:00） |
| 指数日线收盘价 | `2024-01-02 00:00:00` | `2024-01-02 15:00:00` |
```

- [ ] **Step 2: `docs/cli-reference.md` 的 `--aux` 参数说明处追加一句：**

```markdown
aux CSV 的 `time` 列须打「数值可被知晓的时刻」（高周期聚合打周期收盘、公告打发布时刻），详见 [dsl-reference.md](dsl-reference.md) 「时间戳纪律」一节——打错戳引擎无法检测，lookahead 后果自负。
```

- [ ] **Step 3: 验证 + 提交**

Run: `cargo test`（确认文档改动未碰代码）
Expected: 全 PASS

```bash
git add docs/dsl-reference.md docs/cli-reference.md
git commit -m "docs(aux): as-of join timestamp discipline to prevent lookahead"
```

---

## 收尾

- [ ] `cargo test` 全量 + `cargo clippy --all-targets -- -D warnings` 双绿
- [ ] 用 `examples/` 任一树 + 真实 CSV 跑一次 `--sim` 冒烟（参照 `docs/superpowers/2026-06-10-real-smoke-results.md` 的命令），确认报告字段无回归
- [ ] 更新 `docs/architecture.md` 「设计中提及但尚未实现」表（如有条目被本计划覆盖）

## 自检记录（写计划时已核对）

- 报告 7 项全部映射到阶段（含被收窄的第 1 项与并入 Phase 1 的第 7 项）；两处「二选一」设计决策（weight 表达式 vs units 状态量、judges vs 跨节点引用）的取舍理由见映射表。
- 类型一致性：`Weight`/`weight_at`/`scope`/`Expr::Cached`/`eval_bool_series` 等签名在各任务间逐一核对；`sim_step` 新签名 10 参（既有 `#[allow(clippy::too_many_arguments)]` 覆盖）。
- 行为零回归声明：Phase 3/4/5 对既有 YAML（数值 weight、内联 llm、无因子树）均为恒等路径，由全量 `cargo test` 与 examples 加载测试守护。
