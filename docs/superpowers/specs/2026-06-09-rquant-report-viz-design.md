# rquant：报告可视化（`rquant report` → 自包含 HTML）— 设计文档

- **日期**：2026-06-09
- **状态**：设计已确认（写 spec + 计划）
- **关联**：M1–M6 + follow-up + 软遍历已合并 master（HEAD `46988da`）。本设计加一个**纯消费者**子命令，把回测产物渲染成 HTML 图表。

---

## 1. 背景

回测产出 `report.json`（聚合度量 + gaps）与 `traces.jsonl`（逐点 `{t, path, leaf, stance}`）。`print_summary` 已打印数字，但缺图。本次加 `rquant report` 子命令：读这些产物，生成**自包含 HTML**（内联手写 SVG），含累计收益曲线 + 分布 + 分解条形 + headline。回测引擎与输出格式**不改**。

## 2. 目标与非目标

### 目标
1. `rquant report --report report.json --out report.html [--traces traces.jsonl] [--primary csv]` 生成自包含 HTML。
2. 图表：累计前瞻收益曲线、逐点净收益直方图、by_leaf 平均净收益条形、node 命中条形、headline 表（+ 重叠警告 + gaps）。
3. 逐点 net 由可视化器用现有 `forward_return` **重算**（不改 runner/格式）；确定性、与回测同口径。
4. 优雅降级：无 `--traces`/`--primary` 时只画聚合（无时间序列）。

### 非目标（YAGNI / 后续）
- 软模式 `SoftReport` 可视化（无逐点 traces、结构不同）——后续（需先写 soft traces）。
- 交互式图表 / JS 框架 / 在线 CDN（坚持零运行时依赖、离线、自包含）。
- 大 traces 的降采样（先全画；点数巨大再说）。
- 决策树结构图 / 个股逐笔。

## 3. 锁定决策

| # | 维度 | 选定 |
|---|---|---|
| 1 | 形式 | 自包含 HTML + 内联手写 SVG（零新依赖、离线、确定性）|
| 2 | 范围 | 含累计收益曲线（由 traces+primary 重算 net）|
| 3 | 报告类型 | **硬模式 `report.json` 优先**；软报告留后续 |
| 4 | 数据接入 | 给相关类型补 `Deserialize`；可视化器读回 JSON/JSONL |
| 5 | 重算 | 用 `forward_return` 重算逐点 net（不改 runner/格式）|
| 6 | 降级 | `--traces`/`--primary` 可选，缺则只画聚合 |

## 4. 架构

### 4.1 数据接入（补 `Deserialize`）
当前这些类型只 `derive(Serialize)`，需补 `Deserialize`（仅加 derive）：
- `report::{Report, SoftReport 不需}`、`backtest::metrics::{Metrics, SignalStat}`、`backtest::gaps::{GapReport, PartialDay}`、`engine::trace::{Trace, StepRecord}`、`tree::schema::Stance`。
- `NaiveDateTime` 经 chrono serde（已启用）。`BTreeMap` 原生支持。
- 加往返测试（serialize → deserialize 相等）防回归。

### 4.2 曲线重算（`src/report/curve.rs`，新）
```rust
pub struct SeriesPoint { pub t: NaiveDateTime, pub net: f64, pub cum: f64 }
pub struct Histogram { pub bins: Vec<(f64, f64, usize)> } // (lo, hi, count)
pub struct EquitySeries { pub points: Vec<SeriesPoint>, pub hist: Histogram, pub skipped: usize }

/// 逐点重算 net：按 trace.t 定位 primary bar，forward_return(stance) → net，累加 cum。
/// 越界/找不到 bar 的点跳过并计 skipped。直方图对 net 分桶（固定桶数，如 21）。
pub fn derive_series(traces: &[Trace], primary: &[Bar], fw: usize, cost: &CostModel) -> EquitySeries
```
- 建 `HashMap<NaiveDateTime, usize>`（primary 时间→下标）；trace.t 精确匹配（决策 bar 时间）。找不到 → skipped++。
- `fr = forward_return(primary, i, fw, trace.stance, cost)`；`None`（越界）→ skipped++；`Some` → `net`，`cum += net`，push `SeriesPoint`。
- 直方图：min/max net → 固定 21 桶；空序列 → 空。

