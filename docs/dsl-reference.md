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
| `bars_since_exit` | `Context.sim`（sim 模式） | 距最近一次平仓**执行 bar** 的 bar 数（标量）；平仓执行 bar 收盘记 1，其后每 bar +1，**不论当前是否持仓**（单调计数）；翻向也计一次平仓事件；从未平仓 → NaN → 比较弃权。打分/portfolio 模式恒 NaN。Turtle S1 再入场冷却：`bars_since_exit < 3` 作独立阻断分支（见「冷却写法纪律」一节）。 |
| `last_trip_return` | `Context.sim`（sim 模式） | 最近一次平仓回合的净值口径收益率（标量，正/负/零）；从未平仓 → NaN → 比较弃权。打分/portfolio 模式恒 NaN。Turtle S1 跳过规则：`last_trip_return > 0` 跳过本次突破（仅在阻断分支内使用——见「冷却写法纪律」一节）。 |
| `session_open` | 当日可见窗（纯 Context 派生） | 当日尾部连续段首根 bar 的 `open`（标量）；段为空时 NaN 弃权（理论上不出现） |
| `session_high` | 当日可见窗 | 当日尾部连续段内所有 bar 的 `high` 最大值（含当前 bar，标量） |
| `session_low` | 当日可见窗 | 当日尾部连续段内所有 bar 的 `low` 最小值（含当前 bar，标量） |
| `session_vwap` | 当日可见窗 | 日内 VWAP = Σ(close×volume) / Σ(volume)（标量）；Σvolume ≤ 0 → NaN 弃权。与滚动 VWAP 口径的区分：锚定自然日内已发生 bar，不跨日；滚动 VWAP（`sma(close*volume,n)/sma(volume,n)`）则是固定 n 根滑动窗口，可跨日。 |
| `bars_today` | 当日可见窗 | 当日可见 bar 数（标量，≥1）；日线数据退化为 1（无害） |
| `aux.<表名>.<列名>` | 挂载的外部 aux 表 | `--aux <表名>=path.csv` 中 `<列名>` 对应的数值序列（time≤t 截断后的可见部分） |

`resolve_series`（`eval.rs`）实现上述解析：前缀 `aux.` 存在时路由到对应 `AuxView`，前缀 `ctx.` 存在时路由到 `ctx.context`，否则路由到 `ctx.primary`。`hour`/`minute`/`dow` 与 `pos`/`entry_price`/`bars_held`/`unreal_pnl`/`max_price_since_entry`/`min_price_since_entry` 在 `eval` 的 `Ident` 臂中优先匹配，直接从 `ctx` 读取，不参与序列解析。

> **`entry_price` NaN 弃权**：空仓时 `entry_price = NaN`，与 NaN 的任何比较（`>`/`<`/`==`/`!=`）均返回 `false`。因此 `entry_price > 0` 在空仓时永远为 false，可安全用于分支条件而无需额外判空。同理，`max_price_since_entry`/`min_price_since_entry` 在入场决策点（`pos` 尚为 0）亦为 NaN——首个有效值在入场执行 bar 收盘后、下一个决策点才可见。

> **日内锚定族窗口截断说明**：`session_*`/`bars_today` 的"当日可见段"由 Context 窗口（`--window N`）截断；若窗口短于当日已有 bar 数，按可见部分计算（而非整日）。日线数据（一天一根）退化为单根，`session_open=open`、`session_high/low` = 本根 high/low、`bars_today=1`，退化语义无害。

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

### 算术运算的逐位提升（DSL Phase-2）

算术运算符（`+` `-` `*` `/`）和一元负号（`-x`）遵循**逐位提升**规则：

| 两侧形态 | 结果形态 | 说明 |
|---|---|---|
| Scalar ∘ Scalar | Scalar | 双标量保持标量，lint 形态推断依赖此守则 |
| Series ∘ Scalar（或反） | Series | 标量广播为全序列，逐位运算 |
| Series ∘ Series | Series | 尾对齐（取右端公共长度），逐位运算 |
| Bool 参与算术 | 错误 | `(close > 1) + 1` 报错，与旧版同等拒绝 |

**末位恒等定理**：提升后结果序列的最后一个元素，恒等于旧版将两侧各取末位后的标量运算值——标量消费者（`when`/`strength`/叶子 `weight`）只看末位，升级语义零破坏，旧 YAML 无需修改。

