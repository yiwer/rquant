# rquant report 软曲线 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `rquant report --soft` 消费 `soft_report.json` + `soft_traces.jsonl`，渲染软模式自包含 HTML（累计期望收益曲线 + expected_net 直方图 + 各叶平均概率条形 + headline）。

**Architecture:** 在 master(HEAD `d123474`)上扩展。复用 `EquitySeries`/`Histogram` 与 `line_chart`/`histogram_svg`/`bar_chart`；软 traces 已含 `expected_net`，故无需 `--primary`。硬模式渲染逐字不变。

**Tech Stack:** Rust 2024 + 既有（serde/serde_json/chrono）。

> 设计依据：`docs/superpowers/specs/2026-06-10-rquant-report-soft-curve-design.md`。提交信息用英文。

---

## 文件结构
```
改动: src/report/mod.rs        # SoftReport +Deserialize
改动: src/backtest/soft.rs     # SoftMetrics +Deserialize
改动: src/report/curve.rs      # + derive_soft_series + avg_leaf_probs
改动: src/report/viz.rs        # + render_soft_html
改动: src/cli/mod.rs           # report 加 --soft 分流
改动: tests/e2e.rs、README.md
```

---

## Task 1: Deserialize + derive_soft_series + avg_leaf_probs

**Files:**
- Modify: `src/report/mod.rs`（SoftReport +Deserialize）
- Modify: `src/backtest/soft.rs`（SoftMetrics +Deserialize）
- Modify: `src/report/curve.rs`（两个函数 + 测试）
- Test: `src/report/curve.rs`

- [ ] **Step 1: 在 `src/report/curve.rs` 的 `mod tests` 加失败测试**

```rust
    #[test]
    fn derive_soft_series_cumulates_and_skips() {
        use crate::backtest::soft::SoftStepRecord;
        use std::collections::BTreeMap;
        let t = NaiveDateTime::parse_from_str("2024-01-02 09:45:00", "%Y-%m-%d %H:%M:%S").unwrap();
        let mut lp = BTreeMap::new();
        lp.insert("a".to_string(), 0.6);
        lp.insert("b".to_string(), 0.4);
        let recs = vec![
            SoftStepRecord { t, leaf_probs: lp.clone(), expected_net: Some(0.1) },
            SoftStepRecord { t, leaf_probs: lp.clone(), expected_net: Some(0.2) },
            SoftStepRecord { t, leaf_probs: lp, expected_net: None },
        ];
        let es = derive_soft_series(&recs);
        assert_eq!(es.points.len(), 2);
        assert!((es.points[1].cum - 0.3).abs() < 1e-9);
        assert_eq!(es.skipped, 1);
    }

    #[test]
    fn avg_leaf_probs_means_sum_to_one() {
        use crate::backtest::soft::SoftStepRecord;
        use std::collections::BTreeMap;
        let t = NaiveDateTime::parse_from_str("2024-01-02 09:45:00", "%Y-%m-%d %H:%M:%S").unwrap();
        let mut lp1 = BTreeMap::new(); lp1.insert("a".to_string(), 1.0);
        let mut lp2 = BTreeMap::new(); lp2.insert("a".to_string(), 0.5); lp2.insert("b".to_string(), 0.5);
        let recs = vec![
            SoftStepRecord { t, leaf_probs: lp1, expected_net: Some(0.0) },
            SoftStepRecord { t, leaf_probs: lp2, expected_net: Some(0.0) },
        ];
        let avg = avg_leaf_probs(&recs);
        // a: (1.0+0.5)/2 = 0.75 ; b: (0+0.5)/2 = 0.25 ; sorted by name
        assert_eq!(avg.len(), 2);
        assert_eq!(avg[0].0, "a"); assert!((avg[0].1 - 0.75).abs() < 1e-9);
        assert_eq!(avg[1].0, "b"); assert!((avg[1].1 - 0.25).abs() < 1e-9);
        let sum: f64 = avg.iter().map(|(_, v)| v).sum();
        assert!((sum - 1.0).abs() < 1e-9);
    }
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test --lib report::curve::tests::derive_soft_series_cumulates_and_skips`
Expected: 编译失败（`derive_soft_series`/`avg_leaf_probs` 未定义）。

- [ ] **Step 3: 实现两个函数 + 加 Deserialize**

