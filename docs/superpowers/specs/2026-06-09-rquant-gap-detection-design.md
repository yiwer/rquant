# rquant：把 AShareCalendar 接进 runner 做缺口检测 — 设计文档

- **日期**：2026-06-09
- **状态**：设计已确认（待 spec 评审 → 进实现计划）
- **关联**：M1–M6 已合并 master（HEAD `fc81d7b`）。`AShareCalendar`（`src/data/calendar.rs`）在 M1 交付但一直未接入 runner（M1–M4 spec §16 / M6 memo 列为 follow-up）。

---

## 1. 背景

`AShareCalendar` 已有 `is_trading_day(NaiveDate)` 与 `in_session(NaiveDateTime)`，但无人调用。本次把它接进回测 runner：加载 primary K 线后做**数据缺口检测**，让用户知道数据有没有整段/局部缺失（新浪浅历史 + 接口波动下，缺口很常见）。检测到则**警告 + 入报告**，回测继续。

## 2. 目标与非目标

### 目标
1. 回测时对 **primary** 序列检测两类缺口：
   - **缺失交易日**：日历认定的交易日，数据里一根 bar 都没有。
   - **残日**：某交易日 bar 数少于"完整一天"。
2. 缺口写入 `Report.gaps` 并警告 stderr；回测不中断。
3. 可选 `--holidays` 文件喂给日历，避免把 A股假期误报成缺失交易日。

### 非目标（YAGNI / 后置）
- 检测 context（大周期）序列（只检 primary；context 后续）。
- 自动补缺 / 插值 / 修复数据。
- 内置/在线节假日表（仅消费用户提供的文件）。
- 按 scale 硬编码"每日应有根数"（改用数据自校准，见 §4）。
- 缺口即中止回测（选择警告而非报错）。

## 3. 锁定决策

| # | 维度 | 选定 |
|---|---|---|
| 1 | 检测范围 | 两类都做：缺失交易日 + 残日 |
| 2 | 检测对象 | 仅 primary 序列 |
| 3 | "完整一天"判定 | **数据自校准**：`full_day = 各交易日 bar 数的最大值` |
| 4 | 残日边界处理 | **排除首日与末日**（新浪常盘中开始/结束，边界合法残缺，免误报）|
| 5 | 节假日 | 可选 `--holidays` 文件（一行一个 `YYYY-MM-DD`，空行/`#` 注释忽略）；不给则仅按周末判 + 提示 |
| 6 | 处理 | 警告 stderr + 写入 `Report.gaps`，回测继续 |

## 4. 架构与算法

### 组件
- `src/data/calendar.rs`（改）：加
  ```rust
  pub fn read_holidays(path: &Path) -> Result<HashSet<NaiveDate>>;
  ```
  逐行读：trim 后为空或以 `#` 开头则跳过；否则按 `%Y-%m-%d` 解析为 `NaiveDate`，坏行 → `Error::Data`。
- `src/backtest/gaps.rs`（新）：纯函数 + 类型
  ```rust
  #[derive(Debug, Clone, Serialize)]
  pub struct PartialDay { pub date: NaiveDate, pub bars: usize, pub expected: usize }

  #[derive(Debug, Clone, Default, Serialize)]
  pub struct GapReport {
      pub missing_trading_days: Vec<NaiveDate>,
      pub partial_days: Vec<PartialDay>,
  }
  impl GapReport { pub fn is_empty(&self) -> bool { self.missing_trading_days.is_empty() && self.partial_days.is_empty() } }

  pub fn detect_gaps(bars: &[Bar], calendar: &AShareCalendar) -> GapReport;
  ```
- `src/report/mod.rs`（改）：`Report` 加 `pub gaps: GapReport`；`print_summary` 加一行。
- `src/backtest/runner.rs`（改）：`BacktestConfig` 加 `pub holidays_path: Option<PathBuf>`；`run` 加载 holidays、建 calendar、`detect_gaps`、警告、写入 Report。
- `src/cli/mod.rs`（改）：backtest 子命令加 `--holidays <Option<PathBuf>>`。