**NaN 传播**：逐位算术中，某位 NaN（如指标暖机期）→ 该位结果 NaN；进入 `count`/`barssince` 条件后，NaN 位自动弃权（不计入 count，不触发 barssince）。

**新能力（Phase-2 解锁）**：算术结果（派生序列）可直接送入窗口函数和逐位条件，无需任何转换：

```yaml
# 成交额序列进 sma（滚动 VWAP 地基）
when: "sma(close * volume, 20) / sma(volume, 20) > sma(close, 20)"
# 派生序列进逐位条件
when: "count((high - low) > atr(14), 10) >= 3"
# 对数收益进 std（zscore 归一化）
when: "(log(close) - log(ref(close,1))) / std(log(close), 60) > 1.5"
```

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

### 序列函数（续）——滚动窗口统计（返回 `Series`）

| 函数 | 参数 | 头部行为 | 说明 | 示例 |
|---|---|---|---|---|
| `highest(series, n)` | series: Series, n: int | 宽容扩张窗（j<n 时取 [0..j] 最大值，无 NaN 前缀） | 逐位最近 n 根最高值（**含当前 bar**，见下方陷阱 A1）；窗口内 NaN 跳过；标量上下文取末位，语义与旧版相同 | `highest(high, 20)` |
| `lowest(series, n)` | series: Series, n: int | 宽容扩张窗（同 highest） | 逐位最近 n 根最低值（**含当前 bar**）；标量上下文取末位 | `lowest(low, 20)` |
| `std(series, n)` | series: Series, n: int | 严格：j+1<n → NaN | 逐位最近 n 根总体标准差（÷n）；标量上下文取末位 | `std(close, 20)` |
| `slope(series, n)` | series: Series, n: int≥2 | 严格：j+1<n → NaN | 逐位最近 n 根 OLS 线性回归斜率（x=0..n-1，单位 = 输入单位/bar）；标量上下文取末位 | `slope(ema(close,20), 5)` |

> **标量上下文语义零变**：highest/lowest/std/slope 现在返回 Series，但在比较表达式（`when: "highest(high,20) < close"`）外层仍会取末位标量——数值与旧版完全相同，旧 YAML 无需修改。新能力：在 `count`/`barssince` 的逐位条件内，它们作为滚动序列逐位比较（Task1 解锁）。

### 标量/点态函数（输入全标量 → Scalar；含序列 → 逐位提升为 Series）

以下函数均支持**点态提升**：当任一实参为 Series 时，结果升为 Series（逐位运算，NaN 自然传播）；全为 Scalar 时结果仍为 Scalar（lint 形态推断依赖此守则，`weight` 表达式不会意外变 Series）。

| 函数 | 参数 | NaN 行为 | 说明 | 示例 |
|---|---|---|---|---|
| `sigmoid(x)` | x: Scalar 或 Series | NaN 传播 | 1/(1+e^−x)，常用于 strength；**点态提升** | `sigmoid((close - sma(close,20)) / 0.5)` |
| `abs(x)` | x: Scalar 或 Series | NaN 传播 | 绝对值；**点态提升** | `abs(close - sma(close,20))` |
| `max(a, b)` | a, b: Scalar 或 Series | 任一 NaN → NaN（显式传播，不吃弃权）| 较大值；**点态提升** | `max(pos, 0.25)` |
| `min(a, b)` | a, b: Scalar 或 Series | 任一 NaN → NaN（显式传播）| 较小值；**点态提升** | `min(1, pos + 0.25)` |
| `log(x)` | x: Scalar 或 Series | 负域/零 → NaN（弃权） | 自然对数（底 e）；负定义域返回 NaN，不报错；**点态提升** | `log(close)` |
| `exp(x)` | x: Scalar 或 Series | NaN 传播 | 自然指数 e^x；**点态提升** | `exp(slope(close,5))` |
| `sqrt(x)` | x: Scalar 或 Series | 负域 → NaN（弃权） | 平方根；负输入返回 NaN；**点态提升** | `sqrt(abs(close - ref(close,1)))` |
| `floor(x)` | x: Scalar 或 Series | NaN 传播 | 向下取整；**点态提升** | `floor(atr(14) * 10)` |
| `sign(x)` | x: Scalar 或 Series | NaN 传播 | 符号函数：`x>0→1`，`x<0→−1`，**`x=0→0`**（数学惯例，非 Rust `signum`）；**点态提升** | `sign(close - sma(close,20))` |
| `pow(a, b)` | a, b: Scalar 或 Series | NaN 传播 | 幂运算 a^b（对应 Rust `f64::powf`）；**点态提升** | `pow(close / ref(close,1), 252)` |
| `count(cond, n)` | cond: 布尔表达式, n: int≥1 | 序列 < n → NaN | 末 n 位中 cond 为 true 的个数；cond **逐位**求值（见下节） | `count(close > ema(close,20), 10)` |
| `barssince(cond)` | cond: 布尔表达式 | 从未 true → NaN | 距最近一次 cond=true 的 bar 数（当前 bar=0） | `barssince(crossover(close, sma(close,20)))` |
| `valuewhen(cond, expr[, k])` | cond: 布尔表达式, expr: Series/Scalar, k: int≥0（默认 0） | 从未触发或次数不足 → NaN 弃权 | 最近第 k+1 次 cond=true 处的 expr 值（k=0 = 最近一次）；常用于事件锚定（回踩价、突破价） | `valuewhen(crossover(close, ema(close,8)), close)` |
| `percentrank(series, n)` | series: Series, n: int≥2 | n<2 或头部不足 → NaN（严格头）；窗含 NaN → NaN | 位 j = 窗口（含当前，长 n）内**严格小于** s[j] 的个数 / (n−1) ∈ [0,1]；自归一化惯用法：`percentrank(atr(14)/close, 250) > 0.95` | `percentrank(close, 20) > 0.8` |
| `corr(a, b, n)` | a, b: Series, n: int≥2 | n<2 或头部不足 → NaN；窗含 NaN → NaN；任一侧零方差 → NaN | 滚动 Pearson 相关（两序列先尾对齐再逐位滚动）；大盘相关惯用法：`corr(close, ctx.close, 60) > 0.7` | `corr(close, ctx.close, 60) > 0.5` |

