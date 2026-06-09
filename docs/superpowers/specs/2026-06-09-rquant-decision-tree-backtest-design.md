# rquant：基于模糊决策树的 A股回测引擎 — 设计文档

- **日期**：2026-06-09
- **状态**：设计已确认（待 spec 评审 → 进实现计划）
- **作者**：yvvb（需求） / Claude（设计）

---

## 1. 背景与动机

用户拥有一套个人化的金融标的分析方法，形式上是一棵**决策路径树**（二元/有限元决策树，或更一般的 DAG）。人工分析时，用户结合：

- **小周期图表**：最近 100 根 15 分钟 K 线
- **大周期图表**：最近 100 根 1 小时 K 线
- **消息面影响因子**

沿树逐节点做判断（例如根节点判断"趋势方向：向上/向下/无明显趋势"），选择分支下行，直到叶子节点得出对该标的的分析结论。

目标是让 **指标 / 量化因子 / LLM（少量不可量化部分）** 取代人工完成这套逐节点判断，而**决策树本身由用户提供并加载**。

## 2. 第一性原理定位

剥掉金融外壳，本质是：

> **一个"模糊决策树解释器"**——把用户提供的*策略结构*（控制流）与*每个节点的判断*（感知/分类）彻底解耦。

- **决策树 = Policy（用户的认知资产 / IP）**：确定、可读，是 Software 1.0 的控制流。系统加载它、不改它。
- **节点判断 = Mechanism（感知层）**：把 `(图表 + 指标 + 消息面)` 映射为"选哪个分支"。这是被量化因子 / LLM 取代的部分。
- **一次分析 = 一次树遍历**，叶子 = 结论。

**关键设计原则**：LLM 只被用于"单节点的模糊分类"（短缰绳、结构化输出、可缓存），**绝不**用于开放式的"该不该买"推理。整个系统的工程可行性建立在这一分工之上。

## 3. 目标与非目标

### 3.1 目标（MVP）

构建一个**离线、可复现的回测工具**，它：

1. 加载用户用声明式配置编写的决策树；
2. 在历史 A股 K 线上逐时点遍历该树；
3. 将到达的叶子映射为**立场**（看多 / 观望 / 看空）；
4. 用**前瞻收益评分**衡量策略的 edge（预测力）；
5. 产出可审计的 **Trace**（完整路径 + 每节点决策/置信度/理由）与度量报告。

### 3.2 非目标（明确 YAGNI，仅留接口）

- 实盘 / 实时交易、下单
- 新闻采集与打分（只留消息面因子的*消费*接口）
- 软 / 概率遍历（MVP 只做硬遍历，但全程记录置信度，为将来升级留数据）
- 完整事件驱动 P&L（仓位、动态出场、组合资金管理）
- 多标的组合 / 资金分配

## 4. 锁定的根决策

| # | 决策维度 | 选定 | 理由 / 后果 |
|---|---|---|---|
| 1 | 系统形态 | **离线研究/回测工具** | 复现性成为第一约束；延迟不敏感；可批量离线跑 |
| 2 | 验证目标 | **策略盈亏（edge）** | 不验证"复刻用户判断"，而验证"树赚不赚钱"；吃全套量化回测严谨性；引入信用分配问题（靠记录完整路径缓解）|
| 3 | 叶子结果模型 | **前瞻收益评分** | 叶子=立场；决策 bar 收盘后，从 t+1 开盘起算 N 根 bar 的收益；最简、最抗偏差、最快证伪 |
| 4 | 决策树表示 | **声明式 YAML + 表达式 DSL** | 树是纯数据、可热加载、引擎通用；量化谓词写成内嵌表达式，LLM 节点写成 prompt+标签；表达力不足时再开脚本逗口 |
| 5 | 消息面处理 | **留接口、不建采集** | Context 预留可选的消息面因子字段，树可引用；MVP 不做任何抓取/打分（点位历史新闻是未来函数重灾区）|
| 6 | 数据来源 | **新浪 API → 落地缓存 → 回测只读缓存** | 新浪是实时接口、本身不可复现；抓取与回测解耦，回测只读不可变快照 |
| 7 | 标的类型 | **A股（沪深股票/指数）** | T+1、卖出印花税、散户难做空、午休+隔夜缺口——直接影响立场词表、成本模型、前瞻窗口、可执行性 |

