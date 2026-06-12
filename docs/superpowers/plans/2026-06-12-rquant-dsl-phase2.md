# DSL Phase-2（派生序列一等公民 + 数学函数补全）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 落地 DSL 缺口评估第一梯队 #1「派生序列一等公民」：算术与点态函数对序列**逐位提升**，使 `sma(close*volume, 20)`、`count((high-low) > atr(14), 10)`、滚动 VWAP 等自造指标可表达；顺势补全数学函数 log/exp/sqrt/floor/sign/pow。

**Architecture:** 全部在 `src/dsl/eval.rs`（提升）+ `src/tree/{lint.rs,loader.rs}`（形态推断/保留名同步）+ docs。**设计核心定理（已论证闭环）：逐位提升 + 尾对齐后，结果末位元素恒等于旧标量算术值**——而所有标量消费者（普通比较 eval.rs:95-106、fuzzy_cmp:44-45、weight eval_scalar、as_scalar:156 取末位）只看末位 ⇒ 标量上下文观测等价，零破坏。新增语义只出现在序列流入窗口函数实参与逐位条件处。

**Tech Stack:** Rust 2024 + 既有。无新依赖。

**铁律与守则（实现者必读）：**
- **双标量入算术 → 必须仍返回 Scalar**（仅当 ≥1 侧为 Series 才提升）。否则 lint 的 L2 形态推断失准（`count(pos*2 > 1, 5)` 这类真退化条件会漏报）。
- **Bool 进算术仍 Err**（现状 as_scalar(Bool) → "expected number, got bool"，提升路径必须保持同等拒绝）。
- 尾对齐复用既有 `tail_align`（eval.rs:194 区，Series×Series 右对齐公共长度、Scalar 广播）——但注意它对 Scalar×Scalar 返回长度 1 向量，**不可**用它判定双标量分支；先判形态再走对齐。
- NaN 纪律：逐位算术 NaN 传播（该位 NaN）；新数学函数负数定义域（log/sqrt of ≤0/负）→ NaN（Rust 原生行为，恰合弃权语义）；min/max 的逐位提升必须沿用**现有标量版的显式 NaN 传播规则**（docs 注明"任一 NaN → NaN，不吃弃权"——不是 f64::max 的跳过语义，动手前读现实现）。
- 空序列：对齐 m=0 → 空 Series；as_scalar(空) = NaN（既有 :156）——链路自然弃权，不特判。
- 验收双闸：等价锁电池（本计划 T1）+ 真数据冻结闸（T4，基准同 phase-1：sh600519 60m qfq v2 树 sim 五指标 −0.0641 / 0.0885 / 36 / 38.9% / 41.6——v2 树的 vol_ratio/rng_w/rng_pos 等算术因子全部会变成 Series 形态，冻结闸是端到端实证）。
- ⚠️ git 纪律：add 点名文件；杂物（.idea/、report.json、rust_out.*、test_output.log）不碰。分支：`dsl-phase2`。

---

## 文件结构

```
改动: src/dsl/eval.rs          # Binary 算术四臂 + Unary::Neg 提升；abs/min/max/sigmoid 点态提升；
                               # 新 log/exp/sqrt/floor/sign/pow；等价锁电池测试
改动: src/tree/loader.rs       # RESERVED_FNS 23→29
改动: src/tree/lint.rs         # expr_shape 递归化（算术/一元/点态）；同步锁测试扩展
改动: docs/dsl-reference.md    # 算术逐位语义节、新函数表、VWAP/真实波幅/对数收益惯用法
改动: docs/tree-yaml-schema.md # 保留名清单 29 项
```

---

## Task 1: 算术与一元负号的逐位提升 + 等价锁电池

**Files:**
- Modify: `src/dsl/eval.rs`（Binary Add/Sub/Mul/Div 臂 + Unary::Neg 臂 + mod tests）

- [ ] **Step 1: RED——等价锁电池 + 派生序列解锁测试（先写）**

追加到 `src/dsl/eval.rs` 的 `mod tests`：