> **陷阱 A1：`highest`/`lowest` 窗口含当前 bar**。`close > highest(high, n)` 在**裸窗（highest/lowest 的序列参数未经 ref 移位）+ 严格比较（`>`/`<`）** 时恒假——窗口最大值 ≥ 当前 high ≥ 当前 close，严格大于永不成立。注意：`close >= highest(close, n)` 表示"当前 bar 创 n 根新高"，是合法的创新高事件，**不触发此陷阱**。表达 Turtle"超过前 N 根高点"的突破语义必须先用 `ref` 移掉当前 bar：
>
> ```yaml
> when: "close > highest(ref(high, 1), 20)"   # 突破前 20 根高点（ref 移窗，合法）
> when: "close < lowest(ref(low, 1), 20)"     # 跌破前 20 根低点
> when: "close >= highest(close, 20)"         # 创 20 根新高（含当前 bar，合法）
> ```
>
> 加载期 lint 会自动检测"裸价格序列 + 裸窗 + 严格比较"写法并打印告警（见"加载期 lint"一节）。

### 布尔函数（返回 `Bool`）

| 函数 | 参数 | 说明 | 示例 |
|---|---|---|---|
| `crossover(a, b)` | a, b: Series | 上穿：前一根 a≤b 且本根 a>b；序列不足 2 根时 false | `crossover(close, sma(close,20))` |
| `crossunder(a, b)` | a, b: Series | 下穿：前一根 a≥b 且本根 a<b；序列不足 2 根时 false | `crossunder(ema(close,5), ema(close,20))` |

---

## 事件计数与逐位条件（`count` / `barssince` / `valuewhen`）

`count`/`barssince`/`valuewhen` 的条件参数不走「Series → 取末元素」归约，而是**逐位**求值成布尔序列：

- 比较（`> < >= <= == !=`）：两侧序列**尾对齐**（取右端公共长度；标量广播），逐位比较；任一侧该位 NaN → 该位 false（NaN 弃权逐位生效）。
- `and` / `or` / `not`：逐位组合。
- `crossover(a, b)` / `crossunder(a, b)`：逐位事件序列——位 j 为 true 当且仅当前一位未越线且本位越线；首位与含 NaN 位恒 false。**注意与普通 `when` 上下文的标量版语义并存**：普通上下文里 crossover 只看末两位返回单个 Bool，条件序列上下文里它是整个窗口的事件序列。
- **`highest`/`lowest`/`std`/`slope` 在逐位条件内是滚动序列**（DSL Phase-1 后）：`count(close >= highest(close, 20), 5)` 是合法的"5 根内创新高次数"，无需绕开。
- 其余表达式形态（裸序列、算术结果）作为条件 → 求值报错。

