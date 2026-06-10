# rquant 叶子概率堆叠面积图 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `report --soft` 新增叶子概率随时间的堆叠面积图（每层一个 polygon，固定调色板+图例），展示质量在叶子间的转移。

**Architecture:** 在 master(HEAD `32add44`)上扩展。`curve.rs` 加 `StackSeries`/`leaf_prob_stack`（累计边界）；`viz.rs` 加 `stacked_area_chart` 并给 `render_soft_html` 加 `stack: Option<&StackSeries>` 参（3 个调用点涟漪：viz 测试、cli、e2e）。

**Tech Stack:** Rust 2024 + 既有。

> 设计依据：`docs/superpowers/specs/2026-06-10-rquant-leaf-stack-chart-design.md`。提交信息用英文。

---

## 文件结构
```
改动: src/report/curve.rs  # + StackSeries / leaf_prob_stack + 测试
改动: src/report/viz.rs    # + PALETTE / stacked_area_chart；render_soft_html 加参 + 测试更新
改动: src/cli/mod.rs       # report --soft 分支构建 stack 并传入
改动: tests/e2e.rs         # soft_report_html_renders 更新调用 + <polygon> 断言
改动: README.md
```

---

## Task 1: curve.rs — StackSeries / leaf_prob_stack

**Files:**
- Modify: `src/report/curve.rs`
- Test: 同文件

- [ ] **Step 1: 在 `mod tests` 加失败测试**

（测试模块已 `use crate::backtest::soft::SoftStepRecord;` 风格的局部 use；参考既有 `derive_soft_series_cumulates_and_skips`。）
```rust
    #[test]
    fn leaf_prob_stack_cumulative_boundaries() {
        use crate::backtest::soft::SoftStepRecord;
        use std::collections::BTreeMap;
        let t = NaiveDateTime::parse_from_str("2024-01-02 09:45:00", "%Y-%m-%d %H:%M:%S").unwrap();
        let mut lp1 = BTreeMap::new(); lp1.insert("b".to_string(), 0.7); lp1.insert("a".to_string(), 0.3);
        let mut lp2 = BTreeMap::new(); lp2.insert("a".to_string(), 1.0);
        let recs = vec![
            SoftStepRecord { t, leaf_probs: lp1, expected_net: Some(0.0) },
            SoftStepRecord { t, leaf_probs: lp2, expected_net: None },
        ];
        let st = leaf_prob_stack(&recs);
        assert_eq!(st.names, vec!["a".to_string(), "b".to_string()]); // 字典序
        assert_eq!(st.rows.len(), 2);
        // 点1: a 累计 0.3，b 累计 1.0；点2: a 累计 1.0，b 累计 1.0
        assert!((st.rows[0][0] - 0.3).abs() < 1e-9);
        assert!((st.rows[0][1] - 1.0).abs() < 1e-9);
        assert!((st.rows[1][0] - 1.0).abs() < 1e-9);
        assert!((st.rows[1][1] - 1.0).abs() < 1e-9);
        // 空 → 空
        let empty = leaf_prob_stack(&[]);
        assert!(empty.names.is_empty() && empty.rows.is_empty());
    }
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test --lib report::curve::tests::leaf_prob_stack_cumulative_boundaries`
Expected: 编译失败（`leaf_prob_stack`/`StackSeries` 未定义）。

- [ ] **Step 3: 实现（`avg_leaf_probs` 之后、测试模块之前）**

```rust
/// 堆叠面积图数据：names = 全体叶名（字典序），rows[i][k] = 第 i 点前 k+1 层的累计概率边界。
pub struct StackSeries {
    pub names: Vec<String>,
    pub rows: Vec<Vec<f64>>,
}

pub fn leaf_prob_stack(records: &[SoftStepRecord]) -> StackSeries {
    if records.is_empty() {
        return StackSeries { names: vec![], rows: vec![] };
    }
    let mut nameset: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for r in records {
        for k in r.leaf_probs.keys() {
            nameset.insert(k.clone());
        }
    }
    let names: Vec<String> = nameset.into_iter().collect();
    let rows = records
        .iter()
        .map(|r| {
            let mut cum = 0.0;
            names
                .iter()
                .map(|n| {
                    cum += r.leaf_probs.get(n).copied().unwrap_or(0.0);
                    cum
                })
                .collect()
        })
        .collect();
    StackSeries { names, rows }
}
```

