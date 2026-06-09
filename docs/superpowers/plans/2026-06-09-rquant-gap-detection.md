# rquant 缺口检测（接入 AShareCalendar）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 回测加载 primary K 线后，用 `AShareCalendar` 检测数据缺口（缺失交易日 + 残日），警告 stderr 并写入 `Report.gaps`；回测继续。

**Architecture:** 在 M1–M6（HEAD `fc81d7b`）上扩展。新增 `calendar::read_holidays` 与纯函数 `backtest/gaps::detect_gaps`（数据自校准 full_day、排除首/末日），再在 runner 接线、`Report` 加 `gaps` 字段、cli 加 `--holidays`。零新依赖。

**Tech Stack:** Rust 2024 + 既有（chrono / serde / clap）。

> 设计依据：`docs/superpowers/specs/2026-06-09-rquant-gap-detection-design.md`。
> 提交信息用英文（PowerShell 5.1 中文 git 参数会乱码）。单元测试用同文件 `#[cfg(test)] mod tests`。

---

## 文件结构

```
改动: src/data/calendar.rs    # + read_holidays（+ use Path / crate::{Error,Result}）
新增: src/backtest/gaps.rs    # GapReport / PartialDay / detect_gaps
改动: src/backtest/mod.rs     # + pub mod gaps;
改动: src/report/mod.rs       # Report 加 gaps 字段 + print_summary 一行
改动: src/backtest/runner.rs  # BacktestConfig 加 holidays_path；run 接线缺口检测
改动: src/cli/mod.rs          # backtest 加 --holidays
改动: tests/e2e.rs            # 两处 BacktestConfig 补 holidays_path:None + 断言 gaps 为空
```

---

## Task 1: calendar::read_holidays — 节假日文件加载

**Files:**
- Modify: `src/data/calendar.rs`（加 import + `read_holidays` + 2 个测试）
- Test: 同文件

- [ ] **Step 1: 在 `src/data/calendar.rs` 的 `mod tests` 内追加失败测试**

```rust
    #[test]
    fn read_holidays_parses_and_skips_comments_blanks() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, "# 2024 holidays\n2024-01-01\n\n2024-02-10\n").unwrap();
        f.flush().unwrap();
        let h = read_holidays(f.path()).unwrap();
        assert_eq!(h.len(), 2);
        assert!(h.contains(&NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()));
        assert!(h.contains(&NaiveDate::from_ymd_opt(2024, 2, 10).unwrap()));
    }

    #[test]
    fn read_holidays_rejects_bad_date() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, "2024-13-99\n").unwrap();
        f.flush().unwrap();
        assert!(read_holidays(f.path()).is_err());
    }
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test --lib data::calendar`
Expected: 编译失败（`read_holidays` 未定义）。

- [ ] **Step 3: 写实现**

在 `src/data/calendar.rs` 顶部已有的 `use` 之后补两行 import：
```rust
use crate::{Error, Result};
use std::path::Path;
```
然后在 `impl AShareCalendar { ... }` 之后（`#[cfg(test)]` 之前）加自由函数：
```rust
/// 从文件读节假日：一行一个 YYYY-MM-DD；空行与以 # 开头的行忽略。
pub fn read_holidays(path: &Path) -> Result<HashSet<NaiveDate>> {
    let content = std::fs::read_to_string(path)?;
    let mut set = HashSet::new();
    for line in content.lines() {
        let s = line.trim();
        if s.is_empty() || s.starts_with('#') {
            continue;
        }
        let d = NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .map_err(|e| Error::Data(format!("bad holiday '{s}': {e}")))?;
        set.insert(d);
    }
    Ok(set)
}
```

- [ ] **Step 4: 运行验证通过**

Run: `cargo test --lib data::calendar`
Expected: 既有 2 个（weekend/session）+ 新增 2 个 = 4 个 PASS。

- [ ] **Step 5: Commit**

