# 客户端重构 — value 部署 设计 (子项 3a)

> 状态:已 brainstorm 定稿,待 writing-plans。日期:2026-06-20。
> 前序:sub-1(选股&研究台 `a474608`)、sub-2a(认证&分析 `d357bbf`)已合入 master。本子项复用其全部桌面范式。

## 0. 背景与范围

子项 3「数据 & 部署」原捆 3 面(数据管线监控 / value 纸面盘第4本 / 月度部署闭环)。切片:
- **3a(本文档)= value 部署**:value 选股纸面盘第 4 本 + 月度 `as-of→diff→signal→book` 闭环(含 sub-1 缓的 diff/下单清单)。= 把已验证的价值策略前向 go-live 跟踪。
- **3b(后续)= 数据管线监控**:baostock 抓取/build 状态面板。

**关键决策(brainstorm 定论)**:
| 决策 | 结论 |
|---|---|
| 第4本机制 | **screen 驱动**:月度 `rquant screen --as-of`(冻结配置 `deploy/value_pb_deploy_frozen.yaml` top-50)→ diff → 下单清单 → 落账。忠实复现已验证 screen 部署口径(非单 signal 树)。 |
| 调仓触发 | **手动「跑本月」按钮**,两步:预览(不落账)→ 确认 → 落账。无自动副作用。 |
| NAV | **go-live 起 NAV=1 前向滚动**(持有期 EW 收益=持仓名 kday close 月度收益均值),vs 沪深300 超额。历史表现看 sub-1「选股回测」(回测=历史,本账本=前向纸面跟踪)。 |
| IA | 驾驶舱加**第 4 卡「价值选股盘」** + 新顶层 **`部署`** 详情页。 |

**纪律**:纸面盘**只跟踪 NAV、不下真单**(安全规则禁自动真实交易);手动触发、**确认后才落账**;screen 用冻结部署配置;数据假定已刷新,as-of 超数据覆盖则警示。**verdict/裁决不涉**;diff/NAV 纯算术。

## 1. 信息架构

侧栏在 sub-1/2a 基础上新增 `部署`。驾驶舱(Cockpit)在 b1/b2/b3 三卡后加**第 4 卡「价值选股盘」**(独立数据源 `deploy_book_read`,不动现有 `cockpit_overview` 三本装配)。第 4 卡点开 → `部署` 页。

## 2. 页面设计

### 2.1 驾驶舱 第 4 卡「价值选股盘」
- 来自 `deploy_book_read`:NAV · 持仓数 · 上次调仓日 · 累计超额 vs 沪深300 · 状态(空=未 go-live / ok)。空态引导"去部署页跑首月建仓"。点卡 → `/deploy`。

### 2.2 `部署` 页(新顶层)
- **账本概览**:NAV · 累计/各月超额 vs 沪深300 · 持仓数 · 上次调仓。
- **NAV 曲线**:本账本 NAV vs 沪深300(复用 `NavChart`)。
- **跑本月**:选月末日期(默认最新数据日)→ `预览`:当月选股 top-50 + 与上月持仓 **diff**(买/卖/持,复用 `DiffTable`)+ 拟新 NAV/超额(**不落账**)→ `确认调仓` → 落账。
- **月度 journal**:历次调仓(日期 · NAV · 超额 · 持仓数 · 当月买卖数),点开看当月持仓/下单清单。
- **当前持仓**:top-50 列表(symbol · 权重 EW · 上月起持有)。

## 3. 桥架构 & 命令

**新文件**(`desktop/src-tauri/src/`):
- `deploy_book.rs` — 纯逻辑 + TDD:状态模型;`diff(prev, next) -> Vec<DiffRow>`(买/卖/持);`roll_nav(prev_holdings, prev_close→asof_close) -> 月度 EW 实现收益`;NAV/超额滚动。
- `deploy_cmds.rs` — 命令:`deploy_book_read() -> DeployBookDto`;`deploy_run_month(as_of) -> DeployMonthDto`(任务:as-of screen + diff + 拟 NAV,**不写**);`deploy_commit_month(as_of) -> Result<(), String>`(确认落账:滚动 NAV + 写状态 + 追加 journal)。
- `paths.rs` 加 `deploy_book_path() -> .rquant-desktop/deploy_book/value.json`、`deploy_book_journal()`。

