# 数据扩展（深历史）实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为同 10 标的获取尽可能深的日线 qfq 历史 + 质量校验 + 诚实覆盖文档，给后续策略重跑备好经熊市检验的数据底座。

**Architecture:** 复用现有 `fetch`（深度靠 `--datalen` 直通 Tencent，零改 fetch 代码）；新增 `data::quality` 纯函数库做质量分析 + `rquant validate-data` 薄 CLI 壳（硬闸：时间单调 + 粗跳空；缺口信息性上报）；批脚本 `data/fetch_deep.cmd` 拉取到 gitignore 的 `data/`；覆盖报告记录实测深度与 regime。

**Tech Stack:** Rust 2024、chrono、clap、serde；复用 `crate::data::{bar::Bar, reader::read_bars_csv, calendar::AShareCalendar, calendar::read_holidays}` 与 `crate::backtest::gaps::detect_gaps`。设计：`docs/superpowers/specs/2026-06-14-data-expansion-design.md`。

---

## 文件结构

| 文件 | 责任 | 改动 |
|---|---|---|
| `src/data/quality.rs` | 数据质量分析 | 新建：`QualityReport` + `analyze()` 纯函数 + 单测 |
| `src/data/mod.rs` | data 模块表 | 加 `pub mod quality;` |
| `src/cli/mod.rs` | CLI | 加 `Cmd::ValidateData` 薄臂 + `print_quality` |
| `data/fetch_deep.cmd` | 批量深拉脚本 | 新建（提交） |
| `.gitignore` | 忽略大数据 | 加 `data/*.csv` |
| `docs/cli-reference.md` | 文档 | `validate-data` 子命令 |
| `docs/superpowers/2026-06-14-data-expansion-coverage.md` | 覆盖报告 | 新建（实测后回填） |

---

## Task 1: `data::quality` 模块（QualityReport + analyze）

**Files:**
- Create: `src/data/quality.rs`
- Modify: `src/data/mod.rs`（加 `pub mod quality;`）

- [ ] **Step 1: 写失败测试**

