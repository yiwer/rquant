# rquant 报告可视化 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新增 `rquant report` 子命令，把 `report.json`(+可选 `traces.jsonl`+primary CSV) 渲染成自包含 HTML（内联手写 SVG：累计收益曲线、净收益直方图、by_leaf/节点条形、headline 表）。

**Architecture:** 在 master(HEAD `3646c37`)上扩展，纯消费者。给已序列化类型补 `Deserialize` 以读回 JSON；`report/curve.rs` 用现有 `forward_return` 重算逐点 net；`report/viz.rs` 手写 SVG/HTML；cli 加 `report` 分支。零新依赖、回测引擎/格式不改。

**Tech Stack:** Rust 2024 + 既有（serde/serde_json/chrono）。

> 设计依据：`docs/superpowers/specs/2026-06-09-rquant-report-viz-design.md`。提交信息用英文。`Stance` 已 derive Deserialize（无需改）。

---

## 文件结构
```
改动: src/report/mod.rs           # Report +Deserialize；+ pub mod curve; pub mod viz; + 往返测试
改动: src/backtest/metrics.rs     # Metrics, SignalStat +Deserialize
改动: src/backtest/gaps.rs        # GapReport, PartialDay +Deserialize
改动: src/engine/trace.rs         # Trace, StepRecord +Deserialize
新增: src/report/curve.rs         # derive_series（重算逐点 net + 累计 + 直方图）
新增: src/report/viz.rs           # line_chart/bar_chart/histogram_svg/render_html
改动: src/cli/mod.rs              # Cmd::Report 子命令
改动: tests/e2e.rs                # 可视化全链路 e2e
改动: README.md                   # report 子命令一节
```

---

## Task 1: 给报告类型补 `Deserialize`

**Files:**
- Modify: `src/report/mod.rs`, `src/backtest/metrics.rs`, `src/backtest/gaps.rs`, `src/engine/trace.rs`
- Test: `src/report/mod.rs`（往返测试）

- [ ] **Step 1: 在 `src/report/mod.rs` 的 `mod tests` 追加失败测试**

```rust
    #[test]
    fn report_round_trips_json() {
        let metrics = compute_metrics(&[], &[]);
        let report = Report { tree_name: "rt".into(), forward_window: 8, cost_bps: 5.0, metrics, gaps: GapReport::default() };
        let json = serde_json::to_string(&report).unwrap();
        let back: Report = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tree_name, "rt");
        assert_eq!(back.forward_window, 8);
        assert_eq!(back.metrics.total_decisions, 0);
    }

    #[test]
    fn trace_round_trips_json() {
        use crate::engine::trace::Trace;
        use crate::tree::schema::Stance;
        use chrono::NaiveDate;
        let t = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        let tr = Trace { t, path: vec![], leaf: "x".into(), stance: Stance::Long };
        let json = serde_json::to_string(&tr).unwrap();
        let back: Trace = serde_json::from_str(&json).unwrap();
        assert_eq!(back.leaf, "x");
        assert_eq!(back.stance, Stance::Long);
    }
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test --lib report::tests::report_round_trips_json`
Expected: 编译失败（`Report`/`Trace` 未实现 `Deserialize`）。

- [ ] **Step 3: 加 `Deserialize` derive**

(a) `src/report/mod.rs`: `use serde::Serialize;` → `use serde::{Deserialize, Serialize};`；`Report` 的 `#[derive(Debug, Serialize)]` → `#[derive(Debug, Serialize, Deserialize)]`（**仅 `Report`**；`SoftReport` 不动）。
(b) `src/backtest/metrics.rs`: `use serde::Serialize;` → `use serde::{Deserialize, Serialize};`；`SignalStat` 与 `Metrics` 的 derive 各加 `Deserialize`。
(c) `src/backtest/gaps.rs`: 把 `use serde::Serialize;` 改为 `use serde::{Deserialize, Serialize};`；`PartialDay` 的 `#[derive(Debug, Clone, Serialize)]` → `+ Deserialize`；`GapReport` 的 `#[derive(Debug, Clone, Default, Serialize)]` → `+ Deserialize`。
(d) `src/engine/trace.rs`: `use serde::Serialize;` → `use serde::{Deserialize, Serialize};`；`StepRecord` 与 `Trace` 的 derive 各加 `Deserialize`。

