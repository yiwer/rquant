# DSL Phase-1（滚动形态统一 / valuewhen / lint 层）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 落地 DSL 缺口评估（2026-06-12 会话）第一梯队中不动 state schema 的三件：highest/lowest/std/slope 滚动形态统一、`valuewhen` 事件锚定、加载期 lint 告警层。

**Architecture:** 全部在既有四层内：`src/features/indicators.rs` 加 `_roll` 滚动版（标量版保留不动）→ `src/dsl/eval.rs` 四个函数臂改返 Series + 新增 `valuewhen` 截获臂 → 新建 `src/tree/lint.rs`（AST 形态推断 + 两条规则）由 loader 链路 eprintln。**向后兼容铁律：标量上下文语义零变**——`as_scalar(Series)` 取末元素，滚动版末位与旧标量版逐位相等（含短序列宽容/严格差异，见各任务）；验收门 = 全量回归 + v2 树真数据 sim 数字冻结比对。

**Tech Stack:** Rust 2024 + 既有。无新依赖。

**关键背景（实现者必读）：**
- 二元算术对序列**取末值变标量**（eval.rs `BinaryOp::Add..Div` 用 `as_scalar`）——本 phase 不改这个语义（派生序列是 phase-2 设计题）。
- `count`/`barssince` 的条件臂走 `eval_bool_series`（eval.rs:210 起）：比较两侧经 `eval` 后 `tail_align`，序列-序列逐位、标量广播；`and/or/not` 逐位；`crossover/crossunder` 事件序列。**本 phase 后 highest/lowest/std/slope 变 Series，逐位条件里成为滚动序列**——这是 Task 1 的全部意义。
- 现状返回形态：Series = sma/ema/wma/rsi/atr/ref/macd_*；Scalar = slope/highest/lowest/std/sigmoid/abs/min/max/count/barssince。
- `highest/lowest` 标量版对短序列**宽容**（len<n 取全序列最值，indicators.rs:117-136 saturating_sub），`std/slope` **严格**（len<n → NaN）。滚动版必须分别镜像，否则末位等价被破坏（短可见窗口的早期决策点行为会变）。
- 一元负号 parser/eval 均已支持（parser.rs:84 / eval.rs:81）——lint/测试不必绕。
- ⚠️ git 纪律：`git add` 点名文件；提交前 `git status --porcelain`；工作区杂物（.idea/、rust_out.*、test_output.log）不碰。
- 分支：`dsl-phase1`（从 master 建）。

---

## 文件结构

```
改动: src/features/indicators.rs   # + highest_roll/lowest_roll/std_roll/slope_roll（标量版保留）
改动: src/dsl/eval.rs              # 4 臂改 Series + valuewhen 臂 + 测试更新
改动: src/tree/loader.rs           # RESERVED_FNS + lint 接线（一处 chokepoint）
新增: src/tree/lint.rs             # lint_tree + 形态/长度类推断 + L1/L2 规则 + 测试
改动: src/tree/mod.rs              # + pub mod lint;（若 tree 模块声明在别处，跟随现状）
改动: docs/dsl-reference.md        # 返回形态标注、valuewhen、惯用法更新、lint 说明
改动: docs/tree-yaml-schema.md     # 保留名清单 +valuewhen、lint 一节
```

---

## Task 1: 滚动形态统一（highest/lowest/std/slope → Series）

**Files:**
- Modify: `src/features/indicators.rs`（在 lowest 之后追加四个 `_roll`）
- Modify: `src/dsl/eval.rs`（"highest"/"lowest"/"std"/"slope" 四臂 + 既有语义锁测试）

- [ ] **Step 1: RED——末位等价锁 + 逐位条件解锁测试（先写，编译失败即 RED）**

追加到 `src/features/indicators.rs` 的 `mod tests`：