screen 直调 `rquant::screen::run_screen`(冻结配置);diff/NAV 纯 Rust 算术(读 kday close + index_relative 算超额);原子写状态(仿 `screen_runs::write_atomic`)。**复用**:`screen_runs`/`index_relative`/`screen::run_screen`/`TaskRegistry`。

## 4. DTO(新增,`#[derive(Debug,Clone,Serialize,TS)] #[ts(export)]`)

- `DeployHoldingDto { symbol: String, weight: f64, since: String }`
- `DeployNavPointDto { t: String, nav: f64, bench_nav: f64 }`
- `DeployMonthRecDto { as_of: String, nav: f64, excess: f64, n_holdings: u32, n_buy: u32, n_sell: u32 }`
- `DeployBookDto { status: String, nav: Option<f64>, excess_total: Option<f64>, last_rebalance: Option<String>, holdings: Vec<DeployHoldingDto>, nav_history: Vec<DeployNavPointDto>, months: Vec<DeployMonthRecDto> }`
- `DeployMonthDto { as_of: String, picks: Vec<DeployHoldingDto>, diff: Vec<DiffRowDto>, proj_nav: f64, proj_excess: f64, realized_ret: f64 }`(**复用 sub-1 `DiffRowDto`** {symbol, action, from_w, to_w})

状态文件 `value.json`:`{ holdings: [{symbol,weight,since}], nav_history: [{t,nav,bench_nav}], months: [{as_of, picks, diff, nav, excess}] }`。

## 5. 数据流

- 看盘:Cockpit→`deploy_book_read`→第 4 卡;`部署`页→`deploy_book_read`→概览/曲线/journal/持仓。
- 跑本月:`部署`页→`deploy_run_month(as_of)`(task:screen as-of + 读状态 diff + 算拟 NAV)→ 预览 diff/下单清单 →(用户确认)→`deploy_commit_month(as_of)`→ 写状态 + journal → 刷新。
- NAV 滚动(commit 时):若有上月持仓,算 上月持仓 EW 收益(prev_date→as_of,kday close)→ 滚 NAV;bench_nav 用沪深300 同期;再置持仓=当月 picks;首月 NAV=1 建仓。

## 6. 错误处理

`errors.ts` 扩:as-of 超数据覆盖→"该日数据未刷新(去数据工作台/抓取)";冻结配置缺→"部署配置缺失";持仓名 kday 缺→"持仓行情缺失,NAV 可能不全";空账本→引导跑首月。诚实:数据不全明示、不臆造 NAV;确认前不落账。

## 7. 测试

- Rust:`deploy_book.rs` 纯逻辑 TDD —— `diff`(买/卖/持,fixture 已知前后持仓)、`roll_nav`(fixture 已知持仓+价→已知 EW 收益与 NAV)、首月建仓 NAV=1、超额 vs 指数;状态读写往返。
- 前端:vitest 覆盖 `Deploy.tsx` + 价值盘卡 + 复用 DiffTable;注入 store mock。
- 收尾:`cargo test --workspace` + ui build/vitest + 真数据冒烟(对最新数据日跑 `deploy_run_month` 预览,核对选股 top-50 与 `rquant screen --as-of` CLI 一致;commit 后 NAV/journal 落账正确)。

## 8. 边界(YAGNI)

不含:数据管线监控/控制(3b);自动月度排程(仅手动按钮);真实下单/资金(纸面只跟踪 NAV);历史 NAV 回填(go-live 前向跟踪,历史看选股回测)。
