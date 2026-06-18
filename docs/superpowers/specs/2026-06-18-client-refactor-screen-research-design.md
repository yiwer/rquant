# 客户端重构 — 选股 & 迭代研究台 设计 (子项 1/3)

> 状态:已 brainstorm 定稿,待落实现计划(writing-plans)。
> 日期:2026-06-18

## 0. 背景与定位

桌面客户端(`desktop/`,Tauri2 + React18/antd6 + ts-rs)当初围绕**单标的时序决策树回测 + 三本纸面盘**而建(D / M2 / UX 系列)。此后产品整体转向**横截面基本面选股 + 迭代研究 + 认证 + 月度部署**:`rquant screen` 选股引擎、`iterate.py` 迭代 harness + ledger、`eval` 5 门槛认证、指数相对评估、行业中性、两腿组合、value 选股纸面盘——这些当前**客户端均未覆盖**。

重构本质:把客户端重心从"时序回测工具"迁到"选股研究与部署平台",同时**保留**现有时序能力(回测中心/驾驶舱/数据工作台不动)。

规模过大,拆为 3 个子项,各自 spec→plan→实现:

| 子项 | 内容 | 状态 |
|---|---|---|
| **1 选股 & 迭代研究台** | 本文档 | 设计中 |
| 2 认证 & 分析 | `eval` 裁决视图 + 做实 factor/optimize/portfolio 三占位 + 行业归因/两腿/部署加固分析器 | 待启 |
| 3 数据管线 & 部署 | 数据刷新监控(长任务)+ value 纸面盘第 4 本 + 月度 as-of→signal→book 闭环 | 待启 |

## 1. 范围

**纳入(子项 1):**
- **选股页**:`screen` as-of 选股榜(排行榜 + 打分/标签/命中理由 + 点行展开逐树打分)+ 选股回测(指数相对默认 + 基准切换 + 归因/regime/分层)。
- **研究页**:迭代 ledger 轮次台账 + round card / verdict 详情 + 一键跑轮(外壳调 `iterate.py`)+ 研究记忆区(待试/已证伪角度)。
- **指数相对评估**:Rust 桥内从 holdings 净值 + 指数 CSV 重算超额,支持基准即时切换(CSI300/500/1000;等权 EW 标注"不可投·参考")。
- **结构重构**:`commands.rs` / `dto.rs` 按域拆分(纯结构、不改现有行为)。

**排除(YAGNI / 留后续):**
- GUI 内编辑/新建选股配置与决策树 —— future(配置仍由 CLI/Claude 维护文件)。
- 与上期 diff / 导出 / 下单清单 —— 子项 3(部署动作)。
- `optimize` / `factor` / `portfolio` 三占位页做实、`eval` 认证视图 —— 子项 2。
- 数据刷新监控(baostock 抓取/watchdog/build)UI —— 子项 3。

## 2. 关键决策(brainstorm 定论)

| 决策点 | 结论 |
|---|---|
| 信息架构 | **两个新顶层区:`选股` + `研究`**(方案 A)。现有页不动,风险最低。 |
| 研究台职责 | **研究仪表盘 + 一键跑既有配置**。配置文件仍由 CLI/我维护,GUI 不做配置编辑。 |
| 选股榜深度 | 排行榜 + 质量/投机/综合分 + 标签 + 命中树·叶子理由 + 点行展开逐树打分。 |
| 指标口径 | **默认指数相对,可切基准**(CSI300/500/1000);EW 标"不可投·参考";OOS 超额 + harness 裁决摆一等位,绝对口径置次行。 |
| harness 架构 | **外壳调 Python harness(`iterate.py`)= verdict 唯一真源**;读 Python 产物(`.iter/ledger.jsonl` + ledger md);index-relative 在 Rust 桥重算(良性指标,可即时切基准)。 |

