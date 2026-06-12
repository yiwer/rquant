# 决策树 YAML Schema 参考

本文档基于 `src/tree/{schema,loader}.rs` 的实际代码整理。

---

## 顶层结构

```yaml
meta:          # 树元数据（必填）
  name: "..."
  forward_window: 16
  stances: [long, flat]

params:        # 命名数值参数（可选；加载期内联展开）
  ma_n: 20
  mom_n: 5

factors:       # 命名 DSL 因子（可选；按文档顺序有序引用；加载期内联展开）
  mom: "slope(ema(close, ma_n), mom_n)"
  above: "close > sma(close, ma_n)"

root: "node_id"    # 根节点 id（必须是 nodes 中的键，不能是叶子）

nodes:             # 节点映射（HashMap，YAML 键顺序不影响语义）
  node_id:
    type: quant | llm
    # ... 节点字段

leaves:            # 叶子映射
  leaf_id:
    stance: long | flat | short
    weight: 0.5    # 可选；数值形式 ∈ (0,1]，或 DSL 表达式字符串（决策时求值），默认 1.0
    horizon: 8     # 可选，前瞻评分窗口（bar 数，≥1），默认 meta.forward_window
```

---

## `meta` 块

| 字段 | 类型 | 说明 |
|---|---|---|
| `name` | string | 树的可读名称，写入报告 |
| `forward_window` | usize | 前瞻窗口（primary bar 根数），用于前瞻收益评分 |
| `stances` | `[long\|flat\|short, ...]` | 允许的立场词表；叶子 stance 必须在此列表内 |

---

## `nodes` — quant 节点

```yaml
node_id:
  type: quant
  branches:
    - when: "<DSL 布尔表达式>"
      strength: "<可选>"   # 见下方 strength 说明
      goto: "<目标 id>"
      label: "<分支名>"
    - when: "..."
      goto: "..."
      label: "..."
  default:
    goto: "<目标 id>"
    label: "<分支名>"
```

**求值语义**：分支按声明顺序依次求值，**第一个 `when` 为真的分支胜出**（硬遍历）。没有任何分支命中时走 `default`。

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `branches` | 列表 | 是 | 有序分支列表 |
| `branches[i].when` | string | 是 | DSL 布尔表达式，在加载时编译 |
| `branches[i].strength` | string | 否 | 强度表达式（软模式用），见下方 |
| `branches[i].goto` | string | 是 | 目标节点或叶子 id |
| `branches[i].label` | string | 是 | 分支标签，写入 Trace |
| `default.goto` | string | 是 | 无分支命中时的目标 |
| `default.label` | string | 是 | 走 default 时写入 Trace 的标签 |

---

## `nodes` — llm 节点

```yaml
node_id:
  type: llm
  inputs: [news_score, recent_headlines]   # 可选，注入 user message 的 Context 字段
  prompt: "判断指令文本"
  labels:
    label_name: "<目标 id>"
    other_label: "<目标 id>"
  default: "<目标 id>"   # LLM 不可用/出错/弃权时的兜底目标
```

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `inputs` | `[string, ...]` | 否，默认 `[]` | 追加到 user message 的 Context 字段名；目前支持 `news_score`、`recent_headlines` |
| `prompt` | string | 否，默认 `""` | 注入 user message 的判断指令 |
| `labels` | `{label: target_id}` | 否，默认 `{}` | LLM 输出 label 到目标 id 的映射 |
| `default` | string | 是 | 兜底目标（LLM disabled/错误/弃权时使用） |

---

## `judges` 块（可选）

`judges` 块声明可复用的判定器，供多个 llm 节点通过 `judge:` 引用，实现**判定与落点解耦**：同一判定（prompt + labels + inputs）在多个路径节点复用，但每个节点可配置不同的 label→goto 落点映射。

### 语法示例

```yaml
judges:
  veto:
    prompt: "veto?"
    labels: [bad, ok]
    inputs: []          # 可选，默认 []

nodes:
  g_hi:
    type: llm
    judge: veto         # 引用 judges 块中的 veto 定义
    map: { ok: leaf_l } # label→goto 映射（未映射的 label 落 default）
    default: leaf_f
  g_lo:
    type: llm
    judge: veto
    map: { ok: leaf_s } # 同一 judge，不同落点
    default: leaf_f
```

### 物化语义

加载时，`judge:` 形式的节点被**物化**为等价的内联 llm 节点：

- `prompt`、`labels`、`inputs` 来自 judge 定义。
- 每个 judge label 都分配落点：`map` 中已映射的 label → 指定 goto；未映射的 label → 节点 `default`。
- `map` 可**整体省略**（等价于空映射）——所有 judge label 均落 `default`，是全部弃权/否决的合法形态。
- 物化后每个节点的 `labels` 键集 = judge.labels（完整集合），使共享同一 judge 的多个节点渲染串一致。