```rust
    /// 提升定理锁：任意混合算术表达式在标量上下文的值与提升前完全一致。
    /// 每个用例：手算旧标量语义的期望值，断言 as_scalar(eval(expr)) 逐 bits 相等。
    #[test]
    fn arithmetic_lift_scalar_context_equivalence() {
        // closes [1,2,3,4,5]；highs = closes（ctx_from_closes 的构造，动手前核实该工厂的 OHLC 形态）
        let ctx = ctx_from_closes(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        let f = |src: &str| {
            let v = eval(&parse_str(src).unwrap(), &ctx).unwrap();
            match v {
                Value::Scalar(x) => x,
                Value::Series(s) => *s.last().unwrap(),
                Value::Bool(_) => panic!("unexpected bool"),
            }
        };
        // Series∘Series：末位 = 5*5
        assert_eq!(f("close * volume").to_bits(), (5.0_f64 * f("volume")).to_bits());
        // Series∘Scalar 广播：末位 = 5-1
        assert_eq!(f("close - 1"), 4.0);
        // 函数序列参与：sma(close,2).last = 4.5 → 5 - 4.5
        assert_eq!(f("close - sma(close, 2)"), 0.5);
        // 嵌套：(highest3 - lowest3)/close = (5-3)/5
        assert_eq!(f("(highest(close,3) - lowest(close,3)) / close"), 0.4);
        // 一元负号提升：-(close) 末位 = -5
        assert_eq!(f("0 - close"), -5.0);
        // 双标量仍是 Scalar 形态（守则锁——L2 依赖）
        assert!(matches!(
            eval(&parse_str("pos * 2 + 1").unwrap(), &ctx).unwrap(),
            Value::Scalar(_)
        ));
        // Bool 进算术仍 Err
        assert!(eval(&parse_str("(close > 1) + 1").unwrap(), &ctx).is_err());
    }

    /// 派生序列解锁：算术结果可进窗口函数与逐位条件（phase-2 的存在意义）。
    #[test]
    fn derived_series_feed_windows_and_conditions() {
        let ctx = ctx_from_closes(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        let f = |src: &str| eval(&parse_str(src).unwrap(), &ctx).unwrap();
        // sma(close*volume, 2) 末位 = (4*v4 + 5*v5)/2——按 ctx 工厂的 volume 值手算填入
        // （ctx_from_closes 的 volume 若恒为常数 V，期望 = V * 4.5；先读工厂再写死数值）
        // 滚动 VWAP 恒等：volume 为常数时 vwap == sma(close,n)
        assert_eq!(
            f("sma(close * volume, 3) / sma(volume, 3) == sma(close, 3)"),
            Value::Bool(true)
        );
        // 派生序列进逐位条件：close-open 每位 > 0（工厂若 open==close 则改用 close - 1 > 0 等价例）
        assert_eq!(f("count(close - 1 > 0, 4) == 4"), Value::Bool(true));
        // 派生序列进 ref：ref(close*2, 1) 末位 = 4*2
        assert_eq!(f("ref(close * 2, 1) == 8"), Value::Bool(true));
        // 逐位 NaN 传播：sma 暖机段 NaN 进入算术 → 该位弃权（count 不计）
        assert_eq!(f("count(close - sma(close,3) > 0 - 99, 5) == 3"), Value::Bool(true));
    }
```

> 两个测试里凡标注"先读工厂"处：动手前读 `ctx_from_closes` 的实际 OHLC/volume 构造，把期望值改成实算值并在断言旁注明演算——计划数值是按 volume=常数假设写的草稿。

- [ ] **Step 2: 实现（eval.rs Binary/Unary 臂）**

```rust
        Expr::Unary(op, e) => {
            let v = eval(e, ctx)?;
            match op {
                UnaryOp::Neg => match v {
                    Value::Series(s) => Ok(Value::Series(s.iter().map(|x| -x).collect())),
                    other => Ok(Value::Scalar(-as_scalar(&other)?)),
                },
                UnaryOp::Not => Ok(Value::Bool(!as_bool(&v)?)),
            }
        }
```

