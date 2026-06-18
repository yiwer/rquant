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

## 范围边界

本子项不含:GUI 内编辑配置/树、与上期 diff/导出/下单(子项 3)、做实 factor/optimize/portfolio 占位 + eval 认证视图(子项 2)、数据管线监控 UI(子项 3)。