- [ ] **Step 4: 运行验证通过**

Run: `cargo test --lib report`
Expected: 既有 report 测试 + 2 个往返测试全 PASS。
Run: `cargo build`
Expected: 通过。

- [ ] **Step 5: Commit**

```bash
git add src/report/mod.rs src/backtest/metrics.rs src/backtest/gaps.rs src/engine/trace.rs
git commit -m "feat(report): derive Deserialize for Report/Metrics/Trace types (read back for viz)" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: report/curve.rs — derive_series

**Files:**
- Create: `src/report/curve.rs`
- Modify: `src/report/mod.rs`（+ `pub mod curve;`）
- Test: 同文件

- [ ] **Step 1: `src/report/mod.rs` 顶部加 `pub mod curve;`**

- [ ] **Step 2: 写失败测试（`src/report/curve.rs`）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::trace::Trace;
    use crate::tree::schema::Stance;
    use chrono::NaiveDateTime;

    fn bar(t: &str, open: f64, close: f64) -> Bar {
        Bar { time: NaiveDateTime::parse_from_str(t, "%Y-%m-%d %H:%M:%S").unwrap(),
              open, high: open.max(close), low: open.min(close), close, volume: 1.0 }
    }
    fn trace(t: &str, stance: Stance) -> Trace {
        Trace { t: NaiveDateTime::parse_from_str(t, "%Y-%m-%d %H:%M:%S").unwrap(),
                path: vec![], leaf: "l".into(), stance }
    }

    #[test]
    fn derive_series_cumulates_and_skips() {
        let primary = vec![
            bar("2024-01-02 09:45:00", 9.0, 9.0),
            bar("2024-01-02 10:00:00", 10.0, 10.0),
            bar("2024-01-02 10:15:00", 11.0, 11.0),
        ];
        let cost = CostModel { round_trip_bps: 0.0 };
        // decision at bar 0 (long): entry=bar1.open=10, exit=bar2.close=11 → net=0.1
        // decision at bar 2: out of range (i+1=3) → skipped
        let traces = vec![trace("2024-01-02 09:45:00", Stance::Long), trace("2024-01-02 10:15:00", Stance::Long)];
        let es = derive_series(&traces, &primary, 1, &cost);
        assert_eq!(es.points.len(), 1);
        assert!((es.points[0].net - 0.1).abs() < 1e-9);
        assert!((es.points[0].cum - 0.1).abs() < 1e-9);
        assert_eq!(es.skipped, 1);
    }

    #[test]
    fn derive_series_skips_unmatched_time() {
        let primary = vec![bar("2024-01-02 09:45:00", 9.0, 9.0), bar("2024-01-02 10:00:00", 10.0, 10.0)];
        let cost = CostModel { round_trip_bps: 0.0 };
        let traces = vec![trace("2099-01-01 00:00:00", Stance::Long)];
        let es = derive_series(&traces, &primary, 1, &cost);
        assert_eq!(es.points.len(), 0);
        assert_eq!(es.skipped, 1);
    }
}
```

- [ ] **Step 3: 运行验证失败**

Run: `cargo test --lib report::curve`
Expected: 编译失败（`derive_series`/`EquitySeries` 未定义）。

- [ ] **Step 4: 写实现（测试上方）**