```rust
    /// 滚动版末位 == 旧标量版（含 len<n 的宽容/严格差异），NaN 位用 bits 比较。
    #[test]
    fn roll_last_equals_scalar_form() {
        let s = [3.0, f64::NAN, 5.0, 1.0, 4.0, 2.0];
        for n in [1usize, 3, 6, 99] {
            for len in 1..=s.len() {
                let w = &s[..len];
                let pairs: [(f64, f64); 4] = [
                    (*highest_roll(w, n).last().unwrap(), highest(w, n)),
                    (*lowest_roll(w, n).last().unwrap(), lowest(w, n)),
                    (*std_roll(w, n).last().unwrap(), std(w, n)),
                    (*slope_roll(w, n).last().unwrap(), slope(w, n)),
                ];
                for (i, (r, sc)) in pairs.iter().enumerate() {
                    assert!(
                        r.to_bits() == sc.to_bits() || (r.is_nan() && sc.is_nan()),
                        "fn#{i} n={n} len={len}: roll last {r} != scalar {sc}"
                    );
                }
            }
        }
    }

    #[test]
    fn highest_roll_head_is_expanding_window() {
        // 宽容头部：j<n-1 时窗口为 [0..=j]（运行最值），与标量版短序列语义一致
        let s = [2.0, 5.0, 1.0, 4.0];
        assert_eq!(highest_roll(&s, 3), vec![2.0, 5.0, 5.0, 5.0]);
        assert_eq!(lowest_roll(&s, 3), vec![2.0, 2.0, 1.0, 1.0]);
    }

    #[test]
    fn std_slope_roll_head_is_nan_prefix() {
        // 严格头部：j+1<n → NaN（镜像标量版 len<n → NaN）
        let s = [1.0, 2.0, 3.0, 4.0];
        let sd = std_roll(&s, 3);
        assert!(sd[0].is_nan() && sd[1].is_nan());
        assert!((sd[2] - std(&s[..3], 3)).abs() < 1e-12);
        let sl = slope_roll(&s, 3);
        assert!(sl[0].is_nan() && sl[1].is_nan());
        assert!((sl[3] - 1.0).abs() < 1e-12); // 等差数列斜率=1
    }
```

追加到 `src/dsl/eval.rs` 的 `mod tests`（逐位条件解锁——Task 1 的存在意义）：

```rust
    #[test]
    fn rolling_forms_unlock_elementwise_conditions() {
        let ctx = ctx_from_closes(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        let f = |src: &str| eval(&parse_str(src).unwrap(), &ctx).unwrap();
        // highest_roll(close,2)=[1,2,3,4,5]（宽容头），close >= 它 ⇒ 逐位全真（创新高序列）
        assert_eq!(f("count(close >= highest(close, 2), 5) == 5"), Value::Bool(true));
        // barssince + 滚动 highest：最近一次创 2 根新高就在当前 bar
        assert_eq!(f("barssince(close >= highest(close, 2)) == 0"), Value::Bool(true));
        // 标量上下文不变：highest(close,3) 仍按末位取值 = 5
        assert_eq!(f("highest(close, 3) == 5"), Value::Bool(true));
        assert_eq!(f("slope(close, 5) > 0.9 and slope(close, 5) < 1.1"), Value::Bool(true));
    }
```

并**更新**既有语义锁测试（eval.rs `mod tests` 中 `count(highest(close,3) > lowest(close,3), 5) > 0` 断言 false 的那行，及其注释）：

```rust
        // Task1 滚动统一后：highest/lowest 在逐位条件中是滚动序列（不再是双标量弃权）。
        // fixture [1..5]：hi3=[1,2,3,4,5] vs lo3=[1,1,1,2,3] → 逐位 > = [F,T,T,T,T] → count=4
        assert_eq!(f("count(highest(close,3) > lowest(close,3), 5) == 4"), Value::Bool(true));
```

- [ ] **Step 2: 实现（indicators.rs 追加；标量版四函数一字不动）**

