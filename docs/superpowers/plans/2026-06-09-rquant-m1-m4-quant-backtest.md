# rquant M1–M4（纯量化端到端回测）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 rquant 的纯量化端到端回测：加载 YAML 决策树 → 在本地 A股 K 线上逐时点遍历 → 叶子映射立场 → 前瞻收益评分 → 出度量与可审计 Trace。

**Architecture:** 七层、引擎不含金融逻辑。数据(CSV读取+日历) → 特征(指标+Context) → DSL(词法/语法/求值) → 树(schema/加载校验) → 评估器(QuantEvaluator) → 引擎(遍历/Trace) → 回测(前瞻收益/成本/度量) → 报告/CLI。LLM 节点在本阶段走 `default` 分支。

**Tech Stack:** Rust (edition 2024)、chrono(时间)、serde+serde_yaml+serde_json、csv、clap、thiserror/anyhow；dev: approx、tempfile。手写词法+Pratt 解析器（无解析依赖）。

> 设计依据：`docs/superpowers/specs/2026-06-09-rquant-decision-tree-backtest-design.md`
> **提交信息用英文**（Windows PowerShell 5.1 给原生 git 传中文参数易乱码；要写中文用 `git commit -F <utf8文件>`）。单元测试用 Rust 习惯的同文件 `#[cfg(test)] mod tests`。

---

## 文件结构

```
src/
  lib.rs           # crate 根：pub mod + Error/Result
  main.rs          # 瘦 CLI 入口
  data/mod.rs  bar.rs  calendar.rs  reader.rs
  features/mod.rs  indicators.rs  context.rs
  dsl/mod.rs  lexer.rs  ast.rs  parser.rs  eval.rs
  tree/mod.rs  schema.rs  loader.rs
  eval/mod.rs  quant.rs
  engine/mod.rs  trace.rs  traversal.rs
  backtest/mod.rs  costs.rs  forward_return.rs  metrics.rs  runner.rs
  report/mod.rs
  cli/mod.rs
examples/trend_tree.yaml
tests/e2e.rs
```

---

## Task 1: 项目骨架（Cargo + Error/Result + 模块声明）

**Files:**
- Modify: `Cargo.toml`
- Create: `src/lib.rs`
- Modify: `src/main.rs`
- Create (空 mod 占位): `src/data/mod.rs`, `src/features/mod.rs`, `src/dsl/mod.rs`, `src/tree/mod.rs`, `src/eval/mod.rs`, `src/engine/mod.rs`, `src/backtest/mod.rs`, `src/report/mod.rs`, `src/cli/mod.rs`

- [ ] **Step 1: 写 Cargo.toml 依赖**

```toml
[package]
name = "rquant"
version = "0.1.0"
edition = "2024"

[dependencies]
chrono = { version = "0.4", features = ["serde"] }
serde = { version = "1", features = ["derive"] }
serde_yaml = "0.9"
serde_json = "1"
csv = "1"
clap = { version = "4", features = ["derive"] }
thiserror = "2"
anyhow = "1"

[dev-dependencies]
approx = "0.5"
tempfile = "3"
```

- [ ] **Step 2: 写 src/lib.rs（统一 Error/Result + 模块声明）**

```rust
pub mod data;
pub mod features;
pub mod dsl;
pub mod tree;
pub mod eval;
pub mod engine;
pub mod backtest;
pub mod report;
pub mod cli;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("data error: {0}")]
    Data(String),
    #[error("dsl error: {0}")]
    Dsl(String),
    #[error("tree error: {0}")]
    Tree(String),
    #[error("eval error: {0}")]
    Eval(String),
    #[error("engine error: {0}")]
    Engine(String),
    #[error("backtest error: {0}")]
    Backtest(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("csv error: {0}")]
    Csv(#[from] csv::Error),
    #[error("yaml error: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}
```

- [ ] **Step 3: 写各模块占位 mod.rs**

`src/data/mod.rs`:
```rust
pub mod bar;
pub mod calendar;
pub mod reader;
```
`src/features/mod.rs`:
```rust
pub mod indicators;
pub mod context;
```
`src/dsl/mod.rs`:
```rust
pub mod lexer;
pub mod ast;
pub mod parser;
pub mod eval;
```
`src/tree/mod.rs`:
```rust
pub mod schema;
pub mod loader;
```
`src/eval/mod.rs`:
```rust
pub mod quant;

#[derive(Debug, Clone)]
pub struct Decision {
    pub goto: String,
    pub label: String,
    pub confidence: f64,
    pub rationale: String,
}
```
`src/engine/mod.rs`:
```rust
pub mod trace;
pub mod traversal;
```
`src/backtest/mod.rs`:
```rust
pub mod costs;
pub mod forward_return;
pub mod metrics;
pub mod runner;
```
`src/report/mod.rs`:
```rust
// 实现见 Task 17
```
`src/cli/mod.rs`:
```rust
// 实现见 Task 18
```

- [ ] **Step 3b: 把所有叶子源文件创建为空文件**

为保证**每一步都能独立编译**（mod.rs 已声明这些子模块），先把后续 Task 要填充的 .rs 全部创建为空文件（空文件 = 合法的空模块）：

`src/data/{bar,calendar,reader}.rs`、`src/features/{indicators,context}.rs`、`src/dsl/{lexer,ast,parser,eval}.rs`、`src/tree/{schema,loader}.rs`、`src/eval/quant.rs`、`src/engine/{trace,traversal}.rs`、`src/backtest/{costs,forward_return,metrics,runner}.rs`。

> 后续 Task 中标注 “Create” 的 .rs 均已在此创建为空——执行时**填充其内容**即可（先 Read 一眼空文件再 Write，或直接 Edit）。`report/mod.rs` 留空、`cli/mod.rs` 为临时桩，均已在 Step 3 写好；它们的 mod.rs 声明也已在 lib.rs 就位，因此 `cargo build` 此刻即可通过。

- [ ] **Step 4: 写最小 src/main.rs**

```rust
fn main() -> anyhow::Result<()> {
    rquant::cli::main()
}
```

> `cli::main` 的临时桩已在 Step 3 写好，故此处可直接编译通过；Task 18 会用真正的 clap CLI 替换它。

- [ ] **Step 5: 编译验证**

Run: `cargo build`
Expected: 编译通过（report 模块为空、cli 为桩）。可能有 “unused” 警告，忽略。

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/
git commit -m "chore: project skeleton with crate-wide Error and module layout"
```

---

## Task 2: data/bar.rs — Bar 与 Window

**Files:**
- Create: `src/data/bar.rs`
- Test: 同文件 `#[cfg(test)] mod tests`

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn dt(h: u32, m: u32) -> chrono::NaiveDateTime {
        NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn window_accessors_extract_fields() {
        let bars = vec![
            Bar { time: dt(9, 45), open: 1.0, high: 2.0, low: 0.5, close: 1.5, volume: 100.0 },
            Bar { time: dt(10, 0), open: 1.5, high: 2.5, low: 1.0, close: 2.0, volume: 200.0 },
        ];
        let w = Window { bars };
        assert_eq!(w.closes(), vec![1.5, 2.0]);
        assert_eq!(w.opens(), vec![1.0, 1.5]);
        assert_eq!(w.highs(), vec![2.0, 2.5]);
        assert_eq!(w.lows(), vec![0.5, 1.0]);
        assert_eq!(w.volumes(), vec![100.0, 200.0]);
    }
}
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test window_accessors_extract_fields`
Expected: 编译失败（`Bar` / `Window` 未定义）。

- [ ] **Step 3: 写实现**

```rust
use chrono::NaiveDateTime;

/// 一根 K 线。`time` = bar 的收盘时刻（交易所本地 = Asia/Shanghai 墙钟，naive）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bar {
    pub time: NaiveDateTime,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

/// 一个时间窗口（按时间升序，最后一根 = 决策时刻最近的已收盘 bar）。
#[derive(Debug, Clone)]
pub struct Window {
    pub bars: Vec<Bar>,
}

impl Window {
    pub fn closes(&self) -> Vec<f64> { self.bars.iter().map(|b| b.close).collect() }
    pub fn opens(&self) -> Vec<f64> { self.bars.iter().map(|b| b.open).collect() }
    pub fn highs(&self) -> Vec<f64> { self.bars.iter().map(|b| b.high).collect() }
    pub fn lows(&self) -> Vec<f64> { self.bars.iter().map(|b| b.low).collect() }
    pub fn volumes(&self) -> Vec<f64> { self.bars.iter().map(|b| b.volume).collect() }
}
```

- [ ] **Step 4: 运行验证通过**

Run: `cargo test window_accessors_extract_fields`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add src/data/bar.rs
git commit -m "feat(data): Bar and Window types with field accessors"
```

---

## Task 3: data/calendar.rs — A股交易日历

**Files:**
- Create: `src/data/calendar.rs`
- Modify: `src/data/mod.rs`（已含 `pub mod calendar;`，无需改）
- Test: 同文件

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use std::collections::HashSet;

    fn cal() -> AShareCalendar {
        let mut h = HashSet::new();
        h.insert(NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()); // 元旦
        AShareCalendar::new(h)
    }

    #[test]
    fn weekend_and_holiday_are_not_trading_days() {
        let c = cal();
        assert!(!c.is_trading_day(NaiveDate::from_ymd_opt(2024, 1, 6).unwrap())); // 周六
        assert!(!c.is_trading_day(NaiveDate::from_ymd_opt(2024, 1, 1).unwrap())); // 节假日
        assert!(c.is_trading_day(NaiveDate::from_ymd_opt(2024, 1, 2).unwrap()));  // 周二
    }

    #[test]
    fn session_boundaries() {
        let c = cal();
        let d = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
        assert!(c.in_session(d.and_hms_opt(9, 45, 0).unwrap()));
        assert!(c.in_session(d.and_hms_opt(11, 30, 0).unwrap()));
        assert!(!c.in_session(d.and_hms_opt(12, 0, 0).unwrap()));
        assert!(c.in_session(d.and_hms_opt(13, 15, 0).unwrap()));
        assert!(c.in_session(d.and_hms_opt(15, 0, 0).unwrap()));
        assert!(!c.in_session(d.and_hms_opt(15, 15, 0).unwrap()));
    }
}
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test --lib calendar`
Expected: 编译失败（`AShareCalendar` 未定义）。

- [ ] **Step 3: 写实现**

```rust
use chrono::{Datelike, NaiveDate, NaiveDateTime, NaiveTime, Weekday};
use std::collections::HashSet;

/// A股交易日历：工作日且非节假日为交易日；时段 09:30–11:30、13:00–15:00。
/// bar 收盘时刻落在 (start, end] 内视为在交易时段（首根 15m bar 收于 09:45，末根收于 15:00）。
pub struct AShareCalendar {
    holidays: HashSet<NaiveDate>,
}

impl AShareCalendar {
    pub fn new(holidays: HashSet<NaiveDate>) -> Self {
        Self { holidays }
    }

    pub fn is_trading_day(&self, d: NaiveDate) -> bool {
        !matches!(d.weekday(), Weekday::Sat | Weekday::Sun) && !self.holidays.contains(&d)
    }

