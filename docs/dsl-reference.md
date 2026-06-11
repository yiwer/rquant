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
| `pos` | `Context.sim`（sim 模式） | 当前持仓量，∈[−1,1]（标量）；非 sim 模式默认 `0.0` |
| `entry_price` | `Context.sim`（sim 模式） | 入场均价（标量）；空仓或非 sim 模式为 `NaN`，引用它的比较自动弃权（false） |
| `bars_held` | `Context.sim`（sim 模式） | 当前持仓已持 bar 数（标量，从开仓收盘后的第 1 根起计）；非 sim 模式默认 `0.0` |
| `unreal_pnl` | `Context.sim`（sim 模式） | 浮动损益率 `(close/entry_price−1)×sign(pos)`（标量）；空仓或非 sim 模式默认 `0.0` |
| `max_price_since_entry` | `Context.sim`（sim 模式） | 入场以来（含入场执行 bar）最高 `high`（标量）；空仓/非 sim 为 NaN → 比较弃权。Chandelier：`close < max_price_since_entry - 3*atr(22)` |
| `min_price_since_entry` | `Context.sim`（sim 模式） | 入场以来最低 `low`（标量）；空仓/非 sim 为 NaN → 比较弃权。MFE/MAE 自行推导：`max_price_since_entry/entry_price - 1` |
| `aux.<表名>.<列名>` | 挂载的外部 aux 表 | `--aux <表名>=path.csv` 中 `<列名>` 对应的数值序列（time≤t 截断后的可见部分） |

`resolve_series`（`eval.rs`）实现上述解析：前缀 `aux.` 存在时路由到对应 `AuxView`，前缀 `ctx.` 存在时路由到 `ctx.context`，否则路由到 `ctx.primary`。`hour`/`minute`/`dow` 与 `pos`/`entry_price`/`bars_held`/`unreal_pnl`/`max_price_since_entry`/`min_price_since_entry` 在 `eval` 的 `Ident` 臂中优先匹配，直接从 `ctx` 读取，不参与序列解析。

> **`entry_price` NaN 弃权**：空仓时 `entry_price = NaN`，与 NaN 的任何比较（`>`/`<`/`==`/`!=`）均返回 `false`。因此 `entry_price > 0` 在空仓时永远为 false，可安全用于分支条件而无需额外判空。同理，`max_price_since_entry`/`min_price_since_entry` 在入场决策点（`pos` 尚为 0）亦为 NaN——首个有效值在入场执行 bar 收盘后、下一个决策点才可见。

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
| `ref(series, k)` | series: Series, k: int≥0 | `k ≥ 长度` → 空序列 → NaN 弃权 | 去掉末 k 根（输出长度 = len−k），即"k 根前可见的序列"；`k=0` 恒等 | `highest(ref(high,1), 20)` |

### 标量函数（返回 `Scalar`，单个 f64）

| 函数 | 参数 | 不足时 | 说明 | 示例 |
|---|---|---|---|---|
| `slope(series, n)` | series: Series, n: int≥2 | NaN | 最近 n 根的线性回归斜率（OLS，x=0..n-1，单位 = 输入单位/bar） | `slope(ema(close,20), 5)` |
| `highest(series, n)` | series: Series, n: int | NaN | 最近 n 根最高值（**含当前 bar**，见下方陷阱）；窗口内 NaN 跳过，无有限值时返回 NaN（弃权） | `highest(high, 20)` |
| `lowest(series, n)` | series: Series, n: int | NaN | 最近 n 根最低值（**含当前 bar**）；NaN 行为同 highest | `lowest(low, 20)` |
| `std(series, n)` | series: Series, n: int | NaN | 最近 n 根总体标准差（÷n） | `std(close, 20)` |
| `sigmoid(x)` | x: Scalar | — | 1/(1+e^−x)，常用于 strength 表达式 | `sigmoid((close - sma(close,20)) / 0.5)` |
| `abs(x)` | x: Scalar | — | 绝对值 | `abs(close - entry_price)` |
| `max(a, b)` | a, b: Scalar | 任一 NaN → NaN | 较大值；**显式 NaN 传播**（不吃弃权） | `max(pos, 0.25)` |
| `min(a, b)` | a, b: Scalar | 任一 NaN → NaN | 较小值；NaN 传播同 max | `min(1, pos + 0.25)` |
| `count(cond, n)` | cond: 布尔表达式, n: int≥1 | 序列 < n → NaN | 末 n 位中 cond 为 true 的个数；cond **逐位**求值（见下节） | `count(close > ema(close,20), 10)` |
| `barssince(cond)` | cond: 布尔表达式 | 从未 true → NaN | 距最近一次 cond=true 的 bar 数（当前 bar=0） | `barssince(crossover(close, sma(close,20)))` |