**诚实纪律(贯穿):** 裁决逻辑只有一个真源(Python),桥/前端不得二次实现门槛以免漂移放水;FALSIFIED 明确呈现不隐藏;缺数据给引导而非假装成功;指数相对(可投基准)为默认口径,EW 仅作参考。

## 3. 信息架构

侧栏(140px)新增两项,其余不动:

```
驾驶舱
回测中心
数据工作台
选股        ← 新   (选股榜 as-of | 选股回测)
研究        ← 新   (轮次台账 + round card + 跑轮 + 研究记忆)
(占位:策略树 / 因子工作台 / WFO / 组合 / 档案)
```

## 4. 页面设计

mockup 参考(持久化于 `.superpowers/brainstorm/535-1781778948/content/`):`ia-nav.html`、`screen-page.html`、`screen-backtest.html`、`research-page.html`。

### 4.1 选股页 / 选股榜(as-of)

布局沿用回测中心"左配置栏 + 主结果区"。顶部 tab:`选股榜(as-of)` | `选股回测`。

- **左栏**:配置下拉(`examples/screen/iter/*.yaml` + deploy frozen)· as-of 日期 · Top · 可选 membership · 可选 sectors · `运行选股` · 运行历史。
- **主区**:摘要行(配置 · as-of 日期 · 选中 N / 池 M · λ 口径)+ 工具条(排序 / 标签筛选 全部·质量·投机 / 搜索)+ 排行榜表。
- **表列**:`# | 代码 | 名称 | 综合分 | 质量分 | 投机分 | 标签 | 命中(树·叶子)理由`,默认按综合分降序,可改排序。
- **点行展开**:该股在各 quality 树的逐树打分 + 命中叶子路径(**复用 `ReplayView` 组件**)。

### 4.2 选股页 / 选股回测

- **基准切换**:`CSI300 | CSI500 | CSI1000 | 等权EW(不可投·参考)`,切换即时重算(不重跑回测)。
- **一等口径带(指数相对,视觉强调)**:净超额(累计)· 训练超额 · **OOS 超额(高亮·金标准)** · 胜基准年数 · break-even。正式 `PASS`/`FALSIFIED` 裁决见研究页 round card(verdict 唯一真源),本页不下裁决章。
- **绝对口径行(次要、弱化)**:净总收益 · 绝对 Sharpe · 最大回撤 · 日换手 · break-even。
- **净值图**:组合 vs 选定基准,阴影=超额,虚线标 OOS 起点。
- **底部三联**:regime 切片(train/OOS net 超额,OOS 高亮)· 标签归因 · 优质分分层(Q1→Q5 年化 + 单调性)。
- **图表布局优化要求(硬要求)**:真实现用 **ECharts + antd**(复用 `NavChart` 等),非 mockup 的示意 SVG;净值图加大并独立成行、底部三联规整成等高网格、指标带更紧凑、超额曲线可单列;**全应用统一 antd 图标集**。

### 4.3 研究页(迭代研究台)

- **左栏 跑一轮 launcher**:配置 · 假设/note · axis · Top · 基准 · 调仓 · 可选 sectors(行业中性)· `▶ 运行一轮`(→ 子进程跑 Tier-1 gross/net,过 OOS 闸再 Tier-2 敏感扫)· 实时进度。
- **左栏 研究记忆区**:待试角度 queue + 已证伪角度(不再试)——解析自 ledger md 两节。
- **主区上 轮次台账**:列 `# | label | 假设 | net超额 | OOS超额 | netSharpe | flags | verdict`,verdict 着色,筛选 全部 / PASS / 证伪。
- **主区下 round card(选中轮联动)**:
  - **verdict 逐条门槛**:gross 超额>0 · net-OOS 超额>0(高亮) · net Sharpe>0 · break-even≥40bps · 无 sign-flip,逐条 ✓/✗。
  - **Tier-2 敏感扫**:top∈{30,50,100}×reb∈{1,5} 的 net 超额 + 符号一致性。
  - **overfit flags** + 配置路径 + 联动按钮:`在选股回测打开 ↗` / `看配置 yaml`。