    pub fn in_session(&self, dt: NaiveDateTime) -> bool {
        if !self.is_trading_day(dt.date()) {
            return false;
        }
        let t = dt.time();
        let am_start = NaiveTime::from_hms_opt(9, 30, 0).unwrap();
        let am_end = NaiveTime::from_hms_opt(11, 30, 0).unwrap();
        let pm_start = NaiveTime::from_hms_opt(13, 0, 0).unwrap();
        let pm_end = NaiveTime::from_hms_opt(15, 0, 0).unwrap();
        (t > am_start && t <= am_end) || (t > pm_start && t <= pm_end)
    }
}
```

- [ ] **Step 4: 运行验证通过**

Run: `cargo test --lib calendar`
Expected: 两个测试 PASS。

- [ ] **Step 5: Commit**

```bash
git add src/data/calendar.rs
git commit -m "feat(data): A-share trading calendar (sessions + holidays)"
```

---

## Task 4: data/reader.rs — CSV 读取与校验

**Files:**
- Create: `src/data/reader.rs`
- Test: 同文件（用 tempfile 写临时 CSV）

CSV 格式：表头 `time,open,high,low,close,volume`；time 形如 `2024-01-02 09:45:00`。

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_csv(content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        write!(f, "{}", content).unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn reads_valid_csv() {
        let f = write_csv(
            "time,open,high,low,close,volume\n\
             2024-01-02 09:45:00,10.0,10.5,9.8,10.2,1000\n\
             2024-01-02 10:00:00,10.2,10.6,10.1,10.4,1200\n",
        );
        let bars = read_bars_csv(f.path()).unwrap();
        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0].close, 10.2);
        assert_eq!(bars[1].volume, 1200.0);
    }

    #[test]
    fn rejects_non_increasing_time() {
        let f = write_csv(
            "time,open,high,low,close,volume\n\
             2024-01-02 10:00:00,10.0,10.5,9.8,10.2,1000\n\
             2024-01-02 09:45:00,10.2,10.6,10.1,10.4,1200\n",
        );
        assert!(read_bars_csv(f.path()).is_err());
    }

    #[test]
    fn rejects_high_below_low() {
        let f = write_csv(
            "time,open,high,low,close,volume\n\
             2024-01-02 09:45:00,10.0,9.0,9.8,10.2,1000\n",
        );
        assert!(read_bars_csv(f.path()).is_err());
    }
}
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test --lib reader`
Expected: 编译失败（`read_bars_csv` 未定义）。

- [ ] **Step 3: 写实现**

```rust
use crate::data::bar::Bar;
use crate::{Error, Result};
use chrono::NaiveDateTime;
use std::path::Path;

#[derive(serde::Deserialize)]
struct Row {
    time: String,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
}

/// 读取本地 CSV 为按时间升序的 Bar 列表，并做基本校验：
/// 时间严格递增、high >= low。
pub fn read_bars_csv(path: &Path) -> Result<Vec<Bar>> {
    let mut rdr = csv::Reader::from_path(path)?;
    let mut bars: Vec<Bar> = Vec::new();
    for rec in rdr.deserialize() {
        let row: Row = rec?;
        let time = NaiveDateTime::parse_from_str(&row.time, "%Y-%m-%d %H:%M:%S")
            .map_err(|e| Error::Data(format!("bad time '{}': {e}", row.time)))?;
        let bar = Bar {
            time,
            open: row.open,
            high: row.high,
            low: row.low,
            close: row.close,
            volume: row.volume,
        };
        if bar.high < bar.low {
            return Err(Error::Data(format!("high < low at {time}")));
        }
        if let Some(prev) = bars.last() {
            if time <= prev.time {
                return Err(Error::Data(format!("non-increasing time at {time}")));
            }
        }
        bars.push(bar);
    }
    Ok(bars)
}
```

- [ ] **Step 4: 运行验证通过**

Run: `cargo test --lib reader`
Expected: 三个测试 PASS。

- [ ] **Step 5: Commit**

```bash
git add src/data/reader.rs
git commit -m "feat(data): CSV bar reader with monotonic-time and OHLC validation"
```

---

## Task 5: features/indicators.rs — sma / ema / rsi

**Files:**
- Create: `src/features/indicators.rs`
- Test: 同文件

约定：所有“序列型”指标返回与输入等长的 `Vec<f64>`，预热不足的前缀填 `f64::NAN`。

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn sma_basic() {
        let out = sma(&[1.0, 2.0, 3.0, 4.0, 5.0], 3);
        assert!(out[0].is_nan());
        assert!(out[1].is_nan());
        assert_relative_eq!(out[2], 2.0);
        assert_relative_eq!(out[3], 3.0);
        assert_relative_eq!(out[4], 4.0);
    }

    #[test]
    fn ema_constant_series_is_constant() {
        let out = ema(&[5.0, 5.0, 5.0, 5.0], 3);
        assert_relative_eq!(out[0], 5.0);
        assert_relative_eq!(out[3], 5.0);
    }

    #[test]
    fn rsi_increasing_is_100_decreasing_is_0() {
        let up: Vec<f64> = (0..30).map(|i| i as f64).collect();
        let down: Vec<f64> = (0..30).map(|i| (30 - i) as f64).collect();
        assert_relative_eq!(*rsi(&up, 14).last().unwrap(), 100.0);
        assert_relative_eq!(*rsi(&down, 14).last().unwrap(), 0.0);
    }
}
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test --lib indicators`
Expected: 编译失败（`sma`/`ema`/`rsi` 未定义）。

- [ ] **Step 3: 写实现**

```rust
/// 简单移动平均；out[i] = mean(s[i-n+1..=i])，i < n-1 处为 NaN。
pub fn sma(s: &[f64], n: usize) -> Vec<f64> {
    let mut out = vec![f64::NAN; s.len()];
    if n == 0 {
        return out;
    }
    let mut sum = 0.0;
    for i in 0..s.len() {
        sum += s[i];
        if i >= n {
            sum -= s[i - n];
        }
        if i + 1 >= n {
            out[i] = sum / n as f64;
        }
    }
    out
}

/// 指数移动平均；out[0] = s[0]，alpha = 2/(n+1)。
pub fn ema(s: &[f64], n: usize) -> Vec<f64> {
    let mut out = vec![f64::NAN; s.len()];
    if s.is_empty() || n == 0 {
        return out;
    }
    let alpha = 2.0 / (n as f64 + 1.0);
    out[0] = s[0];
    for i in 1..s.len() {
        out[i] = alpha * s[i] + (1.0 - alpha) * out[i - 1];
    }
    out
}

fn rsi_from(avg_gain: f64, avg_loss: f64) -> f64 {
    if avg_loss == 0.0 {
        return if avg_gain == 0.0 { 50.0 } else { 100.0 };
    }
    let rs = avg_gain / avg_loss;
    100.0 - 100.0 / (1.0 + rs)
}

/// Wilder RSI；前 n 个位置为 NaN。
pub fn rsi(s: &[f64], n: usize) -> Vec<f64> {
    let len = s.len();
    let mut out = vec![f64::NAN; len];
    if len <= n || n == 0 {
        return out;
    }
    let (mut gain, mut loss) = (0.0, 0.0);
    for i in 1..=n {
        let d = s[i] - s[i - 1];
        if d >= 0.0 {
            gain += d;
        } else {
            loss -= d;
        }
    }
    let mut avg_gain = gain / n as f64;
    let mut avg_loss = loss / n as f64;
    out[n] = rsi_from(avg_gain, avg_loss);
    for i in (n + 1)..len {
        let d = s[i] - s[i - 1];
        let (g, l) = if d >= 0.0 { (d, 0.0) } else { (0.0, -d) };
        avg_gain = (avg_gain * (n as f64 - 1.0) + g) / n as f64;
        avg_loss = (avg_loss * (n as f64 - 1.0) + l) / n as f64;
        out[i] = rsi_from(avg_gain, avg_loss);
    }
    out
}
```

- [ ] **Step 4: 运行验证通过**

Run: `cargo test --lib indicators`
Expected: 三个测试 PASS。

- [ ] **Step 5: Commit**

```bash
git add src/features/indicators.rs
git commit -m "feat(features): sma, ema, rsi indicators"
```

---

## Task 6: features/indicators.rs — atr / slope / highest / lowest / crossover / crossunder

**Files:**
- Modify: `src/features/indicators.rs`（追加函数与测试）

- [ ] **Step 1: 在 tests 模块追加失败测试**

在 `mod tests` 内追加：
```rust
    #[test]
    fn atr_constant_range() {
        // high-low 恒为 2，close 恒定 => TR 恒为 2 => ATR 恒为 2
        let high = vec![11.0; 10];
        let low = vec![9.0; 10];
        let close = vec![10.0; 10];
        let out = atr(&high, &low, &close, 3);
        assert_relative_eq!(*out.last().unwrap(), 2.0);
    }

    #[test]
    fn slope_of_linear_series() {
        assert_relative_eq!(slope(&[1.0, 2.0, 3.0, 4.0, 5.0], 5), 1.0);
    }

    #[test]
    fn highest_lowest_last_n() {
        let s = [3.0, 1.0, 4.0, 1.0, 5.0, 9.0, 2.0];
        assert_relative_eq!(highest(&s, 3), 9.0);
        assert_relative_eq!(lowest(&s, 3), 2.0);
    }

    #[test]
    fn cross_detection() {
        // a 上穿 b：上一根 a<=b，本根 a>b
        assert!(crossover(&[1.0, 3.0], &[2.0, 2.0]));
        assert!(!crossover(&[3.0, 4.0], &[2.0, 2.0]));
        assert!(crossunder(&[3.0, 1.0], &[2.0, 2.0]));
    }
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test --lib indicators`
Expected: 编译失败（`atr`/`slope`/`highest`/`lowest`/`crossover`/`crossunder` 未定义）。

- [ ] **Step 3: 追加实现**

在 `src/features/indicators.rs` 末尾追加：
```rust
/// Wilder ATR；前 n-1 个位置为 NaN。high/low/close 等长。
pub fn atr(high: &[f64], low: &[f64], close: &[f64], n: usize) -> Vec<f64> {
    let len = high.len();
    let mut out = vec![f64::NAN; len];
    if len == 0 || n == 0 || low.len() != len || close.len() != len || len < n {
        return out;
    }
    let mut tr = vec![0.0; len];
    tr[0] = high[0] - low[0];
    for i in 1..len {
        let a = high[i] - low[i];
        let b = (high[i] - close[i - 1]).abs();
        let c = (low[i] - close[i - 1]).abs();
        tr[i] = a.max(b).max(c);
    }
    let mut sum = 0.0;
    for v in tr.iter().take(n) {
        sum += *v;
    }
    out[n - 1] = sum / n as f64;
    for i in n..len {
        out[i] = (out[i - 1] * (n as f64 - 1.0) + tr[i]) / n as f64;
    }
    out
}

/// 最近 n 根的线性回归斜率（x = 0..n-1）。不足返回 NaN。
pub fn slope(s: &[f64], n: usize) -> f64 {
    let len = s.len();
    if n < 2 || len < n {
        return f64::NAN;
    }
    let w = &s[len - n..];
    let nf = n as f64;
    let mean_x = (nf - 1.0) / 2.0;
    let mean_y = w.iter().sum::<f64>() / nf;
    let (mut num, mut den) = (0.0, 0.0);
    for (i, &y) in w.iter().enumerate() {
        let dx = i as f64 - mean_x;
        num += dx * (y - mean_y);
        den += dx * dx;
    }
    if den == 0.0 { f64::NAN } else { num / den }
}

/// 最近 n 根最高值。
pub fn highest(s: &[f64], n: usize) -> f64 {
    let len = s.len();
    if len == 0 || n == 0 {
        return f64::NAN;
    }
    let start = len.saturating_sub(n);
    s[start..].iter().copied().fold(f64::NEG_INFINITY, f64::max)
}

