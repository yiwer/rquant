# 桌面端「选股」与「研究」页(子项 1/3)

桌面客户端新增两个顶层页,把横截面选股引擎与迭代研究 harness 搬上 GUI。设计见
`docs/superpowers/specs/2026-06-18-client-refactor-screen-research-design.md`,计划见
`docs/superpowers/plans/2026-06-18-client-refactor-screen-research.md`。

## 选股页

- **选股榜(as-of)**:选一个配置(`examples/screen/iter/*.yaml` + deploy 冻结选股配置)+ as-of 日期 + Top → 运行 → 排行榜(综合/质量/投机分、标签、命中树·叶子理由,点行展开逐树打分)。
- **选股回测**:同配置跑 gross+net 历史回测并归档 → 结果以**指数相对为一等口径**(基准切换 CSI300/500/1000;等权 EW 标"不可投·参考"),OOS 超额高亮;次行为绝对口径(净总/Sharpe/回撤/换手/break-even);累计超额曲线 + regime/标签归因/优质分分层三联。

## 研究页(迭代研究台)

- **跑一轮**:选配置 + 假设 note + 基准 → 后台子进程跑 `python scripts/iterate.py`(verdict 唯一真源)。
- **轮次台账**:读 `.iter/ledger.jsonl`,PASS/证伪着色;点行看 **round card**(verdict 逐条门槛 gross/net-OOS/Sharpe/break-even/无 sign-flip + Tier-2 敏感扫 + flags)。
- **研究记忆**:待试 / 已证伪角度(解析 `docs/superpowers/iteration-ledger.md` 两节)。

## 架构要点

- 选股 as-of / 回测**直接调 `rquant::screen` 库**(`tokio` runtime,非子进程);跑轮**外壳调 Python harness**。
- **verdict 裁决永远只在 Python `iterate.py`**;Rust/前端只读取展示(`iter_read::gates_from` 仅把 flags/metrics 映射成展示行,不重判)。`break_even`、指数相对超额是良性算术,在 Rust 桥重算(可即时切基准)。
- 新域代码在新文件:`screen_cmds.rs` / `iter_cmds.rs` / `index_relative.rs` / `iter_read.rs` / `screen_runs.rs` / `dto_screen.rs` / `dto_iter.rs`;现有 `commands.rs` / `dto.rs` 不动。

## 数据依赖

- universe:`data/baostock/universe_baostock_day.csv`
- 基准指数:`data/baostock/index/{csi300,csi500,csi1000}.csv`(缺则跑 `scripts/fetch_index.py`)
- ledger:`.iter/ledger.jsonl`(+ `docs/superpowers/iteration-ledger.md` 队列段)
- 跑轮需机器装好 Python 与 harness 依赖。
- **口径与 harness 一致**:GUI 选股 as-of/回测用 `universe_baostock_day.csv` 且**不加** point-in-time membership 掩码 —— 与 `iterate.py` 的实际口径(显式 `membership="none"`,见 iterate.py)完全相同,故 GUI 数值可与轮次台账直接对照。(daily_eval 的默认 membership 仅用于其独立调用,iterate.py 已覆写为 none。)

## 交互冒烟(GUI,需图形界面)

`cd desktop && npm run tauri dev` 启动后:① 选股页跑 `value_pb_base.yaml` as-of 2026-06-12 top50 → 排行榜非空;② 研究页台账显示 10 轮,点 R10 看 round card,核对 OOS 超额/verdict 与 `.iter/ledger.jsonl` 一致。

## 认证 & 分析(子项 2a,2026-06-20)

- **认证**(新顶层页):选 1+ 个已有 optimize 报告 JSON(扫 `.daily_runs/` 与仓库根)→ 运行认证(`rquant::verdict::certify`,默认阈值,**不重判**)→ 5 门槛 Verdict 矩阵(✓/✗ · 值 · 阈值 · 说明)。
- **因子工作台**(填实 `/factor`):选 universe + 输入因子 DSL(如 `fund.bps/close` 账面市值比、`fund.roe`、`sma(close,20)`)+ horizon/分层/采样 → IC/RankIC 表 + IC 衰减 + 分层收益 + 相关阵(`rquant::factor::run_factor`)。
- **选股回测结果 →「分析」tab**:行业归因(配置/选择拆分)· 两腿(再选成长 run + w 行高亮)· 部署加固(T+1 拖累 + 容量);均为 Rust 端口的后验算术、数值对拍 `analyze_*.py`,**基准固定沪深300**(已在卡片标注)。
- 设计/计划:`docs/superpowers/{specs/2026-06-20-client-refactor-cert-analysis-design.md, plans/2026-06-20-client-refactor-cert-analysis.md}`。

## 部署(value 纸面盘,子项 3a,2026-06-20)

把已验证的价值策略前向 go-live 跟踪搬上 GUI:新顶层 **`部署`** 页 + 驾驶舱 **第 4 卡「价值选股盘」**。

