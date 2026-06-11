# rquant report --sim/--portfolio HTML Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `report` 子命令补全 `--sim`/`--portfolio` 两种 HTML（净值/仓位曲线、回合直方图与回合表；组合 vs 基准双线、选中频率、持仓表），`ReportMode` 四模式互斥。

**Architecture:** 在 master(HEAD `7f2d8d9`)上收口。`curve::histogram` 内核提为 `histogram_of`；新图元 `multi_line_chart`；两个 render 函数全复用既有图元/外壳风格；`render_report_files(soft: bool)` 升级 `ReportMode` 枚举（机械涟漪）。spec：`docs/superpowers/specs/2026-06-11-rquant-report-sim-portfolio-html-design.md`。

**Tech Stack:** Rust 2024 + 既有。

---

## 文件结构
```
改动: src/report/curve.rs   # histogram → histogram_of 内核提取（pub(crate)）
改动: src/report/viz.rs     # + multi_line_chart / render_sim_html / render_portfolio_html + 测试
改动: src/report/mod.rs     # render_report_files: ReportMode 枚举 + sim/portfolio 臂
改动: src/cli/mod.rs        # Report 加 --sim/--portfolio + 互斥校验
改动: tests/e2e.rs、docs/cli-reference.md、README.md
```

---

## Task 1: histogram_of 提取 + multi_line_chart

**Files:**
- Modify: `src/report/curve.rs`、`src/report/viz.rs`

- [ ] **Step 1: curve.rs 内核提取（行为不变，既有测试是保护网）**

```rust
/// 对一组数值做固定 21 桶直方图（原 histogram(points) 的内核）。
pub(crate) fn histogram_of(values: &[f64]) -> Histogram {
    if values.is_empty() {
        return Histogram { bins: vec![] };
    }
    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if (max - min).abs() < 1e-12 {
        return Histogram { bins: vec![(min, max, values.len())] };
    }
    const N: usize = 21;
    let width = (max - min) / N as f64;
    let mut counts = [0usize; N];
    for &x in values {
        let mut k = ((x - min) / width) as usize;
        if k >= N {
            k = N - 1;
        }
        counts[k] += 1;
    }
    let bins = (0..N).map(|k| (min + k as f64 * width, min + (k + 1) as f64 * width, counts[k])).collect();
    Histogram { bins }
}

fn histogram(points: &[SeriesPoint]) -> Histogram {
    let nets: Vec<f64> = points.iter().map(|p| p.net).collect();
    histogram_of(&nets)
}
```
（把原 `histogram` 函数体替换为以上两段；既有 curve 测试必须不改而过。）

- [ ] **Step 2: viz.rs `multi_line_chart`（RED 测试先行）**

测试：
```rust
    #[test]
    fn multi_line_chart_two_lines_with_legend() {
        let series = vec![
            ("组合".to_string(), vec![(0.0, 1.0), (1.0, 1.05), (2.0, 1.02)]),
            ("基准".to_string(), vec![(0.0, 1.0), (1.0, 0.98), (2.0, 0.99)]),
        ];
        let svg = multi_line_chart(&series, "t");
        assert_eq!(svg.matches("<polyline").count(), 2);
        assert!(svg.contains("组合") && svg.contains("基准"));
        assert_eq!(svg, multi_line_chart(&series, "t")); // 确定性
    }
```
实现（PALETTE 已存在；`ny`/`W`/`H` 复用；风格仿 `line_chart`）：
```rust
/// 多线折线图（≤PALETTE 数线），PALETTE 着色 + 顶部图例；y 域 = 全体点 min/max。
pub fn multi_line_chart(series: &[(String, Vec<(f64, f64)>)], title: &str) -> String {
    let pad = 30.0;
    let mut s = String::new();
    let _ = write!(s, "<svg width=\"{W}\" height=\"{H}\" xmlns=\"http://www.w3.org/2000/svg\">");
    let _ = write!(s, "<text x=\"8\" y=\"16\" font-size=\"13\">{title}</text>");
    let all: Vec<f64> = series.iter().flat_map(|(_, pts)| pts.iter().map(|p| p.1)).collect();
    if all.is_empty() {
        let _ = write!(s, "<text x=\"8\" y=\"{}\">no data</text></svg>", H / 2);
        return s;
    }
    let ymin = all.iter().copied().fold(f64::INFINITY, f64::min);
    let ymax = all.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let n = series.iter().map(|(_, p)| p.len()).max().unwrap_or(2).max(2);
    let px = |i: usize| pad + i as f64 / (n - 1) as f64 * (W as f64 - 2.0 * pad);
    for (k, (name, pts)) in series.iter().enumerate() {
        let color = PALETTE[k % PALETTE.len()];
        let path: Vec<String> = pts.iter().enumerate().map(|(i, p)| format!("{:.1},{:.1}", px(i), ny(p.1, ymin, ymax, pad))).collect();
        let _ = write!(s, "<polyline fill=\"none\" stroke=\"{}\" stroke-width=\"1.5\" points=\"{}\"/>", color, path.join(" "));
        let lx = pad + k as f64 * 100.0;
        let _ = write!(s, "<rect x=\"{:.0}\" y=\"22\" width=\"10\" height=\"10\" fill=\"{}\"/>", lx, color);
        let _ = write!(s, "<text x=\"{:.0}\" y=\"31\" font-size=\"10\">{}</text>", lx + 14.0, name);
    }
    let _ = write!(s, "</svg>");
    s
}
```