/// 最近 n 根最低值。
pub fn lowest(s: &[f64], n: usize) -> f64 {
    let len = s.len();
    if len == 0 || n == 0 {
        return f64::NAN;
    }
    let start = len.saturating_sub(n);
    s[start..].iter().copied().fold(f64::INFINITY, f64::min)
}

/// a 上穿 b：上一根 a<=b 且本根 a>b。
pub fn crossover(a: &[f64], b: &[f64]) -> bool {
    let (la, lb) = (a.len(), b.len());
    if la < 2 || lb < 2 {
        return false;
    }
    a[la - 2] <= b[lb - 2] && a[la - 1] > b[lb - 1]
}

/// a 下穿 b：上一根 a>=b 且本根 a<b。
pub fn crossunder(a: &[f64], b: &[f64]) -> bool {
    let (la, lb) = (a.len(), b.len());
    if la < 2 || lb < 2 {
        return false;
    }
    a[la - 2] >= b[lb - 2] && a[la - 1] < b[lb - 1]
}
```

- [ ] **Step 4: 运行验证通过**

Run: `cargo test --lib indicators`
Expected: 全部 PASS（共 7 个）。

- [ ] **Step 5: Commit**

```bash
git add src/features/indicators.rs
git commit -m "feat(features): atr, slope, highest, lowest, crossover, crossunder"
```

---

## Task 7: features/context.rs — Context 与防未来函数窗口

**Files:**
- Create: `src/features/context.rs`
- Test: 同文件（含 look-ahead 属性测试）

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::bar::Bar;
    use chrono::{NaiveDate, NaiveDateTime};

    fn bar_at(min_from_open: i64, price: f64) -> Bar {
        let base = NaiveDate::from_ymd_opt(2024, 1, 2)
            .unwrap()
            .and_hms_opt(9, 45, 0)
            .unwrap();
        let time = base + chrono::Duration::minutes(min_from_open);
        Bar { time, open: price, high: price, low: price, close: price, volume: 1.0 }
    }

    fn series(n: usize) -> Vec<Bar> {
        (0..n).map(|i| bar_at(i as i64 * 15, i as f64)).collect()
    }

    #[test]
    fn window_takes_trailing_visible_bars() {
        let primary = series(10);
        let t = primary[5].time; // 决策时刻 = 第 6 根收盘
        let ctx = build_context(&primary, &[], t, 3);
        // 只应看到 time <= t 的最后 3 根（索引 3,4,5）
        assert_eq!(ctx.primary.bars.len(), 3);
        assert_eq!(ctx.primary.bars.last().unwrap().close, 5.0);
    }

    #[test]
    fn no_future_bar_leaks_property() {
        let primary = series(50);
        for i in 0..primary.len() {
            let t = primary[i].time;
            let ctx = build_context(&primary, &primary, t, 100);
            for b in &ctx.primary.bars {
                assert!(b.time <= t, "future primary bar leaked at i={i}");
            }
            for b in &ctx.context.bars {
                assert!(b.time <= t, "future context bar leaked at i={i}");
            }
        }
    }
}
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test --lib context`
Expected: 编译失败（`Context` / `build_context` 未定义）。

- [ ] **Step 3: 写实现**

```rust
use crate::data::bar::Bar;
use crate::data::bar::Window;
use chrono::NaiveDateTime;

/// 决策时点上下文：节点能看到的全部信息（绝不含未来）。
#[derive(Debug, Clone)]
pub struct Context {
    pub t: NaiveDateTime,
    pub primary: Window,
    pub context: Window,
}

/// 取 bars 中 time <= t 的最后 window 根（要求 bars 已按时间升序）。
/// 用 partition_point 二分，O(log n)。这是防未来函数的唯一闸门。
fn trailing_visible(bars: &[Bar], t: NaiveDateTime, window: usize) -> Vec<Bar> {
    let visible_end = bars.partition_point(|b| b.time <= t);
    let start = visible_end.saturating_sub(window);
    bars[start..visible_end].to_vec()
}

/// 构建 t 时刻的 Context：小周期与大周期各取最近 window 根可见 bar。
pub fn build_context(
    primary: &[Bar],
    context: &[Bar],
    t: NaiveDateTime,
    window: usize,
) -> Context {
    Context {
        t,
        primary: Window { bars: trailing_visible(primary, t, window) },
        context: Window { bars: trailing_visible(context, t, window) },
    }
}
```

- [ ] **Step 4: 运行验证通过**

Run: `cargo test --lib context`
Expected: 两个测试 PASS。

- [ ] **Step 5: Commit**

```bash
git add src/features/context.rs
git commit -m "feat(features): look-ahead-safe Context builder (partition_point gate)"
```

---

## Task 8: dsl/ast.rs + dsl/lexer.rs — AST 与词法分析

**Files:**
- Create: `src/dsl/ast.rs`
- Create: `src/dsl/lexer.rs`
- Test: `lexer.rs` 同文件

- [ ] **Step 1: 写 AST（无测试）**

`src/dsl/ast.rs`:
```rust
#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOp {
    Not,
    Neg,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Gt,
    Lt,
    Ge,
    Le,
    Eq,
    Ne,
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Number(f64),
    Ident(String),
    Index(Box<Expr>, i64),
    Unary(UnaryOp, Box<Expr>),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}
```

- [ ] **Step 2: 写 lexer 失败测试**

`src/dsl/lexer.rs`（先放测试）:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_expression_with_dotted_ident_and_keywords() {
        let toks = tokenize("ema(close,20) >= ctx.close and not down").unwrap();
        assert!(toks.contains(&Token::And));
        assert!(toks.contains(&Token::Not));
        assert!(toks.contains(&Token::Ge));
        assert!(toks.iter().any(|t| matches!(t, Token::Ident(s) if s == "ctx.close")));
        assert!(toks.iter().any(|t| matches!(t, Token::Number(n) if *n == 20.0)));
    }

    #[test]
    fn tokenizes_comparison_and_brackets() {
        let toks = tokenize("close[-1] < 10.5").unwrap();
        assert_eq!(toks[0], Token::Ident("close".to_string()));
        assert_eq!(toks[1], Token::LBracket);
        assert_eq!(toks[2], Token::Minus);
        assert_eq!(toks[3], Token::Number(1.0));
        assert_eq!(toks[4], Token::RBracket);
        assert_eq!(toks[5], Token::Lt);
    }
}
```

- [ ] **Step 3: 运行验证失败**

Run: `cargo test --lib lexer`
Expected: 编译失败（`Token` / `tokenize` 未定义）。

- [ ] **Step 4: 写 lexer 实现**

在测试上方写：
```rust
use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Number(f64),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Gt,
    Lt,
    Ge,
    Le,
    EqEq,
    Ne,
    And,
    Or,
    Not,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
}

pub fn tokenize(src: &str) -> Result<Vec<Token>> {
    let chars: Vec<char> = src.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' | '\n' | '\r' => i += 1,
            '+' => { tokens.push(Token::Plus); i += 1; }
            '-' => { tokens.push(Token::Minus); i += 1; }
            '*' => { tokens.push(Token::Star); i += 1; }
            '/' => { tokens.push(Token::Slash); i += 1; }
            '(' => { tokens.push(Token::LParen); i += 1; }
            ')' => { tokens.push(Token::RParen); i += 1; }
            '[' => { tokens.push(Token::LBracket); i += 1; }
            ']' => { tokens.push(Token::RBracket); i += 1; }
            ',' => { tokens.push(Token::Comma); i += 1; }
            '>' => {
                if chars.get(i + 1) == Some(&'=') { tokens.push(Token::Ge); i += 2; }
                else { tokens.push(Token::Gt); i += 1; }
            }
            '<' => {
                if chars.get(i + 1) == Some(&'=') { tokens.push(Token::Le); i += 2; }
                else { tokens.push(Token::Lt); i += 1; }
            }
            '=' => {
                if chars.get(i + 1) == Some(&'=') { tokens.push(Token::EqEq); i += 2; }
                else { return Err(Error::Dsl("'=' must be '=='".into())); }
            }
            '!' => {
                if chars.get(i + 1) == Some(&'=') { tokens.push(Token::Ne); i += 2; }
                else { return Err(Error::Dsl("'!' must be '!='".into())); }
            }
            c if c.is_ascii_digit() => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') { i += 1; }
                let s: String = chars[start..i].iter().collect();
                let n: f64 = s.parse().map_err(|_| Error::Dsl(format!("bad number: {s}")))?;
                tokens.push(Token::Number(n));
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len()
                    && (chars[i].is_ascii_alphanumeric() || chars[i] == '_' || chars[i] == '.')
                { i += 1; }
                let s: String = chars[start..i].iter().collect();
                match s.as_str() {
                    "and" => tokens.push(Token::And),
                    "or" => tokens.push(Token::Or),
                    "not" => tokens.push(Token::Not),
                    _ => tokens.push(Token::Ident(s)),
                }
            }
            other => return Err(Error::Dsl(format!("unexpected char: {other}"))),
        }
    }
    Ok(tokens)
}
```

- [ ] **Step 5: 运行验证通过**

Run: `cargo test --lib lexer`
Expected: 两个测试 PASS。

- [ ] **Step 6: Commit**

```bash
git add src/dsl/ast.rs src/dsl/lexer.rs
git commit -m "feat(dsl): AST types and tokenizer"
```

---

## Task 9: dsl/parser.rs — Pratt 解析器

**Files:**
- Create: `src/dsl/parser.rs`
- Test: 同文件

优先级（低→高）：`or` < `and` < 比较 < `+ -` < `* /` < 一元(`not`/`neg`) < 后缀索引 < 基元。

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::ast::{BinaryOp, Expr};

    #[test]
    fn precedence_mul_binds_tighter_than_add() {
        let e = parse_str("1 + 2 * 3").unwrap();
        match e {
            Expr::Binary(BinaryOp::Add, l, r) => {
                assert_eq!(*l, Expr::Number(1.0));
                assert!(matches!(*r, Expr::Binary(BinaryOp::Mul, _, _)));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parses_call_and_comparison() {
        let e = parse_str("close > sma(close,5)").unwrap();
        match e {
            Expr::Binary(BinaryOp::Gt, l, r) => {
                assert_eq!(*l, Expr::Ident("close".into()));
                assert!(matches!(*r, Expr::Call(ref name, ref args) if name == "sma" && args.len() == 2));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parses_negative_index() {
        let e = parse_str("close[-1]").unwrap();
        assert_eq!(e, Expr::Index(Box::new(Expr::Ident("close".into())), -1));
    }
}
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test --lib parser`
Expected: 编译失败（`parse_str` / `Parser` 未定义）。

- [ ] **Step 3: 写实现**

