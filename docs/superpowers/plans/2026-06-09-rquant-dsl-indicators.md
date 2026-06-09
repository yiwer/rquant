# rquant DSL 指标补齐（wma/macd/std）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 DSL 已声明但缺失/占位的指标 `wma`（线性加权）、`macd_line`/`macd_signal`/`macd_hist`、`std`（总体），并接进 DSL 求值器。

**Architecture:** 在 M1–M6+（HEAD `2158071`）上扩展。`features/indicators.rs` 追加 5 个纯函数（复用既有 `ema`）；`dsl/eval.rs` 的 `eval_call` 把 `wma` 改真实实现并新增 macd×3 与 std 分支。零新依赖。

**Tech Stack:** Rust 2024 + 既有（approx dev-dep 用于测试）。

> 设计依据：`docs/superpowers/specs/2026-06-09-rquant-dsl-indicators-design.md`。
> 提交信息用英文。单元测试用同文件 `#[cfg(test)] mod tests`。

---

## 文件结构
```
改动: src/features/indicators.rs  # + wma / std / macd_line / macd_signal / macd_hist
改动: src/dsl/eval.rs             # eval_call: wma 改真实、加 macd×3、加 std
```

---

## Task 1: indicators — wma / std / macd

**Files:**
- Modify: `src/features/indicators.rs`（追加 5 个函数 + 3 个测试）
- Test: 同文件

- [ ] **Step 1: 在 `mod tests` 内追加失败测试**

```rust
    #[test]
    fn wma_known_value() {
        let out = wma(&[1.0, 2.0, 3.0], 3);
        assert!(out[0].is_nan());
        assert!(out[1].is_nan());
        assert_relative_eq!(out[2], 14.0 / 6.0); // (1*1 + 2*2 + 3*3) / (1+2+3)
    }

    #[test]
    fn std_population() {
        // [1,2,3,4,5]: mean 3, var=(4+1+0+1+4)/5=2, std=sqrt(2)
        assert_relative_eq!(std(&[1.0, 2.0, 3.0, 4.0, 5.0], 5), 2.0_f64.sqrt());
    }

    #[test]
    fn macd_zero_on_constant_series() {
        let s = vec![5.0; 30];
        assert!(macd_line(&s, 12, 26).last().unwrap().abs() < 1e-9);
        assert!(macd_signal(&s, 12, 26, 9).last().unwrap().abs() < 1e-9);
        assert!(macd_hist(&s, 12, 26, 9).last().unwrap().abs() < 1e-9);
    }
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test --lib indicators`
Expected: 编译失败（`wma`/`std`/`macd_line`/`macd_signal`/`macd_hist` 未定义）。

- [ ] **Step 3: 追加实现（在 `src/features/indicators.rs` 末尾、`#[cfg(test)]` 之前）**

```rust
/// 线性加权移动平均（权重 1..n，最新最重）；前 n-1 位为 NaN。
pub fn wma(s: &[f64], n: usize) -> Vec<f64> {
    let len = s.len();
    let mut out = vec![f64::NAN; len];
    if n == 0 || len < n {
        return out;
    }
    let denom = (n * (n + 1) / 2) as f64;
    for i in (n - 1)..len {
        let mut acc = 0.0;
        for k in 0..n {
            acc += s[i - n + 1 + k] * (k + 1) as f64;
        }
        out[i] = acc / denom;
    }
    out
}

/// 最近 n 根的总体标准差（÷n）；不足返回 NaN。
pub fn std(s: &[f64], n: usize) -> f64 {
    let len = s.len();
    if n == 0 || len < n {
        return f64::NAN;
    }
    let w = &s[len - n..];
    let mean = w.iter().sum::<f64>() / n as f64;
    let var = w.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n as f64;
    var.sqrt()
}

/// MACD 快线：ema(fast) - ema(slow) 逐点（ema 等长，下标对齐）。
pub fn macd_line(s: &[f64], fast: usize, slow: usize) -> Vec<f64> {
    let f = ema(s, fast);
    let g = ema(s, slow);
    f.iter().zip(g.iter()).map(|(a, b)| a - b).collect()
}

/// MACD 信号线：ema(macd_line, sig)。
pub fn macd_signal(s: &[f64], fast: usize, slow: usize, sig: usize) -> Vec<f64> {
    ema(&macd_line(s, fast, slow), sig)
}

/// MACD 柱：macd_line - macd_signal 逐点。
pub fn macd_hist(s: &[f64], fast: usize, slow: usize, sig: usize) -> Vec<f64> {
    let line = macd_line(s, fast, slow);
    let signal = macd_signal(s, fast, slow, sig);
    line.iter().zip(signal.iter()).map(|(a, b)| a - b).collect()
}
```

- [ ] **Step 4: 运行验证通过**

Run: `cargo test --lib indicators`
Expected: 既有 7 个 + 新增 3 个 = 10 个 PASS。

