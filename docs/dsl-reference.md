# DSL 参考手册

本文档基于 `src/dsl/{lexer,parser,ast,eval}.rs` 与 `src/features/indicators.rs` 的实际代码整理。

---

## 标识符

量化谓词中的裸标识符（无 `ctx.` 前缀）从 **primary 窗口**解析；带 `ctx.` 前缀的标识符从 **context 窗口**解析。

| 标识符 | 解析来源 | 说明 |
|---|---|---|
| `close` | primary | 主周期（如 15m）收盘价序列 |
| `open` | primary | 主周期开盘价序列 |
| `high` | primary | 主周期最高价序列 |
| `low` | primary | 主周期最低价序列 |
| `volume` | primary | 主周期成交量序列 |
| `ctx.close` | context | 大周期（如 1h）收盘价序列 |
| `ctx.open` | context | 大周期开盘价序列 |
| `ctx.high` | context | 大周期最高价序列 |
| `ctx.low` | context | 大周期最低价序列 |
| `ctx.volume` | context | 大周期成交量序列 |
| `hour` | 当前 bar 时间 | 小时，0–23（标量，不是序列） |
| `minute` | 当前 bar 时间 | 分钟，0–59（标量） |
| `dow` | 当前 bar 时间 | 星期几，1=周一 … 7=周日（ISO 序，标量） |

`resolve_series`（`eval.rs`）实现上述解析：前缀 `ctx.` 存在时路由到 `ctx.context`，否则路由到 `ctx.primary`。`hour`/`minute`/`dow` 在 `eval` 的 `Ident` 臂中优先匹配，直接从 `ctx.t`（当前 bar 时间戳）读取，不参与序列解析。

```yaml
# 示例：只在早盘（9:45–11:30）且非周五入场
when: "close > sma(close,5) and hour < 12 and dow < 5"
```

---

## 索引运算符 `series[-k]`

```
close[-1]    # 上一根收盘价（倒数第 2 个元素）
close[-3]    # 三根前收盘价
```

语义：对求值得到的 Series，取索引 `(len - 1) + k`（`k` 为负整数）。索引越界时求值报错。

---

## 算术与比较运算符

### 运算符优先级（由低到高）

| 优先级 | 运算符 | 示例 |
|---|---|---|
| 1 | `or` | `a or b` |
| 2 | `and` | `a and b` |
| 3 | `> < >= <= == !=` | `close > 10` |
| 4 | `+ -` | `close - sma(close,20)` |
| 5 | `* /` | `atr(14) * 2` |
| 6 | 前缀 `not` / 一元 `-` | `not crossover(close,sma(close,5))` |

括号 `( )` 可改变优先级。

### 归约语义（Series → 标量）

表达式的最终值类型需满足当前上下文：
- 分支 `when` 需要 **Bool**；
- `strength` 表达式需要 **Scalar**。

当一个 **Series** 出现在算术/比较表达式中时，取**最后一个元素**（即最新已收盘 bar 对应的值）。若 Series 为空，`as_scalar` 返回 `NaN`。

### NaN 弃权语义

所有比较运算符——包括 `==` 和 `!=`——在任一操作数为 `NaN` 时**一律返回 `false`**（代码见 `eval.rs` 的 `BinaryOp::Eq` 与 `BinaryOp::Ne`）：

```rust
BinaryOp::Eq => { let (a,b) = ...; !a.is_nan() && !b.is_nan() && a == b }
BinaryOp::Ne => { let (a,b) = ...; !a.is_nan() && !b.is_nan() && a != b }
```

这是**预热弃权**设计：当指标（如 `sma(close,10)`）处于预热期时返回 NaN，分支条件自动判 `false`，节点走 `default`，不会在预热期产生错误信号。

> **陷阱**：`rsi(close,14) == 50` 在 RSI 恰好等于 50 时为 `true`，但 `==` 是精确浮点比较，几乎不可能精确命中，建议用区间比较替代。

---

## 函数表

所有函数由 `eval_call`（`eval.rs`）调度，底层实现在 `indicators.rs`。

### 序列函数（返回 `Series`，长度与输入相同）