Binary 算术四臂换为统一助手（放 as_scalar 附近）：

```rust
/// 算术逐位提升：≥1 侧 Series → 尾对齐逐位运算返回 Series（末位恒等于旧标量结果）；
/// 双标量 → 标量（形态守则：lint L2 依赖）；Bool → Err（与 as_scalar 同等拒绝）。
fn arith(op: &BinaryOp, lv: &Value, rv: &Value) -> Result<Value> {
    let apply = |a: f64, b: f64| match op {
        BinaryOp::Add => a + b,
        BinaryOp::Sub => a - b,
        BinaryOp::Mul => a * b,
        BinaryOp::Div => a / b,
        _ => unreachable!(),
    };
    match (lv, rv) {
        (Value::Bool(_), _) | (_, Value::Bool(_)) => {
            Err(Error::Eval("expected number, got bool".into()))
        }
        (Value::Scalar(a), Value::Scalar(b)) => Ok(Value::Scalar(apply(*a, *b))),
        _ => {
            let (a, b) = tail_align(lv, rv)?;
            Ok(Value::Series(a.iter().zip(&b).map(|(&x, &y)| apply(x, y)).collect()))
        }
    }
}
```

四臂改调：`BinaryOp::Add | Sub | Mul | Div => arith(op, &lv, &rv)?`（写法以现有 match 结构最小改动为准）。

- [ ] **Step 3: GREEN + 全量 + clippy + Commit**

