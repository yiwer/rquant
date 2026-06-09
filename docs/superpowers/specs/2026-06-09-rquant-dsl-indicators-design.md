# rquant：补齐 DSL 指标 wma / macd / std — 设计文档

- **日期**：2026-06-09
- **状态**：设计已确认（用户已批准默认值，写 spec + 计划）
- **关联**：M1–M6 + 后续 follow-up 已合并 master（HEAD `2158071`）。本次收掉 DSL 函数集的占位/缺口（M1–M4 spec §7.3 列出但未实现：`wma` 用 sma 占位、`macd_*`/`std` 未实现）。

---

## 1. 背景

DSL v1 函数集（spec §7.3）声明了 `wma`/`macd_*`/`std`，但实现里：`wma` 在 `dsl/eval.rs` 暂用 `indicators::sma` 占位，`macd_line`/`macd_signal`/`macd_hist`/`std` 完全未实现（调用会 `Error::Eval("unknown function")`）。本次补齐为真实实现，使决策树可用这些指标。

## 2. 目标与非目标

### 目标
1. `features/indicators.rs` 实现 5 个函数：`wma`、`macd_line`、`macd_signal`、`macd_hist`、`std`。
2. `dsl/eval.rs` 的 `eval_call` 接线：`wma` 改真实实现、新增 macd×3 与 std。
3. 返回类型符合 spec §7.3：`wma`/`macd_*` → 序列；`std` → 标量。

### 非目标（YAGNI）
- 其它指标（KDJ、BOLL 带、OBV 等）——本次只补已声明的三类。
- macd 内置默认参数（12/26/9）——DSL 显式传参，与 `sma`/`ema` 一致。
- 复权/成交额加权等变体。

## 3. 锁定决策

| # | 维度 | 选定 |
|---|---|---|
| 1 | `std` 除数 | **总体标准差（÷n）**（与现有 `metrics` variance 一致、Bollinger 惯例）|
| 2 | `wma` 权重 | **线性加权**（1..n，最新最重）|
| 3 | `macd` 形态 | 拆成 3 个函数 `macd_line`/`macd_signal`/`macd_hist`，参数显式（无内置 12/26/9）|

## 4. 指标实现（`features/indicators.rs`）

约定同既有指标：序列型返回与输入等长的 `Vec<f64>`，预热不足前缀填 `NaN`；标量型不足返回 `NaN`。复用既有 `ema`。

- `pub fn wma(s: &[f64], n: usize) -> Vec<f64>`
  线性加权：对 `i >= n-1`，`wma[i] = (Σ_{k=0}^{n-1} s[i-n+1+k]·(k+1)) / (n(n+1)/2)`；否则 NaN。`n==0` → 全 NaN。
- `pub fn macd_line(s: &[f64], fast: usize, slow: usize) -> Vec<f64>`
  `ema(s, fast)[i] - ema(s, slow)[i]` 逐点（任一为 NaN 则该点 NaN，f64 减法自然传播 NaN）。
- `pub fn macd_signal(s: &[f64], fast: usize, slow: usize, sig: usize) -> Vec<f64>`
  `ema(macd_line(s, fast, slow), sig)`。
- `pub fn macd_hist(s: &[f64], fast: usize, slow: usize, sig: usize) -> Vec<f64>`
  `macd_line[i] - macd_signal[i]` 逐点。
- `pub fn std(s: &[f64], n: usize) -> f64`
  最近 n 根总体标准差：`len<n || n==0` → NaN；否则窗口 `&s[len-n..]`，`mean = Σ/n`，`var = Σ(x-mean)² / n`，返回 `var.sqrt()`。

> 注：`macd_line` 逐点相减时若两个 ema 长度一致（都等于 `s.len()`），按下标对齐即可。`ema` 已保证返回等长序列（`out[0]=s[0]`，无 NaN 前缀），故 macd 序列从 index 0 起有值（不像 sma 有 NaN 前缀）——可接受（与 ema 行为一致）。

## 5. DSL 接线（`dsl/eval.rs` `eval_call`）

把 `"wma"` 分支从 `indicators::sma` 改为 `indicators::wma`；新增：
```rust
"macd_line"   => { need(&vals, 3, name)?; Ok(Value::Series(indicators::macd_line(&as_series(&vals[0])?, as_usize(&vals[1])?, as_usize(&vals[2])?))) }
"macd_signal" => { need(&vals, 4, name)?; Ok(Value::Series(indicators::macd_signal(&as_series(&vals[0])?, as_usize(&vals[1])?, as_usize(&vals[2])?, as_usize(&vals[3])?))) }
"macd_hist"   => { need(&vals, 4, name)?; Ok(Value::Series(indicators::macd_hist(&as_series(&vals[0])?, as_usize(&vals[1])?, as_usize(&vals[2])?, as_usize(&vals[3])?))) }
"std"         => { need(&vals, 2, name)?; Ok(Value::Scalar(indicators::std(&as_series(&vals[0])?, as_usize(&vals[1])?))) }
```
`wma` 保持 2 参、返回 Series。其余 DSL 语义（序列在比较/算术归约为最新值、`as_series`/`as_usize` 等）不变。

## 6. 测试

### 指标单测（`features/indicators.rs`，已知值）
- `wma`: `wma(&[1.0,2.0,3.0], 3)` 末值 `= 14/6 ≈ 2.3333`；前两位 NaN。
- `std`: `std(&[1.0,2.0,3.0,4.0,5.0], 5) = √2 ≈ 1.41421356`（总体）。
- `macd_line`: 常数序列 `[5.0;30]` → 末值 `= 0.0`（两 ema 相等）。
- `macd_signal` / `macd_hist`: 常数序列 → 末值 `= 0.0`（line 全 0 → signal 0 → hist 0）。

### DSL 求值单测（`dsl/eval.rs`）
复用既有 `ctx_from_closes` 测试助手：
- `wma(close,3)` 在 `[1,2,3,4,5]` → `Value::Series`，末值 ≈ wma 末值（或在比较中验证）。
- `std(close,5)` → `Value::Scalar(√2)`。
- `macd_line(close,3,5)` / `macd_hist(close,3,5,2)` 求值成功（返回 Series）。
- `wma(close,3) > 0` → `Value::Bool(true)`（验证 wma 接线 + 归约）。

## 7. 错误处理
参数个数不符 → `eval_call` 的 `need` 返回 `Error::Eval`；预热/空序列 → NaN（既有归约语义：NaN 比较为 false → 分支弃权）。不新增错误路径。

## 8. 里程碑
- **T1** `features/indicators.rs`：`wma`/`std`/`macd_line`/`macd_signal`/`macd_hist` + 单测。
- **T2** `dsl/eval.rs`：`eval_call` 接线（wma 改真实、加 macd×3、加 std）+ eval 单测。同时更新计划/spec 中"wma 占位"的注释表述（代码注释里若有"stand-in"字样一并去掉）。