(a) `src/report/mod.rs`：`SoftReport` 的 `#[derive(Debug, Serialize)]` → `#[derive(Debug, Serialize, Deserialize)]`（`Deserialize` 已在该文件 `use serde::{Deserialize, Serialize};` 中）。
(b) `src/backtest/soft.rs`：`SoftMetrics` 的 `#[derive(Debug, Serialize)]` → `#[derive(Debug, Serialize, Deserialize)]`（`Deserialize` 已 import）。
(c) `src/report/curve.rs`：顶部 `use` 加 `use crate::backtest::soft::SoftStepRecord;`。在 `histogram` 函数之后（测试模块之前）加：
```rust
/// 软序列：net = expected_net(Some)，累计 cum，expected_net 直方图；None 计 skipped。
pub fn derive_soft_series(records: &[SoftStepRecord]) -> EquitySeries {
    let mut points = Vec::new();
    let mut skipped = 0usize;
    let mut cum = 0.0;
    for r in records {
        match r.expected_net {
            Some(x) => {
                cum += x;
                points.push(SeriesPoint { t: r.t, net: x, cum });
            }
            None => skipped += 1,
        }
    }
    let hist = histogram(&points);
    EquitySeries { points, hist, skipped }
}

/// 各叶平均质量：每叶 Σ leaf_probs.get(leaf).unwrap_or(0) / records.len()，按叶名排序。空→空。
pub fn avg_leaf_probs(records: &[SoftStepRecord]) -> Vec<(String, f64)> {
    if records.is_empty() {
        return vec![];
    }
    let mut names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for r in records {
        for k in r.leaf_probs.keys() {
            names.insert(k.clone());
        }
    }
    let n = records.len() as f64;
    names
        .into_iter()
        .map(|name| {
            let sum: f64 = records.iter().map(|r| r.leaf_probs.get(&name).copied().unwrap_or(0.0)).sum();
            (name, sum / n)
        })
        .collect()
}
```

- [ ] **Step 4: 运行验证通过**

Run: `cargo test --lib report::curve`
Expected: 既有 + 2 新测试 PASS。
Run: `cargo build`
Expected: 通过。

- [ ] **Step 5: Commit**

```bash
git add src/report/mod.rs src/backtest/soft.rs src/report/curve.rs
git commit -m "feat(report): derive_soft_series + avg_leaf_probs; Deserialize SoftReport/SoftMetrics" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: render_soft_html

**Files:**
- Modify: `src/report/viz.rs`（`render_soft_html` + 测试）
- Test: 同文件

- [ ] **Step 1: 在 `src/report/viz.rs` 的 `mod tests` 加失败测试**

```rust
    #[test]
    fn render_soft_html_is_self_contained() {
        use crate::report::SoftReport;
        use crate::backtest::soft::SoftMetrics;
        use crate::backtest::metrics::signal_stat;
        use crate::report::curve::{EquitySeries, Histogram, SeriesPoint};
        use chrono::NaiveDate;
        let soft = SoftMetrics {
            total_decisions: 3, scored: 2,
            engaged: signal_stat(&[0.1, 0.2]),
            buy_and_hold: 0.05,
            overlap_warning: "OVLAP".into(),
        };
        let report = SoftReport { tree_name: "softviz".into(), forward_window: 4, cost_bps: 10.0, soft };
        let t = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        let series = EquitySeries {
            points: vec![SeriesPoint { t, net: 0.1, cum: 0.1 }, SeriesPoint { t, net: 0.2, cum: 0.3 }],
            hist: Histogram { bins: vec![(0.0, 0.2, 2)] },
            skipped: 0,
        };
        let avg = vec![("leaf_l".to_string(), 0.7), ("leaf_f".to_string(), 0.3)];
        let a = render_soft_html(&report, &series, &avg);
        let b = render_soft_html(&report, &series, &avg);
        assert_eq!(a, b);
        assert!(a.contains("<!doctype html>"));
        assert!(a.contains("softviz"));
        assert!(a.contains("<polyline"));
        assert!(a.contains("<rect"));
        assert!(a.contains("OVLAP"));
    }