## 5. 系统架构

七层，每层单一职责、可独立测试。**引擎层不含任何金融逻辑**——金融逻辑全部存在于"树（用户数据）+ DSL 函数库"中。

```
新浪API ──fetch──▶ 本地缓存(Parquet + SQLite) ──┐  (抓取与回测解耦)
                                                ▼
                                    [数据读取 + 交易日历]
                                                │  仅 close_time ≤ t 可见  ← 防未来函数闸门
                                                ▼
   指标库(ema/rsi/macd/atr…) ──▶ [特征 / Context 构建] ◀── 消息面因子(预留接口)
                                                ▼
   决策树(YAML) ─load+validate─▶ [遍历引擎] ──uses──▶ NodeEvaluator
                                                │                ├─ QuantEvaluator (DSL)
                                                │                └─ LLMEvaluator (prompt+缓存, temp0)
                                                ▼
                                       [Trace + 叶子立场]
                                                ▼
                            [前瞻收益评分 + 成本 haircut]  (t+1 开盘起, N 根 bar)
                                                ▼
                      [度量: 按叶子/按节点/整体 + 基准对比] ──▶ [报告 JSON/CSV/摘要]
```

**分层职责**：

| 层 | 职责 | 关键约束 |
|---|---|---|
| 数据层 | 抓取（新浪）、缓存、交易日历、读取 | 抓取与回测分离；回测只读快照 |
| 特征/Context 层 | 计算指标、构建时点上下文 | **防未来函数闸门**：只暴露 close_time ≤ t 的 bar |
| 树层 | 加载、校验决策树 | 校验在加载期完成，报错定位到节点 |
| DSL 层 | 解析、求值量化谓词 | 安全访问 Context；清晰报错 |
| 评估器层 | 统一 NodeEvaluator 接口（Quant / LLM） | 引擎不关心节点内部 |
| 引擎层 | 遍历树、生成 Trace | 不含金融逻辑 |
| 回测度量层 | 前瞻收益评分、成本、度量、报告 | 成本后统计；标注样本重叠 |

## 6. 核心抽象与数据流

```rust
// 时点上下文：节点唯一能看到的东西（看不到未来）
struct Context {
    t: Timestamp,
    primary: Window,            // 最近 100 根 15m bar（含 OHLCV 序列）
    context: Window,            // 最近 100 根 1h bar
    news: Option<NewsFactors>,  // 预留消息面接口，可空
    meta: SymbolMeta,           // 标的元数据
}

// 所有节点统一接口——引擎不关心内部怎么判
trait NodeEvaluator {
    fn eval(&self, ctx: &Context) -> Decision;
}
struct Decision {
    branch: BranchId,           // 选中的分支
    confidence: f64,            // [0,1]，硬遍历下仅记录，不影响走向
    rationale: String,          // 可读理由（审计用）
}

// 一次遍历的完整可审计记录（= 产品核心价值载体）
struct Trace {
    t: Timestamp,
    path: Vec<StepRecord>,      // 每步：node_id, 选中分支, label, confidence, rationale
    leaf: LeafId,
    stance: Stance,             // Long | Flat | Short
}
```

**主循环（两遍）**：

1. **遍历遍**：`for t in 回测区间内每根 primary bar（过预热期后）`：构建 `Context(t)` → 遍历树 → 记录 `Trace` + 叶子立场。
2. **评分遍**：对每个 `Trace`，用 `t+1` 开盘起的前瞻窗口计算成本后收益，聚合为度量。

分两遍的原因：遍历只依赖过去，评分需要未来——物理隔离这两者，结构上杜绝未来函数。

## 7. 决策树 Schema + DSL 规范

### 7.1 Schema 示例（用户原始例子的渲染）

