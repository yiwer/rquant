# Eval 门槛裁决机制实现计划（Phase-1）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把严格 WFO 五门槛从文档方法学固化为代码，`rquant eval` 读 N 个标的的 optimize fold JSON、产出策略级机器裁决（certified + 逐门槛 pass/fail + 证据）。

**Architecture:** 三组件——optimize 加 opt-in `--auto-extend N`（默认关＝行为冻结）做门槛④边界逃逸并把内点证据写进输出；新 `verdict` 纯函数库出五门槛 `Verdict`；`rquant eval` 薄壳读 JSON、调 certify、打印 + 退出码（CI 门）。策略级模型：N 个标的 JSON → 一份裁决，每门槛聚合各标的证据。

**Tech Stack:** Rust 2024、serde/serde_json、clap（CLI derive）、tokio（optimize 异步）。设计文档：`docs/superpowers/specs/2026-06-14-eval-gate-verdict-design.md`。

**规划期修正（相对 spec）：** spec 写的新模块路径 `src/eval/gates.rs` 与**既有** `src/eval/`（节点求值 quant/llm）撞名 → 改用 `src/verdict/`。CLI 子命令对外名仍是 `eval`，阈值/公式/语义与 spec §7 完全一致。

---

## 文件结构

| 文件 | 责任 | 改动 |
|---|---|---|
| `src/optimize/mod.rs` | WFO 寻优 | 加 `AxisOutcome` 类型、`OptimizeReport.axes`/`.primary` 字段、`--auto-extend` 算法；`OptimizeConfig.auto_extend` |
| `src/verdict/mod.rs` | 裁决类型 + 入口 | 新建：`GateThresholds`/`GateStatus`/`GateOutcome`/`Verdict` + `certify()` |
| `src/verdict/gates.rs` | 五门槛纯函数 | 新建：`gate_os_breadth`/`gate_degradation`/`gate_param_drift`/`gate_interior`/`gate_not_single` + 取值辅助 |
| `src/lib.rs` | crate 模块表 | 加 `pub mod verdict;` |
| `src/cli/mod.rs` | CLI | 加 `Cmd::Eval` 变体 + 处理臂；`Cmd::Optimize` 加 `--auto-extend` |
| `docs/cli-reference.md` | 文档 | eval 子命令 + optimize `--auto-extend` |

---

## Task 1: OptimizeReport schema —— axes/primary 字段 + AxisOutcome 类型

**Files:**
- Modify: `src/optimize/mod.rs`（`OptimizeReport` 结构 ~303、构造点 ~591、`OptimizeConfig` ~320）

- [ ] **Step 1: 写失败测试**（OptimizeReport 新字段默认值）

加到 `src/optimize/mod.rs` 的 `#[cfg(test)] mod tests`（文件末尾已有测试模块；若无则新建）：

```rust
#[test]
fn optimize_report_new_fields_default_empty() {
    // 旧 JSON（无 axes/primary）反序列化 → 字段取默认（serde default）
    let json = r#"{
        "mode":"sim","objective_name":"sharpe_or_total_return","folds":4,"n_combos":12,
        "fold_results":[],"os_mean_objective":null,"full_sample_best":null,"drift":[],"is_top5":[]
    }"#;
    let r: OptimizeReport = serde_json::from_str(json).unwrap();
    assert!(r.axes.is_empty(), "axes 默认空");
    assert_eq!(r.primary, "", "primary 默认空串");
}

#[test]
fn axis_outcome_roundtrips() {
    let a = AxisOutcome {
        name: "n_s".into(),
        final_values: vec![40.0, 55.0, 60.0, 90.0],
        best_value: Some(55.0),
        interior: true,
        extended_steps: 0,
    };
    let s = serde_json::to_string(&a).unwrap();
    let b: AxisOutcome = serde_json::from_str(&s).unwrap();
    assert_eq!(b.best_value, Some(55.0));
    assert!(b.interior);
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p rquant optimize_report_new_fields_default_empty axis_outcome_roundtrips`
Expected: 编译失败（`AxisOutcome` 未定义、`r.axes`/`r.primary` 无此字段）。

- [ ] **Step 3: 加 AxisOutcome 类型 + OptimizeReport 字段**

在 `src/optimize/mod.rs` 的 `ParamDrift` 定义之后加：

```rust
/// 单条网格轴的内部最优分析结果（仅 --auto-extend 时填充）。
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AxisOutcome {
    pub name: String,
    /// 延伸后该轴实际候选值（升序）。
    pub final_values: Vec<f64>,
    /// 全样本最优在该轴的取值。
    pub best_value: Option<f64>,
    /// best 是否为内部最优（内点收敛 / IS 转劣确认峰值=true；达 N 步仍贴边=false）。
    pub interior: bool,
    /// 实际追加的延伸步数（0=无需延伸）。
    pub extended_steps: usize,
}
```

在 `OptimizeReport`（`pub is_top5: Vec<Vec<ComboScore>>,` 之后）加两个字段：

```rust
    /// 每条网格轴的内部最优分析（仅 --auto-extend；否则空）。
    #[serde(default)]
    pub axes: Vec<AxisOutcome>,
    /// 主数据标识（primary 路径字符串），eval 用作 symbol 标签。
    #[serde(default)]
    pub primary: String,
```

- [ ] **Step 4: 在 OptimizeConfig 加 auto_extend 字段**

在 `OptimizeConfig`（`pub out_path: PathBuf,` 之前）加：

```rust
    /// --auto-extend N：门槛④边界逃逸最大步数（0=关，行为冻结）。
    pub auto_extend: usize,
```

- [ ] **Step 5: 构造点填充新字段**

`run_optimize` 末尾 `let report = OptimizeReport { ... is_top5: is_top5_all, };` 改为（Task 9 再填 axes）：

```rust
    let report = OptimizeReport {
        mode: mode_str.to_string(),
        objective_name: obj_name.to_string(),
        folds: k,
        n_combos,
        fold_results,
        os_mean_objective,
        full_sample_best,
        drift,
        is_top5: is_top5_all,
        axes: Vec::new(), // Task 9 由 auto-extend 填充
        primary: cfg.primary_path.to_string_lossy().to_string(),
    };
```

- [ ] **Step 6: 修复 OptimizeConfig 的现有构造点**

`src/cli/mod.rs` 的 `Cmd::Optimize` 臂里 `OptimizeConfig { ... out_path: out, }` 暂加 `auto_extend: 0,`（Task 10 接 CLI 旗标）：

```rust
            let ocfg = OptimizeConfig {
                tree_path: tree,
                primary_path: primary,
                context_path: context,
                news_path: news,
                aux_paths,
                window,
                warmup,
                cost_bps,
                folds,
                sim,
                soft,
                grids: grid,
                max_combos,
                out_path: out,
                auto_extend: 0,
            };
```

- [ ] **Step 7: 运行测试确认通过 + 行为冻结**

Run: `cargo test -p rquant optimize`
Expected: 新两测试 PASS；**既有 optimize 测试全部不变 PASS**（新字段 serde default，旧 JSON 反序列化兼容，输出多两字段不破坏既有断言）。

- [ ] **Step 8: 提交**