## 5. 桥架构 & 命令

### 5.1 文件组织(结构重构)

避免继续撑大 `commands.rs`(22 命令)/ `dto.rs`(30+ DTO):
- 新增 `desktop/src-tauri/src/commands/screen.rs`、`commands/iter.rs`(若现有 commands 为单文件,则抽为 `commands/mod.rs` + 子模块,现有命令迁入 `commands/legacy.rs` 或保持顶层 re-export,**不改签名/行为**)。
- 新增 `desktop/src-tauri/src/index_relative.rs`(纯 Rust 超额重算)。
- 新增 DTO 文件 `dto_screen.rs` / `dto_iter.rs`;现有 `dto.rs` 不动。
- `paths.rs` 增:screen 配置目录(`examples/screen/iter/`、`deploy/`)、`.iter/ledger.jsonl`、ledger md、`data/baostock/index/`、screen-run 归档目录。
- 复用 `TaskRegistry` + `TauriSink`,新增 task kind:`screen_asof` / `screen_backtest` / `iter_round`。
- Python 外壳沿用 `manual_run`/`backtest_run` 既有"spawn 子进程 + 进度"模式;Python 可执行路径走环境探测,缺失→友好报错。

### 5.2 命令清单(新增)

**选股**
- `screen_configs_list() -> Vec<ScreenConfigDto>` —— 枚举配置(name/frozen/解析错误)。
- `screen_asof(config, as_of, top, membership?, sectors?) -> String(task id)` —— 任务跑 `rquant screen --as-of --out tmp.json`,结果为 `ScreenResultDto`。
- `screen_pick_detail(run_id, symbol) -> ScreenPickDetailDto` —— 逐树打分 + 命中叶子(复用 replay)。
- `screen_backtest_run(config, from, to, top, rebalance, cost, sectors?) -> String(task id)` —— 任务跑 gross(0bps)+net(cost) 两次 `rquant screen --backtest`,归档 run;break-even 由 gross/net 在 Rust 算(良性算术,非裁决)。
- `screen_runs_list() -> Vec<ScreenRunMetaDto>`。
- `screen_run_report(id) -> ScreenBacktestReportDto`。
- `index_list() -> Vec<String>`(csi300/csi500/csi1000)。
- `screen_index_relative(run_id, benchmark) -> IndexRelativeDto` —— Rust 从 holdings 净值 + 指数 CSV 重算超额曲线 + 各 regime 超额;切基准即时、不重跑。

**迭代**
- `iter_ledger() -> Vec<LedgerRoundDto>` —— 解析 `.iter/ledger.jsonl`;假设文本取自 ledger md。
- `iter_round_card(round) -> RoundCardDto` —— verdict 逐条门槛 + Tier-2 扫 + flags + 配置路径。
- `iter_queue() -> IterQueueDto` —— 待试 / 已证伪角度(解析 ledger md 两节)。
- `iter_run_round(config, note, axis, top, benchmark, rebalance, sectors?) -> String(task id)` —— 任务 spawn `python scripts/iterate.py`,进度从 stdout;完成后读 `.iter/ledger.jsonl` 尾行得新轮次。

## 6. DTO(新增,ts-rs export)