窗口纪律：布尔序列长度 < n（或 `barssince`/`valuewhen` 从未触发）→ 返回 NaN，外层比较自动弃权走 default。

> **空转陷阱**：条件两侧均为标量形（如 `count(bars_held > 2, 5)`），逐位序列长度为 1，n>1 时 count 恒弃权。加载期 lint 会对此类写法告警（L2 规则，见"加载期 lint"一节）。

### 派生序列惯用法（DSL Phase-2 新增）

以下三条惯用法均依赖 Phase-2 算术/点态提升，已在合成数据上验证语义正确（无运行错误，结果符合数学预期）。

```yaml
# 滚动 VWAP（成交量加权均价，n 根）
# 验证：volume 为常数时 sma(close*volume, n)/sma(volume, n) == sma(close, n)（数学恒等）
factors:
  vwap_20: "sma(close * volume, 20) / sma(volume, 20)"

# 真实波幅计数（高低差大于 ATR 的 bar 计数，需真实 OHLC 数据；纯平 bar 时 high-low=0 全弃权）
# 注：high-low 是派生序列（phase-2 提升），可直接进 count 逐位条件——无需任何转换
when: "count((high - low) > atr(14), 10) >= 3"

# 对数收益 zscore（自归一化阈值地基）
# log(close)-log(ref(close,1))=ln(close/prev_close) 是对数收益序列（phase-2 提升）
# std(log(close), 60) 是滚动标准差序列，两者做序列除法得 zscore 序列
# 预热期（< 60 根）std 为 NaN → 除以 NaN → NaN → 比较弃权（安全）
when: "(log(close) - log(ref(close,1))) / std(log(close), 60) > 1.5"
```

### 入场时刻锚定惯用法（at_entry 之死）

**核心机制**：`ref(expr, bars_held)` 即信号 bar 锚定——`as_usize(bars_held)` 将当前持仓根数转为 k，`ref(series, k)` 截掉末 k 根，取的就是开仓信号那根 bar 处的值。打分模式下 `bars_held=0`，`ref(expr, 0)` 恒等式安全退化，不影响评分。**已在合成数据上实跑验证**（`ref(atr(14), bars_held)` 返回正有限值；越窗时返回空序列→ NaN 弃权）。

```yaml
# 信号 bar 的 ATR（Turtle 原版 N）：开仓决策发生在 bars_held 根之前
n_at_entry: "ref(atr(14), bars_held)"

# 入场执行 bar 的最低价（信号 bar 止损位挂单）
# max(0, bars_held-1)：打分模式 bars_held=0 时 max 兜 0，ref(low, 0) 恒等式
entry_bar_low: "ref(low, max(0, bars_held - 1))"

# Chandelier 原版（入场时 N 而非当前 N）：跟踪止损随入场价格锁定的波动尺度
when: "pos > 0 and close < max_price_since_entry - 3 * ref(atr(14), bars_held)"
```

**边界**：持仓 `bars_held` 超过 Context 可见窗（`--window N`）→ `ref` 截断后空序列 → `as_scalar` 返回 NaN → 比较弃权，树自然走 default。与极值迁移注同纪律：树内保留固定止损分支兜底（`close < entry_price - 3*atr(14)`），同时覆盖此弃权窗口。

### 冷却写法纪律（关键语义陷阱）

`bars_since_exit` 与 `last_trip_return` 在**打分/portfolio/factor 模式下恒为 NaN**（从未平仓）。冷却条件的正确写法是**独立的阻断分支形态**，错误写法是 **AND 子句**。

#### 正确：阻断分支形态（NaN → false → 自然落空）

```yaml
nodes:
  gate:
    type: quant
    branches:
      - when: "pos > 0 and bars_held >= 2"
        goto: leaf_flat
        label: exit_after_2
      - when: "pos > 0"
        goto: leaf_long
        label: hold
      - when: "bars_since_exit < 3"     # ← 独立阻断分支
        goto: leaf_flat                  #   NaN(打分模式) → false → 此分支落空
        label: cooldown_block            #   打分模式零影响：直接穿透到下方入场分支
      - when: "close > highest(ref(high, 1), 20)"
        goto: leaf_long
        label: enter
    default: { goto: leaf_flat, label: idle }
```

**机理**：`bars_since_exit < 3` 在打分模式下为 `NaN < 3` → `false` → 分支落空，遍历继续到入场分支，**不影响打分评分**。在 sim 模式下正常阻断冷却期内的再入场。

#### 错误：AND 子句（打分模式恒弃权 → 树退化）