```rust
/// highest 的滚动序列版：位 j = 窗口 [max(0,j+1−n)..=j] 的 NaN 跳过最大值。
/// 头部为宽容扩张窗（与标量版 len<n 语义一致）；全 NaN 窗 → NaN。
pub fn highest_roll(s: &[f64], n: usize) -> Vec<f64> {
    let mut out = vec![f64::NAN; s.len()];
    if n == 0 {
        return out;
    }
    for j in 0..s.len() {
        let start = (j + 1).saturating_sub(n);
        let m = s[start..=j].iter().copied().fold(f64::NEG_INFINITY, f64::max);
        out[j] = if m == f64::NEG_INFINITY { f64::NAN } else { m };
    }
    out
}

/// lowest 的滚动序列版（语义镜像 highest_roll）。
pub fn lowest_roll(s: &[f64], n: usize) -> Vec<f64> {
    let mut out = vec![f64::NAN; s.len()];
    if n == 0 {
        return out;
    }
    for j in 0..s.len() {
        let start = (j + 1).saturating_sub(n);
        let m = s[start..=j].iter().copied().fold(f64::INFINITY, f64::min);
        out[j] = if m == f64::INFINITY { f64::NAN } else { m };
    }
    out
}

/// std 的滚动序列版：位 j+1<n → NaN（严格头，镜像标量版 len<n → NaN）；窗含 NaN → NaN 传播。
pub fn std_roll(s: &[f64], n: usize) -> Vec<f64> {
    let mut out = vec![f64::NAN; s.len()];
    if n == 0 {
        return out;
    }
    for j in 0..s.len() {
        if j + 1 < n {
            continue;
        }
        let w = &s[j + 1 - n..=j];
        let mean = w.iter().sum::<f64>() / n as f64;
        let var = w.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n as f64;
        out[j] = var.sqrt();
    }
    out
}

/// slope 的滚动序列版：OLS 斜率逐位；n<2 或 j+1<n → NaN（严格头）。
pub fn slope_roll(s: &[f64], n: usize) -> Vec<f64> {
    let mut out = vec![f64::NAN; s.len()];
    if n < 2 {
        return out;
    }
    for j in 0..s.len() {
        if j + 1 < n {
            continue;
        }
        let w = &s[j + 1 - n..=j];
        let nf = n as f64;
        let mean_x = (nf - 1.0) / 2.0;
        let mean_y = w.iter().sum::<f64>() / nf;
        let (mut num, mut den) = (0.0, 0.0);
        for (i, &y) in w.iter().enumerate() {
            let dx = i as f64 - mean_x;
            num += dx * (y - mean_y);
            den += dx * dx;
        }
        out[j] = if den == 0.0 { f64::NAN } else { num / den };
    }
    out
}
```

eval.rs 四臂改为（保持 `need` 校验不动，只换构造）：

```rust
        "highest" => { need(&vals, 2, name)?; Ok(Value::Series(indicators::highest_roll(&as_series(&vals[0])?, as_usize(&vals[1])?))) }
        "lowest"  => { need(&vals, 2, name)?; Ok(Value::Series(indicators::lowest_roll(&as_series(&vals[0])?, as_usize(&vals[1])?))) }
        "std"     => { need(&vals, 2, name)?; Ok(Value::Series(indicators::std_roll(&as_series(&vals[0])?, as_usize(&vals[1])?))) }
        "slope"   => { need(&vals, 2, name)?; Ok(Value::Series(indicators::slope_roll(&as_series(&vals[0])?, as_usize(&vals[1])?))) }
```

- [ ] **Step 3: GREEN + 全量 + clippy + Commit**