```rust
use crate::dsl::ast::{BinaryOp, Expr, UnaryOp};
use crate::dsl::lexer::{tokenize, Token};
use crate::{Error, Result};

/// 便捷入口：源码字符串 → AST。
pub fn parse_str(src: &str) -> Result<Expr> {
    let tokens = tokenize(src)?;
    Parser::new(tokens).parse()
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn expect(&mut self, t: Token) -> Result<()> {
        if self.peek() == Some(&t) {
            self.pos += 1;
            Ok(())
        } else {
            Err(Error::Dsl(format!("expected {t:?}, got {:?}", self.peek())))
        }
    }

    pub fn parse(&mut self) -> Result<Expr> {
        let e = self.parse_expr(0)?;
        if self.pos != self.tokens.len() {
            return Err(Error::Dsl("trailing tokens after expression".into()));
        }
        Ok(e)
    }

    fn parse_expr(&mut self, min_bp: u8) -> Result<Expr> {
        let mut lhs = self.parse_prefix()?;
        loop {
            let (lbp, rbp, op) = match self.peek() {
                Some(Token::Or) => (1, 2, BinaryOp::Or),
                Some(Token::And) => (3, 4, BinaryOp::And),
                Some(Token::Gt) => (5, 6, BinaryOp::Gt),
                Some(Token::Lt) => (5, 6, BinaryOp::Lt),
                Some(Token::Ge) => (5, 6, BinaryOp::Ge),
                Some(Token::Le) => (5, 6, BinaryOp::Le),
                Some(Token::EqEq) => (5, 6, BinaryOp::Eq),
                Some(Token::Ne) => (5, 6, BinaryOp::Ne),
                Some(Token::Plus) => (7, 8, BinaryOp::Add),
                Some(Token::Minus) => (7, 8, BinaryOp::Sub),
                Some(Token::Star) => (9, 10, BinaryOp::Mul),
                Some(Token::Slash) => (9, 10, BinaryOp::Div),
                _ => break,
            };
            if lbp < min_bp {
                break;
            }
            self.pos += 1;
            let rhs = self.parse_expr(rbp)?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_prefix(&mut self) -> Result<Expr> {
        match self.next() {
            Some(Token::Not) => {
                let e = self.parse_expr(11)?;
                Ok(Expr::Unary(UnaryOp::Not, Box::new(e)))
            }
            Some(Token::Minus) => {
                let e = self.parse_expr(11)?;
                Ok(Expr::Unary(UnaryOp::Neg, Box::new(e)))
            }
            Some(Token::Number(n)) => self.parse_postfix(Expr::Number(n)),
            Some(Token::LParen) => {
                let e = self.parse_expr(0)?;
                self.expect(Token::RParen)?;
                self.parse_postfix(e)
            }
            Some(Token::Ident(name)) => {
                if self.peek() == Some(&Token::LParen) {
                    self.pos += 1;
                    let mut args = Vec::new();
                    if self.peek() != Some(&Token::RParen) {
                        loop {
                            args.push(self.parse_expr(0)?);
                            if self.peek() == Some(&Token::Comma) {
                                self.pos += 1;
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect(Token::RParen)?;
                    self.parse_postfix(Expr::Call(name, args))
                } else {
                    self.parse_postfix(Expr::Ident(name))
                }
            }
            other => Err(Error::Dsl(format!("unexpected token: {other:?}"))),
        }
    }

    fn parse_postfix(&mut self, e: Expr) -> Result<Expr> {
        let mut e = e;
        while self.peek() == Some(&Token::LBracket) {
            self.pos += 1;
            let neg = if self.peek() == Some(&Token::Minus) {
                self.pos += 1;
                true
            } else {
                false
            };
            let idx = match self.next() {
                Some(Token::Number(n)) => n as i64,
                other => return Err(Error::Dsl(format!("expected index number, got {other:?}"))),
            };
            let idx = if neg { -idx } else { idx };
            self.expect(Token::RBracket)?;
            e = Expr::Index(Box::new(e), idx);
        }
        Ok(e)
    }
}
```

- [ ] **Step 4: 运行验证通过**

Run: `cargo test --lib parser`
Expected: 三个测试 PASS。

- [ ] **Step 5: Commit**

```bash
git add src/dsl/parser.rs
git commit -m "feat(dsl): Pratt parser with precedence, calls, and indexing"
```

---

## Task 10: dsl/eval.rs — 求值器（Value、ident 解析、函数派发）

**Files:**
- Create: `src/dsl/eval.rs`
- Test: 同文件