```bash
cargo test
cargo clippy --all-targets -- -D warnings
git add src/dsl/eval.rs
git status --porcelain
git commit -m "feat(dsl): element-wise lift for arithmetic and unary neg - derived series become first-class" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

**任何既有测试变红 = 等价定理被破坏，修实现不改测试**（既有套件没有断言算术返回形态的测试；若意外发现有，停下报告而不是顺手改）。

---

## Task 2: 点态函数提升（abs/min/max/sigmoid）+ 数学补全（log/exp/sqrt/floor/sign/pow）

**Files:**
- Modify: `src/dsl/eval.rs`（点态臂改造 + 六个新函数 + 测试）
- Modify: `src/tree/loader.rs`（RESERVED_FNS 23→29）

- [ ] **Step 1: RED 测试**

```rust
    #[test]
    fn pointwise_fns_lift_and_new_math() {
        let ctx = ctx_from_closes(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        let f = |src: &str| eval(&parse_str(src).unwrap(), &ctx).unwrap();
        // abs 提升：abs(close - 3) 逐位 [2,1,0,1,2]，count >0 的 = 4
        assert_eq!(f("count(abs(close - 3) > 0, 5) == 4"), Value::Bool(true));
        // abs 全标量仍 Scalar 形态
        assert!(matches!(f("abs(pos - 1)"), Value::Scalar(_)));
        // min/max 提升 + 广播：min(close, 3) 逐位 [1,2,3,3,3]
        assert_eq!(f("count(min(close, 3) == 3, 5) == 3"), Value::Bool(true));
        // min/max 双标量仍 Scalar（weight 表达式回归保障）
        assert!(matches!(f("min(1, pos + 0.25)"), Value::Scalar(_)));
        // 新函数标量形态：floor/sign/sqrt/exp/log/pow
        assert_eq!(f("floor(2.9) == 2"), Value::Bool(true));
        assert_eq!(f("sign(0 - 3) == 0 - 1"), Value::Bool(true));
        assert_eq!(f("sqrt(9) == 3"), Value::Bool(true));
        assert_eq!(f("pow(2, 10) == 1024"), Value::Bool(true));
        assert_eq!(f("log(exp(2)) > 1.999 and log(exp(2)) < 2.001"), Value::Bool(true));
        // 负定义域 → NaN 弃权（比较恒 false）
        assert_eq!(f("log(0 - 1) > 0 - 99"), Value::Bool(false));
        assert_eq!(f("sqrt(0 - 1) > 0 - 99"), Value::Bool(false));
        // 序列提升：log(close) 进窗口函数（对数收益惯用法的地基）
        assert_eq!(
            f("count(log(close) - log(ref(close,1)) > 0, 4) == 4"),
            Value::Bool(true)
        );
        // min/max 的 NaN 显式传播在逐位下保持：min(close, sma(close,3)) 暖机段 NaN
        assert_eq!(f("count(min(close, sma(close,3)) > 0, 5) == 3"), Value::Bool(true));
    }
```

（同样规矩：依赖 ctx 工厂细节的期望值先读工厂实算。）

- [ ] **Step 2: 实现**

点态提升助手（eval.rs）：

```rust
/// 一元点态提升：Series → 逐位 map；Scalar → 标量。NaN 自然传播（f(NaN)=NaN）。
fn pointwise1(v: &Value, f: impl Fn(f64) -> f64) -> Result<Value> {
    match v {
        Value::Scalar(x) => Ok(Value::Scalar(f(*x))),
        Value::Series(s) => Ok(Value::Series(s.iter().map(|&x| f(x)).collect())),
        Value::Bool(_) => Err(Error::Eval("expected number, got bool".into())),
    }
}

/// 二元点态提升：≥1 侧 Series → 尾对齐逐位；双标量 → 标量；NaN 规则由闭包自带。
fn pointwise2(a: &Value, b: &Value, f: impl Fn(f64, f64) -> f64) -> Result<Value> {
    match (a, b) {
        (Value::Bool(_), _) | (_, Value::Bool(_)) => {
            Err(Error::Eval("expected number, got bool".into()))
        }
        (Value::Scalar(x), Value::Scalar(y)) => Ok(Value::Scalar(f(*x, *y))),
        _ => {
            let (xs, ys) = tail_align(a, b)?;
            Ok(Value::Series(xs.iter().zip(&ys).map(|(&x, &y)| f(x, y)).collect()))
        }
    }
}
```

函数臂改造/新增（现有 abs/min/max/sigmoid 臂改走 pointwise，**min/max 的 NaN 显式传播闭包照抄现有标量实现的规则**，先读现状再写）：

```rust
        "abs"     => { need(&vals, 1, name)?; pointwise1(&vals[0], f64::abs) }
        "sigmoid" => { need(&vals, 1, name)?; pointwise1(&vals[0], |x| 1.0 / (1.0 + (-x).exp())) }
        "min"     => { need(&vals, 2, name)?; pointwise2(&vals[0], &vals[1], /* 现标量版 NaN 规则 */) }
        "max"     => { need(&vals, 2, name)?; pointwise2(&vals[0], &vals[1], /* 同上 */) }
        "log"     => { need(&vals, 1, name)?; pointwise1(&vals[0], f64::ln) }
        "exp"     => { need(&vals, 1, name)?; pointwise1(&vals[0], f64::exp) }
        "sqrt"    => { need(&vals, 1, name)?; pointwise1(&vals[0], f64::sqrt) }
        "floor"   => { need(&vals, 1, name)?; pointwise1(&vals[0], f64::floor) }
        "sign"    => { need(&vals, 1, name)?; pointwise1(&vals[0], f64::signum) }
        "pow"     => { need(&vals, 2, name)?; pointwise2(&vals[0], &vals[1], f64::powf) }
```

> `sign` 用 `signum` 注意：signum(0.0)=0.0？**不是**——Rust `f64::signum(0.0)=1.0`、`signum(-0.0)=-1.0`、`signum(NaN)=NaN`。若要 sign(0)=0 的数学惯例须自写闭包 `|x| if x == 0.0 { 0.0 } else { x.signum() }`——**采用数学惯例（0→0）**并在文档注明；测试加一条 `sign(0) == 0`。

loader.rs：RESERVED_FNS 追加 `"log", "exp", "sqrt", "floor", "sign", "pow"`（23→29）。

- [ ] **Step 3: GREEN + 全量 + clippy + Commit**

```bash
cargo test
cargo clippy --all-targets -- -D warnings
git add src/dsl/eval.rs src/tree/loader.rs
git status --porcelain
git commit -m "feat(dsl): pointwise lift for abs/min/max/sigmoid; add log/exp/sqrt/floor/sign/pow" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Task 3: lint 形态推断升级 + 同步锁扩展

**Files:**
- Modify: `src/tree/lint.rs`

- [ ] **Step 1: RED 测试（lint.rs mod tests 追加）**

```rust
    #[test]
    fn shape_inference_tracks_lifted_arithmetic() {
        // 派生序列条件不再误报 L2：算术含 Series 侧 → Many
        let ok = [
            "count(close - open > 0, 5) >= 3",
            "count(abs(close - sma(close,3)) > 0.5, 5) >= 1",
            "barssince(close * volume > 1000) <= 5",
            "count(log(close) - log(ref(close,1)) > 0, 4) >= 2",
        ];
        for w in ok {
            let t = load_tree_str(&yaml_one_branch(w)).unwrap();
            assert!(lint_tree(&t).is_empty(), "false positive on: {w}");
        }
        // 双标量算术条件仍报 L2（守则锁的另一半）
        let bad = [
            "count(pos * 2 > 1, 5) >= 1",
            "barssince(abs(pos) > 0.5) <= 3",
            "count(min(1, pos + 0.25) > 0.5, 5) >= 1",
        ];
        for w in bad {
            let t = load_tree_str(&yaml_one_branch(w)).unwrap();
            assert_eq!(lint_tree(&t).len(), 1, "false negative on: {w}");
        }
    }
```

同步锁测试 `series_fns_shape_matches_eval_reality` 扩展：追加派生形态用例——`close * volume`/`abs(close - 3)`/`min(close, 3)` 实际 eval 必须 Series；`pos * 2`/`abs(pos)`/`min(1, pos)`/`floor(2.9)` 必须 Scalar（提升守则的运行时锁）。

- [ ] **Step 2: 实现（lint.rs expr_shape 递归化）**

```rust
/// 点态提升函数：形态 = 实参形态的并（任一 Series → Series）。与 eval.rs pointwise 同步维护。
pub(super) const POINTWISE_FNS: [&str; 10] = [
    "abs", "min", "max", "sigmoid", "log", "exp", "sqrt", "floor", "sign", "pow",
];

fn expr_shape(e: &Expr) -> Shape {
    match e {
        Expr::Number(_) => Shape::Scalar,
        Expr::Ident(name) => match name.as_str() { /* 原样 */ },
        Expr::Index(..) => Shape::Scalar,
        // phase-2：一元负号随内层形态（Not 仍归 Scalar——Bool 在两值 Shape 下并入）
        Expr::Unary(op, inner) => match op {
            UnaryOp::Neg => expr_shape(inner),
            _ => Shape::Scalar,
        },
        // phase-2：算术随两侧形态并；比较/逻辑仍归 Scalar（Bool）
        Expr::Binary(op, l, r) => match op {
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => {
                if expr_shape(l) == Shape::Series || expr_shape(r) == Shape::Series {
                    Shape::Series
                } else {
                    Shape::Scalar
                }
            }
            _ => Shape::Scalar,
        },
        Expr::Call(name, args) => {
            if SERIES_FNS.contains(&name.as_str()) {
                Shape::Series
            } else if POINTWISE_FNS.contains(&name.as_str()) {
                if args.iter().any(|a| expr_shape(a) == Shape::Series) {
                    Shape::Series
                } else {
                    Shape::Scalar
                }
            } else {
                Shape::Scalar
            }
        }
        Expr::Cached(_, inner) => expr_shape(inner),
    }
}
```

（UnaryOp 需要 import；模式名以 ast.rs 实际为准。Unary 注释同步更新。）

- [ ] **Step 3: GREEN + 全量 + clippy + Commit**

```bash
cargo test
cargo clippy --all-targets -- -D warnings
git add src/tree/lint.rs
git status --porcelain
git commit -m "feat(lint): shape inference tracks lifted arithmetic and pointwise fns; sync-lock extended" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

`all_example_trees_lint_clean` 必须仍绿。

---

## Task 4: 文档 + 真数据冻结闸 + 收官

**Files:**
- Modify: `docs/dsl-reference.md`、`docs/tree-yaml-schema.md`

- [ ] **Step 1: dsl-reference.md**

1. 「二元运算」节改写：算术逐位提升语义（≥1 侧序列 → 尾对齐逐位、双标量 → 标量、Bool 拒绝、NaN 逐位传播、**末位恒等于旧标量值——升级零破坏**的一句话定理）。
2. 函数表：abs/min/max/sigmoid 标注"点态提升（实参含序列 → 序列）"；新增 log/exp/sqrt/floor/sign/pow 六行（log=自然对数、负定义域 → NaN 弃权、sign(0)=0、pow=powf）。
3. 惯用法新增（实测可跑后入文）：

```yaml
# 滚动 VWAP（成交量加权均价,n 根）
vwap_n: "sma(close * volume, 20) / sma(volume, 20)"
# 真实波幅近似序列进条件：振幅大于 ATR 的 bar 计数
when: "count((high - low) > atr(14), 10) >= 3"
# 对数收益 zscore（自归一化阈值的地基）
ret_z: "(log(close) - log(ref(close,1))) / std(log(close), 60)"
```

> 第三例注意：`std(log(close), 60)` 的 std 是滚动序列、log(close) 是派生序列——整条是序列除法。入文前实跑验证（合成数据即可），跑不通就修正写法再入文。

4. lint 节补一句：形态推断已覆盖提升后的算术/点态函数。

- [ ] **Step 2: tree-yaml-schema.md**

保留名清单 29 项（+log/exp/sqrt/floor/sign/pow）。

- [ ] **Step 3: 真数据冻结闸**

```bash
cargo run --release -- fetch --symbol sh600519 --scale 60 --datalen 1023 --adjust qfq --out tmps/p.csv
cargo run --release -- backtest --tree examples/regime_adaptive_1.yaml --primary tmps/p.csv --context tmps/p.csv --sim --warmup 80 --out tmps/freeze.json
```

基准：总收益 −0.0641 / 回撤 0.0885 / 回合 36 / 胜率 38.9% / 换手 41.6。v2 树的 vol_ratio/rng_w/rng_pos 算术因子在本 phase 后全部变 Series 形态——五指标精确相等即端到端等价实证。窗口移位/网络失败的降级规则同 phase-1 计划。完毕删 tmps/。

- [ ] **Step 4: 全量 + clippy + Commit**

```bash
cargo test
cargo clippy --all-targets -- -D warnings
git add docs/dsl-reference.md docs/tree-yaml-schema.md
git status --porcelain
git commit -m "docs(dsl): element-wise arithmetic semantics, math fn reference, vwap/log-return idioms" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## 附录 A：验收对照

| 缺口项 | 实现于 |
|---|---|
| #1 派生序列一等公民（算术/一元/点态提升） | T1+T2（等价锁电池 + 解锁测试）|
| #8 数学补全 log/exp/sqrt/floor/sign/pow | T2 |
| lint 形态推断不漂移 | T3（同步锁扩展 + 双向新用例）|
| 标量语义零变铁律 | T1 电池 + T4 冻结闸（v2 树算术因子全换形态下的端到端实证）|

## 附录 B：明确不在本 phase

at_entry 快照族/节流状态量（phase-3，动 state schema）；日内锚定族 session_open/bars_today（phase-3）；cum/累积族（OBV 完整版依赖，随日内锚定设计）；percentrank/corr（建立在本 phase 地基上的独立小件，phase-3 顺手）；比较运算的逐位提升（普通 when 上下文保持标量归约——逐位比较只活在 count/barssince/valuewhen 条件臂，现状语义不动）。