### 校验清单

| 规则 | 错误示例 |
|---|---|
| judge 名必须在 `judges` 块中存在 | `judge: nope` 而 nope 未定义 |
| judge 形式不得同时设置 `prompt`/`labels`/`inputs` | `judge: veto` + `prompt: "x"` → 报错 |
| `map` 键必须 ⊆ judge.labels | `map: { nope: leaf_f }` 而 nope 不在 labels → 报错 |
| judge.labels 必须非空 | `labels: []` → 报错 |
| `map` 只能配合 `judge` 使用 | 内联节点（无 `judge:`）设置 `map:` → 报错 |

### 缓存语义

共享 judge 的节点渲染串一致，且缓存/求值作用域（scope）统一为 `judge:<名>`（而非节点 id）。因此：

- **每 bar 每 judge 至多一次网络请求**：硬遍历路径唯一，至多一次调用；软遍历下多个共享 judge 的节点会各自发起求值，但渲染串与缓存键（scope）完全一致——第二次起命中 `OpenAiLlm` 磁盘缓存，是磁盘读而非网络调用（去重发生在文件缓存层，遍历器内无内存级复用）。
- 落点差异（不同 `map`）完全在本地 label→goto 映射层处理，不产生额外 LLM 调用或缓存条目。
- `StubLlm` 测试时以 `judge:<名>` 为答案键，例如 `answers: HashMap::from([("judge:veto".to_string(), "ok".to_string())])` 可同时驱动所有引用 `veto` 的节点。

### 注意事项

- 同名 judge 重复定义时后者静默覆盖前者（serde_yaml HashMap 行为，与 `nodes`/`leaves` 一致）。
- `judges` 块整体可省略（等价于空映射），树可以全部使用内联 llm 节点。

---

## `leaves`

```yaml
leaves:
  leaf_id:
    stance: long | flat | short
    weight: 0.5    # 可选；数值形式 ∈ (0,1]，或 DSL 表达式字符串
    horizon: 8     # 可选
```

叶子的 `stance` 必须在 `meta.stances` 中声明。

| 字段 | 类型 | 默认值 | 范围 | 打分语义 |
|---|---|---|---|---|
| `stance` | `long\|flat\|short` | 必填 | — | 交易方向 |
| `weight` | f64 或 DSL 字符串 | `1.0` | 见下方 | 仓位大小；硬打分中 `gross/net × weight`；软打分中 `p × weight × net` |
| `horizon` | usize | `meta.forward_window` | `≥ 1` | 前瞻评分窗口（bar 数），覆盖树级全局值 |

**`weight` 字段两种形式**：

- `weight`：仓位大小。两种形式——
  - **数值**：∈(0,1]，加载期校验（既有行为；0 非法）。
  - **表达式字符串**：决策时对当时 Context 求值；求值失败或非有限值 → 0（弃权=不持仓，**表达式结果 0 合法**——与数值形式的 (0,1] 口径刻意不对称），有限值 clamp 到 [0,1]。可引用 params/factors 与 sim 状态量。引用 `pos` 即获得**相对调仓**语义：`"min(1, pos + 0.25)"` 加一单位、`"pos"` 维持现仓、`"max(0, pos - 0.25)"` 减一单位（结果 0 等价于平仓）。
  - 注意：sim 状态量（pos/entry_price/极值等）只在 `--sim` 模式有真实值；打分/portfolio 模式恒为默认（pos=0 等），引用它们的表达式按默认值求值。
  - 带引号的纯数字（`weight: "0.5"`，或经 params 替换坍缩成字面量的 `weight: "unit"`）在加载期被识别为**常量**，套用与数值形式相同的 (0,1] 校验——引号不能绕过范围检查。

**软模式 position 口径**：净仓位 `r` 取分布内所有叶子中最大 `horizon` 对应的 gross 收益（`max_h` 腿），以避免多腿不同窗口下的口径混用。

**sim 模式叶子语义**：叶子表示**目标仓位**，不再用于前瞻打分。硬 sim：`target = stance_dir × weight`（`long=+1`、`flat=0`、`short=−1`，`weight` 默认 1.0）；软 sim：`target = Σ p(leaf) × stance_dir(leaf) × weight(leaf)`（期望净仓位 E）。`horizon` 在 sim 模式下不使用。

### 金字塔加仓示例（Turtle 风格）

```yaml
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
```

> 注：加仓后 `entry_price` 是加权均价，`entry_price + 0.5*atr` 的触发锚点会随加仓上移——与原版 Turtle「按上一加仓价」略有差异，需要精确锚点时用 `max_price_since_entry` 表达。

---

## `strength` 字段详解

