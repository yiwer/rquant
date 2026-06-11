# rquant：report --sim / --portfolio HTML 渲染 — 设计文档

- **日期**：2026-06-11
- **状态**：设计已确认（写 spec + 计划）
- **关联**：master `b955bed`。E4/E5 的 HTML follow-up 收口：四种运行模式（硬/软打分、sim、portfolio）的报告渲染补全。

---

## 1. 目标与非目标

### 目标
1. `report --sim --report sim_report.json [--traces steps.jsonl] --out sim.html`：净值曲线 + 仓位轨迹（均需 traces，无则占位提示）、回合收益直方图 + 回合表（report 内嵌 trades，无需 traces）、headline。
2. `report --portfolio --report portfolio.json --out port.html`（自足，holdings 内嵌）：组合 vs 基准双线、选中频率条形、持仓明细表、headline。
3. `--soft/--sim/--portfolio` 三旗标互斥（多选 → 错误退出）；`render_report_files` 的 `soft: bool` 升级为 `ReportMode { Hard, Soft, Sim, Portfolio }`。
4. 新图元 `multi_line_chart`（≤PALETTE 数线 + 图例）；`curve::histogram` 逻辑提为 `pub(crate) histogram_of(&[f64])`（原函数薄包装，行为不变）。
5. 确定性（同输入同字节）；表截断 50 行注明总数。

### 非目标（YAGNI）
- 交互；sim 回撤独立曲线（净值曲线已含信息）；portfolio 逐期权重堆叠；`--sim`+`--primary` 等无关组合（忽略+提示沿用 soft 的处理风格）。

## 2. 架构

### 2.1 `report/curve.rs`
```rust
pub(crate) fn histogram_of(values: &[f64]) -> Histogram   // 原 histogram(points) 内核提取
```
原 `histogram(points: &[SeriesPoint])` 改为 `histogram_of(&nets)` 薄包装；既有测试不变。

### 2.2 `report/viz.rs`
```rust
/// 多线折线图：每条 (名, 点列)，PALETTE 着色 + 图例；y 域取全体 min/max（含 0 不强制）。
pub fn multi_line_chart(series: &[(String, Vec<(f64, f64)>)], title: &str) -> String

pub fn render_sim_html(report: &SimReport, steps: Option<&[SimStepRecord]>) -> String
pub fn render_portfolio_html(report: &PortfolioReport) -> String
```
- sim：headline 表（7 项）→ 净值 `line_chart`（steps.nav）/ 仓位 `line_chart`(steps.pos)（None → `<p>` 占位）→ 回合收益 `histogram_svg(histogram_of(trip_returns))` → 回合表（≤50 行 + "共 N 回合"）。
- portfolio：headline 表（7 项）→ `multi_line_chart([组合 nav, 基准 nav])`（x=调仓序）→ 选中频率 `bar_chart`（次数/调仓数，按 symbol 字典序）→ 持仓表（≤50 期 + 注明）。
- 确定性同既有约定（定宽格式化、BTreeMap 序）。

### 2.3 CLI / render_report_files
```rust
pub enum ReportMode { Hard, Soft, Sim, Portfolio }
pub fn render_report_files(report_path, out_path, traces_path, primary_path, mode: ReportMode) -> Result<()>
```
- CLI：`--sim`/`--portfolio` 旗标加入 `Cmd::Report`；`(soft as u8 + sim + portfolio) > 1` → anyhow 错误；映射 ReportMode。
- Sim 臂：读 SimReport；traces 给出 → 逐行 SimStepRecord；primary 给出 → 忽略+提示。Portfolio 臂：读 PortfolioReport（traces/primary 给出 → 忽略+提示）。
- 既有 Hard/Soft 臂行为逐字不变（`soft: bool` 调用点改 mode 枚举的机械涟漪：cli + e2e 的 `render_report_files(..., true/false)` 调用）。

## 3. 测试
- `histogram_of` 提取后既有 curve 测试不变（行为等价）。
- `multi_line_chart`：双线含两个 `<polyline>` + 两个图例文本；确定性。
- `render_sim_html`：构造 SimReport（含 2 笔 trades）+ steps Some/None 两态——含 `<polyline>`（Some）/占位（None）、`<rect>`（直方图）、回合表行、确定性。
- `render_portfolio_html`：构造 PortfolioReport（3 期 holdings）——双线、频率条形、持仓表、确定性。
- e2e：run_sim（带 traces）→ render_sim_html 含 `<polyline>`；run_portfolio → render_portfolio_html 含双线图例；CLI 互斥（`--soft --sim` 同给 → Err，经 render_report_files 模式构造层测或 CLI parse 测）。
- 真数据 smoke：E4/E5 的 smoke 命令各加一步 `report --sim`/`--portfolio` 出 HTML 检查关键子串。
- 文档：cli-reference（report 四模式表）、README 两句。

## 4. 里程碑
- **T1** `histogram_of` 提取 + `multi_line_chart` + 测试。
- **T2** `render_sim_html` + `render_portfolio_html` + 测试。
- **T3** CLI `ReportMode` 互斥 + render_report_files 升级（涟漪）+ e2e + 文档 + 真数据 smoke。