| 函数 | 参数 | 预热行为 | 说明 | 示例 |
|---|---|---|---|---|
| `sma(series, n)` | series: Series, n: int | 前 `n-1` 位为 NaN | 简单移动平均，窗口 n | `sma(close, 20)` |
| `ema(series, n)` | series: Series, n: int | `out[0] = s[0]`，无前缀 NaN | 指数移动平均，α=2/(n+1)，从第一根起算 | `ema(close, 12)` |
| `wma(series, n)` | series: Series, n: int | 前 `n-1` 位为 NaN | 线性加权移动平均，权重 1..n（最新最重） | `wma(close, 10)` |
| `rsi(series, n)` | series: Series, n: int | 前 `n` 位为 NaN | Wilder RSI；全涨→100，全跌→0 | `rsi(close, 14)` |
| `atr(n)` | n: int | 前 `n-1` 位为 NaN | Wilder ATR，自动取 primary 的 high/low/close | `atr(14)` |
| `macd_line(series, fast, slow)` | series: Series, fast/slow: int | 无（ema 从第一根起） | MACD 快线 = ema(fast) − ema(slow) | `macd_line(close, 12, 26)` |
| `macd_signal(series, fast, slow, sig)` | +sig: int | 无 | MACD 信号线 = ema(macd_line, sig) | `macd_signal(close, 12, 26, 9)` |
| `macd_hist(series, fast, slow, sig)` | +sig: int | 无 | MACD 柱 = macd_line − macd_signal | `macd_hist(close, 12, 26, 9)` |

### 标量函数（返回 `Scalar`，单个 f64）

| 函数 | 参数 | 不足时 | 说明 | 示例 |
|---|---|---|---|---|
| `slope(series, n)` | series: Series, n: int≥2 | NaN | 最近 n 根的线性回归斜率（OLS，x=0..n-1） | `slope(ema(close,20), 5)` |
| `highest(series, n)` | series: Series, n: int | NaN | 最近 n 根最高值 | `highest(high, 20)` |
| `lowest(series, n)` | series: Series, n: int | NaN | 最近 n 根最低值 | `lowest(low, 20)` |
| `std(series, n)` | series: Series, n: int | NaN | 最近 n 根总体标准差（÷n） | `std(close, 20)` |
| `sigmoid(x)` | x: Scalar | — | 1/(1+e^−x)，常用于 strength 表达式 | `sigmoid((close - sma(close,20)) / 0.5)` |

### 布尔函数（返回 `Bool`）

| 函数 | 参数 | 说明 | 示例 |
|---|---|---|---|
| `crossover(a, b)` | a, b: Series | 上穿：前一根 a≤b 且本根 a>b；序列不足 2 根时 false | `crossover(close, sma(close,20))` |
| `crossunder(a, b)` | a, b: Series | 下穿：前一根 a≥b 且本根 a<b；序列不足 2 根时 false | `crossunder(ema(close,5), ema(close,20))` |

---

## 模糊求值语义（`eval_fuzzy`）

`eval_fuzzy` 由 `strength: "auto"` / `"auto(scale)"` 触发，将布尔比较表达式映射为 [0,1] 的软真值。

### 比较运算符

```
sigmoid( (lhs - rhs) * sign / denom )
```
- `>` / `>=`：sign = +1；`<` / `<=`：sign = −1
- `denom = scale × max(|lhs|, |rhs|)`
- 当 `denom ≤ 1e-12`（两侧均约为 0）时，返回 **0.5**（无信息）

### 布尔组合子（Gödel 模糊逻辑）

| 运算符 | 模糊语义 |
|---|---|
| `and` | min(a, b) |
| `or` | max(a, b) |
| `not` | 1 − x |

### `==` / `!=`

保持**硬求值**：`eval_fuzzy` 对 `==` / `!=` 退回到 `as_bool(eval(expr))`，结果为 0.0 或 1.0。

### `auto` 默认 scale

`strength: "auto"` 默认 scale = **0.02**（2%）。
`strength: "auto(0.05)"` 将 scale 改为 0.05。Scale 必须 > 0，否则加载时报错。

### 适用范围

`eval_fuzzy` 只接受布尔表达式（比较或 and/or/not 组合）；传入纯数值或序列标识符时返回 `Err`。

---

## 命名因子与参数

决策树顶层的 `params`/`factors` 块允许把常用数值和子表达式提取为有名变量，在 `when`/`strength` 中直接引用：

```yaml
params: { ma_n: 20, mom_n: 5 }
factors:
  mom: "slope(ema(close, ma_n), mom_n)"
  above: "close > sma(close, ma_n)"
```

### 引用即内联展开

引用一个 `params`/`factors` 名字等价于**在该位置直接写出对应的字面量或子表达式**。展开在加载时由 `substitute`（`dsl/ast.rs`）执行，运行时的 AST 中不存在任何因子名 Ident——编译后的树与手写展开版本完全等价。

### 重复求值代价

同一因子在多处引用时，**各引用处独立重新求值**，运行时无共享缓存。若因子包含代价较高的计算（如 `ema(close, 200)`），应尽量减少重复引用次数，或将多个引用合并到同一个更高层的因子表达式中。

### 有序引用规则

`factors` 按 YAML 文档顺序处理，每个因子只能引用前序定义的名字，不能向后引用——这保证了因子间无隐式循环依赖，且展开结果唯一确定。

详细的命名限制与加载期报错行为见 [docs/tree-yaml-schema.md](tree-yaml-schema.md) 的"params 与 factors 块"一节。