- [ ] **Step 4: 运行验证通过**

Run: `cargo test --lib report::curve`
Expected: 既有 + 1 新测试 PASS。

- [ ] **Step 5: Commit**

```bash
git add src/report/curve.rs
git commit -m "feat(report): leaf_prob_stack cumulative boundaries for stacked area chart" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: viz stacked_area_chart + render_soft_html 加参 + 涟漪

**Files:**
- Modify: `src/report/viz.rs`、`src/cli/mod.rs`、`tests/e2e.rs`、`README.md`

- [ ] **Step 1: viz 失败测试（`mod tests`）**

```rust
    #[test]
    fn stacked_area_chart_has_polygons_and_legend() {
        use crate::report::curve::StackSeries;
        let st = StackSeries {
            names: vec!["leaf_a".to_string(), "leaf_b".to_string()],
            rows: vec![vec![0.3, 1.0], vec![0.6, 1.0], vec![0.5, 1.0]],
        };
        let svg = stacked_area_chart(&st, "t");
        assert!(svg.contains("<polygon"));
        assert!(svg.contains("leaf_a"));
        assert!(svg.contains("leaf_b"));
        assert_eq!(svg, stacked_area_chart(&st, "t")); // 确定性
    }
```
并把既有 `render_soft_html_is_self_contained` 的调用改为四参并断言堆叠图存在：
```rust
        // （在构造 series/avg 之后）
        let st = crate::report::curve::StackSeries {
            names: vec!["leaf_l".to_string()],
            rows: vec![vec![1.0]],
        };
        let a = render_soft_html(&report, &series, &avg, Some(&st));
        let b = render_soft_html(&report, &series, &avg, Some(&st));
        // 既有断言保留，另加：
        assert!(a.contains("<polygon"));
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test --lib report::viz`
Expected: 编译失败（`stacked_area_chart` 未定义 / render_soft_html 参数不符）。

- [ ] **Step 3: viz 实现**

(a) 顶部 import 把 `use crate::report::curve::{EquitySeries, Histogram};` 扩为 `use crate::report::curve::{EquitySeries, Histogram, StackSeries};`。
(b) 加调色板与图：
```rust
const PALETTE: [&str; 6] = ["#1565c0", "#2e7d32", "#c62828", "#f9a825", "#6a1b9a", "#00838f"];

/// 叶子概率堆叠面积图：y 域固定 [0,1]，每层 polygon（上=本层累计、下=前层累计），图例置顶。
pub fn stacked_area_chart(stack: &StackSeries, title: &str) -> String {
    let pad = 30.0;
    let mut s = String::new();
    let _ = write!(s, "<svg width=\"{W}\" height=\"{H}\" xmlns=\"http://www.w3.org/2000/svg\">");
    let _ = write!(s, "<text x=\"8\" y=\"16\" font-size=\"13\">{title}</text>");
    if stack.rows.is_empty() || stack.names.is_empty() {
        let _ = write!(s, "<text x=\"8\" y=\"{}\">no data</text></svg>", H / 2);
        return s;
    }
    let n = stack.rows.len();
    let px = |i: usize| pad + i as f64 / (n.max(2) - 1) as f64 * (W as f64 - 2.0 * pad);
    let py = |v: f64| ny(v, 0.0, 1.0, pad);
    for (k, name) in stack.names.iter().enumerate() {
        let color = PALETTE[k % PALETTE.len()];
        let mut pts = String::new();
        for (i, row) in stack.rows.iter().enumerate() {
            let _ = write!(pts, "{:.1},{:.1} ", px(i), py(row[k]));
        }
        for (i, row) in stack.rows.iter().enumerate().rev() {
            let lower = if k == 0 { 0.0 } else { row[k - 1] };
            let _ = write!(pts, "{:.1},{:.1} ", px(i), py(lower));
        }
        let _ = write!(s, "<polygon points=\"{}\" fill=\"{}\" fill-opacity=\"0.8\"/>", pts.trim_end(), color);
        // 图例
        let lx = pad + k as f64 * 100.0;
        let _ = write!(s, "<rect x=\"{:.0}\" y=\"22\" width=\"10\" height=\"10\" fill=\"{}\"/>", lx, color);
        let _ = write!(s, "<text x=\"{:.0}\" y=\"31\" font-size=\"10\">{}</text>", lx + 14.0, name);
    }
    let _ = write!(s, "</svg>");
    s
}
```
(c) `render_soft_html` 签名加第四参 `stack: Option<&StackSeries>`，在 avg_leaf 条形之后（`</body>` 之前）加：
```rust
    if let Some(st) = stack {
        let _ = write!(s, "{}", stacked_area_chart(st, "叶子概率随时间（堆叠，Σ=1）"));
    }
