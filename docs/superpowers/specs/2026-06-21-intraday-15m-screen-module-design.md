# 15m 选股并行模块 设计（intraday-15m-screen）

> 状态：已 brainstorm 定稿，待 writing-plans。日期：2026-06-21。
> 前序：选股引擎/选股页（sub-1 SCR）+ 部署盘（sub-3a）+ 30 轮日线因子迭代（冠军=价值成长质量三核）已在 master。本模块复用桌面与 screen 范式。

## 0. 背景

用户要求：**新增一个 15m 选股模块，与日线选股并行**。

诚实约束（已与用户确认，必须在模块内标注）：
- 基本面是**日频**的 → 15m 选股只能用**日内微结构/技术因子**。当年 12 个日内因子（last_leg/intraday_rev/close_vs_vwap/intraday_range/vol_tilt/overnight）**全部证伪、无验证 edge**（见 [2026-06-18-intraday-daily-selection-findings.md](2026-06-18-intraday-daily-selection-findings.md)）。
- 数据：sina 15m、2021-01 起、**幸存者偏差**（无退市/停牌 15m）→ 无法正经 OOS。
- ⇒ 本模块定位为**可配置框架 + 占位**（不预置证伪因子），供用户后续自行迭代 15m 因子；**实验性，非已验证策略**。

用户选定（brainstorm）：

| 决策 | 结论 |
|---|---|
| 因子 | **只搭可配置框架 + 占位树**（不硬接 12 棵证伪树） |
| 范围 | **完整 GUI 并行面板**（选股页加「15m选股（实验）」tab，与日线并列） |

## 1. 架构（引擎零改动，复用 `run_screen`）

15m 选股 = `rquant::screen::run_screen` 跑在 **15m universe + 15m 配置** 上。引擎已支持任意 universe/config（日线选股、部署盘同一函数），无需改 `src/`。

### 1.1 数据：15m universe（新建）
`data/baostock/universe_baostock_15m_feat.csv`，列同 `universe`（`symbol,primary,context,fundamentals`）：
- `primary` = `data/baostock/k15m/<sym>.csv`（15m OHLCV bar）。
- `fundamentals` = `data/baostock/features_15m/<sym>.csv`（**31 个 15m 技术指标**：ret/amplitude/ma5..ma60/ema/volma/macd_dif|dea|hist/rsi14/boll_*/atr14/kdj_k|d|j/cci14/wr14/obv/vwap20/roc12/rvol20/corr_pv20，date-keyed）→ 树里经 `fund.<col>` 取用（如 `fund.rvol20`、`fund.kdj_k`、`fund.macd_hist`）。
- `context` 留空。
- 生成脚本 `scripts/build_universe_15m_feat.py`：取**同时有** `k15m/<sym>.csv` 且 `features_15m/<sym>.csv` 的 symbol（~1034），写绝对路径 CSV（镜像 `build_intraday_universe.py` 风格）。

> 注：现有 `universe_baostock_15m.csv` 的 fundamentals 指向日频财务（roe/bps…），**不适用**于 15m 因子选股 → 不复用，新建 feat 版。

### 1.2 配置：占位可配置 15m 配置（新建）
`examples/screen/intraday/15m_placeholder.yaml`：标准 screen 配置（quality_trees + setup_trees inert + merge top/lambda0 + regimes），`quality_trees` 指向**一棵示例占位树**；顶部大注释列出 features_15m 的 31 个可用列 + "在此替换/新增你的 15m 因子树"。

示例占位树 `examples/trees/screen/intraday15m_example.yaml`：
- `meta{name:intraday15m_example, forward_window:1}`，`weight: "sigmoid(fund.rvol20 - 1)"`（放量示例；**注释明确：仅占位示例，无验证 edge，自行替换**）。gate 仅排除缺数据。
- 走现有 `fund.<col>` 通道，0 引擎改动。

### 1.3 桥层（新增 2 命令，复用既有 DTO/任务）
`desktop/src-tauri/src/screen_cmds.rs` 加：
- `const SCREEN_15M_UNIVERSE = "data/baostock/universe_baostock_15m_feat.csv"`；`SCREEN_15M_CONFIG_DIR = "examples/screen/intraday"`。
- `screen_15m_configs_list() -> Vec<ScreenConfigDto>`：列 `examples/screen/intraday/*.yaml`（镜像 `screen_configs_list`）。
- `screen_15m_asof(config, as_of, top) -> Result<String,String>`：镜像 `screen_asof`，但 `universe_path = SCREEN_15M_UNIVERSE`；经 `state.tasks.start("screen_15m_asof", …)` 异步跑 `run_screen` → 复用 `ScreenResultDto`（同日线，零新 DTO）。窗口 window 取适合 15m 的值（默认 60 根 15m ≈ 1.5 日；spec §6 可调）。
- 在 `generate_handler!` 注册 2 命令。