```yaml
meta:
  name: "我的A股趋势树"
  forward_window: 16          # 前瞻 16 根 15m ≈ 1 交易日（满足 T+1 可执行）
  stances: [long, flat]       # 做空仅作信息、默认不计盈亏
root: trend

nodes:
  trend:                      # 量化节点：大周期趋势方向
    type: quant
    branches:
      - when: "ema(ctx.close,20) > ema(ctx.close,50) and slope(ema(ctx.close,20),5) > 0"
        goto: pullback
        label: up
      - when: "ema(ctx.close,20) < ema(ctx.close,50) and slope(ema(ctx.close,20),5) < 0"
        goto: leaf_avoid
        label: down
    default: { goto: leaf_flat, label: none }     # 无明显趋势

  pullback:                   # 量化节点：小周期是否回调到位
    type: quant
    branches:
      - when: "rsi(close,14) < 35 and close > sma(close,60)"
        goto: news_check
        label: yes
    default: { goto: leaf_flat, label: no }

  news_check:                 # LLM 节点：少量不可量化
    type: llm
    inputs: [news_score, recent_headlines]   # 来自预留消息面接口
    prompt: "给定以下消息面因子与标题，判断是否存在压制性重大利空。"
    labels:
      clear: leaf_buy         # 无利空 → 买点
      risk: leaf_flat         # 有利空 → 观望
    default: leaf_flat        # 无消息面数据 / LLM 不可用时

leaves:
  leaf_buy:   { stance: long }
  leaf_flat:  { stance: flat }
  leaf_avoid: { stance: flat }   # A股不做空 → "看跌"落为 flat（trace 仍记 down）
```

### 7.2 节点类型语义

**量化节点（`type: quant`）**：

- `branches`：有序列表，每项 `{ when: <布尔表达式>, goto: <目标节点/叶子>, label: <分支名> }`。
- 求值顺序自上而下，**第一个 `when` 为真的分支胜出**。
- 都不为真 → 走 `default`（**必填**）。
- 置信度：量化节点默认 `confidence = 1.0`（命中分支）/ 走 default 时记 `0.5`（可后续按"距离阈值的裕度"细化，非 MVP）。

**LLM 节点（`type: llm`）**：

- `inputs`：要注入提示词的 Context 字段名列表（如 `news_score`、`recent_headlines`，或已算好的特征）。
- `prompt`：判断指令。
- `labels`：`{ 标签 → 目标节点/叶子 }` 映射。
- `default`：兜底目标（M1–M4 阶段 LLM 未实现时、或 LLM 出错/弃权时走它）。
- LLM 被强制**结构化输出**：`{ label: <允许标签之一>, confidence: f64, reason: string }`，`temperature = 0`，结果缓存。

**叶子（`leaves`）**：`{ stance: long | flat | short }`。`short` 在 A股下默认仅作信息、不计入盈亏（见 §8）。

### 7.3 DSL 函数集 v1

序列引用（在决策时点 t 的最近一根*已收盘* bar 上求值，`[-k]` 向前移 k 根）：

- 主周期（15m）序列：`close`、`open`、`high`、`low`、`volume`
- 大周期（1h）序列：`ctx.close`、`ctx.open`、`ctx.high`、`ctx.low`、`ctx.volume`

**求值语义（关键）**：

- 指标函数返回一条**序列**（与输入等长）。在**比较 / 算术**中，序列默认对齐到**最新一根已收盘 bar** 求值——如 `ema(close,20) > ema(close,50)` 比较两条均线的最新值。
- 序列可作为其它函数的**序列输入**——如 `slope(ema(close,20), 5)` = 20-EMA 序列最近 5 根的斜率。这正是 §7.1 例子能成立的原因。
- `[-k]` 取序列前 k 根的值（`close[-1]` = 上一根收盘）。
- `slope/std/highest/lowest` 把序列**归约为标量**；`crossover/crossunder` 归约为 **bool**。