```bash
git add src/optimize/mod.rs src/cli/mod.rs
git commit -m "feat(optimize): add AxisOutcome + report.axes/primary fields (serde default)"
```

---

## Task 2: verdict 模块骨架 + 裁决类型

**Files:**
- Create: `src/verdict/mod.rs`
- Modify: `src/lib.rs`（加 `pub mod verdict;`）

- [ ] **Step 1: 写失败测试**（GateThresholds::default 编码文档方法学）

新建 `src/verdict/mod.rs`，先放测试：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thresholds_default_encodes_methodology() {
        let t = GateThresholds::default();
        assert_eq!(t.os_positive_symbol_frac, 0.6);
        assert_eq!(t.min_degradation, 0.5);
        assert_eq!(t.degradation_symbol_frac, 0.6);
        assert_eq!(t.drift_stable_unique_frac, 0.5);
        assert_eq!(t.drift_stable_symbol_frac, 0.6);
        assert_eq!(t.drift_consensus_frac, 0.6);
        assert_eq!(t.interior_symbol_frac, 0.6);
        assert_eq!(t.max_single_symbol_os_share, 0.5);
    }

    #[test]
    fn gate_status_serializes_lowercase() {
        assert_eq!(serde_json::to_string(&GateStatus::Pass).unwrap(), "\"pass\"");
        assert_eq!(serde_json::to_string(&GateStatus::Indeterminate).unwrap(), "\"indeterminate\"");
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p rquant verdict::`
Expected: 编译失败（模块/类型未定义）。

- [ ] **Step 3: 写类型 + Default + 注册模块**

`src/verdict/mod.rs` 顶部（测试模块之前）：

```rust
//! WFO 五门槛策略级自动裁决（设计 2026-06-14-eval-gate-verdict-design.md §7）。
//! 纯函数：吃 N 个 (symbol, OptimizeReport) → Verdict。无 IO。

use crate::optimize::OptimizeReport;
use serde::{Deserialize, Serialize};

pub mod gates;

/// 五门槛阈值（::default() = 文档方法学编码；Phase-1 不暴露 CLI）。
#[derive(Debug, Clone)]
pub struct GateThresholds {
    pub os_positive_symbol_frac: f64,    // ① ≥60% 标的有 OS 正折
    pub min_degradation: f64,            // ② 退化比下限
    pub degradation_symbol_frac: f64,    // ② 健康标的占比下限
    pub drift_stable_unique_frac: f64,   // ③ 参数 n_unique ≤ ⌈frac×OS折数⌉ 为稳
    pub drift_stable_symbol_frac: f64,   // ③ 稳标的占比下限
    pub drift_consensus_frac: f64,       // ③ 跨标的众数共识下限
    pub interior_symbol_frac: f64,       // ④ 内点标的占比下限
    pub max_single_symbol_os_share: f64, // ⑤ 单标的正 OS 份额上限
}

impl Default for GateThresholds {
    fn default() -> Self {
        Self {
            os_positive_symbol_frac: 0.6,
            min_degradation: 0.5,
            degradation_symbol_frac: 0.6,
            drift_stable_unique_frac: 0.5,
            drift_stable_symbol_frac: 0.6,
            drift_consensus_frac: 0.6,
            interior_symbol_frac: 0.6,
            max_single_symbol_os_share: 0.5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GateStatus {
    Pass,
    Fail,
    Indeterminate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateOutcome {
    pub gate: String,
    pub status: GateStatus,
    pub value: f64,
    pub threshold: f64,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Verdict {
    pub strategy: String,
    pub n_symbols: usize,
    pub certified: bool,
    pub gates: Vec<GateOutcome>,
    pub failed_gates: Vec<String>,
}

/// 入口：五门槛全 Pass → certified。Indeterminate ≠ Pass（保守）。
pub fn certify(reports: &[(String, OptimizeReport)], strategy: &str, th: &GateThresholds) -> Verdict {
    let gates = vec![
        gates::gate_os_breadth(reports, th),
        gates::gate_degradation(reports, th),
        gates::gate_param_drift(reports, th),
        gates::gate_interior(reports, th),
        gates::gate_not_single(reports, th),
    ];
    let certified = gates.iter().all(|g| g.status == GateStatus::Pass);
    let failed_gates = gates
        .iter()
        .filter(|g| g.status != GateStatus::Pass)
        .map(|g| g.gate.clone())
        .collect();
    Verdict {
        strategy: strategy.to_string(),
        n_symbols: reports.len(),
        certified,
        gates,
        failed_gates,
    }
}
```

`src/lib.rs` 加（紧邻 `pub mod optimize;` 等模块声明处）：

```rust
pub mod verdict;
```

> 注：`certify` 现在引用 `gates::*`，Task 3-7 填这些函数前编译不过 —— Task 3 起逐个补齐。本任务先建一个 `src/verdict/gates.rs` 占位空文件，避免模块缺失（内容由 Task 3 填）。新建空 `src/verdict/gates.rs` 仅含：`use crate::optimize::OptimizeReport; use super::{GateOutcome, GateStatus, GateThresholds};`（暂留 `#![allow(unused)]`）。

- [ ] **Step 4: 占位 gates.rs 让模块编译**

新建 `src/verdict/gates.rs`：

```rust
//! 五门槛纯函数（设计 §7.2）。
#![allow(unused)] // Task 3-7 逐个实现后移除
use super::{GateOutcome, GateStatus, GateThresholds};
use crate::optimize::OptimizeReport;
```

并在 `certify` 中暂时注释掉对 5 个 gate 函数的调用、用空 `vec![]` 占位，使 Task 2 可独立编译通过测试：

```rust
    let gates: Vec<GateOutcome> = vec![]; // Task 3-7 起替换为 5 个 gate 调用
```

- [ ] **Step 5: 运行确认通过**

Run: `cargo test -p rquant verdict::`
Expected: 两测试 PASS。

- [ ] **Step 6: 提交**

```bash
git add src/verdict/mod.rs src/verdict/gates.rs src/lib.rs
git commit -m "feat(verdict): gate types + thresholds + certify skeleton"
```

---

## Task 3: 门槛① T1_os_breadth + 测试辅助

**Files:**
- Modify: `src/verdict/gates.rs`

- [ ] **Step 1: 写失败测试**

在 `src/verdict/gates.rs` 末尾加测试模块（含后续门槛复用的构造辅助）：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::optimize::{AxisOutcome, ComboScore, FoldResult, OptimizeReport, ParamDrift};
    use chrono::NaiveDate;
    use std::collections::BTreeMap;

    fn dt() -> chrono::NaiveDateTime {
        NaiveDate::from_ymd_opt(2025, 1, 1).unwrap().and_hms_opt(0, 0, 0).unwrap()
    }

    /// 构造一份只关心 os/is/degradation 与 drift/axes 的报告。
    pub(super) fn mk_report(
        primary: &str,
        folds_os_is_deg: &[(Option<f64>, Option<f64>, Option<f64>)],
        drift: Vec<ParamDrift>,
        full_best: Option<ComboScore>,
        axes: Vec<AxisOutcome>,
    ) -> OptimizeReport {
        let fold_results = folds_os_is_deg
            .iter()
            .enumerate()
            .map(|(i, (os, is, deg))| FoldResult {
                fold: i + 2,
                is_from: dt(), is_to: dt(), os_from: dt(), os_to: dt(),
                best_params: None,
                is_objective: *is,
                os_objective: *os,
                degradation: *deg,
            })
            .collect();
        OptimizeReport {
            mode: "sim".into(), objective_name: "sharpe".into(),
            folds: folds_os_is_deg.len() + 1, n_combos: 1,
            fold_results, os_mean_objective: None, full_sample_best: full_best,
            drift, is_top5: vec![], axes, primary: primary.into(),
        }
    }

    fn os_only(primary: &str, os: &[f64]) -> (String, OptimizeReport) {
        let folds: Vec<_> = os.iter().map(|v| (Some(*v), Some(1.0), None)).collect();
        (primary.into(), mk_report(primary, &folds, vec![], None, vec![]))
    }

    #[test]
    fn t1_breadth_pass_when_60pct_symbols_have_positive_os() {
        // 6/10 标的有 ≥1 正 OS 折 → 0.6 ≥ 0.6 Pass
        let reps: Vec<_> = (0..10)
            .map(|i| {
                let os = if i < 6 { vec![1.0, -1.0] } else { vec![-1.0, -1.0] };
                os_only(&format!("s{i}"), &os)
            })
            .collect();
        let g = gate_os_breadth(&reps, &GateThresholds::default());
        assert_eq!(g.status, GateStatus::Pass);
        assert!((g.value - 0.6).abs() < 1e-9);
    }

    #[test]
    fn t1_breadth_fail_when_below_threshold() {
        let reps: Vec<_> = (0..10)
            .map(|i| {
                let os = if i < 5 { vec![1.0] } else { vec![-1.0] };
                os_only(&format!("s{i}"), &os)
            })
            .collect();
        let g = gate_os_breadth(&reps, &GateThresholds::default());
        assert_eq!(g.status, GateStatus::Fail);
        assert!((g.value - 0.5).abs() < 1e-9);
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p rquant verdict::gates`
Expected: 编译失败（`gate_os_breadth` 未定义）。

- [ ] **Step 3: 实现 gate_os_breadth + 辅助**

在 `src/verdict/gates.rs`（测试模块之前）加：

```rust
/// 该标的是否有 ≥1 个 OS 正折。
fn symbol_has_positive_os(r: &OptimizeReport) -> bool {
    r.fold_results.iter().any(|f| f.os_objective.is_some_and(|v| v > 0.0))
}

/// ① OS 广度：有 OS 正折的标的占比 ≥ 阈值。
pub fn gate_os_breadth(reports: &[(String, OptimizeReport)], th: &GateThresholds) -> GateOutcome {
    let n = reports.len();
    let positive = reports.iter().filter(|(_, r)| symbol_has_positive_os(r)).count();
    let value = if n == 0 { 0.0 } else { positive as f64 / n as f64 };
    let status = if n == 0 {
        GateStatus::Indeterminate
    } else if value >= th.os_positive_symbol_frac {
        GateStatus::Pass
    } else {
        GateStatus::Fail
    };
    GateOutcome {
        gate: "T1_os_breadth".into(),
        status,
        value,
        threshold: th.os_positive_symbol_frac,
        note: format!("{positive}/{n} symbols have >=1 positive OS fold"),
    }
}
```

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p rquant verdict::gates`
Expected: 两测试 PASS。

- [ ] **Step 5: 提交**

```bash
git add src/verdict/gates.rs
git commit -m "feat(verdict): gate T1_os_breadth"
```

---

## Task 4: 门槛② T2_degradation

**Files:**
- Modify: `src/verdict/gates.rs`

- [ ] **Step 1: 写失败测试**（加到 `gates.rs` 测试模块）

```rust
    #[test]
    fn t2_degradation_pass_when_majority_healthy() {
        // 7/10 标的中位退化比 >0.5 → 0.7 ≥ 0.6 Pass
        let reps: Vec<_> = (0..10)
            .map(|i| {
                let deg = if i < 7 { 0.8 } else { 0.2 };
                let folds = vec![(Some(1.0), Some(1.0), Some(deg)), (Some(1.0), Some(1.0), Some(deg))];
                (format!("s{i}"), mk_report(&format!("s{i}"), &folds, vec![], None, vec![]))
            })
            .collect();
        let g = gate_degradation(&reps, &GateThresholds::default());
        assert_eq!(g.status, GateStatus::Pass);
    }

    #[test]
    fn t2_degradation_indeterminate_when_too_few_valid() {
        // 多数标的全 None degradation → Indeterminate
        let reps: Vec<_> = (0..10)
            .map(|i| {
                let folds = vec![(Some(1.0), Some(1.0), None), (Some(1.0), Some(1.0), None)];
                (format!("s{i}"), mk_report(&format!("s{i}"), &folds, vec![], None, vec![]))
            })
            .collect();
        let g = gate_degradation(&reps, &GateThresholds::default());
        assert_eq!(g.status, GateStatus::Indeterminate);
    }
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p rquant verdict::gates::tests::t2`
Expected: 编译失败（`gate_degradation` 未定义）。

- [ ] **Step 3: 实现**

```rust
fn median_sorted(xs: &mut [f64]) -> f64 {
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = xs.len();
    if n % 2 == 1 { xs[n / 2] } else { (xs[n / 2 - 1] + xs[n / 2]) / 2.0 }
}

/// 标的非空 per-fold degradation 的中位数（无有效折 → None）。
fn symbol_degradation_median(r: &OptimizeReport) -> Option<f64> {
    let mut ds: Vec<f64> = r.fold_results.iter().filter_map(|f| f.degradation).collect();
    if ds.is_empty() { None } else { Some(median_sorted(&mut ds)) }
}

/// ② 退化比：中位退化 >min 的"健康"标的占可判定标的的比例 ≥ 阈值。
pub fn gate_degradation(reports: &[(String, OptimizeReport)], th: &GateThresholds) -> GateOutcome {
    let n = reports.len();
    let mut determinate = 0usize;
    let mut healthy = 0usize;
    for (_, r) in reports {
        if let Some(med) = symbol_degradation_median(r) {
            determinate += 1;
            if med > th.min_degradation { healthy += 1; }
        }
    }
    // 多数标的无有效退化折 → 无法判定（保守）
    if determinate < n.div_ceil(2) || determinate == 0 {
        return GateOutcome {
            gate: "T2_degradation".into(),
            status: GateStatus::Indeterminate,
            value: 0.0,
            threshold: th.degradation_symbol_frac,
            note: format!("only {determinate}/{n} symbols have valid degradation folds"),
        };
    }
    let value = healthy as f64 / determinate as f64;
    let status = if value >= th.degradation_symbol_frac { GateStatus::Pass } else { GateStatus::Fail };
    GateOutcome {
        gate: "T2_degradation".into(),
        status,
        value,
        threshold: th.degradation_symbol_frac,
        note: format!("{healthy}/{determinate} determinate symbols have median degradation > {}", th.min_degradation),
    }
}
```

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p rquant verdict::gates`
Expected: 全 PASS。

- [ ] **Step 5: 提交**

```bash
git add src/verdict/gates.rs
git commit -m "feat(verdict): gate T2_degradation"
```

---

## Task 5: 门槛③ T3_param_drift

**Files:**
- Modify: `src/verdict/gates.rs`

- [ ] **Step 1: 写失败测试**

```rust
    fn drift1(name: &str, n_unique: usize) -> crate::optimize::ParamDrift {
        crate::optimize::ParamDrift { name: name.into(), values: vec![], n_unique }
    }
    fn best(params: &[(&str, f64)]) -> crate::optimize::ComboScore {
        crate::optimize::ComboScore {
            params: params.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
            objective: Some(1.0),
        }
    }

    #[test]
    fn t3_drift_pass_stable_and_consensus() {
        // 每标的 3 OS 折 → cap=⌈0.5×3⌉=2；n_unique=1 ≤2 稳；全标的 n_s=55 共识
        let reps: Vec<_> = (0..10)
            .map(|i| {
                let folds = vec![(Some(1.0), Some(1.0), None); 3];
                let r = mk_report(&format!("s{i}"), &folds, vec![drift1("n_s", 1)], Some(best(&[("n_s", 55.0)])), vec![]);
                (format!("s{i}"), r)
            })
            .collect();
        let g = gate_param_drift(&reps, &GateThresholds::default());
        assert_eq!(g.status, GateStatus::Pass);
    }

    #[test]
    fn t3_drift_fail_no_consensus() {
        // 一半标的 n_s=40、一半 n_s=90 → 众数共识仅 0.5 <0.6 Fail
        let reps: Vec<_> = (0..10)
            .map(|i| {
                let v = if i < 5 { 40.0 } else { 90.0 };
                let folds = vec![(Some(1.0), Some(1.0), None); 3];
                let r = mk_report(&format!("s{i}"), &folds, vec![drift1("n_s", 1)], Some(best(&[("n_s", v)])), vec![]);
                (format!("s{i}"), r)
            })
            .collect();
        let g = gate_param_drift(&reps, &GateThresholds::default());
        assert_eq!(g.status, GateStatus::Fail);
    }
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p rquant verdict::gates::tests::t3`
Expected: 编译失败（`gate_param_drift` 未定义）。

- [ ] **Step 3: 实现**

```rust
/// 标的内：所有参数 n_unique ≤ ⌈frac×OS折数⌉ 且至少有 drift 记录。
fn symbol_drift_stable(r: &OptimizeReport, frac: f64) -> bool {
    if r.drift.is_empty() { return false; }
    let n_os = r.fold_results.len().max(1);
    let cap = ((frac * n_os as f64).ceil() as usize).max(1);
    r.drift.iter().all(|d| d.n_unique <= cap)
}

/// 跨标的：每参数 full_sample_best 取值众数一致占比，取最小（None best 不计分母）。
/// 无任何可比参数 → None（门由调用方判 Indeterminate）。
fn min_param_consensus(reports: &[(String, OptimizeReport)]) -> Option<f64> {
    let mut names: Vec<String> = Vec::new();
    for (_, r) in reports {
        if let Some(b) = &r.full_sample_best {
            for k in b.params.keys() {
                if !names.contains(k) { names.push(k.clone()); }
            }
        }
    }
    if names.is_empty() { return None; }
    let mut min_frac = 1.0_f64;
    for name in &names {
        let mut bits: Vec<u64> = Vec::new();
        for (_, r) in reports {
            if let Some(b) = &r.full_sample_best {
                if let Some(v) = b.params.get(name) { bits.push(v.to_bits()); }
            }
        }
        if bits.is_empty() { continue; }
        let total = bits.len();
        let mut counts: std::collections::HashMap<u64, usize> = std::collections::HashMap::new();
        for b in &bits { *counts.entry(*b).or_insert(0) += 1; }
        let mode_count = counts.values().copied().max().unwrap_or(0);
        min_frac = min_frac.min(mode_count as f64 / total as f64);
    }
    Some(min_frac)
}

/// ③ 参数漂移：稳标的占比 ≥ 阈值 且 每参数跨标的共识 ≥ 阈值。
pub fn gate_param_drift(reports: &[(String, OptimizeReport)], th: &GateThresholds) -> GateOutcome {
    let n = reports.len();
    if n == 0 {
        return GateOutcome { gate: "T3_param_drift".into(), status: GateStatus::Indeterminate, value: 0.0, threshold: th.drift_consensus_frac, note: "no reports".into() };
    }
    let stable = reports.iter().filter(|(_, r)| symbol_drift_stable(r, th.drift_stable_unique_frac)).count();
    let stable_frac = stable as f64 / n as f64;
    let consensus = match min_param_consensus(reports) {
        Some(c) => c,
        None => {
            return GateOutcome { gate: "T3_param_drift".into(), status: GateStatus::Indeterminate, value: 0.0, threshold: th.drift_consensus_frac, note: "no swept params with full_sample_best to assess consensus".into() };
        }
    };
    let value = stable_frac.min(consensus);
    let status = if stable_frac >= th.drift_stable_symbol_frac && consensus >= th.drift_consensus_frac {
        GateStatus::Pass
    } else {
        GateStatus::Fail
    };
    GateOutcome {
        gate: "T3_param_drift".into(),
        status,
        value,
        threshold: th.drift_consensus_frac,
        note: format!("stable_symbols={stable}/{n} ({stable_frac:.2}), min_param_consensus={consensus:.2}"),
    }
}
```

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p rquant verdict::gates`
Expected: 全 PASS。

- [ ] **Step 5: 提交**

```bash
git add src/verdict/gates.rs
git commit -m "feat(verdict): gate T3_param_drift (within-symbol stability + cross-symbol consensus)"
```

---

## Task 6: 门槛④ T4_interior

**Files:**
- Modify: `src/verdict/gates.rs`

- [ ] **Step 1: 写失败测试**

```rust
    fn axis(name: &str, interior: bool) -> crate::optimize::AxisOutcome {
        crate::optimize::AxisOutcome {
            name: name.into(), final_values: vec![1.0, 2.0, 3.0],
            best_value: Some(2.0), interior, extended_steps: 0,
        }
    }

    #[test]
    fn t4_interior_pass_when_60pct_all_axes_interior() {
        let reps: Vec<_> = (0..10)
            .map(|i| {
                let interior = i < 6;
                let folds = vec![(Some(1.0), Some(1.0), None)];
                let r = mk_report(&format!("s{i}"), &folds, vec![], None, vec![axis("n_s", interior)]);
                (format!("s{i}"), r)
            })
            .collect();
        let g = gate_interior(&reps, &GateThresholds::default());
        assert_eq!(g.status, GateStatus::Pass);
    }

    #[test]
    fn t4_interior_fail_and_flags_missing_axes() {
        // 全部 axes 空（未跑 --auto-extend）→ 0 内点 Fail + note 提示
        let reps: Vec<_> = (0..10)
            .map(|i| {
                let folds = vec![(Some(1.0), Some(1.0), None)];
                (format!("s{i}"), mk_report(&format!("s{i}"), &folds, vec![], None, vec![]))
            })
            .collect();
        let g = gate_interior(&reps, &GateThresholds::default());
        assert_eq!(g.status, GateStatus::Fail);
        assert!(g.note.contains("auto-extend"));
    }
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p rquant verdict::gates::tests::t4`
Expected: 编译失败（`gate_interior` 未定义）。

- [ ] **Step 3: 实现**

```rust
/// ④ 内部最优：所有轴 interior 的标的占比 ≥ 阈值；axes 空的标的保守计非内点。
pub fn gate_interior(reports: &[(String, OptimizeReport)], th: &GateThresholds) -> GateOutcome {
    let n = reports.len();
    let mut interior_ok = 0usize;
    let mut missing = 0usize;
    for (_, r) in reports {
        if r.axes.is_empty() {
            missing += 1; // 无延伸信息 → 保守不计内点
            continue;
        }
        if r.axes.iter().all(|a| a.interior) { interior_ok += 1; }
    }
    let value = if n == 0 { 0.0 } else { interior_ok as f64 / n as f64 };
    let status = if n == 0 {
        GateStatus::Indeterminate
    } else if value >= th.interior_symbol_frac {
        GateStatus::Pass
    } else {
        GateStatus::Fail
    };
    let note = if missing > 0 {
        format!("{interior_ok}/{n} symbols all-axes-interior; {missing} lack axes (re-run optimize --auto-extend)")
    } else {
        format!("{interior_ok}/{n} symbols all-axes-interior")
    };
    GateOutcome { gate: "T4_interior".into(), status, value, threshold: th.interior_symbol_frac, note }
}
```

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p rquant verdict::gates`
Expected: 全 PASS。

- [ ] **Step 5: 提交**

```bash
git add src/verdict/gates.rs
git commit -m "feat(verdict): gate T4_interior (reads axes interior flags, flags missing)"
```

---

## Task 7: 门槛⑤ T5_not_single

**Files:**
- Modify: `src/verdict/gates.rs`

- [ ] **Step 1: 写失败测试**

```rust
    #[test]
    fn t5_not_single_pass_when_no_symbol_dominates() {
        // 5 标的各 +1 正 OS → 每个份额 0.2，max 0.2 ≤0.5 且 ≥2 贡献 Pass
        let reps: Vec<_> = (0..5).map(|i| os_only(&format!("s{i}"), &[1.0, -2.0])).collect();
        let g = gate_not_single(&reps, &GateThresholds::default());
        assert_eq!(g.status, GateStatus::Pass);
        assert!((g.value - 0.2).abs() < 1e-9);
    }

    #[test]
    fn t5_not_single_fail_when_one_dominates() {
        // s0 正 OS=10、其余各 0.1 → s0 份额 ≈0.96 >0.5 Fail
        let mut reps: Vec<_> = vec![os_only("s0", &[10.0])];
        for i in 1..5 { reps.push(os_only(&format!("s{i}"), &[0.1])); }
        let g = gate_not_single(&reps, &GateThresholds::default());
        assert_eq!(g.status, GateStatus::Fail);
    }

    #[test]
    fn t5_not_single_indeterminate_when_no_positive_os() {
        let reps: Vec<_> = (0..5).map(|i| os_only(&format!("s{i}"), &[-1.0])).collect();
        let g = gate_not_single(&reps, &GateThresholds::default());
        assert_eq!(g.status, GateStatus::Indeterminate);
    }
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p rquant verdict::gates::tests::t5`
Expected: 编译失败（`gate_not_single` 未定义）。

- [ ] **Step 3: 实现**

```rust
/// 该标的所有正 OS 折之和。
fn positive_os_sum(r: &OptimizeReport) -> f64 {
    r.fold_results.iter().filter_map(|f| f.os_objective).filter(|v| *v > 0.0).sum()
}

/// ⑤ 非单标的：最大单标的正 OS 份额 ≤ 阈值 且 贡献标的 ≥2。
pub fn gate_not_single(reports: &[(String, OptimizeReport)], th: &GateThresholds) -> GateOutcome {
    let sums: Vec<f64> = reports.iter().map(|(_, r)| positive_os_sum(r)).collect();
    let total: f64 = sums.iter().sum();
    let contributing = sums.iter().filter(|v| **v > 0.0).count();
    if total <= 0.0 {
        return GateOutcome {
            gate: "T5_not_single".into(),
            status: GateStatus::Indeterminate,
            value: 0.0,
            threshold: th.max_single_symbol_os_share,
            note: "no positive OS across any symbol".into(),
        };
    }
    let max_share = sums.iter().map(|v| v / total).fold(0.0_f64, f64::max);
    let status = if max_share <= th.max_single_symbol_os_share && contributing >= 2 {
        GateStatus::Pass
    } else {
        GateStatus::Fail
    };
    GateOutcome {
        gate: "T5_not_single".into(),
        status,
        value: max_share,
        threshold: th.max_single_symbol_os_share,
        note: format!("max single-symbol positive-OS share {max_share:.2}, {contributing} contributing symbols"),
    }
}
```

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p rquant verdict::gates`
Expected: 全 PASS。

- [ ] **Step 5: 提交**

```bash
git add src/verdict/gates.rs
git commit -m "feat(verdict): gate T5_not_single (anti-concentration of positive OS)"
```

---

## Task 8: certify() 接线 + 树4 回归锁

**Files:**
- Modify: `src/verdict/mod.rs`、`src/verdict/gates.rs`

- [ ] **Step 1: certify 接 5 个 gate（去占位）**

`src/verdict/mod.rs` 的 `certify` 把 `let gates: Vec<GateOutcome> = vec![];` 替换为真实 5 调用（Task 2 已给最终形态，此处落实）：

```rust
    let gates = vec![
        gates::gate_os_breadth(reports, th),
        gates::gate_degradation(reports, th),
        gates::gate_param_drift(reports, th),
        gates::gate_interior(reports, th),
        gates::gate_not_single(reports, th),
    ];
```

并移除 `src/verdict/gates.rs` 顶部的 `#![allow(unused)]`（函数都用上了）。

- [ ] **Step 2: 写树4 回归锁测试**（钉死上一弧线手工数错 10/30 的 bug）

在 `src/verdict/mod.rs` 测试模块加：

```rust
    use crate::optimize::OptimizeReport;

    fn os_report(primary: &str, os: &[f64]) -> (String, OptimizeReport) {
        // 复用 gates::tests 的构造思路，这里独立构造仅 OS 折
        let fold_results = os.iter().enumerate().map(|(i, v)| crate::optimize::FoldResult {
            fold: i + 2,
            is_from: chrono::NaiveDate::from_ymd_opt(2025,1,1).unwrap().and_hms_opt(0,0,0).unwrap(),
            is_to: chrono::NaiveDate::from_ymd_opt(2025,1,1).unwrap().and_hms_opt(0,0,0).unwrap(),
            os_from: chrono::NaiveDate::from_ymd_opt(2025,1,1).unwrap().and_hms_opt(0,0,0).unwrap(),
            os_to: chrono::NaiveDate::from_ymd_opt(2025,1,1).unwrap().and_hms_opt(0,0,0).unwrap(),
            best_params: None, is_objective: Some(1.0), os_objective: Some(*v), degradation: None,
        }).collect();
        (primary.into(), OptimizeReport {
            mode: "sim".into(), objective_name: "sharpe".into(), folds: os.len()+1, n_combos: 1,
            fold_results, os_mean_objective: None, full_sample_best: None, drift: vec![],
            is_top5: vec![], axes: vec![], primary: primary.into(),
        })
    }

    #[test]
    fn tree4_regression_lock_os_counts_and_not_certified() {
        // 树4 真实 10 标的 3 OS 折（来自 tmps/wfo_ma_*.json）
        let reps = vec![
            os_report("sh600030", &[1.031, 0.993, -2.281]),
            os_report("sh600036", &[0.555, 0.770, -1.694]),
            os_report("sh600276", &[-2.303, 0.935, 0.0]),
            os_report("sh600519", &[-0.656, -0.637, -2.653]),
            os_report("sh600900", &[-0.211, -0.052, -1.318]),
            os_report("sh601088", &[-1.894, 0.654, -1.186]),
            os_report("sh601318", &[-0.393, 0.728, -0.110]),
            os_report("sz000333", &[-1.427, 0.549, -0.459]),
            os_report("sz000858", &[-0.428, -0.937, 0.0]),
            os_report("sz300750", &[-0.639, 1.964, -0.610]),
        ];
        // 直接锁"正 OS 折总数 = 9"（手工曾误数为 10）
        let total_pos: usize = reps.iter()
            .map(|(_, r)| r.fold_results.iter().filter(|f| f.os_objective.unwrap_or(0.0) > 0.0).count())
            .sum();
        assert_eq!(total_pos, 9, "树4 OS 正折总数必须是 9（非手工误数的 10）");

        let v = certify(&reps, "ma_stack", &GateThresholds::default());
        // 门槛① 广度 = 7/10 标的有正折 → Pass
        let t1 = v.gates.iter().find(|g| g.gate == "T1_os_breadth").unwrap();
        assert_eq!(t1.status, GateStatus::Pass);
        assert!((t1.value - 0.7).abs() < 1e-9, "广度必须是 7/10");
        // 门槛④ axes 全空 → Fail（保守，提示重跑 --auto-extend）
        let t4 = v.gates.iter().find(|g| g.gate == "T4_interior").unwrap();
        assert_eq!(t4.status, GateStatus::Fail);
        // 整体未认证
        assert!(!v.certified, "树4 必须未认证");
    }
```

- [ ] **Step 3: 运行确认通过**

Run: `cargo test -p rquant verdict::`
Expected: 全 PASS（含 tree4_regression_lock）。

- [ ] **Step 4: 提交**

```bash
git add src/verdict/mod.rs src/verdict/gates.rs
git commit -m "feat(verdict): wire certify + Tree-4 regression lock (9/30 not 10/30)"
```

---

## Task 9: optimize `--auto-extend` 边界逃逸算法

**Files:**
- Modify: `src/optimize/mod.rs`（`run_optimize` 内 full_sample_best 计算之后、构造 report 之前；~559–591）

- [ ] **Step 1: 写失败测试**（合成数据，最优落边界触发延伸）

`src/optimize/mod.rs` 测试模块加（用纯函数 `analyze_axis_interior` 单测，避免整跑 optimize）：

```rust
    #[test]
    fn auto_extend_detects_peak_just_outside_grid() {
        // 目标函数：obj(x) = -(x-30)^2 峰在 30；原网格 [40,55,90] best=40(下边界)
        // 向下延伸步长=15：25,10... 实际峰应往下找；模拟"延伸后转劣即停"
        let axis = crate::optimize::grid::GridAxis { name: "n_s".into(), values: vec![40.0, 55.0, 90.0] };
        let objective = |x: f64| -((x - 30.0).powi(2)); // 越靠 30 越大
        let out = analyze_axis_interior(&axis, 40.0, 4, &objective);
        assert_eq!(out.name, "n_s");
        // 向下延伸 40→25(更优)→10(更劣) → 峰在 25 处确认内点
        assert!(out.interior, "延伸后应确认峰值为内点");
        assert!(out.extended_steps >= 1);
        assert!(out.final_values.iter().any(|v| (*v - 25.0).abs() < 1e-9));
    }

    #[test]
    fn auto_extend_marks_boundary_artifact_when_monotone() {
        // 目标单调递增 obj(x)=x：永远贴上边界 → N 步后 interior=false
        let axis = crate::optimize::grid::GridAxis { name: "n_s".into(), values: vec![40.0, 55.0, 90.0] };
        let objective = |x: f64| x;
        let out = analyze_axis_interior(&axis, 90.0, 3, &objective);
        assert!(!out.interior, "单调不收敛 → 边界假象 interior=false");
        assert_eq!(out.extended_steps, 3);
    }

    #[test]
    fn auto_extend_no_op_when_interior() {
        let axis = crate::optimize::grid::GridAxis { name: "k".into(), values: vec![1.0, 2.0, 3.0] };
        let out = analyze_axis_interior(&axis, 2.0, 4, &|x: f64| -((x - 2.0).powi(2)));
        assert!(out.interior);
        assert_eq!(out.extended_steps, 0);
        assert_eq!(out.best_value, Some(2.0));
    }
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p rquant auto_extend`
Expected: 编译失败（`analyze_axis_interior` 未定义）。

- [ ] **Step 3: 实现纯函数 analyze_axis_interior**

在 `src/optimize/mod.rs`（`run_optimize` 之外，模块级）加。`objective` 抽象成闭包便于单测；真实调用时传"在该轴取值 x、其余参数固定为 full_sample_best 时的全样本目标"。

```rust
/// 围绕全样本最优在单条轴上做边界逃逸，判定内部最优。
/// `best_on_axis`：该轴当前最优取值。`objective(x)`：该轴取 x（其余参数=全样本最优）时的全样本目标，越大越好。
/// 返回延伸后的 AxisOutcome。仅当 best 落在边界时延伸；最多 max_steps 步。
fn analyze_axis_interior(
    axis: &crate::optimize::grid::GridAxis,
    best_on_axis: f64,
    max_steps: usize,
    objective: &dyn Fn(f64) -> f64,
) -> AxisOutcome {
    let mut values: Vec<f64> = axis.values.clone();
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    // 单值轴：不在搜索 → 视为内点。
    if values.len() < 2 {
        return AxisOutcome { name: axis.name.clone(), final_values: values, best_value: Some(best_on_axis), interior: true, extended_steps: 0 };
    }
    let lo = values[0];
    let hi = *values.last().unwrap();
    // 方向：贴下界向下、贴上界向上、否则内点无需延伸。
    let dir: i32 = if (best_on_axis - lo).abs() < 1e-9 {
        -1
    } else if (best_on_axis - hi).abs() < 1e-9 {
        1
    } else {
        return AxisOutcome { name: axis.name.clone(), final_values: values, best_value: Some(best_on_axis), interior: true, extended_steps: 0 };
    };
    let step = if dir < 0 { values[1] - values[0] } else { hi - values[values.len() - 2] };
    let mut cur = best_on_axis;
    let mut cur_obj = objective(cur);
    let mut steps = 0usize;
    let mut interior = false;
    while steps < max_steps {
        let cand = cur + dir as f64 * step;
        let cand_obj = objective(cand);
        // 维持升序插入新值
        match values.binary_search_by(|v| v.partial_cmp(&cand).unwrap_or(std::cmp::Ordering::Equal)) {
            Ok(_) => {}
            Err(pos) => values.insert(pos, cand),
        }
        steps += 1;
        if cand_obj <= cur_obj {
            // 越界一步转劣 → 当前 cur 是峰值 → 内点确认
            interior = true;
            break;
        }
        // 仍在改善 → 继续往外
        cur = cand;
        cur_obj = cand_obj;
    }
    // 循环自然结束（达 max_steps 仍在改善）→ interior 保持 false（边界假象）
    AxisOutcome {
        name: axis.name.clone(),
        final_values: values,
        best_value: Some(cur),
        interior,
        extended_steps: steps,
    }
}
```

- [ ] **Step 4: 在 run_optimize 接入（仅 cfg.auto_extend>0）**

在 full_sample_best 计算之后（~559 行后）、`drift` 计算之前插入：

```rust
    // ── Step 5b: auto-extend（门槛④边界逃逸；仅 --auto-extend>0）──────────────
    let axis_outcomes: Vec<AxisOutcome> = if cfg.auto_extend > 0 {
        if let Some(best) = &full_sample_best {
            let mut outs = Vec::with_capacity(axes.len());
            for ax in &axes {
                let best_on_axis = *best.params.get(&ax.name).unwrap_or(&f64::NAN);
                if best_on_axis.is_nan() { continue; }
                // objective(x)：该轴取 x、其余参数固定为 full_sample_best，全样本目标。
                let make_obj = |x: f64| -> f64 {
                    // 同步阻塞地复用 evaluate；用 futures::executor 在此处求值。
                    let mut combo = best.params.clone();
                    combo.insert(ax.name.clone(), x);
                    let res = futures::executor::block_on(async {
                        match crate::tree::loader::load_tree_str_with_overrides(&yaml_src, &combo) {
                            Ok(tree) => evaluate(&tree, &data, llm, full_range.clone(), mode).await.ok().flatten(),
                            Err(_) => None,
                        }
                    });
                    res.unwrap_or(f64::NEG_INFINITY)
                };
                outs.push(analyze_axis_interior(ax, best_on_axis, cfg.auto_extend, &make_obj));
            }
            outs
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };
```

并把构造 report 处的 `axes: Vec::new(),`（Task 1 占位）改为 `axes: axis_outcomes,`。

> 注：若 `futures` 未在依赖中，改用已有运行时；本仓 optimize 是 async fn，`evaluate` 是 async。最简方案：把这段 auto-extend 写成在 `run_optimize`（async）内**直接 `.await`** 而非 `block_on`——即把 `make_obj` 改为内联 await 循环，重构 `analyze_axis_interior` 为接收"已算好的 (cand, obj)"或把 objective 调用移到 async 上下文。实现者择一：**推荐**把 `analyze_axis_interior` 的延伸驱动逻辑保留纯函数（输入候选值序列 + 一个"给定 x 返回 obj"的同步回调），在 async 侧预先不可行（x 是动态生成的）。故采用：在 `run_optimize` 内写一个 async 版延伸循环（复制 analyze_axis_interior 的控制流，把 `objective(cand)` 换成 `evaluate(...).await`），纯函数 `analyze_axis_interior` 仅供单测覆盖控制流。两者控制流相同，async 版多了 .await。

- [ ] **Step 5: 运行确认通过**

Run: `cargo test -p rquant auto_extend optimize`
Expected: analyze_axis_interior 三测试 PASS；既有 optimize 测试不变 PASS（auto_extend 默认 0 → axis_outcomes 空 → 行为冻结）。

- [ ] **Step 6: 提交**

```bash
git add src/optimize/mod.rs
git commit -m "feat(optimize): --auto-extend boundary-escape for gate-4 interior optimum"
```

---

## Task 10: `rquant eval` CLI 子命令

**Files:**
- Modify: `src/cli/mod.rs`（`Cmd` 枚举加 `Eval` + `Optimize` 加 `--auto-extend`；处理臂）
- Test: `tests/`（新增 e2e 或在 cli 测试内）

- [ ] **Step 1: 写失败 e2e 测试**

新建 `tests/eval_cli.rs`：

```rust
use std::process::Command;

fn bin() -> &'static str { env!("CARGO_BIN_EXE_rquant") }

#[test]
fn eval_emits_verdict_and_exit_code() {
    let dir = tempfile::tempdir().unwrap();
    // 两个标的，都无 OS 正折 → 必未认证 → 退出码非 0
    for (name, os) in [("sh000001", -1.0), ("sh000002", -1.0)] {
        let json = format!(r#"{{"mode":"sim","objective_name":"sharpe","folds":2,"n_combos":1,
            "fold_results":[{{"fold":2,"is_from":"2025-01-01T00:00:00","is_to":"2025-01-01T00:00:00",
            "os_from":"2025-01-01T00:00:00","os_to":"2025-01-01T00:00:00","best_params":null,
            "is_objective":1.0,"os_objective":{os},"degradation":null}}],
            "os_mean_objective":null,"full_sample_best":null,"drift":[],"is_top5":[],
            "axes":[],"primary":"{name}"}}"#);
        std::fs::write(dir.path().join(format!("wfo_{name}.json")), json).unwrap();
    }
    let out_path = dir.path().join("verdict.json");
    let status = Command::new(bin())
        .args(["eval", "--name", "t",
               "--reports", dir.path().join("wfo_sh000001.json").to_str().unwrap(),
               "--reports", dir.path().join("wfo_sh000002.json").to_str().unwrap(),
               "--out", out_path.to_str().unwrap()])
        .status().unwrap();
    assert_eq!(status.code(), Some(1), "未认证 → 退出码 1");
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&out_path).unwrap()).unwrap();
    assert_eq!(v["certified"], serde_json::json!(false));
    assert_eq!(v["n_symbols"], serde_json::json!(2));
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p rquant --test eval_cli`
Expected: 失败（`eval` 子命令不存在 → clap 报错，退出码非 1 或 stderr 含 unknown subcommand）。

- [ ] **Step 3: 加 Cmd::Eval 变体 + Optimize 的 --auto-extend**

`src/cli/mod.rs` 的 `Cmd` 枚举 `Optimize { ... }` 加一字段（在 `out` 之后）：

```rust
        #[arg(long, default_value_t = 0)]
        auto_extend: usize,
```

`Cmd` 枚举末尾（`Portfolio { ... }` 之后）加：

```rust
    /// Apply the 5-gate WFO certification to N per-symbol optimize reports.
    Eval {
        /// Repeatable: one optimize JSON per symbol (a strategy's universe).
        #[arg(long = "reports", value_name = "PATH", required = true)]
        reports: Vec<PathBuf>,
        /// Strategy name for the verdict (default: derived).
        #[arg(long, default_value = "")]
        name: String,
        /// Write Verdict JSON here.
        #[arg(long)]
        out: Option<PathBuf>,
    },
```

- [ ] **Step 4: 加 Cmd::Eval 处理臂 + Optimize 传 auto_extend**

`Cmd::Optimize` 解构加 `auto_extend`，`OptimizeConfig` 的 `auto_extend: 0,`（Task 1 占位）改为 `auto_extend,`。

`match` 末尾（`Cmd::Portfolio` 臂之后）加：

```rust
        Cmd::Eval { reports, name, out } => {
            if reports.is_empty() {
                return Err(anyhow::anyhow!("--reports: at least one optimize report is required"));
            }
            let mut loaded: Vec<(String, rquant::optimize::OptimizeReport)> = Vec::new();
            for p in &reports {
                let txt = std::fs::read_to_string(p)
                    .map_err(|e| anyhow::anyhow!("read {}: {e}", p.display()))?;
                let r: rquant::optimize::OptimizeReport = serde_json::from_str(&txt)
                    .map_err(|e| anyhow::anyhow!("parse {}: {e}", p.display()))?;
                let symbol = if r.primary.is_empty() {
                    p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| p.display().to_string())
                } else {
                    r.primary.clone()
                };
                loaded.push((symbol, r));
            }
            let strategy = if name.is_empty() {
                loaded.first().map(|(s, _)| s.clone()).unwrap_or_default()
            } else {
                name
            };
            let verdict = rquant::verdict::certify(&loaded, &strategy, &rquant::verdict::GateThresholds::default());
            print_verdict(&verdict);
            if let Some(op) = out {
                std::fs::write(&op, serde_json::to_string_pretty(&verdict)?)?;
            }
            if !verdict.certified {
                std::process::exit(1);
            }
        }
```

加打印辅助（`print_optimize_summary` 附近，或本文件底部）：

```rust
fn print_verdict(v: &rquant::verdict::Verdict) {
    use rquant::verdict::GateStatus;
    println!("=== WFO 5-Gate Verdict: {} ({} symbols) ===", v.strategy, v.n_symbols);
    println!("{:<16} {:<14} {:>8} {:>8}  note", "gate", "status", "value", "thresh");
    for g in &v.gates {
        let st = match g.status { GateStatus::Pass => "PASS", GateStatus::Fail => "FAIL", GateStatus::Indeterminate => "INDET" };
        println!("{:<16} {:<14} {:>8.3} {:>8.3}  {}", g.gate, st, g.value, g.threshold, g.note);
    }
    if v.certified {
        println!("RESULT: CERTIFIED");
    } else {
        println!("RESULT: NOT CERTIFIED  failed: [{}]", v.failed_gates.join(", "));
    }
}
```

> 注：`rquant::optimize::OptimizeReport` 与 `rquant::verdict::*` 需在 lib 公开导出（optimize 已 pub；verdict 由 Task 2 `pub mod verdict;`）。确认 `OptimizeReport`/`FoldResult`/`ComboScore`/`ParamDrift`/`AxisOutcome` 均 `pub`（Task 1 已是）。

- [ ] **Step 5: 运行确认通过**

Run: `cargo test -p rquant --test eval_cli`
Expected: PASS（退出码 1、verdict.json certified=false、n_symbols=2）。

- [ ] **Step 6: 提交**

```bash
git add src/cli/mod.rs tests/eval_cli.rs
git commit -m "feat(cli): rquant eval subcommand + optimize --auto-extend flag"
```

---

## Task 11: 文档 + 全量收尾闸

**Files:**
- Modify: `docs/cli-reference.md`

- [ ] **Step 1: 文档 eval + --auto-extend**

`docs/cli-reference.md` 加 `eval` 子命令一节（用法、五门槛表、退出码、与 optimize 的关系）和 optimize `--auto-extend N` 说明（默认 0=关、围绕全样本最优做边界逃逸、interior 判定）。逐条对照 Task 10 的 CLI 旗标与 Task 2/§7.2 的门槛语义写，确保一致。

示例段：

```markdown
### `rquant eval` — WFO 5-gate certification

Applies the strict-WFO 5-gate methodology to N per-symbol optimize reports
(one strategy's universe) and emits a machine verdict.

```
rquant eval --reports wfo_ma_sh600030.json --reports wfo_ma_sh600036.json [...] [--name ma_stack] [--out verdict.json]
```

Gates (thresholds in `GateThresholds::default()`):
- T1 OS breadth: ≥60% symbols have ≥1 positive OS fold
- T2 degradation: ≥60% determinate symbols have median per-fold degradation > 0.5
- T3 param drift: ≥60% symbols stable (n_unique ≤ ⌈0.5×folds⌉) AND each param cross-symbol consensus ≥ 0.6
- T4 interior: ≥60% symbols have all axes interior (requires `optimize --auto-extend`)
- T5 not-single: max single-symbol positive-OS share ≤ 0.5 AND ≥2 contributing symbols

Exit code: 0 if certified, 1 otherwise (CI/pre-commit gate).
The verdict is mechanical; the no-edge vs regime-dependent narrative remains a human call.

`optimize --auto-extend N` (default 0 = off): around the full-sample optimum,
escape grid boundaries by widening the boundary axis (reusing the local step)
up to N steps, recording per-axis `interior` in the report's `axes` field for T4.
```

- [ ] **Step 2: 提交文档**

```bash
git add docs/cli-reference.md
git commit -m "docs(cli): eval subcommand + optimize --auto-extend reference"
```

- [ ] **Step 3: 全量测试闸**

Run: `cargo test`
Expected: 全绿（既有 305 单元 + 22 e2e + 新 verdict/optimize/eval_cli 测试），0 失败。

- [ ] **Step 4: clippy 闸**

Run: `cargo clippy --all-targets`
Expected: 零警告（含新模块）。

- [ ] **Step 5: 行为冻结复验**

Run: `cargo test -p rquant optimize`
Expected: 既有 optimize 黄金/行为测试全绿（`--auto-extend` 默认关 → optimize 输出除新增空 `axes`/`primary` 外字节不变）。

- [ ] **Step 6: 最终提交（若有遗留）**

```bash
git add -p   # 仅本计划相关文件，点名
git commit -m "chore(eval): finalize Phase-1 WFO gate verdict mechanism"
```

---

## Self-Review（写计划后自查）

**Spec 覆盖**：①②③④⑤ 五门槛 → Task 3-7；certify + 树4 回归锁 → Task 8；optimize auto-extend（门槛④数据源）→ Task 9；eval CLI + 退出码 → Task 10；OptimizeReport schema（axes/primary）→ Task 1；阈值硬编码 GateThresholds::default → Task 2；文档 → Task 11；行为冻结 → Task 1/9/11。factor F-1、批量 runner、结果库 = spec 显式非目标，未建任务（正确）。✅ 全覆盖。

**占位符扫描**：无 TBD/TODO；每步含真实代码与命令。Task 9 Step 4 的 async/block_on 二选一已写明实现者取舍（非占位，是明确的实现指引）。

**类型一致性**：`GateThresholds`/`GateStatus`/`GateOutcome`/`Verdict`/`certify(reports,strategy,th)` 跨 Task 2/8/10 一致；`AxisOutcome{name,final_values,best_value,interior,extended_steps}` 跨 Task 1/6/9 一致；gate 函数名 `gate_os_breadth`/`gate_degradation`/`gate_param_drift`/`gate_interior`/`gate_not_single` 跨 Task 3-8/certify 一致；`OptimizeReport.axes`/`.primary` 跨 Task 1/9/10 一致。✅