```
> `signal_stat` 是 `pub(crate)`，测试在 crate 内可用。

- [ ] **Step 2: 运行验证失败**

Run: `cargo test --lib report::viz::tests::render_soft_html_is_self_contained`
Expected: 编译失败（`render_soft_html` 未定义）。

- [ ] **Step 3: 实现 `render_soft_html`（`src/report/viz.rs`）**

顶部 `use` 把 `use crate::report::Report;` 改为 `use crate::report::{Report, SoftReport};`。在 `render_html` 之后加：
```rust
/// 软模式报告 HTML：累计期望收益曲线 + expected_net 直方图 + 各叶平均概率条形 + headline。
pub fn render_soft_html(report: &SoftReport, series: &EquitySeries, avg_leaf: &[(String, f64)]) -> String {
    let m = &report.soft;
    let mut s = String::new();
    let _ = write!(s, "<!doctype html><html><head><meta charset=\"utf-8\"><title>rquant soft report: {}</title>", report.tree_name);
    let _ = write!(s, "<style>body{{font-family:system-ui,Arial,sans-serif;margin:24px;max-width:720px}}table{{border-collapse:collapse}}td,th{{border:1px solid #ddd;padding:4px 8px;text-align:right}}th{{text-align:left}}.warn{{background:#fff3cd;border:1px solid #ffe08a;padding:8px;border-radius:4px;margin:12px 0}}svg{{border:1px solid #eee;margin:8px 0}}</style></head><body>");
    let _ = write!(s, "<h1>rquant soft report: {}</h1>", report.tree_name);
    let _ = write!(s, "<table><tr><th>metric</th><th>value</th></tr>");
    let _ = write!(s, "<tr><th>forward_window</th><td>{}</td></tr>", report.forward_window);
    let _ = write!(s, "<tr><th>cost_bps</th><td>{:.1}</td></tr>", report.cost_bps);
    let _ = write!(s, "<tr><th>decisions / scored</th><td>{} / {}</td></tr>", m.total_decisions, m.scored);
    let _ = write!(s, "<tr><th>engaged n</th><td>{}</td></tr>", m.engaged.count);
    let _ = write!(s, "<tr><th>engaged mean_net</th><td>{:.4}</td></tr>", m.engaged.mean_net);
    let _ = write!(s, "<tr><th>engaged hit%</th><td>{:.1}</td></tr>", m.engaged.hit_rate * 100.0);
    let _ = write!(s, "<tr><th>engaged t</th><td>{:.2}</td></tr>", m.engaged.t_stat);
    let _ = write!(s, "<tr><th>buy&amp;hold</th><td>{:.4}</td></tr>", m.buy_and_hold);
    let _ = write!(s, "</table>");
    let _ = write!(s, "<div class=\"warn\">{}</div>", m.overlap_warning);
    let cum: Vec<(f64, f64)> = series.points.iter().enumerate().map(|(i, p)| (i as f64, p.cum)).collect();
    let _ = write!(s, "{}", line_chart(&cum, "累计期望收益（窗口重叠 → 信号质量曲线，非可交易净值）"));
    let _ = write!(s, "{}", histogram_svg(&series.hist, "逐点期望净收益分布"));
    if series.skipped > 0 {
        let _ = write!(s, "<p>{} 点未计入曲线（未计分）</p>", series.skipped);
    }
    let _ = write!(s, "{}", bar_chart(avg_leaf, "各叶平均概率"));
    let _ = write!(s, "</body></html>");
    s
}
```

- [ ] **Step 4: 运行验证通过 + clippy**

Run: `cargo test --lib report::viz`
Expected: 既有 + 新测试 PASS。
Run: `cargo clippy --all-targets`
Expected: 无告警（平铺执行，勿用 `2>&1`）。

- [ ] **Step 5: Commit**

```bash
git add src/report/viz.rs
git commit -m "feat(report): render_soft_html (soft-mode HTML report)" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: cli report --soft + e2e + README

**Files:**
- Modify: `src/cli/mod.rs`（`Report` 加 `--soft` + 分流）
- Modify: `tests/e2e.rs`、`README.md`

- [ ] **Step 1: `src/cli/mod.rs` `Cmd::Report` 变体加 `--soft`**

在 `Report { ... }` 变体里（`primary` 之后）加：
```rust
        /// Render a soft-mode report (soft_report.json + soft_traces.jsonl); no --primary needed
        #[arg(long, default_value_t = false)]
        soft: bool,
```

- [ ] **Step 2: `src/cli/mod.rs` 的 `Cmd::Report` 分流**