语义：序列在算术/比较中归约为最新值（`as_scalar` 取末元素）；`slope/highest/lowest` 归约为标量；`crossover/crossunder` 归约为 bool；分支条件最终须为 bool。

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::bar::{Bar, Window};
    use crate::dsl::parser::parse_str;
    use crate::features::context::Context;
    use chrono::NaiveDate;

    fn ctx_from_closes(closes: &[f64]) -> Context {
        let base = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        let bars: Vec<Bar> = closes
            .iter()
            .enumerate()
            .map(|(i, &c)| Bar {
                time: base + chrono::Duration::minutes(i as i64 * 15),
                open: c, high: c, low: c, close: c, volume: 1.0,
            })
            .collect();
        let t = bars.last().unwrap().time;
        Context { t, primary: Window { bars: bars.clone() }, context: Window { bars } }
    }

    #[test]
    fn comparison_reduces_series_to_latest() {
        let ctx = ctx_from_closes(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        let e = parse_str("close > sma(close,3)").unwrap();
        assert_eq!(eval(&e, &ctx).unwrap(), Value::Bool(true));
    }

    #[test]
    fn index_returns_previous_scalar() {
        let ctx = ctx_from_closes(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        let e = parse_str("close[-1]").unwrap();
        assert_eq!(eval(&e, &ctx).unwrap(), Value::Scalar(4.0));
    }

    #[test]
    fn slope_of_series_is_scalar() {
        let ctx = ctx_from_closes(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        let e = parse_str("slope(close,5)").unwrap();
        assert_eq!(eval(&e, &ctx).unwrap(), Value::Scalar(1.0));
    }

    #[test]
    fn and_of_bools_and_ctx_ident() {
        let ctx = ctx_from_closes(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        let e = parse_str("close > 4 and ctx.close > 0").unwrap();
        assert_eq!(eval(&e, &ctx).unwrap(), Value::Bool(true));
    }
}
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test --lib dsl::eval`
Expected: 编译失败（`Value` / `eval` 未定义）。

- [ ] **Step 3: 写实现**

```rust
use crate::dsl::ast::{BinaryOp, Expr, UnaryOp};
use crate::features::context::Context;
use crate::features::indicators;
use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Series(Vec<f64>),
    Scalar(f64),
    Bool(bool),
}

/// 便捷：求值并强制为 bool（分支条件用）。
pub fn eval_bool(expr: &Expr, ctx: &Context) -> Result<bool> {
    as_bool(&eval(expr, ctx)?)
}

pub fn eval(expr: &Expr, ctx: &Context) -> Result<Value> {
    match expr {
        Expr::Number(n) => Ok(Value::Scalar(*n)),
        Expr::Ident(name) => Ok(Value::Series(resolve_series(name, ctx)?)),
        Expr::Index(inner, k) => {
            let s = as_series(&eval(inner, ctx)?)?;
            let len = s.len() as i64;
            let pos = (len - 1) + *k;
            if pos < 0 || pos >= len {
                return Err(Error::Eval(format!("index {k} out of range (len {len})")));
            }
            Ok(Value::Scalar(s[pos as usize]))
        }
        Expr::Unary(op, e) => {
            let v = eval(e, ctx)?;
            match op {
                UnaryOp::Neg => Ok(Value::Scalar(-as_scalar(&v)?)),
                UnaryOp::Not => Ok(Value::Bool(!as_bool(&v)?)),
            }
        }
        Expr::Binary(op, l, r) => {
            let lv = eval(l, ctx)?;
            let rv = eval(r, ctx)?;
            Ok(match op {
                BinaryOp::And => Value::Bool(as_bool(&lv)? && as_bool(&rv)?),
                BinaryOp::Or => Value::Bool(as_bool(&lv)? || as_bool(&rv)?),
                BinaryOp::Add => Value::Scalar(as_scalar(&lv)? + as_scalar(&rv)?),
                BinaryOp::Sub => Value::Scalar(as_scalar(&lv)? - as_scalar(&rv)?),
                BinaryOp::Mul => Value::Scalar(as_scalar(&lv)? * as_scalar(&rv)?),
                BinaryOp::Div => Value::Scalar(as_scalar(&lv)? / as_scalar(&rv)?),
                BinaryOp::Gt => Value::Bool(as_scalar(&lv)? > as_scalar(&rv)?),
                BinaryOp::Lt => Value::Bool(as_scalar(&lv)? < as_scalar(&rv)?),
                BinaryOp::Ge => Value::Bool(as_scalar(&lv)? >= as_scalar(&rv)?),
                BinaryOp::Le => Value::Bool(as_scalar(&lv)? <= as_scalar(&rv)?),
                BinaryOp::Eq => Value::Bool(as_scalar(&lv)? == as_scalar(&rv)?),
                BinaryOp::Ne => Value::Bool(as_scalar(&lv)? != as_scalar(&rv)?),
            })
        }
        Expr::Call(name, args) => eval_call(name, args, ctx),
    }
}

fn resolve_series(name: &str, ctx: &Context) -> Result<Vec<f64>> {
    let (win, field) = match name.strip_prefix("ctx.") {
        Some(f) => (&ctx.context, f),
        None => (&ctx.primary, name),
    };
    match field {
        "close" => Ok(win.closes()),
        "open" => Ok(win.opens()),
        "high" => Ok(win.highs()),
        "low" => Ok(win.lows()),
        "volume" => Ok(win.volumes()),
        _ => Err(Error::Eval(format!("unknown identifier: {name}"))),
    }
}

fn as_scalar(v: &Value) -> Result<f64> {
    match v {
        Value::Scalar(x) => Ok(*x),
        // 空/预热不足的序列归约为 NaN：NaN 的比较恒为 false → 分支不命中 → 走 default（弃权策略）。
        Value::Series(s) => Ok(s.last().copied().unwrap_or(f64::NAN)),
        Value::Bool(_) => Err(Error::Eval("expected number, got bool".into())),
    }
}

fn as_bool(v: &Value) -> Result<bool> {
    match v {
        Value::Bool(b) => Ok(*b),
        _ => Err(Error::Eval("expected bool".into())),
    }
}

fn as_series(v: &Value) -> Result<Vec<f64>> {
    match v {
        Value::Series(s) => Ok(s.clone()),
        Value::Scalar(x) => Ok(vec![*x]),
        Value::Bool(_) => Err(Error::Eval("expected series".into())),
    }
}

fn as_usize(v: &Value) -> Result<usize> {
    let x = as_scalar(v)?;
    if x < 0.0 {
        return Err(Error::Eval("expected non-negative integer".into()));
    }
    Ok(x as usize)
}

fn need(args: &[Value], n: usize, name: &str) -> Result<()> {
    if args.len() != n {
        return Err(Error::Eval(format!("{name} expects {n} args, got {}", args.len())));
    }
    Ok(())
}

fn eval_call(name: &str, args: &[Expr], ctx: &Context) -> Result<Value> {
    let vals: Result<Vec<Value>> = args.iter().map(|a| eval(a, ctx)).collect();
    let vals = vals?;
    match name {
        "sma" => { need(&vals, 2, name)?; Ok(Value::Series(indicators::sma(&as_series(&vals[0])?, as_usize(&vals[1])?))) }
        "ema" => { need(&vals, 2, name)?; Ok(Value::Series(indicators::ema(&as_series(&vals[0])?, as_usize(&vals[1])?))) }
        "wma" => { need(&vals, 2, name)?; Ok(Value::Series(indicators::sma(&as_series(&vals[0])?, as_usize(&vals[1])?))) } // 见说明
        "rsi" => { need(&vals, 2, name)?; Ok(Value::Series(indicators::rsi(&as_series(&vals[0])?, as_usize(&vals[1])?))) }
        "atr" => {
            need(&vals, 1, name)?;
            let n = as_usize(&vals[0])?;
            Ok(Value::Series(indicators::atr(&ctx.primary.highs(), &ctx.primary.lows(), &ctx.primary.closes(), n)))
        }
        "slope" => { need(&vals, 2, name)?; Ok(Value::Scalar(indicators::slope(&as_series(&vals[0])?, as_usize(&vals[1])?))) }
        "highest" => { need(&vals, 2, name)?; Ok(Value::Scalar(indicators::highest(&as_series(&vals[0])?, as_usize(&vals[1])?))) }
        "lowest" => { need(&vals, 2, name)?; Ok(Value::Scalar(indicators::lowest(&as_series(&vals[0])?, as_usize(&vals[1])?))) }
        "crossover" => { need(&vals, 2, name)?; Ok(Value::Bool(indicators::crossover(&as_series(&vals[0])?, &as_series(&vals[1])?))) }
        "crossunder" => { need(&vals, 2, name)?; Ok(Value::Bool(indicators::crossunder(&as_series(&vals[0])?, &as_series(&vals[1])?))) }
        _ => Err(Error::Eval(format!("unknown function: {name}"))),
    }
}
```

> 说明：`wma` 暂用 `sma` 占位以保证函数表完整可用；真正的加权实现属于后续指标扩展（非本计划范围，spec §16 之外的小增强）。`macd_*`/`std` 未列入 v1 实现集（YAGNI），待有树需要时再加。这是**有意的范围决定**，不是占位符。

- [ ] **Step 4: 运行验证通过**

Run: `cargo test --lib dsl::eval`
Expected: 四个测试 PASS。

- [ ] **Step 5: Commit**

```bash
git add src/dsl/eval.rs
git commit -m "feat(dsl): evaluator with series-reduction semantics and function dispatch"
```

---

## Task 11: tree/schema.rs — YAML serde 结构

**Files:**
- Create: `src/tree/schema.rs`
- Test: 同文件

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const YAML: &str = r#"
meta:
  name: t
  forward_window: 16
  stances: [long, flat]
root: a
nodes:
  a:
    type: quant
    branches:
      - when: "close > 1"
        goto: leaf_l
        label: up
    default: { goto: leaf_f, label: none }
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;

    #[test]
    fn deserializes_tree_spec() {
        let spec: TreeSpec = serde_yaml::from_str(YAML).unwrap();
        assert_eq!(spec.meta.forward_window, 16);
        assert_eq!(spec.root, "a");
        assert!(matches!(spec.nodes.get("a").unwrap(), NodeSpec::Quant { .. }));
        assert_eq!(spec.leaves.get("leaf_l").unwrap().stance, Stance::Long);
    }
}
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test --lib schema`
Expected: 编译失败（类型未定义）。

- [ ] **Step 3: 写实现**

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stance {
    Long,
    Flat,
    Short,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Meta {
    pub name: String,
    pub forward_window: usize,
    pub stances: Vec<Stance>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Target {
    pub goto: String,
    pub label: String,
}

#[derive(Debug, Deserialize)]
pub struct BranchSpec {
    pub when: String,
    pub goto: String,
    pub label: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum NodeSpec {
    Quant {
        branches: Vec<BranchSpec>,
        default: Target,
    },
    Llm {
        #[serde(default)]
        inputs: Vec<String>,
        #[serde(default)]
        prompt: String,
        #[serde(default)]
        labels: HashMap<String, String>,
        default: String,
    },
}

#[derive(Debug, Deserialize)]
pub struct LeafSpec {
    pub stance: Stance,
}

#[derive(Debug, Deserialize)]
pub struct TreeSpec {
    pub meta: Meta,
    pub root: String,
    pub nodes: HashMap<String, NodeSpec>,
    pub leaves: HashMap<String, LeafSpec>,
}
```

- [ ] **Step 4: 运行验证通过**

Run: `cargo test --lib schema`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add src/tree/schema.rs
git commit -m "feat(tree): YAML schema structs (TreeSpec/NodeSpec/Stance)"
```

---

## Task 12: tree/loader.rs — 编译为运行时 Tree + 校验

**Files:**
- Create: `src/tree/loader.rs`
- Test: 同文件

校验：root 是节点；所有 goto 目标存在；所有节点可达；DAG 无环；叶子 stance ∈ meta.stances。量化分支的 `when` 在加载期即编译为 `Expr`（早失败）。

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
root: a
nodes:
  a:
    type: quant
    branches:
      - when: "close > sma(close,3)"
        goto: leaf_l
        label: up
    default: { goto: leaf_f, label: none }
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;

    #[test]
    fn loads_valid_tree() {
        let tree = load_tree_str(VALID).unwrap();
        assert_eq!(tree.root, "a");
        assert_eq!(tree.nodes.len(), 1);
        assert_eq!(tree.leaves.len(), 2);
    }

    #[test]
    fn rejects_unknown_target() {
        let bad = VALID.replace("goto: leaf_l", "goto: nope");
        assert!(load_tree_str(&bad).is_err());
    }

    #[test]
    fn rejects_leaf_stance_not_in_meta() {
        let bad = VALID.replace("leaf_l: { stance: long }", "leaf_l: { stance: short }");
        assert!(load_tree_str(&bad).is_err());
    }

    #[test]
    fn rejects_bad_dsl_at_load() {
        let bad = VALID.replace(r#"when: "close > sma(close,3)""#, r#"when: "close >""#);
        assert!(load_tree_str(&bad).is_err());
    }

    #[test]
    fn rejects_cycle() {
        let cyc = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "close > 1", goto: b, label: x } ]
    default: { goto: leaf_f, label: none }
  b:
    type: quant
    branches: [ { when: "close > 1", goto: a, label: y } ]
    default: { goto: leaf_f, label: none }
leaves:
  leaf_f: { stance: flat }
"#;
        assert!(load_tree_str(cyc).is_err());
    }
}
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test --lib loader`
Expected: 编译失败（`load_tree_str` / `Tree` 未定义）。

- [ ] **Step 3: 写实现**

```rust
use crate::dsl::ast::Expr;
use crate::dsl::parser::parse_str;
use crate::tree::schema::{Meta, NodeSpec, Stance, Target, TreeSpec};
use crate::{Error, Result};
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct Branch {
    pub when: Expr,
    pub when_src: String,
    pub goto: String,
    pub label: String,
}

#[derive(Debug, Clone)]
pub enum Node {
    Quant {
        branches: Vec<Branch>,
        default: Target,
    },
    Llm {
        inputs: Vec<String>,
        prompt: String,
        labels: HashMap<String, String>,
        default: String,
    },
}

#[derive(Debug, Clone)]
pub struct Leaf {
    pub stance: Stance,
}

#[derive(Debug, Clone)]
pub struct Tree {
    pub meta: Meta,
    pub root: String,
    pub nodes: HashMap<String, Node>,
    pub leaves: HashMap<String, Leaf>,
}

pub fn load_tree_file(path: &Path) -> Result<Tree> {
    let src = std::fs::read_to_string(path)?;
    load_tree_str(&src)
}

pub fn load_tree_str(src: &str) -> Result<Tree> {
    let spec: TreeSpec = serde_yaml::from_str(src)?;
    let stances: HashSet<Stance> = spec.meta.stances.iter().copied().collect();

    let mut leaves = HashMap::new();
    for (id, l) in &spec.leaves {
        if !stances.contains(&l.stance) {
            return Err(Error::Tree(format!(
                "leaf '{id}' stance {:?} not in meta.stances",
                l.stance
            )));
        }
        leaves.insert(id.clone(), Leaf { stance: l.stance });
    }

    let mut nodes = HashMap::new();
    for (id, ns) in &spec.nodes {
        match ns {
            NodeSpec::Quant { branches, default } => {
                let mut compiled = Vec::new();
                for b in branches {
                    let expr = parse_str(&b.when).map_err(|e| {
                        Error::Tree(format!("node '{id}' branch '{}': {e}", b.label))
                    })?;
                    compiled.push(Branch {
                        when: expr,
                        when_src: b.when.clone(),
                        goto: b.goto.clone(),
                        label: b.label.clone(),
                    });
                }
                nodes.insert(id.clone(), Node::Quant { branches: compiled, default: default.clone() });
            }
            NodeSpec::Llm { inputs, prompt, labels, default } => {
                nodes.insert(
                    id.clone(),
                    Node::Llm {
                        inputs: inputs.clone(),
                        prompt: prompt.clone(),
                        labels: labels.clone(),
                        default: default.clone(),
                    },
                );
            }
        }
    }

    let tree = Tree {
        meta: spec.meta.clone(),
        root: spec.root.clone(),
        nodes,
        leaves,
    };
    validate(&tree)?;
    Ok(tree)
}

fn node_targets(node: &Node) -> Vec<String> {
    match node {
        Node::Quant { branches, default } => {
            let mut v: Vec<String> = branches.iter().map(|b| b.goto.clone()).collect();
            v.push(default.goto.clone());
            v
        }
        Node::Llm { labels, default, .. } => {
            let mut v: Vec<String> = labels.values().cloned().collect();
            v.push(default.clone());
            v
        }
    }
}

fn validate(tree: &Tree) -> Result<()> {
    let exists = |id: &str| tree.nodes.contains_key(id) || tree.leaves.contains_key(id);
    if !tree.nodes.contains_key(&tree.root) {
        return Err(Error::Tree(format!("root '{}' is not a node", tree.root)));
    }
    for (id, node) in &tree.nodes {
        for tgt in node_targets(node) {
            if !exists(&tgt) {
                return Err(Error::Tree(format!("node '{id}' points to unknown target '{tgt}'")));
            }
        }
    }
    // reachability from root
    let mut seen = HashSet::new();
    let mut stack = vec![tree.root.clone()];
    while let Some(cur) = stack.pop() {
        if !seen.insert(cur.clone()) {
            continue;
        }
        if let Some(node) = tree.nodes.get(&cur) {
            for tgt in node_targets(node) {
                stack.push(tgt);
            }
        }
    }
    for id in tree.nodes.keys() {
        if !seen.contains(id) {
            return Err(Error::Tree(format!("node '{id}' unreachable from root")));
        }
    }
    // DAG check
    let mut color: HashMap<String, u8> = HashMap::new();
    dfs_cycle(&tree.root, tree, &mut color)?;
    Ok(())
}

fn dfs_cycle(cur: &str, tree: &Tree, color: &mut HashMap<String, u8>) -> Result<()> {
    color.insert(cur.to_string(), 1); // 1 = in stack
    if let Some(node) = tree.nodes.get(cur) {
        for tgt in node_targets(node) {
            match color.get(&tgt).copied().unwrap_or(0) {
                1 => return Err(Error::Tree(format!("cycle detected at '{tgt}'"))),
                0 => dfs_cycle(&tgt, tree, color)?,
                _ => {}
            }
        }
    }
    color.insert(cur.to_string(), 2); // 2 = done
    Ok(())
}
```

- [ ] **Step 4: 运行验证通过**

Run: `cargo test --lib loader`
Expected: 五个测试 PASS。

- [ ] **Step 5: Commit**

```bash
git add src/tree/loader.rs
git commit -m "feat(tree): loader compiling DSL and validating refs/reachability/DAG"
```

---

## Task 13: eval/quant.rs — QuantEvaluator

**Files:**
- Create: `src/eval/quant.rs`
- Test: 同文件

行为：按顺序求值各分支 `when`；第一个为真者胜出（confidence=1.0）；都不真走 default（confidence=0.5）。预热不足 → NaN → 不命中 → default（弃权）。`when` 含未知标识符/函数才会硬报错。

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::bar::{Bar, Window};
    use crate::dsl::parser::parse_str;
    use crate::features::context::Context;
    use crate::tree::loader::Branch;
    use crate::tree::schema::Target;
    use chrono::NaiveDate;

    fn ctx(closes: &[f64]) -> Context {
        let base = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        let bars: Vec<Bar> = closes
            .iter()
            .enumerate()
            .map(|(i, &c)| Bar {
                time: base + chrono::Duration::minutes(i as i64 * 15),
                open: c, high: c, low: c, close: c, volume: 1.0,
            })
            .collect();
        let t = bars.last().unwrap().time;
        Context { t, primary: Window { bars: bars.clone() }, context: Window { bars } }
    }

    fn br(when: &str, goto: &str, label: &str) -> Branch {
        Branch { when: parse_str(when).unwrap(), when_src: when.into(), goto: goto.into(), label: label.into() }
    }

    #[test]
    fn matches_first_true_branch() {
        let branches = vec![br("close > 100", "a", "hi"), br("close > 1", "b", "mid")];
        let default = Target { goto: "d".into(), label: "none".into() };
        let d = eval_quant(&branches, &default, &ctx(&[1.0, 2.0, 3.0])).unwrap();
        assert_eq!(d.goto, "b");
        assert_eq!(d.label, "mid");
        assert_eq!(d.confidence, 1.0);
    }

    #[test]
    fn falls_back_to_default_when_none_match() {
        let branches = vec![br("close > 100", "a", "hi")];
        let default = Target { goto: "d".into(), label: "none".into() };
        let d = eval_quant(&branches, &default, &ctx(&[1.0, 2.0, 3.0])).unwrap();
        assert_eq!(d.goto, "d");
        assert_eq!(d.confidence, 0.5);
    }
}
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test --lib eval::quant`
Expected: 编译失败（`eval_quant` 未定义）。

- [ ] **Step 3: 写实现**

```rust
use crate::dsl::eval::eval_bool;
use crate::eval::Decision;
use crate::features::context::Context;
use crate::tree::loader::Branch;
use crate::tree::schema::Target;
use crate::Result;

pub fn eval_quant(branches: &[Branch], default: &Target, ctx: &Context) -> Result<Decision> {
    for b in branches {
        if eval_bool(&b.when, ctx)? {
            return Ok(Decision {
                goto: b.goto.clone(),
                label: b.label.clone(),
                confidence: 1.0,
                rationale: format!("matched: {}", b.when_src),
            });
        }
    }
    Ok(Decision {
        goto: default.goto.clone(),
        label: default.label.clone(),
        confidence: 0.5,
        rationale: "default (no branch matched)".into(),
    })
}
```

- [ ] **Step 4: 运行验证通过**

Run: `cargo test --lib eval::quant`
Expected: 两个测试 PASS。

- [ ] **Step 5: Commit**

```bash
git add src/eval/quant.rs
git commit -m "feat(eval): QuantEvaluator (first-true-branch, default abstain)"
```

---

## Task 14: engine/trace.rs + engine/traversal.rs — Trace 与遍历

**Files:**
- Create: `src/engine/trace.rs`
- Create: `src/engine/traversal.rs`
- Test: `traversal.rs` 同文件

- [ ] **Step 1: 写 Trace 类型（无测试）**

`src/engine/trace.rs`:
```rust
use crate::tree::schema::Stance;
use chrono::NaiveDateTime;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct StepRecord {
    pub node_id: String,
    pub label: String,
    pub confidence: f64,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Trace {
    pub t: NaiveDateTime,
    pub path: Vec<StepRecord>,
    pub leaf: String,
    pub stance: Stance,
}
```

- [ ] **Step 2: 写 traversal 失败测试**

`src/engine/traversal.rs`（先放测试）:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::bar::{Bar, Window};
    use crate::features::context::Context;
    use crate::tree::loader::load_tree_str;
    use crate::tree::schema::Stance;
    use chrono::NaiveDate;

    fn ctx(closes: &[f64]) -> Context {
        let base = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        let bars: Vec<Bar> = closes
            .iter()
            .enumerate()
            .map(|(i, &c)| Bar {
                time: base + chrono::Duration::minutes(i as i64 * 15),
                open: c, high: c, low: c, close: c, volume: 1.0,
            })
            .collect();
        let t = bars.last().unwrap().time;
        Context { t, primary: Window { bars: bars.clone() }, context: Window { bars } }
    }

    const QUANT_TREE: &str = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "close > sma(close,3)", goto: leaf_l, label: up } ]
    default: { goto: leaf_f, label: flat }
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;

    const LLM_TREE: &str = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
root: a
nodes:
  a:
    type: llm
    prompt: "x"
    labels: { yes: leaf_l }
    default: leaf_f
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;

    #[test]
    fn quant_uptrend_reaches_long_leaf() {
        let tree = load_tree_str(QUANT_TREE).unwrap();
        let tr = traverse(&tree, &ctx(&[1.0, 2.0, 3.0, 4.0, 5.0])).unwrap();
        assert_eq!(tr.leaf, "leaf_l");
        assert!(matches!(tr.stance, Stance::Long));
        assert_eq!(tr.path.len(), 1);
        assert_eq!(tr.path[0].node_id, "a");
    }

    #[test]
    fn llm_node_takes_default_branch() {
        let tree = load_tree_str(LLM_TREE).unwrap();
        let tr = traverse(&tree, &ctx(&[1.0, 2.0, 3.0])).unwrap();
        assert_eq!(tr.leaf, "leaf_f");
        assert!(matches!(tr.stance, Stance::Flat));
        assert!(tr.path[0].rationale.contains("LLM deferred"));
    }
}
```

- [ ] **Step 3: 运行验证失败**

Run: `cargo test --lib traversal`
Expected: 编译失败（`traverse` 未定义）。

- [ ] **Step 4: 写实现**

在 `src/engine/traversal.rs` 测试上方：
```rust
use crate::engine::trace::{StepRecord, Trace};
use crate::eval::quant::eval_quant;
use crate::eval::Decision;
use crate::features::context::Context;
use crate::tree::loader::{Node, Tree};
use crate::{Error, Result};