### 4.3 SVG / HTML（`src/report/viz.rs`，新）
纯函数，返回字符串，确定性（浮点定宽格式化，如 `{:.2}`）：
- `fn line_chart(points: &[(f64, f64)], w: u32, h: u32, title: &str) -> String`（`<svg>` + 轴 + `<polyline>`）。
- `fn bar_chart(labels: &[&str], values: &[f64], w: u32, h: u32, title: &str) -> String`（`<rect>` 正负分色 + 标签）。
- `fn histogram(hist: &Histogram, w: u32, h: u32, title: &str) -> String`（`<rect>` 柱）。
- `fn render_html(report: &Report, series: Option<&EquitySeries>) -> String`：拼装自包含 HTML（内联 `<style>` + 各图 SVG + headline 表 + 重叠警告 + gaps）。曲线醒目标注"窗口重叠 → 信号质量曲线，非可交易净值"。

### 4.4 CLI（`src/cli/mod.rs`）
新增 `Cmd::Report`：
```
rquant report --report report.json --out report.html [--traces traces.jsonl] [--primary 15m.csv]
```
流程：读 `report.json` → `Report`；若 `--traces` 且 `--primary` 都给 → 读 traces（逐行 JSON → `Vec<Trace>`）+ primary CSV → `derive_series(traces, primary, report.forward_window, &CostModel{report.cost_bps})` → `Some(series)`；否则 `None`。`render_html(&report, series.as_ref())` → 写 `--out`。

## 5. 图表集（MVP）
1. 累计前瞻收益曲线（折线，需 series）。
2. 逐点净收益直方图（需 series）。
3. by_leaf 平均净收益条形（`report.metrics.by_leaf` 的 mean_net；正负分色）。
4. node 命中条形（`report.metrics.node_label_counts`）。
5. headline 表：active 的 count/mean_net/hit_rate/t_stat、buy_and_hold；重叠警告；gaps（missing_trading_days / partial_days 计数）。

## 6. 错误处理
- `--report` 读取/反序列化失败 → `Error`（冒泡）。
- `--traces` 给了但 `--primary` 没给（或反之）→ 警告 stderr 并降级为仅聚合（不报错）。
- trace.t 找不到对应 bar / 越界 → 跳过并计入 `skipped`（HTML 里标注"N 点未计入曲线"）。
- 空 traces / 空 series → 不画曲线/直方图，只画聚合。

## 7. 确定性（复现性第一约束）
- HTML 不含生成时间戳/随机；SVG 坐标由数据 + 固定布局算出，浮点定宽格式化。
- 聚合条形按 `BTreeMap`（已排序）顺序；曲线按 traces 顺序（回测已确定）。
- 同输入 → 同字节 HTML。

## 8. 测试
- `Deserialize` 往返：`Report`/`Trace` 等 serialize → deserialize 相等。
- `derive_series`：合成 traces + 价格 → 已知 `points`（net/cum）、`skipped`、直方图桶。
- `viz`：`line_chart`/`bar_chart`/`histogram`/`render_html` 输出含预期子串（`<svg`、`<polyline`、`<rect`、标题、重叠警告）；同输入同字节。
- e2e：`backtest` 产出 report.json + traces → `report` → HTML 文件存在且含曲线 + 重叠警告 + headline；无 traces 时降级（无曲线、有聚合）。

## 9. 风险
1. **重叠 → 非可交易净值**：累计曲线是信号质量指标，须 HTML 醒目标注（同 spec §0 的重叠警告）。
2. **文件不匹配**：传了别的 report/traces/primary 组合 → trace.t 匹配不上 → skipped 计数 + 标注（不静默）。
3. **大 traces**：polyline 点多 → SVG 大；MVP 全画，后续可降采样。
4. **手写 SVG 受限**：无交互、样式朴素；够用即可，不引入 JS。

## 10. 里程碑
- **T1** 给 `Report`/`Metrics`/`SignalStat`/`GapReport`/`PartialDay`/`Trace`/`StepRecord`/`Stance` 补 `Deserialize` + 往返单测。
- **T2** `report/curve.rs` `derive_series`（重算 net + 累计 + 直方图）+ 单测。
- **T3** `report/viz.rs` SVG 原语 + `render_html` + 单测。
- **T4** cli `report` 子命令 + e2e（backtest→report→HTML）+ README 一节。