- [ ] **Step 5: Commit**

```bash
git add src/features/indicators.rs
git commit -m "feat(features): wma, std (population), macd_line/signal/hist indicators" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: dsl/eval — 接线 wma/macd/std

**Files:**
- Modify: `src/dsl/eval.rs`（`eval_call`：wma 改真实 + 新增 4 个分支；追加 eval 测试）
- Test: 同文件

- [ ] **Step 1: 在 `mod tests` 内追加失败测试**

（该模块已有 `ctx_from_closes` 助手与 `use crate::dsl::parser::parse_str;`。）
```rust
    #[test]
    fn wma_std_macd_eval() {
        let ctx = ctx_from_closes(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        // wma(close,3) 末值 = (3*1+4*2+5*3)/6 = 26/6 > 0
        assert_eq!(eval(&parse_str("wma(close,3) > 0").unwrap(), &ctx).unwrap(), Value::Bool(true));
        // std(close,5) = sqrt(2)（标量）
        match eval(&parse_str("std(close,5)").unwrap(), &ctx).unwrap() {
            Value::Scalar(x) => assert!((x - 2.0_f64.sqrt()).abs() < 1e-9),
            other => panic!("expected scalar, got {other:?}"),
        }
        // macd_line / macd_hist 求值成功（Series；比较中归约为标量）
        assert_eq!(eval(&parse_str("macd_line(close,3,5) > -1000.0").unwrap(), &ctx).unwrap(), Value::Bool(true));
        assert_eq!(eval(&parse_str("macd_hist(close,3,5,2) > -1000.0").unwrap(), &ctx).unwrap(), Value::Bool(true));
    }
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test --lib dsl::eval`
Expected: 失败 —— `std`/`macd_line`/`macd_hist` 当前是 unknown function（`wma(close,3)>0` 因 wma 用 sma 占位会"通过"，但 std/macd 行报错 → 测试 panic/Err）。

- [ ] **Step 3: 改 `eval_call`**

(a) 把现有 `wma` 分支
```rust
        "wma" => { need(&vals, 2, name)?; Ok(Value::Series(indicators::sma(&as_series(&vals[0])?, as_usize(&vals[1])?))) } // 见说明
```
改为
```rust
        "wma" => { need(&vals, 2, name)?; Ok(Value::Series(indicators::wma(&as_series(&vals[0])?, as_usize(&vals[1])?))) }
```
(b) 在 `crossunder` 分支之后、`_ => Err(...)` 之前，新增 4 个分支：
```rust
        "macd_line" => { need(&vals, 3, name)?; Ok(Value::Series(indicators::macd_line(&as_series(&vals[0])?, as_usize(&vals[1])?, as_usize(&vals[2])?))) }
        "macd_signal" => { need(&vals, 4, name)?; Ok(Value::Series(indicators::macd_signal(&as_series(&vals[0])?, as_usize(&vals[1])?, as_usize(&vals[2])?, as_usize(&vals[3])?))) }
        "macd_hist" => { need(&vals, 4, name)?; Ok(Value::Series(indicators::macd_hist(&as_series(&vals[0])?, as_usize(&vals[1])?, as_usize(&vals[2])?, as_usize(&vals[3])?))) }
        "std" => { need(&vals, 2, name)?; Ok(Value::Scalar(indicators::std(&as_series(&vals[0])?, as_usize(&vals[1])?))) }
```

- [ ] **Step 4: 运行验证通过 + 全量 + clippy**

Run: `cargo test --lib dsl::eval`
Expected: 新增 `wma_std_macd_eval` 等全 PASS。

Run: `cargo test`
Expected: 全量全绿。

Run: `cargo clippy --all-targets`
Expected: 无告警（**平铺执行，勿用 `2>&1`**）。

- [ ] **Step 5: Commit**

```bash
git add src/dsl/eval.rs
git commit -m "feat(dsl): wire wma(real)/macd_line/macd_signal/macd_hist/std into evaluator" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## 附录 A：Spec 覆盖对照（自检）
| Spec 章节 | 实现于 |
|---|---|
| §4 wma/std/macd_line/macd_signal/macd_hist | Task 1 |
| §5 DSL 接线（wma 真实 + macd×3 + std，参数/返回类型）| Task 2 |
| §6 测试（指标已知值 + DSL 求值）| Task 1 / Task 2 |
| §7 错误处理（need 参数校验；NaN 弃权语义不变）| Task 2（need 已有）|

## 附录 B：明确不在范围（YAGNI）
- 其它指标（KDJ/BOLL 带/OBV…）；macd 内置默认参数；样本标准差（÷n-1）；成交额加权 wma。
- `macd_hist` 内部重算了一次 `macd_line`（一次在直接调用、一次经 `macd_signal`）——可读性优先，性能可忽略，不优化。
