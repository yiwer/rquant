# A股日线选股器（优质 + 投机价值标注筛选）· Phase 1 设计文档

- 日期：2026-06-15
- 状态：设计已与用户逐节确认，待审阅 → writing-plans
- 范围：**Phase 1 = 方法学 + 验证**。在已有深 20 标的数据（2018-2026，跨牛熊）上，用**多策略树并行集成**定义"优质 + 3 投机形态"信号，建**历史回测机制 + 迭代修正机制 + 双重验证**，产出 `rquant screen` CLI（出排名+标注清单 + 集成回测）。**不**做广度拉取/日频运行/桌面页（Phase 2，且以 Phase 1 验证通过为闸）。

## 1. 背景与目标

**核心诉求**：对 A 股日线级别标的，筛选并**标注**"优质且具备投机价值"的标的——一个面向用户、每日收盘后生成排名+标签+理由的选股清单（watchlist）。

**诚实锚点**（上轮深重验结论，直接定方向）：触发择时树（均值回归/Donchian/均线堆叠）经深历史重验证明**结构性无 edge / regime 依赖**；**仅横截面强度树显条件 edge**（扛 2018 熊 +24pp 超额）。因此选股器的树**必须是横截面选择/标注树（强度树家族）**，不是择时触发树；每棵树须靠回测/IC 挣得席位。

**目标**：建立一个**经验证的、有深度的**日线选股方法学——多树并行集成给每股算优质分 + 标注命中的投机形态，并通过历史回测+迭代把"哪些信号真有预测力"实测出来。**先证明信号有 edge，再（Phase 2）投入广度+UI**；若证伪则诚实记录、不 productize。

## 2. 已确认决策（brainstorming）

| # | 决策 | 选择 |
|---|---|---|
| Q1 数据/优质口径 | 优质用什么数据 | **OHLCV-only，不考虑基本面**；优质=技术/价量面（趋势健康+受控回撤+流动性+低噪声） |
| Q2 投机口径 | 投机价值核心 | **多形态标注**：动量延续 / 突破临界 / 超跌反弹，逐股标注命中形态 |
| Q3 用法 | 主交付形态 | **每日收盘后生成排名+标签+理由的 watchlist**（Phase 2 日频+桌面；Phase 1 CLI） |
| Q4 universe | 扫描宽度 | **精选几百只流动大中盘**（沪深300/中证500 级，Phase 2）；Phase 1 在深 20 上验证 |
| Q5 验证 | 验证严度 | **双重验证**：每信号因子式前瞻 IC/分层 + "优质+命中标签"组合规则回测（跨牛熊） |
| 分期 | 推进方式 | **先 Phase 1（方法学+验证），Phase 2 另起 spec** |
| 架构 | 信号表达 | **C+：策略树为单元 + 树集成编排器（新、薄 `src/screen/`）+ 复用树评估/portfolio/factor 做评估与验证** |
| 深度/并行 | 选股形态 | **多策略树并行集成 + 具备深度**（无效率顾虑，但过拟合仍是真闸） |
| 合并口径 | 集成输出 | **双输出**：标签投票（形态标注）+ 综合分（优质×投机供排名） |

## 3. 信号定义：多策略树并行集成

**信号单元 = 横截面选择树**（引擎原生模糊决策树，与 `examples/strength_portfolio_2.yaml` 同构：percentrank 横截面、分级叶、软/硬模式）。按角色分组：

**优质树（≥1）** → 分级输出 `quality_score ∈ [0,1]`，子判据（DSL 表达式，作为树的门/叶）：
- 趋势健康：`close > ema(close,200) and ema(close,50) > ema(close,200)`（均线多头排列）
- 受控回撤：`close / highest(close,120)`（距 120 日高点近 ≈ 回撤健康，越接近 1 越优）
- 趋势/噪声比：`slope(ema(close,20),20) / (std(close,20) + 1e-9)`（单位噪声的趋势量，越高越平滑不鬼刀）
- 流动性：`sma(close * volume, 20)` 绝对下限（日均成交额 ≥ 阈值，低流动性出局）——**不**用"跨 universe 分位"（精选大中盘本就流动，横截面流动性排名意义小且增复杂度）

**形态树（每形态 ≥1，可多树集成投票）** → 命中则触发标签 + 强度 `∈ [0,1]`：
- **动量延续**：`percentrank(close/ref(close,20)-1, 60)` 高 **且** `close > ema(close,50)` 且 ema50 上行 → 强者恒强（强度树已证方向）。可放"绝对动量树"+"相对强弱树"两棵集成。
- **突破临界**：`close >= 0.97 * highest(ref(high,1),60)`（距 60 日前高 3% 内）**且** `volume > sma(volume,20)`（量起）**且** `std(close,10) < std(close,40)`（缩量盘整蓄势）→ 突破前夜。
- **超跌反弹**：`close > ema(close,120)`（长趋势在）**且**（`rsi(close,14) < 35` 或 `close < ema(close,20)*0.95`）（短期深跌近支撑）→ 上升趋势中的回调买点。