```bash
git add src/data/calendar.rs
git commit -m "feat(data): read_holidays loader for AShareCalendar" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: backtest/gaps.rs — detect_gaps（纯函数）

**Files:**
- Create: `src/backtest/gaps.rs`
- Modify: `src/backtest/mod.rs`（+ `pub mod gaps;`）
- Test: 同文件

- [ ] **Step 1: 在 `src/backtest/mod.rs` 增加 `pub mod gaps;`**

- [ ] **Step 2: 写失败测试（`src/backtest/gaps.rs`）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::bar::Bar;
    use chrono::NaiveDate;
    use std::collections::HashSet;

    fn day_bars(y: i32, m: u32, d: u32, n: u32) -> Vec<Bar> {
        let base = NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(9, 45, 0).unwrap();
        (0..n)
            .map(|i| Bar {
                time: base + chrono::Duration::minutes(i as i64 * 15),
                open: 1.0, high: 1.0, low: 1.0, close: 1.0, volume: 1.0,
            })
            .collect()
    }
    fn cal(holidays: &[(i32, u32, u32)]) -> AShareCalendar {
        let h: HashSet<NaiveDate> = holidays
            .iter()
            .map(|&(y, m, d)| NaiveDate::from_ymd_opt(y, m, d).unwrap())
            .collect();
        AShareCalendar::new(h)
    }

    #[test]
    fn no_gaps_when_complete() {
        // Jan 2(Tue),3(Wed),4(Thu) 2024, each 4 bars
        let mut bars = day_bars(2024, 1, 2, 4);
        bars.extend(day_bars(2024, 1, 3, 4));
        bars.extend(day_bars(2024, 1, 4, 4));
        assert!(detect_gaps(&bars, &cal(&[])).is_empty());
    }

    #[test]
    fn flags_missing_trading_day() {
        // Jan 2 and Jan 4 present; Jan 3 (Wed, trading) missing
        let mut bars = day_bars(2024, 1, 2, 4);
        bars.extend(day_bars(2024, 1, 4, 4));
        let r = detect_gaps(&bars, &cal(&[]));
        assert_eq!(r.missing_trading_days, vec![NaiveDate::from_ymd_opt(2024, 1, 3).unwrap()]);
    }

    #[test]
    fn holiday_not_flagged_missing() {
        let mut bars = day_bars(2024, 1, 2, 4);
        bars.extend(day_bars(2024, 1, 4, 4));
        let r = detect_gaps(&bars, &cal(&[(2024, 1, 3)]));
        assert!(r.missing_trading_days.is_empty());
    }

    #[test]
    fn flags_partial_interior_day() {
        // Jan 2(4), Jan 3(2 = partial), Jan 4(4). full_day=4. Jan 3 interior → flagged.
        let mut bars = day_bars(2024, 1, 2, 4);
        bars.extend(day_bars(2024, 1, 3, 2));
        bars.extend(day_bars(2024, 1, 4, 4));
        let r = detect_gaps(&bars, &cal(&[]));
        assert_eq!(r.partial_days.len(), 1);
        assert_eq!(r.partial_days[0].date, NaiveDate::from_ymd_opt(2024, 1, 3).unwrap());
        assert_eq!(r.partial_days[0].bars, 2);
        assert_eq!(r.partial_days[0].expected, 4);
    }

    #[test]
    fn boundary_partial_days_not_flagged() {
        // first(Jan 2)=2, Jan 3=4, last(Jan 4)=2. first/last excluded → no partials.
        let mut bars = day_bars(2024, 1, 2, 2);
        bars.extend(day_bars(2024, 1, 3, 4));
        bars.extend(day_bars(2024, 1, 4, 2));
        assert!(detect_gaps(&bars, &cal(&[])).partial_days.is_empty());
    }
}
```

- [ ] **Step 3: 运行验证失败**

Run: `cargo test --lib backtest::gaps`
Expected: 编译失败（`detect_gaps` / `GapReport` 未定义）。

- [ ] **Step 4: 写实现（测试上方）**