> **陷阱：`highest`/`lowest` 窗口含当前 bar**。`close > highest(high, n)` 恒假（窗口最大值 ≥ 当前 high ≥ 当前 close，严格大于永不成立）。表达 Turtle"超过前 N 根高点"的突破语义必须先用 `ref` 移掉当前 bar：
>
> ```yaml
> when: "close > highest(ref(high, 1), 20)"   # 突破前 20 根高点
> when: "close < lowest(ref(low, 1), 20)"     # 跌破前 20 根低点
> ```

### 布尔函数（返回 `Bool`）

| 函数 | 参数 | 说明 | 示例 |
|---|---|---|---|
| `crossover(a, b)` | a, b: Series | 上穿：前一根 a≤b 且本根 a>b；序列不足 2 根时 false | `crossover(close, sma(close,20))` |
| `crossunder(a, b)` | a, b: Series | 下穿：前一根 a≥b 且本根 a<b；序列不足 2 根时 false | `crossunder(ema(close,5), ema(close,20))` |

---

## 事件计数与逐位条件（`count` / `barssince`）

`count`/`barssince` 的条件参数不走「Series → 取末元素」归约，而是**逐位**求值成布尔序列：

- 比较（`> < >= <= == !=`）：两侧序列**尾对齐**（取右端公共长度；标量广播），逐位比较；任一侧该位 NaN → 该位 false（NaN 弃权逐位生效）。
- `and` / `or` / `not`：逐位组合。
- `crossover(a, b)` / `crossunder(a, b)`：逐位事件序列——位 j 为 true 当且仅当前一位未越线且本位越线；首位与含 NaN 位恒 false。**注意与普通 `when` 上下文的标量版语义并存**：普通上下文里 crossover 只看末两位返回单个 Bool，条件序列上下文里它是整个窗口的事件序列。
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

### 因子按决策点 memoize

同一因子在多处引用时共享一个缓存槽（加载期由 loader 包裹 `Cached` 节点）：**每个决策点首个引用处真算一次，其余引用命中缓存**。语义与内联展开完全等价（因子是 Context 的纯函数），高频引用的重型因子（如 `atr(14)`、`ema(close,200)`）不再有重复求值代价。缓存随 Context 新建/销毁，不跨决策点、不跨标的。

例外：`strength: "auto"` 的模糊求值路径对布尔因子透传重算（模糊真值依赖 scale，不消费缓存值），正确性不受影响——只是该路径上布尔因子无缓存收益；其内部嵌套的数值因子照常命中缓存。

### 有序引用规则

`factors` 按 YAML 文档顺序处理，每个因子只能引用前序定义的名字，不能向后引用——这保证了因子间无隐式循环依赖，且展开结果唯一确定。

详细的命名限制与加载期报错行为见 [docs/tree-yaml-schema.md](tree-yaml-schema.md) 的"params 与 factors 块"一节。

---

## 外部 aux 序列（`aux.<表>.<列>`）

通过 `--aux name=path.csv` 挂载的外部数值序列，DSL 以 `aux.<表名>.<列名>` 三段格式引用：

```yaml
when: "close/close[-5] > aux.idx.v/aux.idx.v[-5]"
```

### time≤t 闸门

`build_context` 对每个决策点 `t`，将 aux 表按 `time ≤ t` 截断，向 DSL 暴露截断后的可见切片。**不会泄露未来数据**。

### 低频序列的最近已知值

若 aux 是日频（每日一行）而 primary 是 15m 级，截断后每个决策点自动取该日（及之前）最近一行的值——无需手动重采样。公告、财务数据等低频序列通过行时间表达滞后（例如，公告发布当天收盘后才写入 aux CSV，则当天日内不可见，次日起可见）。

### 空截断弃权

若当前 `t` 早于 aux 首行时间，截断结果为空序列。空序列经 `as_scalar` 得 `NaN`，所有比较运算对 `NaN` 返回 `false`（NaN 弃权语义），分支走 `default`，不产生错误。

### 缺表运行时报错

若 DSL 表达式引用了 `aux.<name>.<col>` 但 `--aux <name>=...` 未给出，引擎在运行时报错：
```
aux table '<name>' not mounted (use --aux <name>=path.csv)
```

### 格式校验（加载期左移）

`aux.<表>.<列>` 必须是三段（恰好两个 `.`），表名与列名均非空且列名不含 `.`——此检查在树加载时（`check_no_unknown_idents`）完成，格式错误不会等到运行时才暴露。