### detect_gaps 算法（数据自校准）
1. `bars` 空 → 返回空 `GapReport`。
2. 按 `bar.time.date()` 分组计数：`counts: BTreeMap<NaiveDate, usize>`（BTreeMap 保证确定序）。
3. `full_day = counts.values().copied().max().unwrap()`（数据里最完整一天的根数）。
4. `first = *counts.keys().next().unwrap()`，`last = *counts.keys().next_back().unwrap()`。
5. **缺失交易日**：从 `first` 到 `last` 逐日（`d + Duration::days(1)`）遍历日历日；若 `calendar.is_trading_day(d)` 且 `!counts.contains_key(&d)` → push 到 `missing_trading_days`。
6. **残日**：遍历 `counts`（已按日期升序）；对 `d != first && d != last && c < full_day` 的项 → push `PartialDay { date: d, bars: c, expected: full_day }`。
7. 返回 `GapReport`。

> 设计取舍：自校准 `full_day` 避开了"按 scale 算每日应有根数"（A股 1h 因午休不整除、需硬编码时间表）的复杂与脆弱；代价是若**所有**交易日都被同等截断，则学不出真正的满日（可接受，罕见）。边界日排除避免新浪盘中起止的常见误报。

### 集成（runner.run 内，读完 primary 后）
```rust
let holidays = match &cfg.holidays_path {
    Some(p) => crate::data::calendar::read_holidays(p)?,
    None => std::collections::HashSet::new(),
};
let calendar = crate::data::calendar::AShareCalendar::new(holidays);
let gaps = crate::backtest::gaps::detect_gaps(&primary, &calendar);
if !gaps.is_empty() {
    eprintln!("[rquant] data gaps on primary: {} missing trading day(s), {} partial day(s) (see report.gaps)",
        gaps.missing_trading_days.len(), gaps.partial_days.len());
    if cfg.holidays_path.is_none() {
        eprintln!("  note: no --holidays provided; A-share holidays may be reported as missing trading days");
    }
}
// ... Report { ..., gaps }
```
`print_summary` 追加：`gaps : {m} missing trading days, {p} partial days`。

## 5. 错误处理
- `read_holidays`：坏日期行 → `Error::Data("bad holiday '<line>': ...")`；IO 错经 `?` → `Error::Io`。
- `detect_gaps`：纯函数，不报错；空输入 → 空报告。
- runner：holidays 加载失败经 `?` 冒泡（回测无法在坏配置下进行）。

## 6. 字段涟漪（编译耦合，须同任务）
- `Report` 加 `gaps` → `report` 单测的 `Report{}` + runner 构造点要补。
- `BacktestConfig` 加 `holidays_path` → cli + e2e（2 处 `BacktestConfig{}`）要补 `holidays_path: None`。
- 故 T3 一次性改 report/runner/cli/e2e/report-test，保持 `cargo test` 全绿。

## 7. 测试
- `read_holidays`（tempfile）：正常解析 + 跳过空行/`#` 注释 + 坏行报错。
- `detect_gaps`（纯函数，构造 Bar 列表 + 自建 AShareCalendar）：
  - 连续交易日、每日满根 → 空报告。
  - 中间缺一个交易日（无该日 bar）→ 记 `missing_trading_days`。
  - 把该缺日设为 holiday（calendar 含之）→ **不**记（验证节假日排除）。
  - 某中间日 bar 数 < full_day → 记 `partial_days{bars, expected}`。
  - 首/末日残缺 → **不**记（边界排除）。
- `report`：序列化含 `gaps`。
- e2e：现有合成数据（Jan 2–6，每日 8 根，full_day=8，无中间缺失）→ 断言 `report.gaps.is_empty()`（顺带验证 `holidays_path: None` 接线）。

## 8. 风险
1. **无节假日表→假期误报**：已通过 `--holidays` + 提示缓解；用户责任。
2. **全段同等截断学不出满日**：自校准的固有限制；罕见，文档说明。
3. **边界日排除可能漏报真实首/末日残缺**：取舍——优先减少新浪盘中起止的误报。
4. **非交易日有数据**（如合成数据含周六）：不报错也不标记（只查"交易日缺数据"与"残日"），符合 spec 范围。

## 9. 里程碑
- **T1** `calendar::read_holidays` + 测试。
- **T2** `backtest/gaps.rs`（`GapReport`/`PartialDay`/`detect_gaps`）+ 测试 + `pub mod gaps`。
- **T3** 集成：`Report.gaps` + `print_summary` + runner（holidays/calendar/detect/warn）+ `BacktestConfig.holidays_path` + cli `--holidays` + e2e/report 测试涟漪。一次切，全绿。