- `ScreenConfigDto { path, name?, frozen, error? }`
- `ScreenResultDto { config, as_of, n_selected, n_pool, lambda, rows: Vec<ScreenPickDto> }`
- `ScreenPickDto { rank, symbol, name?, combined, quality, speculative, tags: Vec<String>, reason }`
- `ScreenPickDetailDto { symbol, per_tree: Vec<{tree, score, leaf, path: Vec<ReplayStepDto>}> }`(复用 `ReplayStepDto`)
- `ScreenRunMetaDto { id, config, from, to, top, rebalance, created, ok, error? }`
- `ScreenBacktestReportDto { meta, net_total_return, gross_total_return, abs_sharpe, max_drawdown, turnover, break_even, nav: Vec<{t, nav}>, tag_attribution: Vec<{tag, contrib}>, regime_slices: Vec<{label, from, to, net_excess?, net_sharpe?}>, quality_layers: Vec<{q, ann_return}> }`(net 主显,gross 仅供 break-even)
- `IndexRelativeDto { benchmark, excess_cum, excess_curve: Vec<{t, excess}>, per_regime: Vec<{label, excess}>, win_years?: String }`
- `LedgerRoundDto { round, label, note, benchmark, rebalance, axis, net_excess?, net_oos_excess?, net_sharpe?, flags: Vec<String>, verdict }`
- `RoundCardDto { round, label, benchmark, rebalance, verdict, gates: Vec<{name, pass, value?, threshold?}>, tier2: Vec<{top, rebalance, net_excess}>, flags: Vec<String>, config_path }`
- `IterQueueDto { queue: Vec<String>, falsified: Vec<String> }`

## 7. 数据流

- **跑轮**:UI → `iter_run_round` → `TaskRegistry` spawn `python iterate.py` → stdout 进度 → done 解析 `ledger.jsonl` 尾 → 刷新台账 + round card。
- **as-of 选股**:UI → `screen_asof` → spawn `rquant screen --as-of --out tmp` → 解析 `ScreenResult` → 表。
- **选股回测**:UI → `screen_backtest_run` → spawn gross+net 两次 `rquant screen --backtest` → 归档 → `screen_run_report`(break-even 由 Rust 从 gross/net 算)。
- **切基准**:UI → `screen_index_relative(run_id, bench)` → Rust 重算(不重跑)。
- **唯一真源**:verdict 裁决永远只在 Python `iterate.py`;桥/前端只读取与展示。

## 8. 错误处理

`errors.ts` 扩展友好映射:
- Python 缺失 / 依赖缺 → "未找到 Python 或 harness 依赖"(指引安装)。
- 数据缺(universe/index/baostock)→ 引导去数据工作台或抓取(子项 3)。
- 配置解析失败 → "选股配置解析失败"。
- index CSV 缺 → "缺少基准指数数据(运行 fetch_index)"。
- as-of 当日空池 → "该日无可选标的(成分/数据范围)"。

诚实文化:FALSIFIED 明确呈现;缺数据/缺依赖给可操作引导,绝不静默或伪装成功。

## 9. 测试

- **Rust 单测**:`index_relative` 重算 vs Python `to_index_relative` 在固定 fixture 上**对拍**(数值一致);`ledger.jsonl` 解析、round-card 解析、`screen_configs_list` 枚举;命令走 fixture 工作区(无网络、无真引擎依赖处用样例 JSON)。
- **前端 vitest**:覆盖 `Screen.tsx` / `Research.tsx` 两页 + 新组件(`ScreenPickTable` / `ScreenBacktestResult` / `LedgerTable` / `RoundCard` / `RunRoundForm`),仿现有 `.test.tsx`。
- **收尾闸**:`cargo test --workspace` 全绿 + `ui` build/vitest 通过 + 真数据冒烟(GUI 跑一轮 vs CLI `iterate.py` 对账,round card 数值一致)。

## 10. 后续子项(本文档不实现)

- **子项 2 认证 & 分析**:`eval` 5 门槛裁决视图;做实 `optimize`(WFO 网格热图)/ `factor`(IC/分层)/ `portfolio`(组合回测)三占位;行业归因 / 两腿 / 部署加固分析器(`analyze_*.py`)。
- **子项 3 数据管线 & 部署**:数据刷新监控(baostock 抓取/watchdog/build 长任务进度);value 选股纸面盘第 4 本(扩展驾驶舱);月度 as-of→signal→book 部署闭环(含与上期 diff / 导出 / 下单清单)。