新建 `src/data/quality.rs`，先放测试模块（复用 Bar + AShareCalendar）：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use std::collections::HashSet;

    fn day(y: i32, m: u32, d: u32) -> chrono::NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(15, 0, 0).unwrap()
    }
    fn bar(t: chrono::NaiveDateTime, close: f64) -> Bar {
        Bar { time: t, open: close, high: close, low: close, close, volume: 100.0 }
    }
    fn empty_cal() -> AShareCalendar { AShareCalendar::new(HashSet::new()) }

    #[test]
    fn clean_series_all_clear() {
        // 连续四个交易日（2024-01-02 二 ~ 01-05 五），收盘平滑
        let bars = vec![
            bar(day(2024,1,2), 10.0), bar(day(2024,1,3), 10.1),
            bar(day(2024,1,4), 10.2), bar(day(2024,1,5), 10.3),
        ];
        let q = analyze(&bars, &empty_cal(), 0.21);
        assert!(q.strictly_increasing);
        assert!(q.suspicious_jumps.is_empty());
        assert_eq!(q.calendar_gaps, 0);
        assert_eq!(q.n_bars, 4);
        assert_eq!(q.first, day(2024,1,2));
        assert_eq!(q.last, day(2024,1,5));
    }

    #[test]
    fn out_of_order_flagged_non_monotonic() {
        let bars = vec![bar(day(2024,1,3), 10.0), bar(day(2024,1,2), 10.1)];
        let q = analyze(&bars, &empty_cal(), 0.21);
        assert!(!q.strictly_increasing);
    }

    #[test]
    fn gross_jump_flagged() {
        // +30% 跳（超 ±21%）→ 可疑
        let bars = vec![bar(day(2024,1,2), 10.0), bar(day(2024,1,3), 13.0)];
        let q = analyze(&bars, &empty_cal(), 0.21);
        assert_eq!(q.suspicious_jumps.len(), 1);
        assert_eq!(q.suspicious_jumps[0].0, day(2024,1,3));
        assert!((q.max_abs_daily_return - 0.30).abs() < 1e-9);
    }

    #[test]
    fn legit_limit_move_not_flagged() {
        // +10%（主板涨停）< 0.21 → 不报
        let bars = vec![bar(day(2024,1,2), 10.0), bar(day(2024,1,3), 11.0)];
        let q = analyze(&bars, &empty_cal(), 0.21);
        assert!(q.suspicious_jumps.is_empty());
    }

    #[test]
    fn missing_trading_day_counted_as_gap() {
        // 缺 2024-01-03（周三，空日历视其为交易日）→ detect_gaps 计 1
        let bars = vec![bar(day(2024,1,2), 10.0), bar(day(2024,1,4), 10.1)];
        let q = analyze(&bars, &empty_cal(), 0.21);
        assert_eq!(q.calendar_gaps, 1);
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p rquant data::quality`
Expected: 编译失败（`QualityReport`/`analyze` 未定义）。

- [ ] **Step 3: 实现**

`src/data/quality.rs` 顶部（测试模块之前）：

```rust
//! 数据质量分析（设计 2026-06-14-data-expansion-design.md §5）。纯函数，无 IO。
use crate::backtest::gaps::detect_gaps;
use crate::data::bar::Bar;
use crate::data::calendar::AShareCalendar;
use chrono::NaiveDateTime;

/// 一条序列的质量画像。
#[derive(Debug, Clone)]
pub struct QualityReport {
    pub n_bars: usize,
    pub first: NaiveDateTime,
    pub last: NaiveDateTime,
    /// 时间严格递增（无重复、无逆序）。
    pub strictly_increasing: bool,
    /// 最大 |相邻收盘收益|。
    pub max_abs_daily_return: f64,
    /// |收益| > 阈值的可疑跳空（时刻, 收益）。
    pub suspicious_jumps: Vec<(NaiveDateTime, f64)>,
    /// 对日历的意外缺交易日数（detect_gaps；无 --holidays 时含市场假日，信息性）。
    pub calendar_gaps: usize,
}

/// 分析一段（已按时间排序的）bar 序列。空序列返回零值画像。
pub fn analyze(bars: &[Bar], calendar: &AShareCalendar, jump_threshold: f64) -> QualityReport {
    if bars.is_empty() {
        let zero = NaiveDateTime::default();
        return QualityReport {
            n_bars: 0, first: zero, last: zero, strictly_increasing: true,
            max_abs_daily_return: 0.0, suspicious_jumps: Vec::new(), calendar_gaps: 0,
        };
    }
    let mut strictly_increasing = true;
    let mut max_abs = 0.0_f64;
    let mut jumps = Vec::new();
    for w in bars.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        if b.time <= a.time {
            strictly_increasing = false;
        }
        if a.close != 0.0 {
            let ret = b.close / a.close - 1.0;
            if ret.abs() > max_abs {
                max_abs = ret.abs();
            }
            if ret.abs() > jump_threshold {
                jumps.push((b.time, ret));
            }
        }
    }
    let gaps = detect_gaps(bars, calendar);
    QualityReport {
        n_bars: bars.len(),
        first: bars[0].time,
        last: bars[bars.len() - 1].time,
        strictly_increasing,
        max_abs_daily_return: max_abs,
        suspicious_jumps: jumps,
        calendar_gaps: gaps.missing_trading_days.len(),
    }
}
```

`src/data/mod.rs` 加（紧邻其它 `pub mod`）：

```rust
pub mod quality;
```

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p rquant data::quality`
Expected: 5 测试 PASS。

- [ ] **Step 5: 提交**

```bash
git add src/data/quality.rs src/data/mod.rs
git commit -m "feat(data): quality::analyze (monotonic/jump/gap/coverage)"
```

---

## Task 2: `rquant validate-data` CLI 子命令

**Files:**
- Modify: `src/cli/mod.rs`（`Cmd` 枚举加 `ValidateData` + 处理臂 + `print_quality`）
- Test: `tests/validate_data_cli.rs`

- [ ] **Step 1: 写失败 e2e 测试**

新建 `tests/validate_data_cli.rs`：