/// 从 root 走树到叶子，沿途记录 Trace。量化节点用 QuantEvaluator；
/// LLM 节点在本阶段走 default 分支。
pub fn traverse(tree: &Tree, ctx: &Context) -> Result<Trace> {
    let mut path: Vec<StepRecord> = Vec::new();
    let mut current = tree.root.clone();
    let max_steps = tree.nodes.len() + 1;
    for _ in 0..=max_steps {
        if let Some(leaf) = tree.leaves.get(&current) {
            return Ok(Trace { t: ctx.t, path, leaf: current.clone(), stance: leaf.stance });
        }
        let node = tree
            .nodes
            .get(&current)
            .ok_or_else(|| Error::Engine(format!("dangling node '{current}'")))?;
        let decision = match node {
            Node::Quant { branches, default } => eval_quant(branches, default, ctx)?,
            Node::Llm { default, .. } => Decision {
                goto: default.clone(),
                label: "default".into(),
                confidence: 0.0,
                rationale: "LLM deferred (M5): took default branch".into(),
            },
        };
        path.push(StepRecord {
            node_id: current.clone(),
            label: decision.label.clone(),
            confidence: decision.confidence,
            rationale: decision.rationale.clone(),
        });
        current = decision.goto;
    }
    Err(Error::Engine("traversal exceeded max steps (cycle?)".into()))
}
```

- [ ] **Step 5: 运行验证通过**

Run: `cargo test --lib traversal`
Expected: 两个测试 PASS。

- [ ] **Step 6: Commit**

```bash
git add src/engine/trace.rs src/engine/traversal.rs
git commit -m "feat(engine): Trace types and tree traversal (quant + llm-default)"
```

---

## Task 15: backtest/costs.rs + backtest/forward_return.rs — 成本与前瞻收益

**Files:**
- Create: `src/backtest/costs.rs`
- Create: `src/backtest/forward_return.rs`
- Test: 各自同文件

- [ ] **Step 1: 写 costs 失败测试**

`src/backtest/costs.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn applies_round_trip_haircut() {
        let c = CostModel { round_trip_bps: 10.0 }; // 0.10%
        assert_relative_eq!(c.apply(0.05), 0.049, epsilon = 1e-9);
    }
}
```

- [ ] **Step 2: 运行 costs 验证失败**

Run: `cargo test --lib costs`
Expected: 编译失败（`CostModel` 未定义）。

- [ ] **Step 3: 写 costs 实现**

测试上方：
```rust
/// 简化成本模型：对非空仓收益统一扣往返成本（bps）。
#[derive(Debug, Clone, Copy)]
pub struct CostModel {
    pub round_trip_bps: f64,
}

impl CostModel {
    pub fn apply(&self, gross_return: f64) -> f64 {
        gross_return - self.round_trip_bps / 10000.0
    }
}
```

- [ ] **Step 4: 运行 costs 验证通过**

Run: `cargo test --lib costs`
Expected: PASS。

- [ ] **Step 5: 写 forward_return 失败测试**

`src/backtest/forward_return.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::backtest::costs::CostModel;
    use crate::data::bar::Bar;
    use crate::tree::schema::Stance;
    use approx::assert_relative_eq;
    use chrono::NaiveDateTime;

    fn bar(t: &str, open: f64, close: f64) -> Bar {
        Bar {
            time: NaiveDateTime::parse_from_str(t, "%Y-%m-%d %H:%M:%S").unwrap(),
            open,
            high: open.max(close),
            low: open.min(close),
            close,
            volume: 1.0,
        }
    }

    fn data() -> Vec<Bar> {
        vec![
            bar("2024-01-02 14:45:00", 9.0, 9.5),   // i=0 决策
            bar("2024-01-02 15:00:00", 10.0, 10.2), // 入场：open=10（次日 = 01-02 当日收盘 bar）
            bar("2024-01-03 09:45:00", 10.2, 11.0), // 出场：close=11（01-03，跨日 → T+1 可执行）
        ]
    }

    #[test]
    fn long_return_with_costs_and_t1() {
        let c = CostModel { round_trip_bps: 10.0 };
        let r = forward_return(&data(), 0, 2, Stance::Long, &c).unwrap();
        assert_relative_eq!(r.gross, 0.10, epsilon = 1e-9); // 11/10 - 1
        assert_relative_eq!(r.net, 0.099, epsilon = 1e-9);
        assert!(r.t1_executable);
    }

    #[test]
    fn flat_is_zero_and_out_of_range_is_none() {
        let c = CostModel { round_trip_bps: 10.0 };
        let rf = forward_return(&data(), 0, 2, Stance::Flat, &c).unwrap();
        assert_eq!(rf.net, 0.0);
        assert_eq!(rf.gross, 0.0);
        assert!(forward_return(&data(), 1, 2, Stance::Long, &c).is_none());
    }
}
```

- [ ] **Step 6: 运行 forward_return 验证失败**

Run: `cargo test --lib forward_return`
Expected: 编译失败（`forward_return` / `ForwardResult` 未定义）。

- [ ] **Step 7: 写 forward_return 实现**

测试上方：
```rust
use crate::backtest::costs::CostModel;
use crate::data::bar::Bar;
use crate::tree::schema::Stance;

#[derive(Debug, Clone, Copy)]
pub struct ForwardResult {
    pub gross: f64,
    pub net: f64,
    pub t1_executable: bool,
}

