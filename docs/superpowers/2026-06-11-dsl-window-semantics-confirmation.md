# 确认报告：highest/lowest 窗口语义与内置函数签名契约

- **日期**：2026-06-11
- **触发**：`examples/regime_adaptive_1.yaml`（双态自适应 v1）加载前的两项写法假设核实（原 A1/A2）
- **结论提交**：`976bf9f` feat(dsl): ref(series,k) shift fn; fix highest/lowest all-NaN window（分支 `f7b-adjust`）

---

## 问题 1（原 A1）：highest/lowest 窗口是否包含当前 bar？

**结论：包含。原 YAML 写法 `close > rng_hi` 数学恒假，四条突破分支为死代码。已通过新增 `ref(series, k)` 修复。**

### 证据链

| 环节 | 位置 | 事实 |
|---|---|---|
| 决策时点 | `src/backtest/sim.rs:266-273` | Context 在 `t = primary[i].time` 处构建 |
| 窗口闸门 | `src/features/context.rs:40` | `partition_point(\|b\| b.time <= t)`——**第 i 根自身在窗口内** |
| 窗口取值 | `src/features/indicators.rs` `highest`/`lowest` | 取 `s[len-n..]`，即**含末根**的最近 n 根 |
| 求值路径 | `src/dsl/eval.rs` `eval_call` | 直接对可见窗口求值，无任何移位 |

由此 `rng_hi = highest(high, n) ≥ high[i] ≥ close[i]`，严格不等式 `close > rng_hi` 永不成立（close 收在最高点也只能取等）。

### 受影响范围（修复前）

`examples/regime_adaptive_1.yaml` 四条分支及其下游子树不可达：

- `range_ctx` 节点：`close > rng_hi`（channel_break_up）、`close < rng_lo`（channel_break_dn）→ `range_break_up/dn` 子树全死
- 趋势节点：`close > swing_hi`（swing_breakout）、`close < swing_lo`（swing_breakdown）→ `bull_break_check`/`bear_break_check` 子树全死

且修复前 DSL **无法在 YAML 层面绕过**：无 ref/shift 函数；`series[-k]` 索引把序列归约为标量，不能产生移位序列。

### 处置

1. **新增 `ref(series, k)`**（`src/dsl/eval.rs`）：去掉序列末 k 根（输出长度 = len−k），即「k 根前可见的序列」；`k=0` 恒等；`k ≥ 长度` → 空序列 → NaN 弃权。
2. **保持 highest/lowest inclusive 语义不变**（设计取舍）：`rng_pos` 的 [0,1] 区间保证与既有文档契约依赖它；exclusive 语义由 `ref` 组合表达，正交且向后兼容。
3. **YAML 修正**：`rng_hi`/`rng_lo`/`swing_hi`/`swing_lo` 四个因子改为 `highest(ref(high,1), n)` / `lowest(ref(low,1), n)`，恢复 Turtle「前 N 根高/低点」原义。注意 `rng_pos` 在突破 bar 上可越出 [0,1]——安全，`range_ctx` 先检查突破分支再走分位分支。
4. **加载期防护**：`ref` 加入 loader `RESERVED_FNS`，factor 重名在加载期拒绝。

---

## 问题 2（原 A2）：函数签名与语义是否文档化、NaN 弃权是否为保证契约？

**结论：A2 假设全部成立；签名表已存在于 `docs/dsl-reference.md`（函数表一节）；NaN 弃权是有测试背书的契约——但核实过程中发现并修复了该契约的一个漏洞。**

### 签名核实

| 假设 | 实际 | 判定 |
|---|---|---|
| `rsi(close, n)` | `rsi(series, n)`，Wilder RSI，前 n 位 NaN | ✅ |
| `atr(n)` | 单参数，**隐式取 primary 的 high/low/close**（无 ctx 周期版本，写在 `ctx.` 表达式里也只算 primary），Wilder，前 n−1 位 NaN | ✅ |
| `slope` 为回归斜率，单位价格/bar | `slope(series, n)`，OLS，x = 0..n−1（bar 序号），单位 = 输入单位/bar，返回标量；`n<2` 或长度不足 → NaN | ✅ |

### NaN 弃权契约

- **契约内容**：所有比较运算符（含 `==`/`!=`）任一操作数为 NaN 时一律返回 false，分支弃权落 default。
- **背书**：文档 `docs/dsl-reference.md`「NaN 弃权语义」一节 + 回归测试 `nan_comparisons_abstain_including_ne`（`src/dsl/eval.rs`）。**是保证契约，不是巧合。**
- **发现的漏洞（已修）**：`highest`/`lowest` 用 `f64::max`/`min` 折叠，IEEE 语义跳过 NaN；**全 NaN/空窗口返回 ±∞ 而非 NaN**，使 `close < lowest(sma(close,20), n)` 这类表达式在深度预热期得到 `close < +∞ = true`——该弃权的分支反而触发。修复为：无有限值时返回 NaN；窗口内混有 NaN 仍跳过取有限值（既定行为，已用测试锁定并写入文档）。

---

## 验证证据

- **TDD**：5 个新断言先以预期原因失败（`unknown function: ref` ×2、`-inf` 非 NaN、遮蔽未拒、元数错误信息不符），实现后转绿。
- **测试清单**：
  - `dsl::eval`：`ref_shifts_series_for_turtle_breakout`（含旧写法恒假的对照断言）、`ref_beyond_history_abstains`、`ref_wrong_arity_errors`
  - `features::indicators`：`highest_lowest_all_nan_abstains`、`highest_lowest_skip_nan_keep_finite`
  - `tree::loader`：`params_and_factors_inline_and_validate`（ref 遮蔽拒绝）、`loads_regime_adaptive_example`（示例树加载回归）
- **全量**：193 lib + 16 e2e 全通过，clippy 零警告（提交前新鲜运行）。

## 文档更新

`docs/dsl-reference.md`：序列函数表补 `ref(series, k)` 条目；`highest`/`lowest` 行标注**含当前 bar** 与 NaN 行为；新增陷阱 callout 给出 Turtle 突破标准写法：

```yaml
when: "close > highest(ref(high, 1), 20)"   # 突破前 20 根高点
when: "close < lowest(ref(low, 1), 20)"     # 跌破前 20 根低点
```

## 残留注意事项（未在本次范围内）

1. `atr(n)` 无大周期版本：`ctx.` 表达式中亦取 primary，跨周期 ATR 需求需另行实现。
2. factors 引用为内联展开、无运行时缓存：`rng_hi` 等被多分支引用时各处重复求值，性能敏感时注意。
3. 本提交落在 `f7b-adjust` 分支（提交时工作区分支已被并行的 F-7b 复权工作线切换），合入 master 时随该分支一并处理。