**并行**：N 棵树每日并行评估全 universe（无效率顾虑 → 可上大集成 + 深历史）。

**横截面语义（实现要点，与引擎对齐）**：引擎的 `percentrank(expr, n)` 是**单标的时间序列自归一**（每只对自己近 n 根历史的分位，强度树正是这么用），使每股得分跨标的可比。**真正的横截面**（跨 universe 排名）发生在**编排器层**——它收集所有标的当日的每棵树标量后做投票/排名/选 top-N（复用 portfolio 已验证的 `select_top`：score>0 降序、并列 symbol 升序）。故树内只做自归一，编排器做真横截面，两层分明。

**深度由验证驱动地长出**：种子树（优质 + 3 形态各 1）是起点假设，非上限；靠 §5 回测/IC 留强汰弱 + 加候选树，沉淀出"已验证因子/树库"。**树多 ≠ 乱堆**——每棵须过验证门，过拟合（样本外/regime 稳健）是真闸，不是算力。

## 4. 集成合并：双输出（标签投票 + 综合分）

每股每日，N 棵树输出合并为一条记录：

- **形态强度**：每形态 S 的树集 `T_S`，每树出强度 `s_i ∈ [0,1]`（0=未触发）。标签 S 命中当 `count(s_i ≥ θ_fire) ≥ ceil(|T_S| * vote_frac)`（默认 `vote_frac=0.5` 多数）；形态 S 强度 = 命中树 `s_i` 均值。
- **`tags[]`** = 命中的形态列表（投票结果）——**标注输出**。
- **`quality_score`** = 优质树分级输出加权均值（起始等权）。
- **`speculative_score`** = `max_S(形态 S 强度)`（当前最强形态强度，可交易性）。
- **`combined_score`** = `quality_score * speculative_score`（起始式，权重/形式经 §5 迭代定）——**排名输出**。
- **选择**（回测/top）：不合格股（`tags 为空` 或 `quality_score < q_floor` 优质门）的 `combined_score` 置 0，再喂 portfolio `select_top`（自动滤 score≤0、降序、并列 symbol 升序）取 top-N——复用已验证选择逻辑。
- **`reasons[]`** = 每棵触发树的 `{tree, leaf, 关键值}`，供"为何命中"可解释。

起始参数（经 §5 迭代定参）：`θ_fire=0.5`，`vote_frac=0.5`，优质子分等权，`q_floor=0.5`，`combined = quality*speculative`。

## 5. 历史回测机制 + 迭代修正机制 + 双重验证

### 5.1 历史回测机制（`rquant screen --backtest`）

复用 portfolio 横截面回放引擎，在 `[from,to]` 内每个调仓日回放整个树集成：按当日选择规则（§4）选 top-N、持仓 `rebalance` 根、等权，度量前瞻表现。产出 `ScreenBacktestReport`：
- **picks 净值曲线** vs 等权 universe 基准（整体 edge）。
- **按标签归因**：动量延续/突破临界/超跌反弹 各标签 picks 的命中率 + 前瞻收益（哪个形态真值钱）。
- **跨 regime 切片**（复用强度树切片法，按熊/牛窗口）：超额是否扛熊。
- **优质分分层**：标签股按优质分分位的前瞻风险调整收益（优质轴是否加值）。

### 5.2 双重验证

- **每信号因子式**（复用 `rquant factor --universe`）：把每棵树输出当因子求前瞻 IC/RankIC/ICIR/分层/衰减。**门槛（沿用 F-1）**：`|RankIC|>0.03 ∧ |ICIR|>0.3`，对应 horizon（动量延续 H=20、突破 H=10、超跌 H=10、优质 H=60）；两两 `corr<0.7`（冗余只留一）。同一 DSL 定义同源喂 screen 与 factor，口径一致。
- **集成式**（5.1 回测）：跨 regime 稳定正超额 + 扛熊（熊切片有超额）+ 敏感面非尖峰（参数鲁棒）。复用 eval 门槛哲学（保守，Indeterminate≠Pass）。

### 5.3 迭代修正机制（工具 + 人工纪律，**非自动调参器**）