/// 决策在 bar i（收盘）。入场 = bar[i+1] 开盘；出场 = bar[i+n] 收盘（持有 n 根）。
/// i+n 越界返回 None。flat 收益 0、无成本。T+1：出场日 > 入场日 才算可执行。
pub fn forward_return(
    primary: &[Bar],
    i: usize,
    n: usize,
    stance: Stance,
    costs: &CostModel,
) -> Option<ForwardResult> {
    if n == 0 {
        return None;
    }
    let entry_idx = i + 1;
    let exit_idx = i + n;
    if exit_idx >= primary.len() {
        return None;
    }
    let entry = primary[entry_idx].open;
    let exit = primary[exit_idx].close;
    if entry <= 0.0 {
        return None;
    }
    let dir = match stance {
        Stance::Long => 1.0,
        Stance::Short => -1.0,
        Stance::Flat => 0.0,
    };
    let gross = (exit / entry - 1.0) * dir;
    let net = if dir == 0.0 { 0.0 } else { costs.apply(gross) };
    let t1_executable = primary[exit_idx].time.date() > primary[entry_idx].time.date();
    Some(ForwardResult { gross, net, t1_executable })
}
```

- [ ] **Step 8: 运行 forward_return 验证通过**

Run: `cargo test --lib forward_return`
Expected: 两个测试 PASS。

- [ ] **Step 9: Commit**

```bash
git add src/backtest/costs.rs src/backtest/forward_return.rs
git commit -m "feat(backtest): cost model and look-ahead-safe forward return with T+1 flag"
```

---

## Task 16: backtest/metrics.rs — 度量聚合

**Files:**
- Create: `src/backtest/metrics.rs`
- Test: 同文件

用 `BTreeMap` 保证输出键序确定（复现性）。

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::backtest::forward_return::ForwardResult;
    use crate::engine::trace::{StepRecord, Trace};
    use crate::tree::schema::Stance;
    use chrono::NaiveDate;

    fn trace(leaf: &str, stance: Stance) -> Trace {
        let t = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        Trace {
            t,
            path: vec![StepRecord { node_id: "a".into(), label: "up".into(), confidence: 1.0, rationale: "".into() }],
            leaf: leaf.into(),
            stance,
        }
    }

    #[test]
    fn aggregates_active_leaf_and_node_stats() {
        let items = vec![
            (trace("leaf_l", Stance::Long), Some(ForwardResult { gross: 0.05, net: 0.04, t1_executable: true })),
            (trace("leaf_l", Stance::Long), Some(ForwardResult { gross: -0.02, net: -0.03, t1_executable: false })),
            (trace("leaf_f", Stance::Flat), Some(ForwardResult { gross: 0.0, net: 0.0, t1_executable: false })),
        ];
        let primary = vec![];
        let m = compute_metrics(&items, &primary);
        assert_eq!(m.total_decisions, 3);
        assert_eq!(m.scored, 3);
        assert_eq!(m.active.count, 2); // 两个 long；flat 不计入 active
        assert_eq!(m.t1_executable.count, 1);
        assert_eq!(m.by_leaf.get("leaf_l").unwrap().count, 2);
        assert_eq!(*m.node_label_counts.get("a::up").unwrap(), 3);
    }
}
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test --lib metrics`
Expected: 编译失败（`compute_metrics` / `Metrics` 未定义）。

- [ ] **Step 3: 写实现**

```rust
use crate::backtest::forward_return::ForwardResult;
use crate::data::bar::Bar;
use crate::engine::trace::Trace;
use crate::tree::schema::Stance;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Serialize)]
pub struct SignalStat {
    pub count: usize,
    pub mean_net: f64,
    pub hit_rate: f64,
    pub std: f64,
    pub t_stat: f64,
}

#[derive(Debug, Serialize)]
pub struct Metrics {
    pub total_decisions: usize,
    pub scored: usize,
    pub active: SignalStat,
    pub t1_executable: SignalStat,
    pub by_leaf: BTreeMap<String, SignalStat>,
    pub by_stance: BTreeMap<String, SignalStat>,
    pub node_label_counts: BTreeMap<String, usize>,
    pub buy_and_hold: f64,
    pub overlap_warning: String,
}

fn signal_stat(nets: &[f64]) -> SignalStat {
    let count = nets.len();
    if count == 0 {
        return SignalStat { count: 0, mean_net: 0.0, hit_rate: 0.0, std: 0.0, t_stat: 0.0 };
    }
    let mean = nets.iter().sum::<f64>() / count as f64;
    let var = nets.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / count as f64;
    let std = var.sqrt();
    let wins = nets.iter().filter(|x| **x > 0.0).count();
    let hit_rate = wins as f64 / count as f64;
    let t_stat = if std == 0.0 { 0.0 } else { mean / (std / (count as f64).sqrt()) };
    SignalStat { count, mean_net: mean, hit_rate, std, t_stat }
}

pub fn compute_metrics(items: &[(Trace, Option<ForwardResult>)], primary: &[Bar]) -> Metrics {
    let total = items.len();
    let mut active_nets: Vec<f64> = Vec::new();
    let mut t1_nets: Vec<f64> = Vec::new();
    let mut by_leaf: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut by_stance: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut node_label_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut scored = 0;

    for (trace, fr) in items {
        for step in &trace.path {
            *node_label_counts
                .entry(format!("{}::{}", step.node_id, step.label))
                .or_insert(0) += 1;
        }
        if let Some(fr) = fr {
            scored += 1;
            let stance_name = format!("{:?}", trace.stance).to_lowercase();
            by_leaf.entry(trace.leaf.clone()).or_default().push(fr.net);
            by_stance.entry(stance_name).or_default().push(fr.net);
            if !matches!(trace.stance, Stance::Flat) {
                active_nets.push(fr.net);
                if fr.t1_executable {
                    t1_nets.push(fr.net);
                }
            }
        }
    }

    let buy_and_hold = if primary.len() >= 2 {
        primary.last().unwrap().close / primary[0].open - 1.0
    } else {
        0.0
    };

    Metrics {
        total_decisions: total,
        scored,
        active: signal_stat(&active_nets),
        t1_executable: signal_stat(&t1_nets),
        by_leaf: by_leaf.iter().map(|(k, v)| (k.clone(), signal_stat(v))).collect(),
        by_stance: by_stance.iter().map(|(k, v)| (k.clone(), signal_stat(v))).collect(),
        node_label_counts,
        buy_and_hold,
        overlap_warning: "前瞻窗口重叠 → 样本自相关，t 值偏乐观，勿据此鼓吹显著性".into(),
    }
}
```

- [ ] **Step 4: 运行验证通过**

Run: `cargo test --lib metrics`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add src/backtest/metrics.rs
git commit -m "feat(backtest): metrics aggregation (active/T+1/leaf/stance/node, baseline)"
```

---

## Task 17: report/mod.rs — 报告（JSON + Trace JSONL + 摘要）

**Files:**
- Modify: `src/report/mod.rs`（替换 Task 1 的占位注释）
- Test: 同文件

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::backtest::metrics::compute_metrics;

    #[test]
    fn report_serializes_to_json() {
        let metrics = compute_metrics(&[], &[]);
        let report = Report { tree_name: "t".into(), forward_window: 16, cost_bps: 10.0, metrics };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"tree_name\":\"t\""));
        assert!(json.contains("overlap_warning"));
    }

    #[test]
    fn traces_jsonl_writes_one_line_per_trace() {
        use crate::engine::trace::Trace;
        use crate::tree::schema::Stance;
        use chrono::NaiveDate;
        let t = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        let traces = vec![
            Trace { t, path: vec![], leaf: "x".into(), stance: Stance::Flat },
            Trace { t, path: vec![], leaf: "y".into(), stance: Stance::Long },
        ];
        let f = tempfile::NamedTempFile::new().unwrap();
        write_traces_jsonl(&traces, f.path()).unwrap();
        let content = std::fs::read_to_string(f.path()).unwrap();
        assert_eq!(content.lines().count(), 2);
    }
}
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test --lib report`
Expected: 编译失败（`Report` / `write_traces_jsonl` 未定义）。

- [ ] **Step 3: 写实现**

```rust
use crate::backtest::metrics::Metrics;
use crate::engine::trace::Trace;
use crate::Result;
use serde::Serialize;
use std::io::Write;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct Report {
    pub tree_name: String,
    pub forward_window: usize,
    pub cost_bps: f64,
    pub metrics: Metrics,
}

pub fn write_report(report: &Report, path: &Path) -> Result<()> {
    let json = serde_json::to_string_pretty(report)?;
    std::fs::write(path, json)?;
    Ok(())
}

pub fn write_traces_jsonl(traces: &[Trace], path: &Path) -> Result<()> {
    let mut f = std::fs::File::create(path)?;
    for t in traces {
        let line = serde_json::to_string(t)?;
        writeln!(f, "{line}")?;
    }
    Ok(())
}

pub fn print_summary(report: &Report) {
    let m = &report.metrics;
    println!("=== rquant backtest: {} ===", report.tree_name);
    println!("forward_window={} cost_bps={}", report.forward_window, report.cost_bps);
    println!("decisions={} scored={}", m.total_decisions, m.scored);
    println!(
        "active  : n={} mean_net={:.4} hit={:.1}% t={:.2}",
        m.active.count,
        m.active.mean_net,
        m.active.hit_rate * 100.0,
        m.active.t_stat
    );
    println!(
        "T+1 exec: n={} mean_net={:.4} hit={:.1}%",
        m.t1_executable.count,
        m.t1_executable.mean_net,
        m.t1_executable.hit_rate * 100.0
    );
    println!("buy&hold={:.4}", m.buy_and_hold);
    println!("[warn] {}", m.overlap_warning);
}
```

- [ ] **Step 4: 运行验证通过**

Run: `cargo test --lib report`
Expected: 两个测试 PASS。

- [ ] **Step 5: Commit**

```bash
git add src/report/mod.rs
git commit -m "feat(report): JSON report, trace JSONL, and text summary"
```

---

## Task 18: backtest/runner.rs + cli/mod.rs + examples/trend_tree.yaml — 端到端编排与 CLI

**Files:**
- Create: `src/backtest/runner.rs`
- Modify: `src/cli/mod.rs`（替换 Task 1 的桩）
- Create: `examples/trend_tree.yaml`
- Test: `runner.rs` 同文件（加载示例树）

- [ ] **Step 1: 写示例树 examples/trend_tree.yaml**

```yaml
meta:
  name: "我的A股趋势树"
  forward_window: 16          # 前瞻 16 根 15m ≈ 1 交易日（满足 T+1）
  stances: [long, flat]       # 做空仅信息、默认不计盈亏
root: trend
nodes:
  trend:                      # 量化节点：大周期趋势
    type: quant
    branches:
      - when: "ema(ctx.close,20) > ema(ctx.close,50) and slope(ema(ctx.close,20),5) > 0"
        goto: pullback
        label: up
      - when: "ema(ctx.close,20) < ema(ctx.close,50) and slope(ema(ctx.close,20),5) < 0"
        goto: leaf_avoid
        label: down
    default: { goto: leaf_flat, label: none }
  pullback:                   # 量化节点：小周期回调到位?
    type: quant
    branches:
      - when: "rsi(close,14) < 35 and close > sma(close,60)"
        goto: news_check
        label: yes
    default: { goto: leaf_flat, label: no }
  news_check:                 # LLM 节点：本阶段走 default（M5 接入）
    type: llm
    inputs: [news_score, recent_headlines]
    prompt: "给定消息面因子与标题，判断是否存在压制性重大利空。"
    labels:
      clear: leaf_buy
      risk: leaf_flat
    default: leaf_flat
leaves:
  leaf_buy:   { stance: long }
  leaf_flat:  { stance: flat }
  leaf_avoid: { stance: flat }
```