```rust
use crate::data::bar::Bar;
use crate::data::calendar::AShareCalendar;
use chrono::{Duration, NaiveDate};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize)]
pub struct PartialDay {
    pub date: NaiveDate,
    pub bars: usize,
    pub expected: usize,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct GapReport {
    pub missing_trading_days: Vec<NaiveDate>,
    pub partial_days: Vec<PartialDay>,
}

impl GapReport {
    pub fn is_empty(&self) -> bool {
        self.missing_trading_days.is_empty() && self.partial_days.is_empty()
    }
}

/// 检测 primary 序列缺口：缺失交易日（日历交易日无 bar）+ 残日
/// （bar 数 < 数据自校准的 full_day，排除首/末日）。纯函数，不报错。
pub fn detect_gaps(bars: &[Bar], calendar: &AShareCalendar) -> GapReport {
    let mut report = GapReport::default();
    if bars.is_empty() {
        return report;
    }
    let mut counts: BTreeMap<NaiveDate, usize> = BTreeMap::new();
    for b in bars {
        *counts.entry(b.time.date()).or_insert(0) += 1;
    }
    let full_day = counts.values().copied().max().unwrap_or(0);
    let first = *counts.keys().next().unwrap();
    let last = *counts.keys().next_back().unwrap();

    // 缺失交易日：[first, last] 内日历交易日但无 bar
    let mut d = first;
    while d <= last {
        if calendar.is_trading_day(d) && !counts.contains_key(&d) {
            report.missing_trading_days.push(d);
        }
        d += Duration::days(1);
    }

    // 残日：bar 数 < full_day，排除首/末日（边界常合法残缺）
    for (&date, &c) in &counts {
        if date != first && date != last && c < full_day {
            report.partial_days.push(PartialDay { date, bars: c, expected: full_day });
        }
    }

    report
}
```

- [ ] **Step 5: 运行验证通过**

Run: `cargo test --lib backtest::gaps`
Expected: 五个测试 PASS。

- [ ] **Step 6: Commit**

```bash
git add src/backtest/gaps.rs src/backtest/mod.rs
git commit -m "feat(backtest): detect_gaps (missing trading days + partial days)" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: 集成（Report.gaps + runner + cli + e2e）

> **编译耦合**：给 `Report` 加 `gaps`、给 `BacktestConfig` 加 `holidays_path`，会牵动 runner/cli/e2e/report-test 的构造点；本任务一次性改完，保持 `cargo test` 全绿。

**Files:**
- Modify: `src/report/mod.rs`（Report 加 `gaps` + print_summary + report 测试）
- Modify: `src/backtest/runner.rs`（BacktestConfig 加 `holidays_path` + run 接线）
- Modify: `src/cli/mod.rs`（backtest 加 `--holidays`）
- Modify: `tests/e2e.rs`（两处 config 补 `holidays_path: None` + 断言 gaps 空）

- [ ] **Step 1: `src/report/mod.rs` — Report 加字段 + import + print_summary + 测试**

在文件顶部 `use` 区加：
```rust
use crate::backtest::gaps::GapReport;
```
`Report` 结构体加字段（在 `metrics` 之后）：
```rust
    pub gaps: GapReport,
```
`print_summary` 末尾（`[warn]` 行之前或之后）加：
```rust
    println!(
        "gaps    : {} missing trading day(s), {} partial day(s)",
        report.gaps.missing_trading_days.len(),
        report.gaps.partial_days.len()
    );
```
把 `report_serializes_to_json` 测试里的 `Report { ... }` 字面量补 `gaps`：
```rust
        let report = Report { tree_name: "t".into(), forward_window: 16, cost_bps: 10.0, metrics, gaps: GapReport::default() };
```
并在该测试末尾追加一条断言：
```rust
        assert!(json.contains("missing_trading_days"));
```

- [ ] **Step 2: `src/backtest/runner.rs` — BacktestConfig 字段 + run 接线**

`BacktestConfig` 加字段（在 `concurrency` 旁）：
```rust
    pub holidays_path: Option<PathBuf>,