```bash
cargo test
cargo clippy --all-targets -- -D warnings
git add src/features/indicators.rs src/dsl/eval.rs
git status --porcelain
git commit -m "feat(dsl): rolling series forms for highest/lowest/std/slope, scalar-context semantics frozen" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

预期：除被刻意更新的语义锁测试外全量绿——任何其他测试变红都意味着末位等价被破坏，必须修实现而不是改测试。

---

## Task 2: `valuewhen(cond, expr[, occurrence])`

**Files:**
- Modify: `src/dsl/eval.rs`（count/barssince 截获区追加臂 + 测试）
- Modify: `src/tree/loader.rs:13-17`（RESERVED_FNS 22→23 加 "valuewhen"）

- [ ] **Step 1: RED 测试（eval.rs mod tests）**

```rust
    #[test]
    fn valuewhen_anchors_event_values() {
        // closes [1,2,3,4,3]：crossover(close,2.5) 事件仅在 idx2（2→3 上穿）
        let ctx = ctx_from_closes(&[1.0, 2.0, 3.0, 4.0, 3.0]);
        let f = |src: &str| eval(&parse_str(src).unwrap(), &ctx).unwrap();
        // 最近一次上穿时的 close = 3
        assert_eq!(f("valuewhen(crossover(close, 2.5), close) == 3"), Value::Bool(true));
        // occurrence=1：再往前一次——不存在 → NaN 弃权（比较恒 false）
        assert_eq!(f("valuewhen(crossover(close, 2.5), close, 1) > 0"), Value::Bool(false));
        // 从未触发 → NaN 弃权
        assert_eq!(f("valuewhen(close > 99, close) > 0"), Value::Bool(false));
        // 条件与取值序列尾对齐：ref(close,1)=[1,2,3,4]，事件位取移后值 = 2
        assert_eq!(f("valuewhen(crossover(close, 2.5), ref(close, 1)) == 2"), Value::Bool(true));
        // 锚定惯用法：最近一次创 2 根新高那根 bar 的 close（Task1 滚动版前置）
        assert_eq!(f("valuewhen(close >= highest(close, 2), close) == 3"), Value::Bool(true));
        // 参数个数校验
        assert!(eval(&parse_str("valuewhen(close > 0)").unwrap(), &ctx).is_err());
        assert!(eval(&parse_str("valuewhen(close > 0, close, 1, 2)").unwrap(), &ctx).is_err());
    }
```

注意倒数第 2 条断言的预期值推导（实现者自己演算一遍）：closes=[1,2,3,4,3]，`highest_roll(close,2)`=[1,2,3,4,4]，`close >=` 逐位 = [T,T,T,T,F]，最近真位 = idx3，close[3]=4——**所以断言应为 `== 4` 而非 3**。计划在此故意放了一个待修值：RED 阶段跑出实际值后修正断言为演算值，并在断言旁注明逐位演算（这一步是防"抄测试不思考"）。

- [ ] **Step 2: 实现（eval.rs，紧跟 "barssince" 臂之后）**

```rust
        "valuewhen" => {
            if !(2..=3).contains(&args.len()) {
                return Err(Error::Eval(format!(
                    "valuewhen expects 2 or 3 args (cond, expr[, occurrence]), got {}",
                    args.len()
                )));
            }
            let cond = eval_bool_series(&args[0], ctx)?;
            let vals = as_series(&eval(&args[1], ctx)?)?;
            let occ = if args.len() == 3 { as_usize(&eval(&args[2], ctx)?)? } else { 0 };
            let m = cond.len().min(vals.len());
            let (cond, vals) = (&cond[cond.len() - m..], &vals[vals.len() - m..]);
            let mut seen = 0usize;
            for j in (0..m).rev() {
                if cond[j] {
                    if seen == occ {
                        return Ok(Value::Scalar(vals[j]));
                    }
                    seen += 1;
                }
            }
            return Ok(Value::Scalar(f64::NAN)); // 触发次数不足 → 弃权
        }
```

（放进 count/barssince 的截获 `match`——条件参数必须原始 AST 逐位求值，不能走统一参数求值。）

loader.rs：`RESERVED_FNS` 数组加 `"valuewhen"`，长度 22→23。

- [ ] **Step 3: GREEN + 全量 + clippy + Commit**

```bash
cargo test
cargo clippy --all-targets -- -D warnings
git add src/dsl/eval.rs src/tree/loader.rs
git status --porcelain
git commit -m "feat(dsl): valuewhen(cond, expr[, occurrence]) event-anchored value lookup" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Task 3: 加载期 lint 层（L1 恒假陷阱 / L2 单长度条件空转）

**Files:**
- Create: `src/tree/lint.rs`
- Modify: `src/tree/mod.rs`（+ `pub mod lint;`——若 tree 模块以其他方式声明则跟随现状）
- Modify: `src/tree/loader.rs`（加载链路唯一 chokepoint 处调用 + eprintln）