```yaml
# ❌ 危险写法：永远不要这样做
- when: "pos == 0 and bars_since_exit >= 3 and close > highest(ref(high,1), 20)"
  goto: leaf_long
  label: enter
```

**退化机理**：打分模式下 `bars_since_exit = NaN`，`NaN >= 3` 返回 `false`，整个 AND 表达式为 `false` → **此分支在打分模式下永远不成立** → 树在打分口径退化为纯 flat → IC/Sharpe/前瞻评分/WFO 优化全部在一棵哑树上运行，评分/优化结果完全无效。

**判断规则**：凡是引用 `bars_since_exit`/`last_trip_return` 的条件，**必须单独作为一个分支写在入场条件之前**，永远不要用 AND 将其与入场条件合并。

### 滚动统计惯用法（percentrank/corr）

```yaml
# 自归一化 ATR：ATR/收盘在过去 250 根中的百分位排名 > 95%→ 极高波动期
when: "percentrank(atr(14) / close, 250) > 0.95"

# 大盘相关性过滤：60 根内 primary 与 context 的相关系数 > 0.7 → 跟盘运行，适合趋势跟随
when: "corr(close, ctx.close, 60) > 0.7"

# 对数收益百分位（相对强度入场）：近 120 根收益率创 95% 分位
when: "percentrank(log(close) - log(ref(close, 1)), 120) > 0.95"
```

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
# 突破事件序列（Phase-1 后 highest 在逐位条件内是滚动序列，以下写法合法）：最近一次 Turtle 突破在 ≤5 根内
when: "barssince(close > highest(ref(high,1), 20)) <= 5"
# 事件锚定：最近一次上穿 EMA8 那根 bar 的收盘（measured move / 回踩锚）
when: "close < valuewhen(crossover(close, ema(close,8)), close) * 0.99"
```

---

## 加载期 lint

树加载时对全部 quant 节点分支的 `when` 表达式（及 `strength` / 叶子 `weight` 表达式）运行静态规则检查。发现问题时向 stderr 打印告警（`[rquant] tree lint:` 前缀），**不阻断加载**。规则保守：宁缺勿滥，不确定是否有问题时不报。

### 规则 L1：恒假突破陷阱（A1 陷阱）

```
[rquant] tree lint: node 'gate' when "close > highest(high, 20)": 突破条件恒假——highest/lowest
窗口含当前 bar；表达"前 N 根高/低点"请先 ref(series, 1) 移窗（docs/dsl-reference.md A1 陷阱）
```

**触发条件**：裸价格序列标识符（`close`/`open`/`high`/`low`，未经 `ref` 或索引移位）与 `highest`/`lowest`（首参同为裸价格序列）之间使用严格比较（`>` / `<`）或其镜像。

**不触发**：`>=`/`<=` 比较（创新高事件合法）；首参经过 `ref(high,1)` 移位（Turtle 突破正确写法）。

### 规则 L2：单长度条件空转

```
[rquant] tree lint: node 'gate' when "count(bars_held > 2, 5) >= 3": count(...) 条件两侧均为
标量形——逐位布尔序列长度 1，将恒弃权空转；至少一侧需要序列（close/ema(...)/ref(...) 等）
```

**触发条件**：`count`/`barssince`/`valuewhen` 的条件表达式两侧均为标量形（数值字面量、持仓状态量 `pos`/`bars_held` 等），推断布尔序列长度必然为 1——`count(n>1)` 恒弃权、`barssince`/`valuewhen` 仅看 1 个位置。

**不触发**：条件含任意 Series 形一侧（`close`/`ema(...)`/`highest(...)`/`ref(...)` 等），逐位序列长度来自数据窗口。

**形态推断覆盖提升后的算术与点态函数**：L2 的静态形态推断已完整跟踪 Phase-2 的逐位提升规则——`close - open`、`abs(close - sma(close,3))`、`log(close)` 等派生序列在条件中被正确识别为 Series 形，不会产生误报；`pos * 2`、`abs(pos)`、`floor(2.9)` 等全标量路径仍被识别为 Scalar 形并正常告警。

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

### 时间戳纪律（as-of join，防 lookahead）

闸门是**含端点的 as-of join**：决策点 `t` 可见所有 `time ≤ t` 的行——与 primary bar 的可见性约定一致（时间戳为 `t` 的 bar 在 `t` 时刻其 close 已可见）。因此 aux 行时间戳必须满足同一纪律：

> **行时间 = 该行数值完全确定（可被知晓）的时刻。**

- **高周期重采样**（如用 4h K 线做日内 regime 过滤）：行必须打在**周期收盘时刻**。打在周期开始时刻 = 在该周期进行中就泄露其收盘值，lookahead 直接进来。
- **公告 / 财务 / 舆情**：行时间 = 发布时刻（精确到日内则当日盘中即可见；只精确到日，按「当日收盘后写入」处理 → 次日起可见）。
- **指数日线**：打收盘时刻（如 `2024-01-02 15:00:00`），不要打 `00:00:00`——后者会让当日开盘即可见当日收盘价。

这与 `--sim` 的 SimState 注入、`build_context` 的 bar 闸门是同一条防未来函数纪律：**引擎只保证 `time ≤ t` 截断正确，时间戳本身的语义由数据制备方负责**。引擎无法检测打错戳的 aux 表——错误的时间戳产生的回测收益是假的。

| 数据 | 错误打法 | 正确打法 |
|---|---|---|
| 4h bar (10:00–14:00) 的 ema20 | `10:00:00`（周期开始） | `14:00:00`（周期收盘） |
| 1 月 5 日盘后年报 | `2024-01-05 00:00:00` | `2024-01-05 17:00:00`（或次日 00:00） |
| 指数日线收盘价 | `2024-01-02 00:00:00` | `2024-01-02 15:00:00` |

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

---

## 逐股基本面（`fund.<列>`）

universe 第 4 列 `fundamentals` 挂载的**逐股**财务 CSV（akshare 管线产出，见 `scripts/fetch_fundamentals.py`），DSL 以 `fund.<列名>` 两段格式引用：

```yaml
when: "fund.roe > 15 and close / fund.eps < 30"   # ROE>15% 且 PE<30（PE 派生）
```

与 `aux.` 的区别：aux 是**全标的共享**表（指数等横截面数据）；fund 是**逐股**序列（每只股自己的财报）。

### 时点语义（公告日 as-of，防前视）

`fund.<col>` 是 **as-of-t 标量**：决策点 `t` 取该股**公告日 ≤ t 的最近一行**财报值（季频）。CSV 首列 `time` = **公告日（最新公告日期）**，非报告期——这是 point-in-time 命根：Q1 财报约 4 月才披露，引擎在公告日前看不到它。**首份财报公告前 → NaN（弃权，比较恒 false → 走 default）**，绝不前视。与 aux 同一条 time≤t 闸门纪律（见上）。

### 单位（铁律，按 akshare yjbb 原样存）

| 列 | 含义 | 单位 |
|---|---|---|
| `fund.roe` | 净资产收益率 | **百分数**（34.1 = 34.1%）|
| `fund.np_yoy` | 净利润同比增长 | 百分数 |
| `fund.rev_yoy` | 营收同比增长 | 百分数 |
| `fund.gross_margin` | 销售毛利率 | 百分数 |
| `fund.eps` | 每股收益 | 元 |
| `fund.bps` | 每股净资产 | 元 |

故写 `fund.roe > 15`（15%）**而非** `> 0.15`。估值派生：`close / fund.eps`（PE）、`close / fund.bps`（PB）。

### 缺列/缺表弃权（与 aux 的差异）

缺列或首报前 → `NaN`（弃权，比较恒 false 走 default）。注意：`aux.<表>.<列>` 缺表/缺列会**报错**；`fund.<col>` 缺列**不报错、直接弃权**（财报字段缺失是常态、公告前无数据是 point-in-time 正常状态）。

### 格式校验（加载期左移）

`fund.<列>` 必须是两段（恰好一个 `.`），列名非空且不含 `.`——树加载时 `check_no_unknown_idents` 校验。

### screen 树中的 `fund.*`

**screen 树（`rquant screen` 子命令的 quality_trees / setup_trees）可以直接使用 `fund.*`**，与 `factor`/`portfolio`/`backtest` 子命令的同一 point-in-time 基本面通道完全一致——前提是 universe CSV 第 4 列 `fundamentals` 已挂载逐股财务 CSV。PB 价值树的典型写法：

```yaml
# examples/trees/screen/value_pb.yaml
when: "fund.bps > 0"           # 闸：有正净资产才进 cheapness 评分
weight: "1 / (1 + close / fund.bps)"   # PB 越低 → weight 越高 → 单调无饱和 ∈(0,1)
```

`fund.bps` 首份财报公告前为 NaN → 分支自动弃权走 default（flat），不产生错误信号。