### 1.4 as-of 语义
15m bar 为日内时间戳。用户在 GUI 选**日期**；`pick_as_of`（[screen/mod.rs:127](../../src/screen/mod.rs)）取 ≤ 该日的最后一根 15m bar（即该日**尾盘 15:00 那根**）。无该日数据 → 回退最近可用 + 结果 `as_of` 字段如实显示（同日线 Q3 语义）。

### 1.5 前端（选股页加 tab）
`desktop/ui/src/pages/Screen.tsx`：`Tabs.items` 加第 3 项 `{ key:"intraday15m", label:"15m选股（实验）", children:<Intraday15mTab/> }`。
- `Intraday15mTab`（镜像 `AsofTab`）：选 15m 配置（`screen15mConfigsList`）+ 日期 + top + 「运行选股」→ 全局任务 store 跑 `screen15mAsof` → 复用 `ScreenPickTable` 渲染排行榜 + `TaskRunning` + `friendlyError` + `SymbolLabel`。
- **顶部醒目红字标注**：「⚠️ 实验模块：15m 因子无验证 edge、数据有幸存者偏差/无 OOS；占位配置仅供你迭代因子，勿当已验证策略」。
- `desktop/ui/src/api/ipc.ts`：加 `screen15mAsof`、`screen15mConfigsList`（invoke 包装）。

## 2. 数据流
GUI 选 15m 配置 + 日期 + top → `screen_15m_asof` → `run_screen(universe_15m_feat, config_15m)` 读 k15m bar + features_15m（经 fund.<col>）→ 跑占位/用户树 → 排行榜 → `ScreenResultDto` → 前端 `ScreenPickTable`。日线选股路径完全不受影响（独立 universe/命令/tab）。

## 3. 错误处理 / 诚实
- 缺 15m universe / features 文件 → `run_screen` 报错经 `friendlyError` 红字透出，不臆造。
- 占位因子缺值（features_15m 有 NaN 预热）→ gate 弃权（树已处理）。
- 模块全程标注**实验·无验证 edge·无 OOS**（UI 红字 + 配置注释 + 文档）。
- 不写任何账本、不接部署盘（纯只读选股展示）。

## 4. 测试
- 后端：`screen_cmds` 若有可单测的纯逻辑（配置列举/universe 路径）加单测；`screen_15m_asof` 经任务异步，靠真数据冒烟。
- 前端：`Intraday15mTab` vitest（mock api → 渲染排行榜 / 运行态 / 报错）；复用 ScreenPickTable 既有测试。
- 收尾：`cargo test --workspace` + `tsc --noEmit` + `npm run test -- --run` + `npm run build` 全绿；**真数据冒烟**：`build_universe_15m_feat.py` 生成 universe → GUI 15m tab 选最近日跑 → 排行榜非空（占位树按 rvol20 排序）。
- CLI 对拍：`rquant screen --config examples/screen/intraday/15m_placeholder.yaml --universe data/baostock/universe_baostock_15m_feat.csv --as-of <最近日> --top 50` 与 GUI 同源。

## 5. 范围边界（YAGNI）
- **仅 as-of 选股**（用户要的"选股"）；**不做 15m 回测 tab**（占位因子无 edge，回测低价值；研究用 iterate.py intraday 轴已有）——留future。
- 不预置/复用 12 棵证伪树（用户选框架+占位）。
- 不改日线选股/部署/引擎。
- 不抓新数据（用现有 k15m 到 2026-06-18 + features_15m）；15m 数据刷新沿用既有管线，非本模块范围。
- 不做因子有效性验证（本模块是迭代框架；验证靠用户后续用 iterate.py/eval 跑）。

## 6. 参数缺省（实现时定，可后续调）
- `top` 默认 50；`window` 默认 60（15m 根）；occurrence/regimes 用占位配置内缺省。
- 15m universe 取全部有 features 的 ~1034 只（非流动性筛选；用户可后续加流动性闸树）。