| 函数 | 签名 | 返回 | 含义 |
|---|---|---|---|
| `sma` | `sma(series, n)` | 序列 | 简单移动平均 |
| `ema` | `ema(series, n)` | 序列 | 指数移动平均 |
| `wma` | `wma(series, n)` | 序列 | 加权移动平均 |
| `rsi` | `rsi(series, n)` | 序列 | 相对强弱 |
| `macd_line` | `macd_line(series, fast, slow)` | 序列 | MACD 快线 |
| `macd_signal` | `macd_signal(series, fast, slow, sig)` | 序列 | MACD 信号线 |
| `macd_hist` | `macd_hist(series, fast, slow, sig)` | 序列 | MACD 柱 |
| `atr` | `atr(n)` | 序列 | 真实波幅均值（用主周期 high/low/close）|
| `slope` | `slope(series, n)` | 标量 | 最近 n 根的线性回归斜率 |
| `std` | `std(series, n)` | 标量 | 最近 n 根标准差 |
| `highest` | `highest(series, n)` | 标量 | 最近 n 根最高 |
| `lowest` | `lowest(series, n)` | 标量 | 最近 n 根最低 |
| `crossover` | `crossover(a, b)` | bool | a 上穿 b（上一根 a≤b 且本根 a>b）|
| `crossunder` | `crossunder(a, b)` | bool | a 下穿 b |

算子：算术 `+ - * /`；比较 `> < >= <= == !=`；布尔 `and or not`；分组 `( )`；索引 `series[-k]`。表达式最终须归约为 **bool**（分支 `when` 条件）。

### 7.4 加载期校验规则

1. 所有 `goto` / `labels` 目标必须存在（无悬空引用）。
2. 所有节点从 `root` 可达（无孤岛）。
3. 图为 DAG（无环）。
4. 量化节点必须有 `default`；LLM 节点的 `labels` 至少一项且有 `default`。
5. 叶子 `stance` 合法且在 `meta.stances` 内。
6. 报错精确到 `节点 id + 表达式片段`。

## 8. A股专属规则

- **立场词表默认 `{long, flat}`**：散户难做空 → "看跌"叶子落为 `flat`，但 Trace 中仍记录 `down`（保留信息；将来能做空时改 `meta.stances` 一键启用）。
- **前瞻窗口按"交易 bar"计**：走交易日历，跳过午休 / 隔夜 / 周末 / 节假日；且 **≥ 1 交易日**（T+1：当日买不能当日卖，更短窗口不可执行 → 引擎拒绝或标记"仅信息"）。
- **成本 haircut**：佣金（双边约 0.025%，最低 ¥5）+ 印花税（卖出 0.05%）+ 滑点。MVP 用**整体往返扣 ~0.1%** 的简化模型（可配），前瞻收益统一扣成本后统计。
- **涨跌停可执行性**：信号要求在涨停买入 / 跌停卖出 → 标记"不可执行"，单列统计（A股特有的隐形未来函数）。

## 9. 复现性与防未来函数

- **闸门**：`Context(t)` 在结构上只暴露 `close_time ≤ t` 的 bar；前瞻收益只用 `t+1 开盘`及之后。用**属性测试**守护此不变量（随机 t，断言 Context 中不含 close_time > t 的 bar）。
- **LLM 决定论**：`temperature = 0` + 锁定模型版本 + 缓存键 = `hash(node_id, prompt, 规范化输入特征)`；缓存落 SQLite。重跑零成本且逐字节一致。
- **数据不可变**：回测只读缓存快照；抓取是独立命令，回测时不联网。

## 10. 度量体系

- **按叶子**：样本数、成本后平均前瞻收益、胜率、std、t 值。
- **按节点**：分支分布、置信度分布 ← 信用分配抓手（叶子表现差时回看哪个节点带偏）。
- **整体**：立场加权累计收益曲线；**样本内 / 样本外切分**。
- **基准对比**：vs 买入持有、vs 随机分支（验证树确实加了信息，而非马后炮）。
- **诚实警告（写入报告）**：每根 bar 都出信号 → 前瞻窗口**重叠** → 样本自相关 → t 值虚高。报告显式标注，禁止用重叠样本鼓吹显著性。

## 11. 错误处理

| 阶段 | 错误 | 处理 |
|---|---|---|
| 加载期 | 树校验失败 | 精确定位节点 + 表达式，拒绝加载 |
| 运行期 | DSL 缺指标 / 预热不足出 NaN | 节点级错误，可配 `弃权(走 default)` 或 `快速失败` |
| 运行期 | LLM 超时 / 网络错误 | 重试 → 仍失败则走 `default` |
| 运行期 | 数据缺口 | 日历感知，标记并跳过受影响时点 |
| 评分期 | 前瞻窗口越界（区间末尾） | 该时点不计入度量，单独计数 |

