# rquant Walk-forward（固定树滚动分折）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `backtest --folds K` 把决策点按索引等分 K 个连续折，逐折 SignalStat+同口径 bh+时间范围，汇总 positive/worst，进 Report/SoftReport（serde 兼容）+ 摘要 + HTML 条形图。

**Architecture:** 在 master(HEAD `ef4c878`)上扩展。决策无状态 ⇒ 一次回测分桶。新纯函数模块 `backtest/walkforward.rs`；接线波及 BacktestConfig/cli/runner/run_soft/Report/SoftReport + 多处字面量（grep 找全）。`Option<WalkForward>` + `skip_serializing_if/default` 保旧 JSON 兼容。

**Tech Stack:** Rust 2024 + 既有。

> 设计依据：`docs/superpowers/specs/2026-06-10-rquant-walkforward-design.md`。提交信息用英文。

---

## 文件结构
```
新增: src/backtest/walkforward.rs  # FoldMetrics/WalkForward/walk_forward + 测试
改动: src/backtest/mod.rs          # + pub mod walkforward;
改动: src/backtest/runner.rs       # BacktestConfig.folds；run 提 nets→walk_forward；Report 构造
改动: src/backtest/soft.rs         # run_soft 提 nets→walk_forward；SoftReport 构造
改动: src/report/mod.rs            # Report/SoftReport + walk_forward 字段；print 两处；测试字面量+兼容测试
改动: src/report/viz.rs            # render_html/render_soft_html 折条形图；sample_report/soft 测试字面量
改动: src/cli/mod.rs               # --folds
改动: tests/e2e.rs                 # 全部 BacktestConfig 字面量 + folds:0；新 e2e
改动: README.md
```

---

## Task 1: walkforward.rs 纯函数

**Files:**
- Create: `src/backtest/walkforward.rs`；Modify: `src/backtest/mod.rs`（+ `pub mod walkforward;`）
- Test: 同文件