```rust
use std::process::Command;

fn bin() -> &'static str { env!("CARGO_BIN_EXE_rquant") }

#[test]
fn validate_data_flags_gross_jump_with_nonzero_exit() {
    let dir = tempfile::tempdir().unwrap();
    let csv = dir.path().join("bad.csv");
    // 表头 + 两行，+30% 跳（超 0.21）
    std::fs::write(&csv,
        "time,open,high,low,close,volume\n\
         2024-01-02 15:00:00,10,10,10,10,100\n\
         2024-01-03 15:00:00,13,13,13,13,100\n").unwrap();
    let status = Command::new(bin())
        .args(["validate-data", "--csv", csv.to_str().unwrap()])
        .status().unwrap();
    assert_eq!(status.code(), Some(1), "可疑跳空 → 退出码 1");
}

#[test]
fn validate_data_clean_series_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    let csv = dir.path().join("ok.csv");
    std::fs::write(&csv,
        "time,open,high,low,close,volume\n\
         2024-01-02 15:00:00,10,10,10,10.0,100\n\
         2024-01-03 15:00:00,10.1,10.1,10.1,10.1,100\n\
         2024-01-04 15:00:00,10.2,10.2,10.2,10.2,100\n").unwrap();
    let status = Command::new(bin())
        .args(["validate-data", "--csv", csv.to_str().unwrap()])
        .status().unwrap();
    assert_eq!(status.code(), Some(0), "干净序列 → 退出码 0");
}
```

> 注：CSV 表头格式以 `read_bars_csv` 实际接受的为准——实现前先看 `src/data/reader.rs` 的列序与表头处理，必要时调整测试 CSV 文本以匹配（保持两测试的语义：一跳空、一干净）。

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p rquant --test validate_data_cli`
Expected: 失败（`validate-data` 子命令不存在）。

- [ ] **Step 3: 加 Cmd::ValidateData 变体**

`src/cli/mod.rs` 的 `Cmd` 枚举末尾（`Eval { ... }` 之后）加：

```rust
    /// Validate fetched CSV data quality (monotonic time, gross jumps, gaps, coverage).
    ValidateData {
        /// Repeatable: one CSV per call.
        #[arg(long = "csv", value_name = "PATH", required = true)]
        csv: Vec<PathBuf>,
        /// Optional holidays file (YYYY-MM-DD per line) for accurate gap counting.
        #[arg(long)]
        holidays: Option<PathBuf>,
        /// Suspicious-jump threshold on |daily return| (default 0.21 = beyond ChiNext ±20%).
        #[arg(long, default_value_t = 0.21)]
        jump: f64,
    },
```

- [ ] **Step 4: 加处理臂 + print_quality**

`match` 末尾（`Cmd::Eval` 臂之后）加：

```rust
        Cmd::ValidateData { csv, holidays, jump } => {
            if csv.is_empty() {
                return Err(anyhow::anyhow!("--csv: at least one CSV path is required"));
            }
            let calendar = match holidays {
                Some(hp) => crate::data::calendar::AShareCalendar::new(
                    crate::data::calendar::read_holidays(&hp)?,
                ),
                None => crate::data::calendar::AShareCalendar::new(std::collections::HashSet::new()),
            };
            let mut any_fail = false;
            for path in &csv {
                let bars = crate::data::reader::read_bars_csv(path)
                    .map_err(|e| anyhow::anyhow!("read {}: {e}", path.display()))?;
                let q = crate::data::quality::analyze(&bars, &calendar, jump);
                print_quality(path, &q, holidays.is_none());
                if !q.strictly_increasing || !q.suspicious_jumps.is_empty() {
                    any_fail = true;
                }
            }
            if any_fail {
                std::process::exit(1);
            }
        }