```rust
use crate::backtest::costs::CostModel;
use crate::backtest::forward_return::forward_return;
use crate::data::bar::Bar;
use crate::engine::trace::Trace;
use chrono::NaiveDateTime;
use std::collections::HashMap;

pub struct SeriesPoint {
    pub t: NaiveDateTime,
    pub net: f64,
    pub cum: f64,
}

pub struct Histogram {
    /// (lo, hi, count) 桶
    pub bins: Vec<(f64, f64, usize)>,
}

pub struct EquitySeries {
    pub points: Vec<SeriesPoint>,
    pub hist: Histogram,
    pub skipped: usize,
}

/// 逐点重算 net：按 trace.t 定位 primary bar，forward_return(stance) → net，累加 cum。
/// 找不到 bar / 越界 → 跳过并计 skipped。
pub fn derive_series(traces: &[Trace], primary: &[Bar], fw: usize, cost: &CostModel) -> EquitySeries {
    let index: HashMap<NaiveDateTime, usize> = primary.iter().enumerate().map(|(i, b)| (b.time, i)).collect();
    let mut points = Vec::new();
    let mut skipped = 0usize;
    let mut cum = 0.0;
    for tr in traces {
        let Some(&i) = index.get(&tr.t) else {
            skipped += 1;
            continue;
        };
        match forward_return(primary, i, fw, tr.stance, cost) {
            Some(fr) => {
                cum += fr.net;
                points.push(SeriesPoint { t: tr.t, net: fr.net, cum });
            }
            None => skipped += 1,
        }
    }
    let hist = histogram(&points);
    EquitySeries { points, hist, skipped }
}

fn histogram(points: &[SeriesPoint]) -> Histogram {
    if points.is_empty() {
        return Histogram { bins: vec![] };
    }
    let nets: Vec<f64> = points.iter().map(|p| p.net).collect();
    let min = nets.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = nets.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    if (max - min).abs() < 1e-12 {
        return Histogram { bins: vec![(min, max, nets.len())] };
    }
    const N: usize = 21;
    let width = (max - min) / N as f64;
    let mut counts = vec![0usize; N];
    for &x in &nets {
        let mut k = ((x - min) / width) as usize;
        if k >= N {
            k = N - 1;
        }
        counts[k] += 1;
    }
    let bins = (0..N).map(|k| (min + k as f64 * width, min + (k + 1) as f64 * width, counts[k])).collect();
    Histogram { bins }
}
```

- [ ] **Step 5: 运行验证通过**

Run: `cargo test --lib report::curve`
Expected: 2 个测试 PASS。

- [ ] **Step 6: Commit**

```bash
git add src/report/curve.rs src/report/mod.rs
git commit -m "feat(report): derive_series recomputes per-decision net + cumulative + histogram" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: report/viz.rs — SVG + render_html

**Files:**
- Create: `src/report/viz.rs`
- Modify: `src/report/mod.rs`（+ `pub mod viz;`）
- Test: 同文件

- [ ] **Step 1: `src/report/mod.rs` 顶部加 `pub mod viz;`**

- [ ] **Step 2: 写失败测试（`src/report/viz.rs`）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::backtest::metrics::compute_metrics;
    use crate::backtest::gaps::GapReport;
    use crate::report::curve::{EquitySeries, Histogram, SeriesPoint};
    use chrono::NaiveDate;

    fn sample_report() -> Report {
        let metrics = compute_metrics(&[], &[]);
        Report { tree_name: "viz".into(), forward_window: 8, cost_bps: 10.0, metrics, gaps: GapReport::default() }
    }

    #[test]
    fn line_chart_has_polyline() {
        let pts = vec![(0.0, 0.0), (1.0, 0.5), (2.0, 0.3)];
        let svg = line_chart(&pts, "t");
        assert!(svg.contains("<svg"));
        assert!(svg.contains("<polyline"));
    }

    #[test]
    fn bar_chart_has_rect() {
        let items = vec![("a".to_string(), 0.2), ("b".to_string(), -0.1)];
        let svg = bar_chart(&items, "t");
        assert!(svg.contains("<rect"));
    }

    #[test]
    fn render_html_is_self_contained_and_deterministic() {
        let report = sample_report();
        let t = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        let es = EquitySeries {
            points: vec![SeriesPoint { t, net: 0.1, cum: 0.1 }],
            hist: Histogram { bins: vec![(0.0, 0.1, 1)] },
            skipped: 0,
        };
        let a = render_html(&report, Some(&es));
        let b = render_html(&report, Some(&es));
        assert_eq!(a, b); // 确定性
        assert!(a.contains("<!doctype html>"));
        assert!(a.contains("viz")); // tree_name
        assert!(a.contains("<svg"));
        assert!(a.contains(&report.metrics.overlap_warning));
    }
}
```