```
在 `run` 里、读完 `primary`/`context`/`news` 之后、构建 `results` 之前，插入：
```rust
    let holidays = match &cfg.holidays_path {
        Some(p) => crate::data::calendar::read_holidays(p)?,
        None => std::collections::HashSet::new(),
    };
    let calendar = crate::data::calendar::AShareCalendar::new(holidays);
    let gaps = crate::backtest::gaps::detect_gaps(&primary, &calendar);
    if !gaps.is_empty() {
        eprintln!(
            "[rquant] data gaps on primary: {} missing trading day(s), {} partial day(s) (see report.gaps)",
            gaps.missing_trading_days.len(),
            gaps.partial_days.len()
        );
        if cfg.holidays_path.is_none() {
            eprintln!("  note: no --holidays provided; A-share holidays may be reported as missing trading days");
        }
    }
```
在构造 `Report { ... }` 处加字段 `gaps`：
```rust
    let report = Report {
        tree_name: tree.meta.name.clone(),
        forward_window: fw,
        cost_bps: cfg.cost_bps,
        metrics,
        gaps,
    };
```

- [ ] **Step 3: `src/cli/mod.rs` — backtest 加 --holidays**

在 `Cmd::Backtest { ... }` 变体里（`window` 旁）加：
```rust
        /// Optional A-share holidays file (one YYYY-MM-DD per line) for gap detection
        #[arg(long)]
        holidays: Option<PathBuf>,
```
在 `main` 的 `Cmd::Backtest { ... }` 解构里加上 `holidays`，并在构造 `BacktestConfig { ... }` 处加：
```rust
                holidays_path: holidays,
```

- [ ] **Step 4: `tests/e2e.rs` — 两处 config 补字段 + 断言**

两处 `BacktestConfig { ... }`（`end_to_end_uptrend_yields_positive_long_edge` 与 `run_llm_e2e`）各加一行：
```rust
        holidays_path: None,
```
在 `end_to_end_uptrend_yields_positive_long_edge` 的断言区追加（合成数据每日 8 根、无中间缺失 → 应无缺口）：
```rust
    assert!(report.gaps.is_empty(), "synthetic data should have no gaps");
```
（`report` 是 `run(...).await.unwrap()` 的返回值；`gaps` 是 pub 字段、`is_empty()` 是 pub 方法，无需额外 import。）

- [ ] **Step 5: 全量验证 + 构建**

Run: `cargo test`
Expected: 全部 PASS（含新断言）。

Run: `cargo build`
Expected: 通过。

Run: `cargo clippy --all-targets`
Expected: 无告警（**平铺执行，勿用 `2>&1`**——会触发 PowerShell 退出码 255 假象）。

Run: `cargo run -- backtest --help`
Expected: 用法含 `--holidays`。

- [ ] **Step 6: Commit**

```bash
git add src/report/mod.rs src/backtest/runner.rs src/cli/mod.rs tests/e2e.rs
git commit -m "feat(backtest): wire gap detection into runner/report/cli" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## 附录 A：Spec 覆盖对照（自检）

| Spec 章节 | 实现于 |
|---|---|
| §4 read_holidays | Task 1 |
| §4 detect_gaps + GapReport/PartialDay（自校准/边界排除）| Task 2 |
| §4 Report.gaps + print_summary | Task 3 |
| §4 runner 接线（holidays/calendar/detect/warn）| Task 3 |
| §4 cli `--holidays` | Task 3 |
| §5 错误处理（坏 holiday 行；detect 不报错）| Task 1 / Task 2 |
| §6 字段涟漪（Report/BacktestConfig 构造点）| Task 3 |
| §7 测试（read_holidays / detect_gaps 五例 / report 序列化 / e2e 空缺口）| Task 1/2/3 |
| §9 里程碑 T1–T3 | Task 1/2/3 |

## 附录 B：明确不在范围（YAGNI）
- 检测 context 大周期序列；自动补缺/插值；内置/在线节假日表；按 scale 硬编码每日根数；缺口即中止。