- [ ] **Step 1: 失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::bar::Bar;
    use chrono::NaiveDate;

    fn bars(n: usize) -> Vec<Bar> {
        let base = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        (0..n).map(|i| Bar {
            time: base + chrono::Duration::minutes(i as i64 * 15),
            open: 10.0 + i as f64, high: 13.0 + i as f64, low: 10.0 + i as f64,
            close: 12.5 + i as f64, volume: 1.0,
        }).collect()
    }

    #[test]
    fn three_folds_known_values() {
        let p = bars(9);
        let nets = vec![
            Some(0.01), None, Some(0.03),       // fold0: mean 0.02
            None, None, None,                   // fold1: count 0
            Some(-0.01), Some(0.02), None,      // fold2: mean 0.005
        ];
        let wf = walk_forward(&nets, &p, 3);
        assert_eq!(wf.folds.len(), 3);
        assert_eq!(wf.folds[0].stat.count, 2);
        assert!((wf.folds[0].stat.mean_net - 0.02).abs() < 1e-12);
        // bh fold0 = close[2]/open[0] - 1 = 14.5/10 - 1 = 0.45
        assert!((wf.folds[0].buy_and_hold - 0.45).abs() < 1e-12);
        assert_eq!(wf.folds[0].from, p[0].time);
        assert_eq!(wf.folds[0].to, p[2].time);
        assert_eq!(wf.folds[1].stat.count, 0);
        assert!((wf.folds[2].stat.mean_net - 0.005).abs() < 1e-12);
        // 汇总：空折不计入；positive = fold0,fold2；worst = 0.005
        assert_eq!(wf.positive_folds, 2);
        assert!((wf.worst_mean_net - 0.005).abs() < 1e-12);
    }

    #[test]
    fn fewer_points_than_folds_skips_empty_ranges() {
        let p = bars(2);
        let nets = vec![Some(0.01), Some(0.02)];
        let wf = walk_forward(&nets, &p, 5);
        assert_eq!(wf.folds.len(), 2); // 空索引段折被省略
        assert_eq!(wf.positive_folds, 2);
    }
}
```

- [ ] **Step 2: 验证失败**

Run: `cargo test --lib backtest::walkforward`
Expected: 编译失败（模块/函数未定义）。

- [ ] **Step 3: 实现**

`src/backtest/mod.rs` 加 `pub mod walkforward;`。`src/backtest/walkforward.rs`：
```rust
use crate::backtest::metrics::{signal_stat, SignalStat};
use crate::data::bar::Bar;
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FoldMetrics {
    pub from: NaiveDateTime,
    pub to: NaiveDateTime,
    pub stat: SignalStat,
    pub buy_and_hold: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalkForward {
    pub folds: Vec<FoldMetrics>,
    pub positive_folds: usize,
    pub worst_mean_net: f64,
}

/// 固定树滚动分折：决策点按索引等分 k 个连续折（空索引段省略）。
/// nets_per_point[i] = 第 i 点的参与净收益（未参与/未计分=None），与 primary_slice 一一对齐。
pub fn walk_forward(nets_per_point: &[Option<f64>], primary_slice: &[Bar], k: usize) -> WalkForward {
    let n = nets_per_point.len().min(primary_slice.len());
    let mut folds = Vec::new();
    for j in 0..k {
        let lo = j * n / k;
        let hi = (j + 1) * n / k;
        if hi <= lo {
            continue;
        }
        let nets: Vec<f64> = nets_per_point[lo..hi].iter().flatten().copied().collect();
        let bh = if primary_slice[lo].open > 0.0 {
            primary_slice[hi - 1].close / primary_slice[lo].open - 1.0
        } else {
            0.0
        };
        folds.push(FoldMetrics {
            from: primary_slice[lo].time,
            to: primary_slice[hi - 1].time,
            stat: signal_stat(&nets),
            buy_and_hold: bh,
        });
    }
    let positive_folds = folds.iter().filter(|f| f.stat.count > 0 && f.stat.mean_net > 0.0).count();
    let worst = folds.iter().filter(|f| f.stat.count > 0).map(|f| f.stat.mean_net).fold(f64::INFINITY, f64::min);
    let worst_mean_net = if worst.is_finite() { worst } else { 0.0 };
    WalkForward { folds, positive_folds, worst_mean_net }
}
```

- [ ] **Step 4: 验证通过**

Run: `cargo test --lib backtest::walkforward` → 2 PASS。`cargo build`。

- [ ] **Step 5: Commit**

```bash
git add src/backtest/walkforward.rs src/backtest/mod.rs
git commit -m "feat(backtest): walk_forward fold metrics (fixed-tree rolling stability)" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: 接线（Config/cli/runner/run_soft/Report 字段/print/字面量涟漪）

**Files:**
- Modify: `src/backtest/runner.rs`、`src/backtest/soft.rs`、`src/report/mod.rs`、`src/report/viz.rs`（仅测试字面量）、`src/cli/mod.rs`、`tests/e2e.rs`（仅字面量）

> 一次切：`BacktestConfig.folds` 与 `Report/SoftReport.walk_forward` 都是字段涟漪。**grep 找全字面量**：`BacktestConfig {`（cli 1 + e2e 全部）、`Report {`（runner + report 测试 ×2 + viz sample_report）、`SoftReport {`（run_soft + viz soft 测试）。

- [ ] **Step 1: 失败测试（`src/report/mod.rs` 的 `mod tests`）**

```rust
    #[test]
    fn walk_forward_field_is_optional_and_compatible() {
        let metrics = compute_metrics(&[], &[]);
        let report = Report { tree_name: "wf".into(), forward_window: 8, cost_bps: 5.0, metrics, gaps: GapReport::default(), walk_forward: None };
        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("walk_forward"), "None must not serialize");
        let back: Report = serde_json::from_str(&json).unwrap(); // 旧 JSON（无键）可反序列化
        assert!(back.walk_forward.is_none());
    }
```

- [ ] **Step 2: 验证失败**

Run: `cargo test --lib report::tests::walk_forward_field_is_optional_and_compatible`
Expected: 编译失败（无 `walk_forward` 字段）。

- [ ] **Step 3: 字段 + 接线**

(a) `src/report/mod.rs`：`use` 区加 `use crate::backtest::walkforward::WalkForward;`；`Report` 与 `SoftReport` 各加：
```rust
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub walk_forward: Option<WalkForward>,
```
（`SoftReport` 已 derive Deserialize ✓。）`print_summary` 与 `print_soft_summary` 末尾（warn 行之前）各加：
```rust
    if let Some(wf) = &report.walk_forward {
        for (i, f) in wf.folds.iter().enumerate() {
            println!(
                "wf {}/{} [{} → {}]: n={} mean={:.4} hit={:.1}% | bh={:.4}",
                i + 1, wf.folds.len(), f.from, f.to, f.stat.count, f.stat.mean_net, f.stat.hit_rate * 100.0, f.buy_and_hold
            );
        }
        println!("wf summary: positive {}/{}, worst mean={:.4}", wf.positive_folds, wf.folds.len(), wf.worst_mean_net);
    }
```
(b) `src/backtest/runner.rs`：`BacktestConfig` 加 `pub folds: usize,`；`use` 区补 `use crate::tree::schema::Stance;`（若无）。`compute_metrics` 之后加：
```rust
    let walk_forward = if cfg.folds >= 2 {
        let nets: Vec<Option<f64>> = results
            .iter()
            .map(|(tr, fr)| match fr {
                Some(f) if tr.stance != Stance::Flat => Some(f.net),
                _ => None,
            })
            .collect();
        Some(crate::backtest::walkforward::walk_forward(&nets, &primary[start..], cfg.folds))
    } else {
        None
    };
```
`Report { ... }` 构造加 `walk_forward,`。
(c) `src/backtest/soft.rs`：`soft_metrics` 调用之后加：
```rust
    let walk_forward = if cfg.folds >= 2 {
        let nets: Vec<Option<f64>> = results
            .iter()
            .map(|(_, s)| match s {
                Some(x) if x.engaged > 0.0 => Some(x.expected_net),
                _ => None,
            })
            .collect();
        Some(crate::backtest::walkforward::walk_forward(&nets, &primary[start..], cfg.folds))
    } else {
        None
    };
```
`SoftReport { ... }` 构造加 `walk_forward,`。
(d) `src/cli/mod.rs`：`Backtest` 变体加
```rust
        /// Walk-forward folds (>=2 enables fixed-tree rolling-fold stability metrics)
        #[arg(long, default_value_t = 0)]
        folds: usize,
```
解构加 `folds`，`BacktestConfig` 构造加 `folds,`。
(e) 字面量涟漪：`tests/e2e.rs` 全部 `BacktestConfig {` 加 `folds: 0,`；`src/report/mod.rs` 既有两个 `Report {` 测试字面量、`src/report/viz.rs` 的 `sample_report` 与 soft 测试的 `SoftReport {` 各加 `walk_forward: None,`。

- [ ] **Step 4: 验证**

Run: `cargo test` → 全量全绿（含新兼容测试）。
Run: `cargo clippy --all-targets` → 无告警（平铺执行，勿用 `2>&1`）。
Run: `cargo run -- backtest --help` → 含 `--folds`。

- [ ] **Step 5: Commit**

```bash
git add src/backtest/runner.rs src/backtest/soft.rs src/report/mod.rs src/report/viz.rs src/cli/mod.rs tests/e2e.rs
git commit -m "feat(backtest,cli): --folds wiring; walk_forward into Report/SoftReport (serde-compatible)" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: HTML 折条形图 + e2e + README

**Files:**
- Modify: `src/report/viz.rs`、`tests/e2e.rs`、`README.md`

- [ ] **Step 1: viz 折图（`render_html` 与 `render_soft_html` 各自 `</body>` 之前）**

```rust
    if let Some(wf) = &report.walk_forward {
        let items: Vec<(String, f64)> = wf
            .folds
            .iter()
            .enumerate()
            .map(|(i, f)| (format!("f{}", i + 1), f.stat.mean_net))
            .collect();
        let _ = write!(s, "{}", bar_chart(&items, "walk-forward 各折 mean_net"));
        let _ = write!(s, "<p>walk-forward: positive {}/{}, worst mean {:.4}</p>", wf.positive_folds, wf.folds.len(), wf.worst_mean_net);
    }
```
并给 viz 既有 `render_soft_html_is_self_contained` 测试的 `SoftReport` 改一份带 `walk_forward: Some(...)` 的变体断言？**不必**——加一个轻量断言即可：在该测试中把 `walk_forward: None` 临时换为：
```rust
            walk_forward: Some(crate::backtest::walkforward::WalkForward {
                folds: vec![crate::backtest::walkforward::FoldMetrics {
                    from: t, to: t, stat: signal_stat(&[0.1]), buy_and_hold: 0.0,
                }],
                positive_folds: 1,
                worst_mean_net: 0.1,
            }),
```
并追加 `assert!(a.contains("walk-forward"));`。

- [ ] **Step 2: e2e（`tests/e2e.rs`）**

把 `soft_mode_yields_positive_engaged_edge` 的 config `folds: 0` 改为 `folds: 3`，断言区追加：
```rust
    let wf = report.walk_forward.as_ref().expect("folds=3 should produce walk_forward");
    assert_eq!(wf.folds.len(), 3);
    assert!(wf.worst_mean_net > 0.0, "uptrend: every non-empty fold should be positive");
    assert!(wf.positive_folds >= 1);
```

- [ ] **Step 3: 验证**

Run: `cargo test --test e2e soft_mode_yields_positive_engaged_edge` → PASS。
Run: `cargo test` → 全量全绿。
Run: `cargo clippy --all-targets` → 无告警。

- [ ] **Step 4: README**（backtest 一节补一段）

````markdown
### Walk-forward（`--folds K`）

`--folds 3` 把决策点按时间等分 3 折，逐折输出 n/mean/hit/buy&hold 与汇总（positive 折数、最差折均值），
HTML 报告附各折 mean_net 条形图——回答"edge 是全程稳定还是一段行情撞的"。
注意：这是**固定树的时间稳定性分析**（树无参数寻优，决策无状态，一次回测分桶即得），
不是含样本内参数优化的完整 WFO；前瞻窗口跨折边界未裁剪（与全局重叠警告同口径）。
````

- [ ] **Step 5: Commit**

```bash
git add src/report/viz.rs tests/e2e.rs README.md
git commit -m "feat(report): walk-forward fold chart in HTML; e2e folds=3; README" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## 附录 A：Spec 覆盖对照（自检）
| Spec § | 实现于 |
|---|---|
| §3.1 纯函数（等分/空折省略/汇总）| Task 1 |
| §3.2 Config/cli/runner/run_soft/字段/serde 兼容/print | Task 2 |
| §3.2 HTML 折图 | Task 3 |
| §4 测试（已知值/n<k/兼容/e2e folds=3）| Task 1/2/3 |
| §1.4 README 诚实标注 | Task 3 |

## 附录 B：明确不在范围（YAGNI）
- 参数寻优/树模板；日历分折；soft position 折线；anchored 窗口；折边界前瞻裁剪。