- [ ] **Step 3: GREEN + clippy + Commit**

```bash
git add src/report/curve.rs src/report/viz.rs
git commit -m "feat(report): extract histogram_of; multi_line_chart primitive" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: render_sim_html + render_portfolio_html

**Files:**
- Modify: `src/report/viz.rs`（import `SimReport/SimStepRecord/PortfolioReport` + 两函数 + 测试）

- [ ] **Step 1: RED 测试（构造小报告，仿 `render_soft_html_is_self_contained` 风格）**

```rust
    #[test]
    fn render_sim_html_with_and_without_steps() {
        use crate::backtest::sim::{RoundTrip, SimReport, SimStepRecord};
        use chrono::NaiveDate;
        let t0 = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(10, 0, 0).unwrap();
        let trip = |r: f64| RoundTrip {
            entry_t: t0, exit_t: t0, entry_px: 10.0, exit_px: 10.0,
            max_abs_pos: 1.0, trip_return: r, bars_held: 2, reason: "tree".into(),
        };
        let rep = SimReport {
            tree_name: "simviz".into(), cost_bps: 10.0, total_return: 0.1, max_drawdown: 0.05,
            n_round_trips: 2, win_rate: 0.5, avg_hold_bars: 2.0, turnover: 4.0, buy_and_hold: 0.02,
            trades: vec![trip(0.05), trip(-0.01)],
        };
        let steps = vec![
            SimStepRecord { t: t0, target: 1.0, pos: 1.0, nav: 1.0 },
            SimStepRecord { t: t0, target: 1.0, pos: 1.0, nav: 1.02 },
        ];
        let a = render_sim_html(&rep, Some(&steps));
        assert_eq!(a, render_sim_html(&rep, Some(&steps))); // 确定性
        assert!(a.contains("<!doctype html>") && a.contains("simviz"));
        assert!(a.contains("<polyline")); // 净值曲线
        assert!(a.contains("<rect"));     // 直方图
        assert!(a.contains("tree"));      // 回合表
        let b = render_sim_html(&rep, None);
        assert!(!b.contains("<polyline") && b.contains("未提供")); // 占位
    }

    #[test]
    fn render_portfolio_html_self_contained() {
        use crate::backtest::portfolio::{HoldingsRecord, PortfolioReport};
        use chrono::NaiveDate;
        let t0 = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(10, 0, 0).unwrap();
        let h = |nav: f64, b: f64, sel: &str| HoldingsRecord {
            t: t0, nav, benchmark_nav: b, selected: vec![(sel.to_string(), 1.0)],
        };
        let rep = PortfolioReport {
            tree_name: "pfviz".into(), cost_bps: 10.0, top_n: 1, rebalance: 4,
            n_rebalances: 3, avg_members: 1.0, total_return: 0.06, max_drawdown: 0.02,
            turnover: 3.0, benchmark_return: 0.01,
            holdings: vec![h(1.0, 1.0, "A"), h(1.03, 1.0, "A"), h(1.06, 1.01, "B")],
        };
        let a = render_portfolio_html(&rep);
        assert_eq!(a, render_portfolio_html(&rep));
        assert!(a.contains("pfviz"));
        assert_eq!(a.matches("<polyline").count(), 2); // 组合+基准双线
        assert!(a.contains("<rect"));                  // 频率条形（A:2/3, B:1/3）+ 图例 rect
        assert!(a.contains(">A<") || a.contains("A")); // 持仓表
    }
```

- [ ] **Step 2: 实现两函数（外壳/表风格逐字仿 `render_soft_html`；import 路径按需补）**

`render_sim_html(report: &SimReport, steps: Option<&[SimStepRecord]>) -> String`：
1. 外壳 + `<h1>rquant sim report: {tree_name}</h1>`。
2. headline 表 7 行：total_return/max_drawdown/n_round_trips/win_rate(%)/avg_hold_bars/turnover/buy&hold。
3. `match steps`：Some → `line_chart(nav 点列, "净值曲线（顺序权益）")` + `line_chart(pos 点列, "仓位轨迹")`；None → `<p>（未提供 --traces，省略净值/仓位曲线）</p>`。
4. `histogram_svg(&crate::report::curve::histogram_of(&trip_returns), "回合收益分布")`（trades 的 trip_return；trades 空 → histogram_of 给空桶 → 图元自己画 no data）。
5. 回合表：表头 entry_t/exit_t/entry_px/exit_px/trip_return/bars_held/reason；`trades.iter().take(50)` 行，若 `trades.len() > 50` 表后加 `<p>（共 {n} 回合，仅显示前 50）</p>`。
`render_portfolio_html(report: &PortfolioReport) -> String`：
1. 外壳 + `<h1>rquant portfolio report: {tree_name}</h1>`。
2. headline 表 7 行：total/benchmark/超额(差)/max_drawdown/turnover/n_rebalances/avg_members。
3. `multi_line_chart([("组合", nav 点列), ("基准", benchmark_nav 点列)], "组合 vs 基准净值（x=调仓序）")`。
4. 选中频率：`BTreeMap<String, usize>` 统计 holdings[].selected 符号次数 → `bar_chart(次数/n_rebalances, "选中频率")`。
5. 持仓表：t / selected（`sym(score)` 逗号连接，score `{:.3}`）；`take(50)` + 截断注明。

- [ ] **Step 3: GREEN + clippy + Commit**

```bash
git add src/report/viz.rs
git commit -m "feat(report): render_sim_html and render_portfolio_html" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: ReportMode + CLI 互斥 + e2e + 文档 + smoke