- [ ] **Step 2: 写 runner 失败测试**

`src/backtest/runner.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn example_tree_loads_and_validates() {
        let tree = crate::tree::loader::load_tree_file(Path::new("examples/trend_tree.yaml")).unwrap();
        assert_eq!(tree.root, "trend");
        assert_eq!(tree.nodes.len(), 3);
        assert_eq!(tree.leaves.len(), 3);
    }
}
```

- [ ] **Step 3: 运行验证失败**

Run: `cargo test --lib runner`
Expected: 编译失败（`runner` 模块内容缺失 / `BacktestConfig` 未定义）。

- [ ] **Step 4: 写 runner 实现**

测试上方：
```rust
use crate::backtest::costs::CostModel;
use crate::backtest::forward_return::{forward_return, ForwardResult};
use crate::backtest::metrics::compute_metrics;
use crate::engine::trace::Trace;
use crate::features::context::build_context;
use crate::report::Report;
use crate::Result;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct BacktestConfig {
    pub tree_path: PathBuf,
    pub primary_path: PathBuf,
    pub context_path: PathBuf,
    pub out_path: PathBuf,
    pub traces_path: Option<PathBuf>,
    pub cost_bps: f64,
    pub warmup: usize,
    pub window: usize,
}

/// 端到端：加载树+数据 → 逐时点遍历 → 前瞻收益 → 度量 → 写报告。返回 Report。
pub fn run(cfg: &BacktestConfig) -> Result<Report> {
    let tree = crate::tree::loader::load_tree_file(&cfg.tree_path)?;
    let primary = crate::data::reader::read_bars_csv(&cfg.primary_path)?;
    let context = crate::data::reader::read_bars_csv(&cfg.context_path)?;
    let costs = CostModel { round_trip_bps: cfg.cost_bps };

    let mut items: Vec<(Trace, Option<ForwardResult>)> = Vec::new();
    let mut traces: Vec<Trace> = Vec::new();
    let start = cfg.warmup.min(primary.len());
    for i in start..primary.len() {
        let t = primary[i].time;
        let ctx = build_context(&primary, &context, t, cfg.window);
        let trace = crate::engine::traversal::traverse(&tree, &ctx)?;
        let fr = forward_return(&primary, i, tree.meta.forward_window, trace.stance, &costs);
        traces.push(trace.clone());
        items.push((trace, fr));
    }

    let metrics = compute_metrics(&items, &primary);
    let report = Report {
        tree_name: tree.meta.name.clone(),
        forward_window: tree.meta.forward_window,
        cost_bps: cfg.cost_bps,
        metrics,
    };
    crate::report::write_report(&report, &cfg.out_path)?;
    if let Some(tp) = &cfg.traces_path {
        crate::report::write_traces_jsonl(&traces, tp)?;
    }
    Ok(report)
}
```

- [ ] **Step 5: 写 cli 实现（替换 Task 1 的桩）**

`src/cli/mod.rs`:
```rust
use crate::backtest::runner::{run, BacktestConfig};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "rquant", about = "Fuzzy decision-tree A-share backtester")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run a quant backtest over local CSV bars
    Backtest {
        #[arg(long)]
        tree: PathBuf,
        #[arg(long)]
        primary: PathBuf,
        #[arg(long)]
        context: PathBuf,
        #[arg(long, default_value = "report.json")]
        out: PathBuf,
        #[arg(long)]
        traces: Option<PathBuf>,
        #[arg(long, default_value_t = 10.0)]
        cost_bps: f64,
        #[arg(long, default_value_t = 100)]
        warmup: usize,
        #[arg(long, default_value_t = 100)]
        window: usize,
    },
}

pub fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Backtest { tree, primary, context, out, traces, cost_bps, warmup, window } => {
            let cfg = BacktestConfig {
                tree_path: tree,
                primary_path: primary,
                context_path: context,
                out_path: out,
                traces_path: traces,
                cost_bps,
                warmup,
                window,
            };
            let report = run(&cfg)?;
            crate::report::print_summary(&report);
        }
    }
    Ok(())
}
```

- [ ] **Step 6: 运行验证通过 + 构建**

Run: `cargo test --lib runner`
Expected: `example_tree_loads_and_validates` PASS。

Run: `cargo build`
Expected: 编译通过（CLI 不再是桩）。

Run: `cargo run -- backtest --help`
Expected: 打印 backtest 子命令用法（含 --tree/--primary/--context 等）。

- [ ] **Step 7: Commit**

```bash
git add src/backtest/runner.rs src/cli/mod.rs examples/trend_tree.yaml
git commit -m "feat(cli): end-to-end backtest runner and clap CLI; add example tree"
```

---

## Task 19: tests/e2e.rs — 端到端集成测试（合成上升趋势）

**Files:**
- Create: `tests/e2e.rs`

构造严格上升、跨多日的合成 K 线 + 一棵小树（`close > sma(close,5)` → 看多），断言：有评分信号、看多在上升趋势中成本后均值为正、存在 T+1 可执行样本、报告文件写出。

- [ ] **Step 1: 写测试**

```rust
use rquant::backtest::runner::{run, BacktestConfig};
use std::io::Write;

fn write_file(content: &str, suffix: &str) -> tempfile::NamedTempFile {
    let mut f = tempfile::Builder::new().suffix(suffix).tempfile().unwrap();
    write!(f, "{content}").unwrap();
    f.flush().unwrap();
    f
}

fn tree_yaml() -> String {
    r#"
meta: { name: e2e, forward_window: 2, stances: [long, flat] }
root: entry
nodes:
  entry:
    type: quant
    branches: [ { when: "close > sma(close,5)", goto: leaf_long, label: above } ]
    default: { goto: leaf_flat, label: below }
leaves:
  leaf_long: { stance: long }
  leaf_flat: { stance: flat }
"#
    .to_string()
}

fn gen_primary_csv() -> String {
    // 5 个交易日 × 每日 8 根 15m bar；价格全局严格上升。
    // 时间戳仅需严格递增且跨日（reader 不校验交易时段）。
    let mut s = String::from("time,open,high,low,close,volume\n");
    let mut idx = 0;
    for day in 0..5 {
        for k in 0..8 {
            let price = 10.0 + 0.1 * idx as f64;
            let hour = 9 + (45 + k * 15) / 60;
            let minute = (45 + k * 15) % 60;
            s.push_str(&format!(
                "2024-01-{:02} {:02}:{:02}:00,{p},{p},{p},{p},1000\n",
                2 + day,
                hour,
                minute,
                p = price
            ));
            idx += 1;
        }
    }
    s
}

fn gen_context_csv() -> String {
    // 本测试的树不引用 ctx.*，给几根占位即可。
    String::from(
        "time,open,high,low,close,volume\n\
         2024-01-02 10:30:00,10.0,10.0,10.0,10.0,1\n\
         2024-01-02 11:30:00,10.1,10.1,10.1,10.1,1\n\
         2024-01-03 10:30:00,10.2,10.2,10.2,10.2,1\n",
    )
}

#[test]
fn end_to_end_uptrend_yields_positive_long_edge() {
    let tree_f = write_file(&tree_yaml(), ".yaml");
    let primary_f = write_file(&gen_primary_csv(), ".csv");
    let context_f = write_file(&gen_context_csv(), ".csv");
    let out_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();

    let cfg = BacktestConfig {
        tree_path: tree_f.path().to_path_buf(),
        primary_path: primary_f.path().to_path_buf(),
        context_path: context_f.path().to_path_buf(),
        out_path: out_f.path().to_path_buf(),
        traces_path: None,
        cost_bps: 10.0,
        warmup: 5,
        window: 100,
    };

    let report = run(&cfg).unwrap();
    let m = &report.metrics;
    assert!(m.scored > 0, "should have scored signals");
    assert!(m.active.count > 0, "uptrend should trigger long signals");
    assert!(m.active.mean_net > 0.0, "long edge in an uptrend should be positive after costs");
    assert!(m.t1_executable.count > 0, "some signals should cross a day boundary (T+1)");
    assert!(m.buy_and_hold > 0.0);

    let content = std::fs::read_to_string(out_f.path()).unwrap();
    assert!(content.contains("e2e"));
}
```

- [ ] **Step 2: 运行验证通过**

Run: `cargo test --test e2e`
Expected: `end_to_end_uptrend_yields_positive_long_edge` PASS。

- [ ] **Step 3: 全量回归**

Run: `cargo test`
Expected: 所有单元测试 + e2e 全 PASS。

Run: `cargo build --release`
Expected: release 构建通过。

- [ ] **Step 4: Commit**

```bash
git add tests/e2e.rs
git commit -m "test: end-to-end backtest on synthetic uptrend data"
```

---

## 附录 A：Spec 覆盖对照（自检）

| Spec 章节 | 实现于 |
|---|---|
| §5 七层架构 / §13 crate 结构 | Task 1（骨架）+ 各层 Task |
| §6 核心抽象（Context/Decision/Trace）| Task 7 / Task 1+13 / Task 14 |
| §7 决策树 Schema + DSL | Task 8–12 |
| §7.3 DSL 函数集 v1（sma/ema/rsi/atr/slope/highest/lowest/crossover/crossunder）| Task 5–6, 10 |
| §7.4 加载期校验（引用/可达/DAG/stance）| Task 12 |
| §8 A股规则（立场默认 long/flat、T+1 标记、成本 haircut）| Task 12/15（成本+T+1）/示例树 Task 18 |
| §9 防未来函数（Context 闸门 + 两遍）| Task 7（partition_point + 属性测试）+ Task 18 runner |
| §10 度量（按叶子/节点/整体 + 买入持有 + 重叠警告）| Task 16 |
| §11 错误处理（加载报错、预热不足弃权走 default、越界不计分）| Task 12 / Task 10+13 / Task 15 |
| §12 测试（DSL/指标/防未来函数/复现性/集成）| Task 5–10 / Task 7 / Task 19 |
| §14 里程碑 M1–M4 | 本计划全部任务 |

## 附录 B：明确不在本计划范围（YAGNI / 后置）

- **M5**：LLMEvaluator + 缓存（本阶段 LLM 节点走 `default`，见 Task 14）。
- **M6**：新浪 fetcher（本阶段手动丢 CSV；缓存层 Parquet/SQLite 一并后置）。
- **DSL**：`wma`（暂用 sma 占位）、`macd_*`、`std` 未实现——有树需要时再加，函数派发处一行可扩展。
- **度量**：随机分支基准、样本内/外自动切分——用户可对不同日期区间分别跑 CLI 获得内/外对比，无需额外代码。
- **复现性逐字节测试**：纯量化路径本就确定（无随机、无 LLM）；待 M5 引入 LLM 缓存后再加“跑两遍逐字节相同”测试。
- **LLM 节点目标校验已就绪**（loader 校验 labels 目标），但运行时不调用 LLM。
- **AShareCalendar（Task 3）本阶段独立交付并测试，但暂不接入** runner：因为 reader 只读已是交易 bar 的 CSV，“前瞻 N 根”天然按交易 bar 计数，T+1 判定直接用 bar 日期即可。日历留给 M6（缺口检测）与未来的时段感知逻辑。

> **复现性现状**：M1–M4 全程无随机源、无浮点非确定操作、输出用 `BTreeMap` 定序，因此同输入→同输出天然成立；e2e 可重复运行得到相同 Report。