`strength` 是 quant 分支的可选软模式字段，在 **`--soft` 遍历**时生效（硬遍历忽略）。

### 三种形式

| 形式 | 解析为 | 说明 |
|---|---|---|
| `"auto"` | `Auto(0.02)` | 对该支的 `when` 表达式做模糊求值，默认 scale=0.02 |
| `"auto(0.05)"` | `Auto(0.05)` | 指定 scale 的模糊求值，scale 必须 > 0 |
| `"sigmoid((close - sma(close,20)) * 50)"` | `Expr(...)` | 任意 DSL 标量表达式，加载时编译 |

### 软模式下的首真泄漏语义

软遍历中，quant 节点按有序分支做**首真泄漏**（`quant_branch_dist`，`eval/quant.rs`）：

```
remaining = 1.0
对每个 when==true 的分支 i（按顺序）：
  raw_strength = 求值 strength（Expr/Auto；NaN→0；clamp[0,1]）
  w_i = remaining × raw_strength
  remaining = remaining × (1 - raw_strength)
  若 remaining ≤ 1e-12 则停止
最终将 remaining 分配给 default
```

- 无 `strength` 时等价于 `strength=1.0`（软模式退化为硬首真，remaining 全给当前分支）。
- NaN strength 归 0（等价于跳过该分支，remaining 全给后续）。
- 所有权重之和恒为 1.0。

### 示例

```yaml
branches:
  - when: "close > sma(close,20)"
    strength: "sigmoid((close - sma(close,20)) / (0.02 * sma(close,20)))"
    goto: leaf_long
    label: above_ma
```

参见 `examples/strength_tree.yaml`。

---

## `risk:` 块（可选，sim 模式专用）

```yaml
risk:
  stop_loss: 0.05        # 可选：止损幅度（> 0）
  take_profit: 0.10      # 可选：止盈幅度（> 0）
  max_hold_bars: 60      # 可选：最大持仓 bar 数（≥ 1）
```

`risk:` 块是可选的顶层字段，三个字段均为可选（至少给出一个有意义）。校验规则：`stop_loss` / `take_profit` 若给出必须 `> 0`；`max_hold_bars` 若给出必须 `≥ 1`，否则加载报错。

**触发语义（sim 模式下，bar 收盘检查）**

| 字段 | 触发条件（pos≠0 时） | 覆盖结果 |
|---|---|---|
| `stop_loss` | `unreal_pnl ≤ −stop_loss` | `target=0`，reason="stop" |
| `take_profit` | `unreal_pnl ≥ take_profit` | `target=0`，reason="tp" |
| `max_hold_bars` | `bars_held ≥ max_hold_bars` | `target=0`，reason="max_hold" |

三者按 stop→tp→max_hold 顺序检查，命中即短路，优先于树决策（reason="tree"）。非 sim 模式下 `risk:` 块被解析但不使用。

> 风控触发时 `target=0` 直接产生，**不经过树遍历，也不求值任何叶子 `weight` 表达式**——金字塔类 weight 表达式与 `risk:` 块组合时，离场 bar 上 weight 表达式被完全旁路。

---

## `params` 与 `factors` 块

### 语法

```yaml
params:
  ma_n: 20       # 命名数值（f64）
  mom_n: 5

factors:
  mom: "slope(ema(close, ma_n), mom_n)"   # 命名 DSL 表达式字符串
  above: "close > sma(close, ma_n)"
```

两者均为可选，缺省等效空映射。

### 有序引用规则

`factors` 按 YAML **文档顺序**逐条展开（`serde_yaml::Mapping` 保序）。每条因子表达式中只能引用**在它之前**已定义的 `params` 或 `factors` 名字——后向引用（引用后定义的名字）在加载时报错。这让因子定义形成一条显式的依赖链，避免循环引用。

### 加载期内联展开

加载时，所有 `params` 名字替换为 `Expr::Number`，所有 `factors` 名字替换为对应的已展开子树（深拷贝）。展开完成后，`when`/`strength` 中残余的裸 Ident 必须是内置标识符（`close`/`open`/`high`/`low`/`volume`/`hour`/`minute`/`dow`）——否则视为未知名，**在加载时（而非运行时）报错**（"未知名左移到加载错"）。

同一因子在多处引用时**各处独立展开、重复求值**——无运行时缓存。若因子计算代价高（如 `ema(close, 200)`），建议仅引用一次或在信号合并处统一处理。

### 命名限制

以下名字**不得**用作 `params`/`factors` 的键：