- [ ] **Step 1: RED 测试（lint.rs 内嵌 mod tests，直调 lint_tree 断言返回值——不测 stderr）**

```rust
    use crate::tree::loader::load_tree_str;

    fn yaml_one_branch(when: &str) -> String {
        format!(
            r#"
meta: {{ name: t, forward_window: 2, stances: [long, flat] }}
root: gate
nodes:
  gate:
    type: quant
    branches:
      - {{ when: "{when}", goto: leaf_l, label: b }}
    default: {{ goto: leaf_f, label: d }}
leaves:
  leaf_l: {{ stance: long }}
  leaf_f: {{ stance: flat }}
"#
        )
    }

    #[test]
    fn l1_flags_constant_false_breakout() {
        // 经典 A1 陷阱：窗口含当前 bar，恒假
        let t = load_tree_str(&yaml_one_branch("close > highest(high, 20)")).unwrap();
        let w = lint_tree(&t);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("gate") && w[0].contains("恒假"), "{w:?}");
        // 镜像：lowest + < ；以及反转操作数次序
        assert_eq!(lint_tree(&load_tree_str(&yaml_one_branch("close < lowest(low, 20)")).unwrap()).len(), 1);
        assert_eq!(lint_tree(&load_tree_str(&yaml_one_branch("highest(high, 20) < close")).unwrap()).len(), 1);
    }

    #[test]
    fn l1_silent_on_ref_shifted_window() {
        // ref 移窗后是合法 Turtle 突破——不得误报
        let t = load_tree_str(&yaml_one_branch("close > highest(ref(high, 1), 20)")).unwrap();
        assert!(lint_tree(&t).is_empty());
    }

    #[test]
    fn l2_flags_length_one_condition() {
        // count 条件两侧均标量形 → 布尔序列长 1 → n>1 恒弃权空转
        let t = load_tree_str(&yaml_one_branch("count(bars_held > 2, 5) >= 3")).unwrap();
        let w = lint_tree(&t);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("count") && w[0].contains("弃权"), "{w:?}");
        // barssince 同理；valuewhen 同理
        assert_eq!(lint_tree(&load_tree_str(&yaml_one_branch("barssince(pos > 0) <= 3")).unwrap()).len(), 1);
        assert_eq!(lint_tree(&load_tree_str(&yaml_one_branch("valuewhen(pos > 0, close) > 1")).unwrap()).len(), 1);
        // and 任一侧塌缩到长 1 → 整体长 1，也要报
        assert_eq!(lint_tree(&load_tree_str(&yaml_one_branch("count(close > 2 and pos > 0, 5) >= 1")).unwrap()).len(), 1);
    }

    #[test]
    fn l2_silent_on_series_conditions() {
        let ok = [
            "count(close > ema(close, 5), 5) >= 3",
            "barssince(close < ema(close, 5)) <= 3",
            "count(crossover(close, ema(close, 5)), 5) >= 1",
            "count(close >= highest(close, 2), 5) == 5", // Task1 后 highest 是序列
        ];
        for w in ok {
            let t = load_tree_str(&yaml_one_branch(w)).unwrap();
            assert!(lint_tree(&t).is_empty(), "false positive on: {w}");
        }
    }

    #[test]
    fn all_example_trees_lint_clean() {
        // 防误报总闸：仓库全部示例树零告警
        for p in std::fs::read_dir("examples").unwrap() {
            let p = p.unwrap().path();
            if p.extension().and_then(|e| e.to_str()) != Some("yaml") {
                continue;
            }
            let t = crate::tree::loader::load_tree_file(&p).unwrap();
            let w = lint_tree(&t);
            assert!(w.is_empty(), "{}: {w:?}", p.display());
        }
    }
```

- [ ] **Step 2: 实现 src/tree/lint.rs**