- **机制(screen 驱动,忠实)**:月度 `deploy_run_month(as_of)` 跑冻结配置 `deploy/value_pb_deploy_frozen.yaml`(top-50)的 as-of 选股 → 与上月持仓 **diff**(买/卖/持,等权)→ 拟 NAV/超额 vs 沪深300。两步手动:`跑本月(预览)`(**不落账**)→ `确认调仓`(`deploy_commit_month` 才写状态 + 滚 NAV + 追加 journal)。状态文件 `.rquant-desktop/deploy_book/value.json`(原子写,gitignored)。
- **NAV**:go-live 起 NAV=1 前向滚动,持有期收益=上月持仓 `last_date→as_of` 的等权 kday close 收益;超额=本盘累计 −(沪深300 同期),首月归零。历史表现仍看「选股回测」(回测=历史,本账本=前向纸面跟踪)。
- **纪律(诚实)**:纸面**只跟踪 NAV、不下真单**;预览绝不写、确认才落账;数据缺/损坏不臆造——零行情覆盖、沪深300 不覆盖该日、`as_of` 超数据(实际交易日≠所选)、`value.json` 损坏 均**报错拒绝**而非伪造 NAV 或静默重置;commit 失败不弹成功提示,确认按钮防重复提交。**verdict/裁决不涉**(deploy 只展示,不判 PASS/证伪)。
- **复用**:`screen::run_screen`(冻结配置)、`index_relative`(沪深300 超额)、`TaskRegistry`、`crate::dto::DiffRowDto` + 前端 `DiffTable`;NAV 曲线因 `NavChart` 与 journal 形态耦合,内联 ECharts 双线图(本盘 NAV + 沪深300 虚线)。
- **验证**:`deploy_book.rs` 纯逻辑 TDD(diff 买卖持、ew_return 等权收益、状态读写);前端 `stores/deploy.test.ts`(commit 清预览+重载);**真数据对账**:`rquant screen --config deploy/value_pb_deploy_frozen.yaml --as-of <最新数据日> --top 50 --window 260` 与部署页预览同走 `run_screen`(同冻结配置/top/window、均无 LLM 走默认枝),数值可直接对照(2026-06-17:universe 1073、top-50)。**GUI 交互冒烟**(需图形界面):部署页选最新数据日→跑本月→首月全 Buy 50→确认→NAV=1 建仓 + journal 一条。
- **已知取舍**:部分持仓行情缺失时 `ew_return` 按已覆盖名等权(零覆盖才报错);`deploy_commit_month` 同步执行(单用户月频,可接受);严格幂等(同 as_of 防重追加)留后续,前端确认后清预览+防重复点缓解。
- 设计/计划:`docs/superpowers/{specs/2026-06-20-client-refactor-deploy-design.md, plans/2026-06-20-client-refactor-deploy.md}`。

## 任务运行体验(task-ux,2026-06-20)

所有任务驱动页(选股指定日/选股回测/部署跑本月/因子/研究跑轮/回测中心)统一运行反馈,解决"只显示『选股中…』、不知要多久、切页任务/结果丢失"。

- **全局任务态**:新 `stores/tasks.ts` 是 `task://progress` 的**唯一订阅者**(+ `task_list` 播种),持 `tasks{id→TaskInfoDto}`+`startedAt`;`App` 启动 `init()` 一次;`TaskDrawer` 改读它(去重)。`trackTask(id,{done,failed,cancelled})` 把任务终态一次性回调给域 store。
- **运行态/结果在 store 不在组件**:各域 store 持 `*TaskId`+结果/`runError`,故**切页不丢**(全局监听常驻,即便页面卸载也能捕获结果)。共享 `components/TaskRunning.tsx`:阶段(中文)+ 已耗时计时 + 进度条(`pct∈(0,1)` 才用百分比,否则 indeterminate,不伪造)+ 取消。
- **诚实**:失败/取消在页内红字显示(`friendlyError`),不再静默;`startedAt` 对启动前已在跑的任务为近似(标"约")。**单监听不变量**:全 `desktop/ui/src` 仅 `stores/tasks.ts` 一处 `listen("task://progress")`。
- **性能注**:选股慢的根因是 **debug 构建**(`cargo tauri dev` 同一选股 16.5s vs release 1.8s,~9×)——用 `cargo tauri dev --release`(桥优化)或 `cargo tauri build` 解决;本项让等待**可见**,速度由 release 构建解决。
- 设计/计划:`docs/superpowers/{specs/2026-06-20-client-task-ux-design.md, plans/2026-06-20-client-task-ux.md}`。

## 范围边界

本子项(选股&研究台 + 认证&分析 + value 部署)不含:GUI 内编辑配置/树、optimize(WFO 网格)/ portfolio(组合回测)做实(子项 2b)、数据管线监控 UI(子项 3b)、自动月度排程(仅手动按钮)、真实下单/资金(纸面只跟踪 NAV)。