- [ ] **Step 3: 运行验证失败**

Run: `cargo test --lib report::viz`
Expected: 编译失败（`line_chart`/`bar_chart`/`render_html` 未定义）。

- [ ] **Step 4: 写实现（测试上方）**

```rust
use crate::report::Report;
use crate::report::curve::{EquitySeries, Histogram};
use std::fmt::Write;

const W: u32 = 640;
const H: u32 = 240;

fn ny(v: f64, lo: f64, hi: f64, pad: f64) -> f64 {
    let span = if (hi - lo).abs() < 1e-12 { 1.0 } else { hi - lo };
    (H as f64 - pad) - (v - lo) / span * (H as f64 - 2.0 * pad)
}

/// 折线图：points 为 (x_index, y) 序列。
pub fn line_chart(points: &[(f64, f64)], title: &str) -> String {
    let pad = 30.0;
    let mut s = String::new();
    let _ = write!(s, "<svg width=\"{W}\" height=\"{H}\" xmlns=\"http://www.w3.org/2000/svg\">");
    let _ = write!(s, "<text x=\"8\" y=\"16\" font-size=\"13\">{title}</text>");
    if points.is_empty() {
        let _ = write!(s, "<text x=\"8\" y=\"{}\">no data</text></svg>", H / 2);
        return s;
    }
    let mut ymin = points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let mut ymax = points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    ymin = ymin.min(0.0);
    ymax = ymax.max(0.0);
    let n = points.len().max(2);
    let px = |i: usize| pad + i as f64 / (n - 1) as f64 * (W as f64 - 2.0 * pad);
    let y0 = ny(0.0, ymin, ymax, pad);
    let _ = write!(s, "<line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"#ccc\"/>", pad, y0, W as f64 - pad, y0);
    let pts: Vec<String> = points.iter().enumerate().map(|(i, p)| format!("{:.1},{:.1}", px(i), ny(p.1, ymin, ymax, pad))).collect();
    let _ = write!(s, "<polyline fill=\"none\" stroke=\"#1565c0\" stroke-width=\"1.5\" points=\"{}\"/>", pts.join(" "));
    let _ = write!(s, "<text x=\"{:.0}\" y=\"{:.0}\" font-size=\"10\">{:.3}</text>", pad, pad, ymax);
    let _ = write!(s, "<text x=\"{:.0}\" y=\"{:.0}\" font-size=\"10\">{:.3}</text>", pad, H as f64 - pad + 8.0, ymin);
    let _ = write!(s, "</svg>");
    s
}

/// 条形图：items 为 (label, value)，正绿负红。
pub fn bar_chart(items: &[(String, f64)], title: &str) -> String {
    let pad = 30.0;
    let mut s = String::new();
    let _ = write!(s, "<svg width=\"{W}\" height=\"{H}\" xmlns=\"http://www.w3.org/2000/svg\">");
    let _ = write!(s, "<text x=\"8\" y=\"16\" font-size=\"13\">{title}</text>");
    if items.is_empty() {
        let _ = write!(s, "<text x=\"8\" y=\"{}\">no data</text></svg>", H / 2);
        return s;
    }
    let maxabs = items.iter().map(|(_, v)| v.abs()).fold(0.0_f64, f64::max).max(1e-12);
    let n = items.len();
    let bw = (W as f64 - 2.0 * pad) / n as f64;
    let y0 = H as f64 / 2.0;
    let _ = write!(s, "<line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"#ccc\"/>", pad, y0, W as f64 - pad, y0);
    for (i, (label, v)) in items.iter().enumerate() {
        let x = pad + i as f64 * bw + bw * 0.15;
        let bh = (v.abs() / maxabs) * (H as f64 / 2.0 - pad);
        let (y, color) = if *v >= 0.0 { (y0 - bh, "#2e7d32") } else { (y0, "#c62828") };
        let _ = write!(s, "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" fill=\"{}\"/>", x, y, bw * 0.7, bh, color);
        let _ = write!(s, "<text x=\"{:.1}\" y=\"{:.0}\" font-size=\"9\" text-anchor=\"middle\">{}</text>", x + bw * 0.35, H as f64 - 6.0, label);
    }
    let _ = write!(s, "</svg>");
    s
}

/// 直方图。
pub fn histogram_svg(hist: &Histogram, title: &str) -> String {
    let pad = 30.0;
    let mut s = String::new();
    let _ = write!(s, "<svg width=\"{W}\" height=\"{H}\" xmlns=\"http://www.w3.org/2000/svg\">");
    let _ = write!(s, "<text x=\"8\" y=\"16\" font-size=\"13\">{title}</text>");
    if hist.bins.is_empty() {
        let _ = write!(s, "<text x=\"8\" y=\"{}\">no data</text></svg>", H / 2);
        return s;
    }
    let maxc = hist.bins.iter().map(|(_, _, c)| *c).max().unwrap_or(1).max(1);
    let n = hist.bins.len();
    let bw = (W as f64 - 2.0 * pad) / n as f64;
    let base = H as f64 - pad;
    for (i, (_, _, c)) in hist.bins.iter().enumerate() {
        let x = pad + i as f64 * bw;
        let bh = (*c as f64 / maxc as f64) * (H as f64 - 2.0 * pad);
        let _ = write!(s, "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" fill=\"#5472d3\"/>", x, base - bh, bw * 0.9, bh);
    }
    let _ = write!(s, "</svg>");
    s
}

/// 拼装自包含 HTML 报告。
pub fn render_html(report: &Report, series: Option<&EquitySeries>) -> String {
    let m = &report.metrics;
    let mut s = String::new();
    let _ = write!(s, "<!doctype html><html><head><meta charset=\"utf-8\"><title>rquant report: {}</title>", report.tree_name);
    let _ = write!(s, "<style>body{{font-family:system-ui,Arial,sans-serif;margin:24px;max-width:720px}}table{{border-collapse:collapse}}td,th{{border:1px solid #ddd;padding:4px 8px;text-align:right}}th{{text-align:left}}.warn{{background:#fff3cd;border:1px solid #ffe08a;padding:8px;border-radius:4px;margin:12px 0}}svg{{border:1px solid #eee;margin:8px 0}}</style></head><body>");
    let _ = write!(s, "<h1>rquant report: {}</h1>", report.tree_name);
    let _ = write!(s, "<table><tr><th>metric</th><th>value</th></tr>");
    let _ = write!(s, "<tr><th>forward_window</th><td>{}</td></tr>", report.forward_window);
    let _ = write!(s, "<tr><th>cost_bps</th><td>{:.1}</td></tr>", report.cost_bps);
    let _ = write!(s, "<tr><th>decisions / scored</th><td>{} / {}</td></tr>", m.total_decisions, m.scored);
    let _ = write!(s, "<tr><th>active n</th><td>{}</td></tr>", m.active.count);
    let _ = write!(s, "<tr><th>active mean_net</th><td>{:.4}</td></tr>", m.active.mean_net);
    let _ = write!(s, "<tr><th>active hit%</th><td>{:.1}</td></tr>", m.active.hit_rate * 100.0);
    let _ = write!(s, "<tr><th>active t</th><td>{:.2}</td></tr>", m.active.t_stat);
    let _ = write!(s, "<tr><th>buy&amp;hold</th><td>{:.4}</td></tr>", m.buy_and_hold);
    let _ = write!(s, "<tr><th>gaps (missing/partial)</th><td>{} / {}</td></tr>", report.gaps.missing_trading_days.len(), report.gaps.partial_days.len());
    let _ = write!(s, "</table>");
    let _ = write!(s, "<div class=\"warn\">{}</div>", m.overlap_warning);
    match series {
        Some(es) => {
            let cum: Vec<(f64, f64)> = es.points.iter().enumerate().map(|(i, p)| (i as f64, p.cum)).collect();
            let _ = write!(s, "{}", line_chart(&cum, "累计前瞻收益（窗口重叠 → 信号质量曲线，非可交易净值）"));
            let _ = write!(s, "{}", histogram_svg(&es.hist, "逐点净收益分布"));
            if es.skipped > 0 {
                let _ = write!(s, "<p>{} 点未计入曲线（越界或时间未匹配）</p>", es.skipped);
            }
        }
        None => {
            let _ = write!(s, "<p>（未提供 --traces/--primary，省略时间序列图）</p>");
        }
    }
    let by_leaf: Vec<(String, f64)> = m.by_leaf.iter().map(|(k, v)| (k.clone(), v.mean_net)).collect();
    let _ = write!(s, "{}", bar_chart(&by_leaf, "各叶子平均净收益"));
    let node: Vec<(String, f64)> = m.node_label_counts.iter().map(|(k, c)| (k.clone(), *c as f64)).collect();
    let _ = write!(s, "{}", bar_chart(&node, "节点命中计数"));
    let _ = write!(s, "</body></html>");
    s
}
```