## 12. 测试策略

- **单元**：DSL 解析/求值（golden 表达式）；指标对标已知数值；交易日历/缺口逻辑；**防未来函数属性测试**；成本模型。
- **集成**：合成树 + 合成已知结果价格序列 → 断言度量符合预期。
- **复现性**：同输入跑两遍 → Trace 与度量逐字节相同（LLM 用 stub 或缓存）。
- **LLM 评估器**：用 stub/mock，覆盖缓存命中/未命中与出错兜底路径。

## 13. Crate / 模块结构

```
src/
  data/        # fetcher(新浪), cache(parquet+sqlite), calendar(交易日历), reader
  features/    # indicators(指标库), context(Context 构建)
  tree/        # schema, loader, validate
  dsl/         # parser, eval
  eval/        # NodeEvaluator trait, quant, llm
  engine/      # traversal, trace
  backtest/    # forward_return, costs, metrics
  report/      # json/csv/摘要
  cli/         # 子命令: fetch / backtest / report
```

## 14. 里程碑与关键路径

**关键路径原则**：先把**纯量化端到端**跑通，把最外部、最不确定的两块（LLM、新浪 fetcher）后置。M1–M4 阶段，树里的 LLM 节点先走 `default` 分支照常运行。

| 里程碑 | 内容 | 产出 |
|---|---|---|
| **M1** | 数据缓存 + 读取 + 交易日历（先手动丢 CSV，不依赖新浪）| 防未来函数的取数 |
| **M2** | 指标库 + Context 构建 | 时点特征 |
| **M3** | 树 Schema + 加载 + DSL + QuantEvaluator + 遍历 + Trace | 能走树 |
| **M4** | 前瞻收益评分 + 成本 + 度量 + 报告 | **首个端到端价值（纯量化）** |
| **M5** | LLMEvaluator + 缓存 | 接入少量不可量化节点 |
| **M6** | 新浪 fetcher（导出到缓存）| 自动化取数 |

## 15. 已确认的默认参数

| 参数 | 默认值 | 备注 |
|---|---|---|
| 前瞻窗口 | 16 根 15m bar（≈1 交易日）| 可在 `meta.forward_window` 配置 |
| 决策频率 | 每根 primary bar 出一次信号 | 可改为特定时点（非 MVP）|
| 成本 | 往返 ~0.1% | 可配 |
| LLM 提供方 | 留到 M5 再定，设计为可插拔 | 不阻塞 M1–M4 |
| 立场词表 | `{long, flat}`（short 仅信息）| `meta.stances` 可改 |

## 16. 未来工作 / 预留接口

- **软 / 概率遍历**：当数据显示硬分支误差累积时，升级为按置信度加权传播，叶子变为分布。Decision 已含 `confidence`，为此预留。
- **消息面采集 + 打分**：消费接口已留（`Context.news`），将来补点位对齐的采集层。
- **完整事件驱动 P&L**：在前瞻收益评分之上增加仓位/出场/组合层。
- **做空**：A股规则放开或换标的后，`meta.stances` 启用 `short`。
- **更深历史数据源**：缓存层设计为可插拔，换源不动引擎（应对新浪分钟级历史浅的问题）。

## 17. 风险与诚实警告

1. **新浪分钟级历史浅**（15m/1h 往往只给最近一段）→ 长周期回测会被数据卡住；缓存层须可插拔以便换源。
2. **节点判断器的校准**是价值与风险的核心；但本 MVP 选择*只验证策略盈亏*，不直接验证节点保真度，故须靠 Trace 做事后信用分配。
3. **前瞻窗口重叠**导致样本自相关、统计显著性虚高——报告必须显式标注。
4. **视觉读图不可靠**：尽量把图表特征化、文本化喂给 LLM，避免直接喂 K 线图。
5. **过拟合 / p-hacking**：树是用户假设，须用样本外 + 基准对比防止"马后炮"式拟合。