**Files:**
- Modify: `src/report/mod.rs`、`src/cli/mod.rs`、`tests/e2e.rs`、`docs/cli-reference.md`、`README.md`

- [ ] **Step 1: render_report_files 升级**

`src/report/mod.rs`：
```rust
/// report 子命令的渲染模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportMode { Hard, Soft, Sim, Portfolio }
```
`render_report_files` 第 5 参 `soft: bool` → `mode: ReportMode`；函数体 `match mode`：`Hard`/`Soft` 臂 = 现有两分支逐字搬入；`Sim` 臂：读 `SimReport`，traces 给出 → 逐行 `SimStepRecord`，primary 给出 → eprintln 忽略提示，`render_sim_html` 写出 + println；`Portfolio` 臂：读 `PortfolioReport`（traces/primary 给出 → 各 eprintln 忽略提示），`render_portfolio_html` 写出 + println。
涟漪：grep `render_report_files(`——cli 调用 + e2e `render_report_files_soft_end_to_end`（`true` → `ReportMode::Soft`）。

- [ ] **Step 2: CLI**

`Cmd::Report` 加：
```rust
        /// Render a sim_report.json (use with --traces for nav/pos curves)
        #[arg(long, default_value_t = false)]
        sim: bool,
        /// Render a portfolio.json (self-contained)
        #[arg(long, default_value_t = false)]
        portfolio: bool,
```
分流（解构加 `sim, portfolio`）：
```rust
        Cmd::Report { report, out, traces, primary, soft, sim, portfolio } => {
            let picked = [soft, sim, portfolio].iter().filter(|b| **b).count();
            if picked > 1 {
                return Err(anyhow::anyhow!("--soft / --sim / --portfolio are mutually exclusive"));
            }
            let mode = if soft {
                crate::report::ReportMode::Soft
            } else if sim {
                crate::report::ReportMode::Sim
            } else if portfolio {
                crate::report::ReportMode::Portfolio
            } else {
                crate::report::ReportMode::Hard
            };
            crate::report::render_report_files(&report, &out, traces.as_deref(), primary.as_deref(), mode)?;
        }
```

- [ ] **Step 3: e2e（`tests/e2e.rs`）**

- `sim_report_html_renders`：复用 `sim_full_chain` fixture 思路（带 `traces_path: Some`），`run_sim` 后 `render_report_files(out_f, html_f, Some(traces_f), None, ReportMode::Sim)` → HTML 含 `<polyline` 与 `回合`。
- `portfolio_report_html_renders`：复用 `portfolio_full_chain` fixture，`run_portfolio` 后 `render_report_files(out_f, html_f, None, None, ReportMode::Portfolio)` → HTML `<polyline` 计数 == 2、含 "基准"。
- 既有 `render_report_files_soft_end_to_end` 改传 `ReportMode::Soft`（行为不变）。

- [ ] **Step 4: 验证 + 文档 + smoke**

`cargo test` 全绿；clippy 干净；`cargo run -- report --help` 含两新旗标。
文档：cli-reference 的 report 节改为四模式表（hard/soft/sim/portfolio：输入文件、是否需 traces/primary、产出图表清单）；README report 节补两行。
真数据 smoke（手动不入库）：复用 E4/E5 smoke 命令各加 `report --sim`/`--portfolio` 步骤，检查 HTML 关键子串后清理。

- [ ] **Step 5: Commit**

```bash
git add src/report/mod.rs src/cli/mod.rs tests/e2e.rs docs/cli-reference.md README.md
git commit -m "feat(cli,report): ReportMode with --sim/--portfolio HTML rendering; e2e + docs" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## 附录 A：Spec 覆盖对照（自检）
| Spec § | 实现于 |
|---|---|
| §2.1 histogram_of | Task 1 |
| §2.2 multi_line_chart / render_sim_html / render_portfolio_html | Task 1/2 |
| §2.3 ReportMode + CLI 互斥 + 忽略提示 | Task 3 |
| §3 测试（提取等价/双线/两 render/e2e/互斥/smoke）| Task 1-3 |

## 附录 B：明确不在范围（YAGNI）
- 交互；回撤独立曲线；逐期权重堆叠；表分页。