- [ ] **Step 5: 运行验证通过**

Run: `cargo test --lib report::viz`
Expected: 3 个测试 PASS。
Run: `cargo clippy --all-targets`
Expected: 无告警（平铺执行，勿用 `2>&1`）。

- [ ] **Step 6: Commit**

```bash
git add src/report/viz.rs src/report/mod.rs
git commit -m "feat(report): hand-rolled SVG charts + self-contained HTML render" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: cli `report` 子命令 + e2e + README

**Files:**
- Modify: `src/cli/mod.rs`（`Cmd::Report` + 分支）
- Modify: `tests/e2e.rs`（可视化 e2e）
- Modify: `README.md`

- [ ] **Step 1: `src/cli/mod.rs` 加 `Cmd::Report` 变体**

在 `Cmd` enum 里（`Fetch` 之后）加：
```rust
    /// Render a report.json (+ optional traces/primary) into a self-contained HTML report
    Report {
        #[arg(long)]
        report: PathBuf,
        #[arg(long, default_value = "report.html")]
        out: PathBuf,
        #[arg(long)]
        traces: Option<PathBuf>,
        #[arg(long)]
        primary: Option<PathBuf>,
    },
```

- [ ] **Step 2: `src/cli/mod.rs` 的 `match` 加 `Cmd::Report` 分支**

在 `Cmd::Fetch { .. } => { .. }` 之后加：
```rust
        Cmd::Report { report, out, traces, primary } => {
            let json = std::fs::read_to_string(&report)?;
            let rep: crate::report::Report = serde_json::from_str(&json)?;
            let series = match (&traces, &primary) {
                (Some(tp), Some(pp)) => {
                    let content = std::fs::read_to_string(tp)?;
                    let mut tr = Vec::new();
                    for line in content.lines().filter(|l| !l.trim().is_empty()) {
                        tr.push(serde_json::from_str::<crate::engine::trace::Trace>(line)?);
                    }
                    let bars = crate::data::reader::read_bars_csv(pp)?;
                    let costs = crate::backtest::costs::CostModel { round_trip_bps: rep.cost_bps };
                    Some(crate::report::curve::derive_series(&tr, &bars, rep.forward_window, &costs))
                }
                (None, None) => None,
                _ => {
                    eprintln!("[rquant] --traces and --primary must be given together to draw the curve; rendering aggregates only");
                    None
                }
            };
            let html = crate::report::viz::render_html(&rep, series.as_ref());
            std::fs::write(&out, html)?;
            println!("wrote HTML report to {}", out.display());
        }