自动优化选股器参数 = 过拟合，正是 eval 机制要防的。故：
- **每版选股器**（树集 + 权重 + 阈值）→ 跑 5.2 因子 IC + 5.1 集成回测 → 出**诚实裁决**（整体有无 edge / 各信号贡献 / 该汰谁）。
- **修正动作**（基于裁决，人工把关）：汰 IC 不达标的树、按 IC 重配权重、`corr>0.7` 冗余只留一、加候选树再测。
- **收敛判据**：组合回测跨牛熊稳定 edge + 各保留信号 IC 达标 + 敏感面非尖峰。
- **版本留痕**：每版配置 + 回测裁决存档（`tmps/screen-iter/` 或报告 md），可追溯"为何这版更好"，防瞎调。

## 6. CLI 交付面 + 配置（薄壳，复用现成评估/回测）

- `rquant screen --universe <csv> [--config <screen.yaml>] [--as-of <date>] [--top N] [--out <json>]`
  → 并行跑树集成 as-of 最新（或指定）K → 逐股 `{symbol, rank, quality_score, speculative_score, combined_score, tags[], reasons[]}`；打印表 + JSON。
- `rquant screen --backtest --universe <csv> [--config <screen.yaml>] --from <date> --to <date> [--rebalance N] [--top N] [--out <json>]`
  → 回放集成 → `ScreenBacktestReport`（净值 vs 基准 + 标签归因 + regime 切片 + 优质分层）；打印摘要 + JSON。

**集成配置（数据驱动，加/裁树=改配置非改码 → 深度无 churn）** `examples/screen_v1.yaml`：
```yaml
quality_trees:                  # 优质树（≥1）
  - examples/trees/screen/quality_v1.yaml
setup_trees:                    # 每形态可多树
  动量延续: [examples/trees/screen/momentum_v1.yaml]
  突破临界: [examples/trees/screen/breakout_v1.yaml]
  超跌反弹: [examples/trees/screen/pullback_v1.yaml]
merge:
  theta_fire: 0.5
  vote_frac: 0.5
  quality_weights: equal
  q_floor: 0.5
  combined: "quality*speculative"
  top: 10
```

## 7. 改动文件

| 文件 | 改动 |
|---|---|
| `src/screen/mod.rs` | 新建：树集成编排器——载配置+universe → 并行跑树（复用树评估）→ 合并成 `ScreenResult`（双输出）；`--backtest` 模式（复用 portfolio 横截面回放）→ `ScreenBacktestReport`（标签归因/regime 切片/优质分层）。纯函数核心 + 薄 IO。 |
| `src/screen/config.rs` | 新建：screen YAML 配置类型（quality_trees/setup_trees/merge）+ serde + 解析校验。 |
| `src/cli/mod.rs` | `Cmd::Screen{...}` 子命令 + `run_screen` / `run_screen_backtest` + `print_screen` / `print_screen_backtest`。 |
| `examples/screen_v1.yaml` + `examples/trees/screen/*.yaml` | 新建：集成配置 + 4 棵种子树（优质 + 3 形态，强度树同构）。 |
| 复用（不改或仅扩） | DSL/树评估、portfolio 横截面回测、`factor` IC、`backtest::gaps`/`data::quality`、`verdict` 门槛哲学。 |
| 测试 | 合并逻辑（标签投票 + 综合分）、回测归因（标签/regime/优质分层）、配置解析、种子树评估、`--as-of` 边界；**闸：`cargo test --workspace` + `cargo clippy --workspace`**（根 crate 公共 API 变动须 --workspace，吸取上次桥接漏编译教训）。 |

## 8. 诚实边界小结（Phase 1 非目标）

- **仅深 20 标的数据**（`data/universe_20.csv`，2018-2026）；**不**做几百只广度拉取、**不**做日频收盘运行、**不**做桌面"筛选"页——全是 Phase 2，且 **Phase 2 以 Phase 1 验证通过为闸**。
- **横截面偏薄的已知局限**：20 只做 `percentrank` 横截面排名偏薄（强度树 RV-5 即在 20 上验证过、可用）。Phase 1 证明的是**方法学/方向**（信号有无预测力、集成有无 edge）；横截面的统计功效与排名稳定性须在 Phase 2 的几百只广度上复核。诚实记录，不把 20 只的结果当广度结论。
- **仅横截面选择/标注树**（强度树家族，已证方向）；择时触发树排除。
- **OHLCV-only**，无基本面。
- **深度 = 已验证集成**：每棵树靠回测/IC 挣席位，不堆未验证的树；**过拟合（样本外/regime 稳健）是真闸，不是算力**。
- **可能证伪**：若集成无 edge / 信号不预测 → 诚实负结论，不 productize；这本身是有价值的产出（避免在错方向上投广度+UI）。
- 根 rquant crate 新增 `src/screen/` 模块 + CLI 子命令；不改既有回测/optimize/verdict 业务逻辑（仅复用）。