```rust
//! 加载期 lint：检出"语法合法但语义必然空转/恒假"的条件写法，eprintln 告警不阻断。
//! 规则随 DSL 形态表演进——形态推断表（expr_shape）必须与 eval.rs 实际返回形态同步。

use crate::dsl::ast::{BinaryOp, Expr};
use crate::tree::loader::{Node, Strength, Tree, Weight};

/// 表达式形态：与 eval.rs 各臂返回的 Value 形态一一对应。
#[derive(Debug, Clone, Copy, PartialEq)]
enum Shape {
    Series,
    Scalar,
}

/// 返回 Series 的内置函数（Task1 后含 highest/lowest/std/slope）。与 eval.rs 同步维护。
const SERIES_FNS: [&str; 12] = [
    "sma", "ema", "wma", "rsi", "atr", "ref",
    "macd_line", "macd_signal", "macd_hist",
    "highest", "lowest", "std",
];

fn expr_shape(e: &Expr) -> Shape {
    match e {
        Expr::Number(_) => Shape::Scalar,
        Expr::Ident(name) => match name.as_str() {
            "hour" | "minute" | "dow" | "pos" | "entry_price" | "bars_held"
            | "unreal_pnl" | "max_price_since_entry" | "min_price_since_entry" => Shape::Scalar,
            _ => Shape::Series, // close/open/high/low/volume/aux.*/ctx.*
        },
        Expr::Index(..) => Shape::Scalar,
        Expr::Unary(..) => Shape::Scalar,
        Expr::Binary(..) => Shape::Scalar, // 算术/比较在标量语义下归约
        Expr::Call(name, _) => {
            if name == "slope" {
                return Shape::Series; // Task1 后 slope 也是序列
            }
            if SERIES_FNS.contains(&name.as_str()) {
                Shape::Series
            } else {
                Shape::Scalar // count/barssince/valuewhen/abs/min/max/sigmoid 等
            }
        }
        Expr::Cached(inner, _) => expr_shape(inner),
    }
}

/// 逐位条件的"长度类"：One = 必然塌缩到长度 1（count n>1/barssince/valuewhen 恒弃权）。
#[derive(Debug, Clone, Copy, PartialEq)]
enum LenClass {
    One,
    Many,
}

fn cond_len_class(e: &Expr) -> LenClass {
    match e {
        Expr::Binary(op, l, r) => match op {
            BinaryOp::And | BinaryOp::Or => {
                // 逐位组合尾对齐取公共长度：任一侧塌缩则整体塌缩
                if cond_len_class(l) == LenClass::One || cond_len_class(r) == LenClass::One {
                    LenClass::One
                } else {
                    LenClass::Many
                }
            }
            _ => {
                // 比较：至少一侧 Series 形才有逐位长度
                if expr_shape(l) == Shape::Series || expr_shape(r) == Shape::Series {
                    LenClass::Many
                } else {
                    LenClass::One
                }
            }
        },
        Expr::Unary(_, inner) => cond_len_class(inner), // not
        Expr::Call(name, args) if name == "crossover" || name == "crossunder" => {
            if args.iter().any(|a| expr_shape(a) == Shape::Series) {
                LenClass::Many
            } else {
                LenClass::One
            }
        }
        Expr::Cached(inner, _) => cond_len_class(inner),
        _ => LenClass::One, // 其余形态本就会被 eval 拒绝；保守归 One 不告警双份
    }
}

/// 裸价序列标识符（无 ref/索引移位）——L1 恒假陷阱的构成要件。
fn is_bare_price_ident(e: &Expr) -> Option<&str> {
    match e {
        Expr::Ident(n) if matches!(n.as_str(), "close" | "open" | "high" | "low") => Some(n),
        Expr::Cached(inner, _) => is_bare_price_ident(inner),
        _ => None,
    }
}

/// e 是否为 highest/lowest(裸价序列, _) 调用（首参未经 ref/索引移位）。
fn bare_window_call<'a>(e: &'a Expr, fname: &str) -> bool {
    match e {
        Expr::Call(n, args) if n == fname && !args.is_empty() => {
            is_bare_price_ident(&args[0]).is_some()
        }
        Expr::Cached(inner, _) => bare_window_call(inner, fname),
        _ => false,
    }
}

/// L1：`X > highest(Y, n)`（及镜像/换序）——窗口含当前 bar，条件恒假。
fn l1_check(e: &Expr, where_: &str, out: &mut Vec<String>) {
    if let Expr::Binary(op, l, r) = e {
        let hit = match op {
            BinaryOp::Gt | BinaryOp::Ge => {
                (is_bare_price_ident(l).is_some() && bare_window_call(r, "highest"))
                    || (bare_window_call(l, "lowest") && is_bare_price_ident(r).is_some())
            }
            BinaryOp::Lt | BinaryOp::Le => {
                (is_bare_price_ident(l).is_some() && bare_window_call(r, "lowest"))
                    || (bare_window_call(l, "highest") && is_bare_price_ident(r).is_some())
            }
            _ => false,
        };
        if hit {
            out.push(format!(
                "{where_}: 突破条件恒假——highest/lowest 窗口含当前 bar；\
                 表达\"前 N 根高/低点\"请先 ref(series, 1) 移窗（docs/dsl-reference.md A1 陷阱）"
            ));
        }
    }
    walk_children(e, &mut |c| l1_check(c, where_, out));
}

/// L2：count/barssince/valuewhen 的条件长度类为 One → 必然弃权空转。
fn l2_check(e: &Expr, where_: &str, out: &mut Vec<String>) {
    if let Expr::Call(name, args) = e
        && matches!(name.as_str(), "count" | "barssince" | "valuewhen")
        && !args.is_empty()
        && cond_len_class(&args[0]) == LenClass::One
    {
        out.push(format!(
            "{where_}: {name}(...) 条件两侧均为标量形——逐位布尔序列长度 1，\
             将恒弃权空转；至少一侧需要序列（close/ema(...)/ref(...) 等）"
        ));
    }
    walk_children(e, &mut |c| l2_check(c, where_, out));
}

fn walk_children(e: &Expr, f: &mut impl FnMut(&Expr)) {
    match e {
        Expr::Binary(_, l, r) => {
            f(l);
            f(r);
        }
        Expr::Unary(_, inner) | Expr::Cached(inner, _) => f(inner),
        Expr::Call(_, args) => args.iter().for_each(f),
        Expr::Index(inner, _) => f(inner),
        _ => {}
    }
}

/// 对整棵树跑全部 lint 规则，返回告警清单（调用方决定如何呈现）。
pub fn lint_tree(tree: &Tree) -> Vec<String> {
    let mut out = Vec::new();
    for (id, node) in &tree.nodes {
        if let Node::Quant { branches, .. } = node {
            for b in branches {
                let where_ = format!("node '{id}' when \"{}\"", b.when_src);
                l1_check(&b.when, &where_, &mut out);
                l2_check(&b.when, &where_, &mut out);
                if let Some(Strength::Expr(se)) = &b.strength {
                    l2_check(se, &format!("node '{id}' strength"), &mut out);
                }
            }
        }
    }
    for (id, leaf) in &tree.leaves {
        if let Weight::Expr(we) = &leaf.weight {
            l2_check(we, &format!("leaf '{id}' weight"), &mut out);
        }
    }
    out.sort();
    out
}
```