```

- [ ] **Step 3: 构建 + help 检查**

Run: `cargo build`
Expected: 通过。
Run: `cargo run -- report --help`
Expected: 用法含 `--report`/`--out`/`--traces`/`--primary`。

- [ ] **Step 4: `tests/e2e.rs` 加可视化 e2e**

> 复用现有 `end_to_end_uptrend_yields_positive_long_edge` 的 fixture 构造（tree/primary/context CSV + `BacktestConfig` + `run`）。新测试跑一次 `run` 拿 `report`，写出 report.json + traces.jsonl，再读回 + `derive_series` + `render_html`，断言 HTML 自包含且含曲线。

```rust
#[tokio::test]
async fn report_html_renders_with_curve() {
    use rquant::report::{curve::derive_series, viz::render_html, write_report, write_traces_jsonl, Report};
    use rquant::backtest::costs::CostModel;
    // —— 复用 end_to_end_uptrend 的 fixture：构造 tree_f / primary_f / context_f / cfg，跑 run 拿 (report, traces)。——
    // run() 只返回 Report；traces 由 write_traces_jsonl 写出需要 Vec<Trace>。这里改为：
    //   1) 跑 backtest 写出 report.json（cfg.out_path）与 traces.jsonl（cfg.traces_path = Some(...)）。
    //   2) 读回 report.json → Report；读回 traces.jsonl → Vec<Trace>；读 primary CSV → bars。
    //   3) derive_series + render_html，断言。
    // 具体 fixture 代码照搬 end_to_end_uptrend_yields_positive_long_edge，并把 cfg.traces_path 设为一个 tempfile。
    // 断言：
    //   let html = render_html(&rep, Some(&series));
    //   assert!(html.contains("<!doctype html>"));
    //   assert!(html.contains("<polyline"));            // 曲线（上升趋势有多个 scored 点）
    //   assert!(html.contains(&rep.metrics.overlap_warning));
    //   assert!(series.points.len() > 0);
}
```
> 把注释展开为真实代码：照搬 `end_to_end_uptrend_yields_positive_long_edge` 的 fixture（含 `traces_path: Some(traces_f.path().to_path_buf())`），`run(&cfg, &LlmEvaluator::Disabled).await.unwrap()` 写 `write_report`/`write_traces_jsonl`，或直接用 run 内部已写出的文件（cfg.out_path / cfg.traces_path）。然后 `let rep: Report = serde_json::from_str(&std::fs::read_to_string(out_f.path()).unwrap()).unwrap();`，读 traces 行→`Vec<Trace>`，`read_bars_csv(primary_f.path())`，`derive_series(&tr, &bars, rep.forward_window, &CostModel{round_trip_bps: rep.cost_bps})`，再断言如上。

- [ ] **Step 5: 运行验证**

Run: `cargo test --test e2e report_html_renders_with_curve`
Expected: PASS。
Run: `cargo test`
Expected: 全量全绿。
Run: `cargo clippy --all-targets`
Expected: 无告警。

- [ ] **Step 6: README 加一节**（fetch 一节之后）

````markdown
## 报告可视化（`rquant report`）

把回测产物渲染成**自包含 HTML**（内联 SVG，离线可分享）：

```bash
cargo run --release -- report --report report.json --out report.html \
  --traces traces.jsonl --primary 15m.csv