把 `Cmd::Report { report, out, traces, primary } => { <现有硬体> }` 改为 `Cmd::Report { report, out, traces, primary, soft } => { ... }`，并把现有硬渲染体放进 `else {}`，前面加软分支：
```rust
        Cmd::Report { report, out, traces, primary, soft } => {
            if soft {
                let rep: crate::report::SoftReport = serde_json::from_str(&std::fs::read_to_string(&report)?)?;
                if primary.is_some() {
                    eprintln!("[rquant] --primary ignored in --soft report (expected_net is in traces)");
                }
                let (series, avg) = match &traces {
                    Some(tp) => {
                        let content = std::fs::read_to_string(tp)?;
                        let mut recs = Vec::new();
                        for line in content.lines().filter(|l| !l.trim().is_empty()) {
                            recs.push(serde_json::from_str::<crate::backtest::soft::SoftStepRecord>(line)?);
                        }
                        (crate::report::curve::derive_soft_series(&recs), crate::report::curve::avg_leaf_probs(&recs))
                    }
                    None => (
                        crate::report::curve::EquitySeries {
                            points: vec![],
                            hist: crate::report::curve::Histogram { bins: vec![] },
                            skipped: 0,
                        },
                        vec![],
                    ),
                };
                let html = crate::report::viz::render_soft_html(&rep, &series, &avg);
                std::fs::write(&out, html)?;
                println!("wrote soft HTML report to {}", out.display());
            } else {
                // —— 现有硬渲染体原样放这里 ——
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
        }
```
> `EquitySeries`/`Histogram` 字段为 `pub`，可在 cli 直接构造。`SoftStepRecord`/`SoftReport`/`derive_soft_series`/`avg_leaf_probs`/`render_soft_html` 均 pub。

- [ ] **Step 3: 构建 + help**

Run: `cargo build`
Expected: 通过。
Run: `cargo run -- report --help`
Expected: 用法含 `--soft`。

- [ ] **Step 4: `tests/e2e.rs` 加软渲染 e2e**

> 复用 `soft_traces_written_when_path_given` 的 fixture（含 LLM 节点的树 + Stub + 上升趋势 + `BacktestConfig` 且 `traces_path: Some(traces_f)`），跑 `run_soft` 后读回 SoftReport + SoftStepRecord，`derive_soft_series` + `avg_leaf_probs` + `render_soft_html`，断言 HTML 自包含含曲线。

```rust
#[tokio::test]
async fn soft_report_html_renders() {
    // 复用 soft_traces_written_when_path_given 的 fixture（tree/primary/context/Stub ev/cfg，
    // cfg.out_path = out_f(.json)、cfg.traces_path = Some(traces_f(.jsonl))）。然后：
    let report = rquant::backtest::soft::run_soft(&cfg, &ev).await.unwrap();
    let recs: Vec<rquant::backtest::soft::SoftStepRecord> = std::fs::read_to_string(traces_f.path())
        .unwrap().lines().filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap()).collect();
    let series = rquant::report::curve::derive_soft_series(&recs);
    let avg = rquant::report::curve::avg_leaf_probs(&recs);
    let html = rquant::report::viz::render_soft_html(&report, &series, &avg);
    assert!(html.contains("<!doctype html>"));
    assert!(html.contains("<polyline"));
    assert!(html.contains(&report.soft.overlap_warning));
    assert!(series.points.len() > 0);
}
```
> 把注释展开为真实代码：照搬 `soft_traces_written_when_path_given` 的 fixture（`cfg`/`ev`/`traces_f`）。`run_soft` 已写 SoftReport 与 traces；这里直接用其返回的 `report` + 读 traces 文件。

- [ ] **Step 5: 运行验证**

Run: `cargo test --test e2e soft_report_html_renders`
Expected: PASS。
Run: `cargo test`
Expected: 全量全绿。
Run: `cargo clippy --all-targets`
Expected: 无告警。

- [ ] **Step 6: README**（`报告可视化` / `--soft` 一节）

````markdown
软模式报告：`rquant report --soft --report soft_report.json --traces soft_traces.jsonl --out soft.html`
渲染累计期望收益曲线、expected_net 直方图、各叶平均概率条形、headline。软模式**不需 `--primary`**（expected_net 已在 traces 里）。
````

- [ ] **Step 7: Commit**

```bash
git add src/cli/mod.rs tests/e2e.rs README.md
git commit -m "feat(cli): report --soft renders soft HTML; e2e + README" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## 附录 A：Spec 覆盖对照（自检）
| Spec § | 实现于 |
|---|---|
| §4.1 Deserialize SoftReport/SoftMetrics | Task 1 |
| §4.2 derive_soft_series + avg_leaf_probs | Task 1 |
| §4.3 render_soft_html（复用图元）| Task 2 |
| §4.4 cli `report --soft` 分流（无需 --primary）| Task 3 |
| §6 测试（derive/avg/render/往返/e2e）| Task 1/2/3 |
| §5 错误处理（解析冒泡；无 traces headline-only；--primary 忽略提示）| Task 3 |

## 附录 B：明确不在范围（YAGNI）
- 叶子概率随时间堆叠面积图；自动探测 report 类型；软模式用 --primary 重算。
- `render_soft_html` 与 `render_html` 的 HTML 外壳少量重复（可接受）。