> ⚠️ 实现注意（实现者动手前核对，与现状不符就以现状为准并汇报）：
> - `Expr` 各 variant 名/结构以 `src/dsl/ast.rs` 实际为准（尤其 `Cached` 的字段形态、`Index` 的元组形）；
> - `Node::Quant`/`Branch`/`Strength`/`Weight` 的可见性——lint.rs 在同一 `tree` 模块内，`pub(crate)`/private 字段可达性先确认，必要时给 loader 加最小 `pub(crate)` 暴露而不是改公有 API；
> - `Tree.nodes` 的迭代类型（HashMap）→ 告警顺序不定，所以 `out.sort()` 保确定性。

- [ ] **Step 3: loader 接线**

在加载链路唯一 chokepoint（`load_tree_str_with_overrides` 构出 `Tree` 之后、返回之前；`load_tree_str`/`load_tree_file` 若是它的包装则自动继承）：

```rust
    for w in crate::tree::lint::lint_tree(&tree) {
        eprintln!("[rquant] tree lint: {w}");
    }
```

- [ ] **Step 4: GREEN + 全量 + clippy + Commit**

```bash
cargo test
cargo clippy --all-targets -- -D warnings
git add src/tree/lint.rs src/tree/mod.rs src/tree/loader.rs
git status --porcelain
git commit -m "feat(tree): load-time lint - constant-false breakout trap, length-one elementwise conditions" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

全部示例树零告警测试（`all_example_trees_lint_clean`）是防误报总闸——它红了说明规则太宽，收窄规则而不是改示例。

---

## Task 4: 文档同步 + 行为冻结验收

**Files:**
- Modify: `docs/dsl-reference.md`、`docs/tree-yaml-schema.md`

- [ ] **Step 1: dsl-reference.md**

1. 函数表：highest/lowest/std/slope 行的说明改为序列返回（注明"标量上下文取末位，语义与旧版逐位一致；highest/lowest 头部宽容扩张窗、std/slope 头部严格 NaN"）；新增 `valuewhen(cond, expr[, k])` 行（语义：最近第 k+1 次 cond=true 处的 expr 值；从未/次数不足 → NaN 弃权）。
2. "事件计数与逐位条件"一节：函数形态清单更新（滚动四件加入序列侧）；惯用法块追加现在合法的写法：

```yaml
# 突破事件序列（Task1 前会静默空转，现合法）：最近一次 Turtle 突破在 ≤5 根内
when: "barssince(close > highest(ref(high,1), 20)) <= 5"
# 事件锚定：最近一次上穿 EMA8 那根 bar 的收盘（measured move / 回踩锚）
when: "close < valuewhen(crossover(close, ema(close,8)), close) * 0.99"
```

3. 新增"加载期 lint"小节：两条规则、告警样例、"告警不阻断、规则保守宁缺勿滥"的定位。

- [ ] **Step 2: tree-yaml-schema.md**

保留名清单 `内置函数名` 行加 `valuewhen`；校验清单后追加一行指向 lint（"语义陷阱在加载期以 [rquant] tree lint: 前缀告警，不阻断加载"）。

- [ ] **Step 3: 行为冻结验收（real-data freeze gate）**

```bash
cargo run --release -- fetch --symbol sh600519 --scale 60 --datalen 1023 --adjust qfq --out tmps/p.csv
cargo run --release -- backtest --tree examples/regime_adaptive_1.yaml --primary tmps/p.csv --context tmps/p.csv --sim --warmup 80 --out tmps/freeze.json
```

**冻结基准（2026-06-11 实测，phase-1 改造前）：总收益 −0.0641 / 最大回撤 0.0885 / 回合 36 / 胜率 38.9% / 换手 41.6**。四舍五入到打印精度逐项相等 = 标量语义零变的实证闸（v2 树重度使用 highest/lowest 因子）。完毕删除 tmps/。网络不可用 → 跳过并在汇报注明（lint 的 example 零告警测试 + 末位等价锁仍是主闸）。

- [ ] **Step 4: 全量 + clippy + Commit**

```bash
cargo test
cargo clippy --all-targets -- -D warnings
git add docs/dsl-reference.md docs/tree-yaml-schema.md
git status --porcelain
git commit -m "docs(dsl): rolling-form shapes, valuewhen reference, load-time lint section" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## 附录 A：验收对照（自检）

| 评估缺口项 | 实现于 |
|---|---|
| #2 滚动形态统一（含 E2 当年被迫改写的根因） | T1（末位等价锁 + 逐位解锁测试）|
| #3 valuewhen 事件锚定 | T2 |
| #13 lint：A1 恒假陷阱 + 双标量空转 | T3（示例树零告警防误报总闸）|
| 向后兼容：标量语义零变 | T1 等价锁 + T4 真数据冻结闸 |

## 附录 B：明确不在本 phase（防 scope 蔓延）

派生序列一等公民（#1，需设计先行——逐位算术与现有标量语义的兼容形态）；at_entry 快照与节流状态量（#4/#6，动 state schema，须配黄金不变量与迁移）；日内锚定族（#5）；percentrank/corr/数学补全（#7/8/9）；树镜像宏（#11）。lint 不做成 error（永远告警不阻断）。