```

- 含累计前瞻收益曲线、逐点净收益直方图、各叶子平均净收益条形、节点命中条形、headline 表。
- `--traces`/`--primary` 二者都给才画时间序列（可视化器用 `forward_return` 重算逐点 net）；只给 `--report` 则仅画聚合图。
- 累计曲线因前瞻窗口重叠是**信号质量曲线、非可交易净值**（HTML 内有标注）。
````

- [ ] **Step 7: Commit**

```bash
git add src/cli/mod.rs tests/e2e.rs README.md
git commit -m "feat(cli): report subcommand renders HTML; e2e + README" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## 附录 A：Spec 覆盖对照（自检）
| Spec § | 实现于 |
|---|---|
| §4.1 Deserialize 接入 | Task 1 |
| §4.2 derive_series（重算 net + cum + 直方图） | Task 2 |
| §4.3 SVG 原语 + render_html | Task 3 |
| §4.4 cli report + 降级 | Task 4 |
| §5 图表集（曲线/直方图/by_leaf/node/headline） | Task 3 (render_html) |
| §6 错误处理（缺失文件降级、skipped 计数） | Task 2 / Task 4 |
| §7 确定性（同输入同字节 HTML） | Task 3（测试断言） |
| §8 测试（往返/derive/viz/e2e） | Task 1/2/3/4 |

## 附录 B：明确不在范围（YAGNI）
- 软报告可视化（需先写 soft traces）；交互式/JS 图表；大 traces 降采样；决策树结构图。
- `by_stance` 条形（与 `by_leaf` 重复度高，MVP 略；需要可仿 by_leaf 再加）。