- 内置标识符：`close` `open` `high` `low` `volume` `hour` `minute` `dow`
- **持仓状态标识符（sim 专用保留名）**：`pos` `entry_price` `bars_held` `unreal_pnl` `max_price_since_entry` `min_price_since_entry` `bars_since_exit` `last_trip_return`
- **日内锚定标识符（纯 Context 派生保留名）**：`session_open` `session_high` `session_low` `session_vwap` `bars_today`
- 内置函数名：`sma` `ema` `wma` `rsi` `atr` `slope` `ref` `highest` `lowest` `crossover` `crossunder` `macd_line` `macd_signal` `macd_hist` `std` `sigmoid` `auto` `abs` `max` `min` `count` `barssince` `valuewhen` `log` `exp` `sqrt` `floor` `sign` `pow` `percentrank` `corr`

与上述任一名字冲突，或在同一块中重复定义，均在加载时报错。

> 持仓标识符 `pos`/`entry_price`/`bars_held`/`unreal_pnl` 在非 sim 模式下取默认值（`pos=0`、`bars_held=0`、`unreal_pnl=0`、`entry_price=NaN`），因此同一棵树可以在打分模式和 sim 模式下都运行——打分模式时 `pos==0` 永远为真，分支退化为 flat 路径，不影响前瞻评分语义。`entry_price` 为 NaN 时所有比较运算返回 false（NaN 弃权），空仓状态下引用它的条件自动弃权走 default；`max_price_since_entry`/`min_price_since_entry` 同此纪律（空仓/非 sim 恒为 NaN）。**节流状态量** `bars_since_exit`/`last_trip_return` 在打分/portfolio/factor 模式下恒为 NaN（从未平仓），引用它们的冷却条件自动弃权为 false——**必须写为独立阻断分支**（见 [docs/dsl-reference.md 冷却写法纪律一节](dsl-reference.md#冷却写法纪律关键语义陷阱)），不能写入入场条件的 AND 子句（否则打分模式下该入场分支永不触发，树退化为纯 flat，评分/优化全废）。保留名总计：idents 21 / fns 31。

---

## 校验规则（`validate`，`loader.rs`）

加载时执行以下全部检查，任何一项不通过则报错：

1. **root 必须是节点**：`root` 键必须在 `nodes` 中，不能是 `leaves` 中的叶子。
2. **无悬空引用**：所有 `goto` / `labels` 目标必须存在于 `nodes` 或 `leaves` 中。
3. **所有节点从 root 可达**：BFS 从 root 出发，发现不可达节点则报错（防孤岛节点积累）。
4. **无环（DAG 检查）**：DFS 染色，检测到后向边（颜色=in-stack）即报错。
5. **叶子 stance 合法**：每个叶子的 `stance` 必须在 `meta.stances` 中。
6. **strength 表达式可编译**：`strength` 字段若存在，在加载时解析为 DSL Expr 或 Auto(scale)；格式错误立即报错。
7. **params/factors 命名合法**：键不得与内置标识符/函数名冲突，同块内不得重复定义；详见上方命名限制。
8. **factors 无后向引用**：factors 表达式中只能引用前序定义的名字；引用后定义名字报错。
9. **when/strength 无未知 Ident**：params/factors 内联展开后，残余裸 Ident 必须是内置标识符或合法 `aux.<表>.<列>` 三段标识符，否则报错（未知名左移到加载期）。
9a. **aux 三段格式**：以 `aux.` 开头的标识符必须满足 `aux.<table>.<column>` 格式（恰好两个 `.`，表名与列名均非空且列名不含 `.`）；格式不合法在加载时报错，早于运行时。
10. **叶子 weight 合法**：数值形式的 `weight` 若给出，必须满足 `0 < weight ≤ 1`，否则报错；表达式字符串形式在加载期仅校验语法与标识符合法性，运行时弃权值（0/NaN）均合法。
11. **叶子 horizon ≥ 1**：`horizon` 若给出，必须 ≥ 1，否则报错。
12. **语义陷阱 lint**：语义陷阱（恒假突破条件、单长度条件空转等）在加载期以 `[rquant] tree lint:` 前缀向 stderr 告警，不阻断加载。详见 [docs/dsl-reference.md 加载期 lint 一节](dsl-reference.md#加载期-lint)。

---

## 最小合法示例

```yaml
meta:
  name: "minimal"
  forward_window: 16
  stances: [long, flat]

root: check

nodes:
  check:
    type: quant
    branches:
      - when: "close > sma(close,20)"
        goto: leaf_long
        label: up
    default:
      goto: leaf_flat
      label: none

leaves:
  leaf_long: { stance: long }
  leaf_flat: { stance: flat }
```

---

## 参考示例

- `examples/trend_tree.yaml`：两级量化节点 + 一个 LLM 节点的完整树
- `examples/strength_tree.yaml`：带显式 strength 表达式的软模式示例
- `examples/factor_tree.yaml`：`params`/`factors` 命名块 + 叶子 `weight`/`horizon` 的完整示例
