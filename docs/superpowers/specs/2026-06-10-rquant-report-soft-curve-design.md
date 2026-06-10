# rquant：report 软曲线（`rquant report --soft`）— 设计文档

- **日期**：2026-06-10
- **状态**：设计已确认（写 spec + 计划）
- **关联**：报告可视化（`rquant report` 硬模式，HEAD 谱系 `c2e59a6`）与 soft traces（`bea64d7`，写 `SoftStepRecord` JSONL）已合并。本设计让 `report` 也能渲染软模式产物。

---

## 1. 背景

`rquant report` 现只渲染硬模式 `report.json`(+traces+primary)。软模式现在产出 `soft_report.json`(SoftReport) 与 `soft_traces.jsonl`(SoftStepRecord `{t, leaf_probs, expected_net}`)。本设计加 `--soft` 渲染软产物为自包含 HTML。关键便利：**软 traces 已含 `expected_net`**，故软曲线无需 `--primary` 重算（不像硬模式要用 forward_return）。复用既有 SVG 图元；硬模式渲染不变。

## 2. 目标与非目标

### 目标
1. `rquant report --soft --report soft_report.json --traces soft_traces.jsonl --out soft.html` 生成自包含 HTML。
2. 图：累计期望收益曲线、expected_net 直方图、各叶平均概率条形、headline 表（SoftMetrics）+ 重叠警告。
3. 复用 `EquitySeries`/`Histogram` 与 `line_chart`/`histogram_svg`/`bar_chart`。

### 非目标（YAGNI / 后续）
- 叶子概率随时间堆叠面积图（需新 SVG，留后续）。
- 自动探测 report 类型（用显式 `--soft` 旗）。
- 软模式用 `--primary` 重算（traces 已含 expected_net）。

## 3. 锁定决策

| # | 维度 | 选定 |
|---|---|---|
| 1 | 触发 | `report` 上的 `--soft` 旗（显式）|
| 2 | 图表集 | 曲线 + 直方图 + 各叶平均概率条形 + headline（复用图元）|
| 3 | avg_leaf | 缺省计 0、除以总点数（各叶平均质量，和≈1），按叶名排序 |
| 4 | primary | 软渲染不需 `--primary`（给了则忽略 + stderr 提示）|
| 5 | 反序列化 | `SoftReport`/`SoftMetrics` 补 `Deserialize`（`SignalStat`/`SoftStepRecord` 已有）|

## 4. 架构

### 4.1 反序列化（`report/mod.rs`）
`SoftReport` 与 `SoftMetrics`（`backtest/soft.rs`）当前只 `Serialize`，补 `Deserialize`。`SignalStat`（report-viz 已加）与 `SoftStepRecord`（soft-traces 已加）已具备。

### 4.2 软曲线/聚合（`report/curve.rs`）
```rust
/// 软序列：net = expected_net(Some)、累计 cum、expected_net 直方图；None 计 skipped。复用 EquitySeries。
pub fn derive_soft_series(records: &[SoftStepRecord]) -> EquitySeries

/// 各叶平均质量：对每个叶名，Σ_records leaf_probs.get(leaf).unwrap_or(0) / records.len()。按叶名排序。空→空。
pub fn avg_leaf_probs(records: &[SoftStepRecord]) -> Vec<(String, f64)>
```
- `derive_soft_series`：遍历 records，`expected_net` 为 `Some(x)` → `cum += x`，push `SeriesPoint{t, net:x, cum}`；`None` → `skipped += 1`。直方图复用 `histogram`（私有，对 points 的 net 分桶）——若 `histogram` 私有，把它在 curve.rs 内复用即可（同模块）。
- `avg_leaf_probs`：先收集所有叶名（BTreeSet 保序），对每名求均值（缺省 0），返回排序后的 `Vec<(name, mean)>`。

### 4.3 渲染（`report/viz.rs`）
```rust
pub fn render_soft_html(report: &SoftReport, series: &EquitySeries, avg_leaf: &[(String, f64)]) -> String
```
复用 `line_chart`(cum points)/`histogram_svg`(series.hist)/`bar_chart`(avg_leaf) + headline 表（`report.soft` 的 total_decisions/scored、engaged 的 count/mean_net/hit_rate/t_stat、buy_and_hold）+ 重叠警告 + `series.skipped>0` 提示。HTML 外壳（doctype/style/标题）与 `render_html` 同款（可小幅重复，外壳约十余行）。曲线题标注"窗口重叠 → 信号质量曲线，非可交易净值"。

### 4.4 CLI（`cli/mod.rs`）
`Cmd::Report` 加 `#[arg(long, default_value_t=false)] soft: bool`。match 分流：
```rust
if soft {
    let rep: SoftReport = serde_json::from_str(&read(report)?)?;
    if primary.is_some() { eprintln!("[rquant] --primary ignored in --soft report (expected_net is in traces)"); }
    let series = match &traces {
        Some(tp) => { 逐行 SoftStepRecord → Vec; derive_soft_series(&recs) }
        None => EquitySeries{points:vec![], hist:Histogram{bins:vec![]}, skipped:0},
    };
    let avg = match &traces { Some(_) => avg_leaf_probs(&recs), None => vec![] };
    let html = render_soft_html(&rep, &series, &avg);
    write(out, html)?;
} else { 现有硬渲染 }
```
（无 `--traces` → 软曲线/条形为空，仍出 headline。）

## 5. 错误处理
- `--soft` 但 `--report` 解析为 SoftReport 失败 → 冒泡 `Error`。
- `--traces` 给了但某行解析失败 → 冒泡。
- 无 `--traces` → 无曲线/条形（headline-only），不报错。
- `--primary` 在 `--soft` 下被忽略并提示（不报错）。

## 6. 测试
- `derive_soft_series`：records（含 Some/None）→ 已知 points/cum/skipped + 直方图非空。
- `avg_leaf_probs`：两叶 records → 各叶均值正确、和≈1、按名排序；空 records → 空。
- `render_soft_html`：含 `<svg`/`<polyline`/`<rect`、tree_name、重叠警告；同输入同字节。
- `SoftReport` 往返（serialize → deserialize）。
- e2e：`run_soft`(traces_path=Some) 写产物 → 读回 SoftReport + SoftStepRecord → derive_soft_series + render_soft_html → HTML 含曲线 + 重叠警告。

## 7. 风险
1. **伪概率**：leaf_probs/expected_net 是未校准伪概率，软曲线解读谨慎（沿用软遍历警告）。
2. **外壳重复**：`render_soft_html` 与 `render_html` 共享 HTML 外壳样式，少量重复（可接受；如增长再抽 helper）。
3. **空 traces**：headline-only 输出，须不 panic（测试覆盖）。

## 8. 里程碑
- **T1** `SoftReport`/`SoftMetrics` 加 `Deserialize` + `derive_soft_series` + `avg_leaf_probs` + 测试。
- **T2** `render_soft_html` + 测试。
- **T3** cli `report --soft` 分流 + e2e + README。