```

- [ ] **Step 4: cli + e2e 涟漪**

(a) `src/cli/mod.rs` `Cmd::Report` 软分支：traces 给出时同时构建 stack——把 `(series, avg)` 元组扩成三元：
```rust
                let (series, avg, stack) = match &traces {
                    Some(tp) => {
                        let content = std::fs::read_to_string(tp)?;
                        let mut recs = Vec::new();
                        for line in content.lines().filter(|l| !l.trim().is_empty()) {
                            recs.push(serde_json::from_str::<crate::backtest::soft::SoftStepRecord>(line)?);
                        }
                        (
                            crate::report::curve::derive_soft_series(&recs),
                            crate::report::curve::avg_leaf_probs(&recs),
                            Some(crate::report::curve::leaf_prob_stack(&recs)),
                        )
                    }
                    None => (
                        crate::report::curve::EquitySeries {
                            points: vec![],
                            hist: crate::report::curve::Histogram { bins: vec![] },
                            skipped: 0,
                        },
                        vec![],
                        None,
                    ),
                };
                let html = crate::report::viz::render_soft_html(&rep, &series, &avg, stack.as_ref());
```
(b) `tests/e2e.rs` `soft_report_html_renders`：构建 stack 并传第四参 + 断言：
```rust
    let stack = rquant::report::curve::leaf_prob_stack(&recs);
    let html = rquant::report::viz::render_soft_html(&report, &series, &avg, Some(&stack));
    // 既有断言保留，另加：
    assert!(html.contains("<polygon"), "stacked area chart present");
```

- [ ] **Step 5: 全量验证**

Run: `cargo test`
Expected: 全量全绿。
Run: `cargo clippy --all-targets`
Expected: 无告警（平铺执行，勿用 `2>&1`）。
Run: `cargo run -- report --help`
Expected: 正常（无签名外泄）。

- [ ] **Step 6: README**（软报告一节补一句）

````markdown
软报告还含**叶子概率堆叠面积图**（质量随时间在叶子间的转移；Σ=1 恒满幅，固定调色板+图例）。
````

- [ ] **Step 7: Commit**

```bash
git add src/report/viz.rs src/cli/mod.rs tests/e2e.rs README.md
git commit -m "feat(report): leaf-probability stacked area chart in soft HTML" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## 附录 A：Spec 覆盖对照（自检）
| Spec § | 实现于 |
|---|---|
| §1.1 leaf_prob_stack（并集字典序/累计边界/空）| Task 1 |
| §1.2 stacked_area_chart（polygon/调色板/图例/确定性）| Task 2 |
| §1.3 render_soft_html 加参 + 3 调用点涟漪 | Task 2 |
| §4 测试 | Task 1/2 |

## 附录 B：明确不在范围（YAGNI）
- 交互/悬浮；自适应配色；硬模式版本；降采样。