```

加打印辅助（`print_optimize_summary`/`print_verdict` 附近）：

```rust
fn print_quality(path: &std::path::Path, q: &rquant::data::quality::QualityReport, no_holidays: bool) {
    println!("=== {} ===", path.display());
    println!("  bars       : {}", q.n_bars);
    println!("  coverage   : {} .. {}", q.first, q.last);
    println!("  monotonic  : {}", q.strictly_increasing);
    println!("  max |ret|  : {:.4}", q.max_abs_daily_return);
    println!("  jumps>thr  : {}", q.suspicious_jumps.len());
    for (t, r) in &q.suspicious_jumps {
        println!("    - {t}  ret={r:+.4}");
    }
    let gap_note = if no_holidays { " (incl. market holidays; pass --holidays for accuracy)" } else { "" };
    println!("  gaps       : {}{}", q.calendar_gaps, gap_note);
}
```

> 路径前缀：按 `src/cli/mod.rs` 既有惯例（`crate::data::...` 或 `rquant::data::...`）。`print_quality` 的参数类型同样以编译通过的前缀为准（参考 `print_verdict` 用的是哪种）。

- [ ] **Step 5: 运行确认通过**

Run: `cargo test -p rquant --test validate_data_cli`
Expected: 两测试 PASS（跳空退 1、干净退 0）。

- [ ] **Step 6: 提交**

```bash
git add src/cli/mod.rs tests/validate_data_cli.rs
git commit -m "feat(cli): rquant validate-data subcommand"
```

---

## Task 3: 批拉脚本 + .gitignore

**Files:**
- Create: `data/fetch_deep.cmd`
- Modify: `.gitignore`

- [ ] **Step 1: 加 .gitignore 条目**

在 `.gitignore` 末尾追加（仅忽略 CSV，脚本仍提交）：

```
# deep-history research data (reproducible via data/fetch_deep.cmd)
data/*.csv
```

- [ ] **Step 2: 写批拉脚本**

新建 `data/fetch_deep.cmd`（ASCII 安全，仿 `deploy/paper_run.cmd`；`<D>` 由 Task 4 探测后回填脚本头注释，命令里用占位的 3000 起，Task 4 据实测调整）：

```bat
@echo off
REM Deep-history daily qfq fetch for the 10-symbol research universe.
REM Fetch date: <FILL by Task 4>   Probed Tencent max depth D: <FILL by Task 4>
REM Output: data\<symbol>.csv (gitignored). Re-run overwrites (idempotent).
setlocal
set RQ=target\release\rquant.exe
set DATALEN=3000
for %%S in (sh600030 sh600036 sh600276 sh600519 sh600900 sh601088 sh601318 sz000333 sz000858 sz300750) do (
  echo [fetch] %%S
  %RQ% fetch --symbol %%S --scale 240 --datalen %DATALEN% --adjust qfq --out data\%%S.csv
)
echo [done] deep fetch complete
endlocal
```

- [ ] **Step 3: 提交脚本与忽略规则**

```bash
git add .gitignore data/fetch_deep.cmd
git commit -m "chore(data): deep-fetch batch script + gitignore data CSVs"
```

> 本任务无自动化测试（脚本 + 忽略规则）。验证：`git status --porcelain data/` 在 Task 4 拉数据后应只显示 `fetch_deep.cmd` 已跟踪、`*.csv` 被忽略。

---

## Task 4: 深度探测 + 批量拉取 + 校验（执行任务，需网络）

**Files:**
- 产出（gitignore）：`data/*.csv`、`tmps/probe_*.csv`
- Modify: `data/fetch_deep.cmd`（回填实测 D 与抓取日期注释）

> 本任务是**联网执行**，非 TDD。需可访问 Tencent/Sina（`web.ifzq.gtimg.cn`、`quotes.sina.cn`）。先 `cargo build --release` 确保 `target/release/rquant.exe` 最新（含 Task 1-2 的 validate-data）。

- [ ] **Step 1: 深度探测**

对老股探 Tencent 实际上限：

```
target/release/rquant.exe fetch --symbol sh600519 --scale 240 --datalen 5000 --adjust qfq --out tmps/probe_sh600519.csv
```

记录实际返回 bar 数与最早日期（看 stdout 报告或 `validate-data` 覆盖行）。若返回 < 5000，说明 Tencent 封顶在该值——这就是 D。若 ≈5000，再试 `--datalen 8000` 确认是否更深。**把探测出的 D 写进 `data/fetch_deep.cmd` 的 `set DATALEN=` 与头部注释（连同抓取日期）。**

- [ ] **Step 2: 批量拉取**

```
data\fetch_deep.cmd
```

10 个 `data\<symbol>.csv` 落地。逐行确认无 fetch 报错（网络/截断会报错并重试）。

- [ ] **Step 3: 校验全量**

```
target/release/rquant.exe validate-data --csv data/sh600030.csv --csv data/sh600036.csv --csv data/sh600276.csv --csv data/sh600519.csv --csv data/sh600900.csv --csv data/sh601088.csv --csv data/sh601318.csv --csv data/sz000333.csv --csv data/sz000858.csv --csv data/sz300750.csv
```

记录每标的输出（bars/coverage/monotonic/max|ret|/jumps/gaps）。**退出码须为 0**（任一可疑跳空或非单调 → 退 1 → 必须排查：是 Tencent 数据问题还是真实极端行情；记入报告）。

- [ ] **Step 4: 提交脚本回填**

```bash
git add data/fetch_deep.cmd
git commit -m "chore(data): backfill probed depth D and fetch date in fetch_deep.cmd"
```

> 数据 CSV 不提交（gitignore）。本任务交付物 = 落地的 data/*.csv（本地）+ 回填的脚本 + Step 1/3 的实测数字（供 Task 5 写报告）。

---

## Task 5: 覆盖报告 + 文档 + 收尾闸

**Files:**
- Create: `docs/superpowers/2026-06-14-data-expansion-coverage.md`
- Modify: `docs/cli-reference.md`

- [ ] **Step 1: 写覆盖报告**

`docs/superpowers/2026-06-14-data-expansion-coverage.md`，用 Task 4 实测数据填写：

```markdown
# 数据扩展覆盖报告

- 抓取日期：<Task 4 实际日期>
- 数据源：Tencent fqkline（日线 qfq），探测最大深度 D = <实测>
- 标的：同 10（与纯量化弧线可比）

## 每标的覆盖（validate-data 输出）

| 标的 | bars | 起 | 止 | max|日收益| | 可疑跳空 | 缺口* |
|---|---|---|---|---|---|---|
| sh600030 | … | … | … | … | … | … |
| …（10 行） | | | | | | |

\* 缺口含市场假日（未传 --holidays），信息性。

## Regime 覆盖判读

- 最早起始日 <date> → 含 2018 全年熊：<是/否>；含 2020 COVID 暴跌：<是/否>；含 2022 回调：<是/否>。
- 结论：<本次深度对纯牛市样本的改善程度，是否拿到系统性熊市>。

## 诚实边界

- 幸存者偏差：同 10 幸存大盘股，无历史时点成分，偏差不可消除只能声明。
- qfq 锚定抓取日（<日期>）；旧 paper/pd_*.csv 与本 data/ 跨日不可混用。
- 仅日线——60m 执行树（Sina 分钟 ~1 年硬限）不受益。
- data/*.csv 已 gitignore，经 data/fetch_deep.cmd 可复现。
- 若某标的有可疑跳空/缺口，逐条说明排查结论（数据问题 vs 真实行情/停牌）。
```

- [ ] **Step 2: 文档 validate-data**

`docs/cli-reference.md` 加 `validate-data` 一节（flags `--csv`(可重复必填)/`--holidays`/`--jump`；硬闸=单调+跳空、缺口信息性；退出码 0/1；与 fetch/深历史工作流的关系）。对照 Task 2 实际旗标写。

- [ ] **Step 3: 提交文档**

```bash
git add docs/superpowers/2026-06-14-data-expansion-coverage.md docs/cli-reference.md
git commit -m "docs(data): deep-history coverage report + validate-data reference"
```

- [ ] **Step 4: 全量收尾闸**

Run: `cargo test`
Expected: 全绿、0 失败（既有 + data::quality 5 测试 + validate_data_cli 2 e2e）。

Run: `cargo clippy --all-targets`
Expected: 零警告。

- [ ] **Step 5: 行为冻结复验**

Run: `cargo test -p rquant data backtest`
Expected: 既有 data/backtest 测试（含 detect_gaps）全绿——quality 复用 detect_gaps，未改其行为。

- [ ] **Step 6: 最终提交（若有遗留）**

```bash
git add -p   # 仅本计划相关、点名
git commit -m "chore(data): finalize deep-history expansion"
```

---

## Self-Review（写计划后自查）

**Spec 覆盖**：data::quality 纯函数（§5.1）→ Task 1；validate-data CLI + 硬闸退出码（§5.2）→ Task 2；qfq 信任 Tencent 直接复权（§5.3）→ 文档于 Task 5 + analyze 不做逐除息核对（设计如此）；单测四例（§5.4）→ Task 1 五测试（含 coverage）；深度探测 + 批拉 + data/ 隔离 + qfq 锚定（§4）→ Task 3/4；gitignore（§3）→ Task 3；覆盖报告 + regime 标注 + 诚实边界（§6）→ Task 5；改动文件表（§7）全覆盖。✅

**占位符扫描**：Task 4/5 的 `<D>`/`<date>`/表格值是**执行期实测回填**（联网拉取的真实产物），非可避免的占位——计划已明确"运行此命令、把输出填入此结构"。代码步骤均含完整代码。✅

**类型一致性**：`QualityReport{n_bars,first,last,strictly_increasing,max_abs_daily_return,suspicious_jumps,calendar_gaps}` 跨 Task 1/2 一致；`analyze(bars,calendar,jump_threshold)` 签名跨 Task 1/2 一致；复用的 `detect_gaps(bars,calendar)`/`read_bars_csv(path)`/`AShareCalendar::new(holidays)`/`read_holidays(path)` 均为既有真实 API（已核 src/data 与 src/backtest/gaps.rs）。✅
