# rquant F-4 风险指标集 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** sim/portfolio 报告挂 `RiskMetrics`（CAGR/年化波动/Sharpe/Sortino/Calmar/VaR95/CVaR95，时间戳推断年化，样本不足给 None）；打分模式 SignalStat 加 t_stat。

**Architecture:** 在 master(HEAD `841976b`)上新增 `report/risk.rs` 纯函数（公式权威=spec §3：`docs/superpowers/specs/2026-06-11-rquant-f4-risk-metrics-design.md`，实现者先通读），三处报告挂载（serde skip/default 旧 JSON 兼容），print/HTML 各加行。

**Tech Stack:** Rust 2024 + 既有。

---

## 文件结构
```
新增: src/report/risk.rs       # RiskMetrics / risk_metrics / t_stat + 黄金闭式测试
改动: src/report/mod.rs        # + pub mod risk;（SignalStat 若在此/或 backtest 侧——grep 后就地改）
改动: src/backtest/sim.rs      # SimReport.risk + run_sim 计算 + print 行
改动: src/backtest/portfolio.rs# PortfolioReport.risk + run_portfolio 计算 + print 行
改动: SignalStat 定义与构造处   # t_stat 字段（grep "struct SignalStat" 定位）
改动: src/report/viz.rs        # sim/portfolio headline 加行
改动: tests/e2e.rs、docs/cli-reference.md、README.md
```

---

## Task 1: risk.rs 纯函数 + 黄金闭式

**Files:**
- Create: `src/report/risk.rs`；Modify: `src/report/mod.rs`（+ `pub mod risk;`）

- [ ] **Step 1: RED 黄金测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use chrono::{Duration, NaiveDate};

    fn t0() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2025, 1, 1).unwrap().and_hms_opt(12, 0, 0).unwrap()
    }

    /// 构造 nav 序列：首点 t0，末点 t0+span_secs，中间点 +1s 递增（span 只看首末）。
    fn nav_with_rets(rets: &[f64], span_secs: i64) -> Vec<(NaiveDateTime, f64)> {
        let mut nav = vec![(t0(), 1.0)];
        let mut v = 1.0;
        for (i, r) in rets.iter().enumerate() {
            v *= 1.0 + r;
            let t = if i == rets.len() - 1 {
                t0() + Duration::seconds(span_secs)
            } else {
                t0() + Duration::seconds(i as i64 + 1)
            };
            nav.push((t, v));
        }
        nav
    }

    const YEAR_SECS: i64 = (365.25 * 86_400.0) as i64; // 31_557_600

    #[test]
    fn golden_constant_return_one_year() {
        // 恒定 r=0.1% × 252 步，跨度恰一年 → CAGR=1.001^252−1；vol≈0 → sharpe None
        let m = risk_metrics(&nav_with_rets(&[0.001; 252], YEAR_SECS), 0.05).unwrap();
        assert_relative_eq!(m.ann_return.unwrap(), 1.001f64.powi(252) - 1.0, epsilon = 1e-9);
        assert!(m.ann_vol.unwrap() < 1e-12);
        assert!(m.sharpe.is_none()); // vol≈0 拒绝假 Sharpe
        assert!(m.sortino.is_none()); // 无负收益
        assert_relative_eq!(m.calmar.unwrap(), m.ann_return.unwrap() / 0.05, epsilon = 1e-9);
        assert_relative_eq!(m.var95, 0.001, epsilon = 1e-12); // 恒定收益分位=自身
    }

    #[test]
    fn golden_alternating_vol_sortino_annualization() {
        // +1%/−0.5% × 10 对（n=20），跨度恰 1/4 年 → bpy=80（钉年化接线）
        let rets: Vec<f64> = (0..20).map(|i| if i % 2 == 0 { 0.01 } else { -0.005 }).collect();
        let m = risk_metrics(&nav_with_rets(&rets, YEAR_SECS / 4), 0.05).unwrap();
        let span_years = 0.25;
        let bpy = 20.0 / span_years;
        let ar = (1.01f64 * 0.995).powi(10).powf(1.0 / span_years) - 1.0;
        assert_relative_eq!(m.ann_return.unwrap(), ar, epsilon = 1e-9);
        // sample std: mean=0.0025, 偏差 ±0.0075 → std=0.0075·√(20/19)
        let std = 0.0075 * (20.0f64 / 19.0).sqrt();
        assert_relative_eq!(m.ann_vol.unwrap(), std * bpy.sqrt(), epsilon = 1e-9);
        assert_relative_eq!(m.sharpe.unwrap(), ar / (std * bpy.sqrt()), epsilon = 1e-9);
        // downside = √(10·0.005²/20) = 0.005·√0.5（全量 n 分母约定，spec §3）
        let downside = 0.005 * 0.5f64.sqrt();
        assert_relative_eq!(m.sortino.unwrap(), ar / (downside * bpy.sqrt()), epsilon = 1e-9);
        // VaR：n=20 → idx=⌈1⌉−1=0 → 最差收益 −0.005
        assert_relative_eq!(m.var95, -0.005, epsilon = 1e-12);
        assert_relative_eq!(m.cvar95, -0.005, epsilon = 1e-12);
    }

    #[test]
    fn short_span_gives_none_annualized_but_var_present() {
        let m = risk_metrics(&nav_with_rets(&[0.01, -0.02, 0.005], 20 * 86_400), 0.05).unwrap();
        assert!(m.ann_return.is_none() && m.ann_vol.is_none() && m.sharpe.is_none());
        assert!(m.sortino.is_none() && m.calmar.is_none());
        assert_relative_eq!(m.var95, -0.02, epsilon = 1e-12);
    }

    #[test]
    fn degenerate_inputs_give_overall_none() {
        assert!(risk_metrics(&[(t0(), 1.0)], 0.05).is_none()); // 单点
        assert!(risk_metrics(&[(t0(), 1.0), (t0() + Duration::seconds(1), 0.0)], 0.05).is_none()); // nav≤0
    }

    #[test]
    fn t_stat_closed_form() {
        // [1,2,3]: mean=2, std=1 → t = 2/(1/√3) = 2√3
        assert_relative_eq!(t_stat(&[1.0, 2.0, 3.0]).unwrap(), 2.0 * 3.0f64.sqrt(), epsilon = 1e-12);
        assert!(t_stat(&[1.0]).is_none());
        assert!(t_stat(&[2.0, 2.0, 2.0]).is_none()); // std≈0
    }
}
```

- [ ] **Step 2: 实现（spec §3 公式逐条）**

```rust
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

/// 风险指标（公式约定见 spec 2026-06-11-rquant-f4-risk-metrics-design.md §3）。
/// Option=None 表示样本不足/除零——拒绝假数字。VaR 族恒可算。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RiskMetrics {
    pub ann_return: Option<f64>,
    pub ann_vol: Option<f64>,
    pub sharpe: Option<f64>,
    pub sortino: Option<f64>,
    pub calmar: Option<f64>,
    pub var95: f64,
    pub cvar95: f64,
}

const EPS: f64 = 1e-12;
const MIN_SPAN_DAYS: f64 = 30.0;

/// 净值点列 → 风险指标。len<2 或任一 nav≤0 → None。
/// 年化基准由时间戳推断：bpy = n_rets / 首末跨度年数；跨度 < 30 天 → 年化族 None。
pub fn risk_metrics(nav: &[(NaiveDateTime, f64)], max_drawdown: f64) -> Option<RiskMetrics> {
    if nav.len() < 2 || nav.iter().any(|(_, v)| *v <= 0.0) {
        return None;
    }
    let rets: Vec<f64> = nav.windows(2).map(|w| w[1].1 / w[0].1 - 1.0).collect();
    let n = rets.len() as f64;
    // VaR95/CVaR95：升序，idx = max(⌈0.05n⌉−1, 0)
    let mut sorted = rets.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = ((0.05 * n).ceil() as usize).max(1) - 1;
    let var95 = sorted[idx];
    let cvar95 = sorted[..=idx].iter().sum::<f64>() / (idx + 1) as f64;
    // 年化族
    let span_secs = (nav[nav.len() - 1].0 - nav[0].0).num_seconds() as f64;
    let span_days = span_secs / 86_400.0;
    let span_years = span_secs / (365.25 * 86_400.0);
    let (ann_return, ann_vol, sharpe, sortino, calmar) =
        if span_days < MIN_SPAN_DAYS || span_years <= 0.0 {
            (None, None, None, None, None)
        } else {
            let bpy = n / span_years;
            let ar = (nav[nav.len() - 1].1 / nav[0].1).powf(1.0 / span_years) - 1.0;
            let mean = rets.iter().sum::<f64>() / n;
            let av = if rets.len() >= 2 {
                let var = rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (n - 1.0);
                Some(var.sqrt() * bpy.sqrt())
            } else {
                None
            };
            let sharpe = av.filter(|v| *v > EPS).map(|v| ar / v);
            let downside =
                (rets.iter().map(|r| r.min(0.0).powi(2)).sum::<f64>() / n).sqrt() * bpy.sqrt();
            let sortino = (downside > EPS).then(|| ar / downside);
            let calmar = (max_drawdown > EPS).then(|| ar / max_drawdown);
            (Some(ar), av, sharpe, sortino, calmar)
        };
    Some(RiskMetrics { ann_return, ann_vol, sharpe, sortino, calmar, var95, cvar95 })
}

/// t 统计量 = mean/(sample_std/√n)；n<2 或 std≈0 → None。
pub fn t_stat(values: &[f64]) -> Option<f64> {
    if values.len() < 2 {
        return None;
    }
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    let var = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1.0);
    let std = var.sqrt();
    (std > EPS).then(|| mean / (std / n.sqrt()))
}
```
`src/report/mod.rs` 加 `pub mod risk;`。

- [ ] **Step 3: GREEN + clippy + Commit**

```bash
git add src/report/risk.rs src/report/mod.rs
git commit -m "feat(report): risk metrics pure fns (CAGR/vol/Sharpe/Sortino/Calmar/VaR, timestamp-inferred annualization)" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: 挂载三报告 + print/HTML

**Files:**
- Modify: `src/backtest/sim.rs`、`src/backtest/portfolio.rs`、SignalStat 定义与构造处（`grep -rn "struct SignalStat" src/` 定位）、`src/report/viz.rs`

- [ ] **Step 1: SimReport（READ run_sim 先）**

`SimReport` 加：
```rust
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub risk: Option<RiskMetrics>,
```
run_sim 聚合处：`let risk = crate::report::risk::risk_metrics(&records.iter().map(|r| (r.t, r.nav)).collect::<Vec<_>>(), max_drawdown_value);`（records=内存 step 记录，与 traces 是否写出无关；用 acc 的 max_drawdown）。`print_sim_summary` 加行（None → "—"，格式 `{:.2}`/var `{:+.4}`）：
```
年化收益  : … | 年化波动 : … | Sharpe : … | Sortino : … | Calmar : … | VaR95 : … | CVaR95 : …
```
（逐项一行，风格与现有行一致。）兼容测试：旧 sim JSON 字符串（无 risk 字段）`serde_json::from_str::<SimReport>` 成功且 `risk.is_none()`。

- [ ] **Step 2: PortfolioReport（READ run_portfolio 先）**

同式：`risk: Option<RiskMetrics>`（skip/default）；nav 点列 = `holdings.iter().map(|h| (h.t, h.nav))`（含期末段则补末点——以 run_portfolio 内实际 nav 序列变量为准，选含最终 nav 的那个序列）；max_drawdown 用已算值。print 加行同 Step 1。兼容测试同式。

- [ ] **Step 3: SignalStat.t_stat（grep 构造处，硬/软/walk-forward 共用一处最佳）**

`SignalStat` 加 `#[serde(default)] pub t_stat: Option<f64>,`；在其构造函数/构造点统一 `t_stat: crate::report::risk::t_stat(&nets)`（nets = 该 stat 聚合的逐点净收益样本；若多个构造点逐一补）。print_summary/print_soft_summary 各加一行 `t统计量 : {:.2}`（None → "—"）。测试：已知 nets 构造 SignalStat → t_stat 与 risk::t_stat 一致；旧 Report JSON 兼容。

- [ ] **Step 4: HTML（READ render_sim_html/render_portfolio_html 的 headline 表）**

两个 headline 表各加 7 行（Sharpe/Sortino/Calmar/年化收益/年化波动/VaR95/CVaR95；None → `—`）。更新 viz 测试断言（确定性测试若有字节断言需同步）。

- [ ] **Step 5: `cargo test` 全绿 + clippy + Commit**

```bash
git add -A src
git commit -m "feat(report): mount RiskMetrics on sim/portfolio reports; SignalStat t_stat; print+HTML rows" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: e2e + 文档 + 真数据 smoke

**Files:**
- Modify: `tests/e2e.rs`、`docs/cli-reference.md`、`README.md`

- [ ] **Step 1: e2e**

- 复用 `sim_full_chain`/`portfolio_full_chain` fixtures：断言 `report.risk.is_some()`（合成数据跨多日；若跨度 < 30 天则断言 `risk.unwrap().var95.is_finite()` 且年化族 None——按 fixture 实际跨度选断言并注释）。
- `report --sim`/`--portfolio` HTML 含 `"Sharpe"`。
- 打分 e2e：`report.metrics.active.t_stat`（字段路径按实际）存在性断言。

- [ ] **Step 2: 文档**

cli-reference：指标表（七项 + t_stat，None 语义一句，公式指向 spec）；README 摘要示例更新一行。

- [ ] **Step 3: 真数据 smoke（手动不入库）**

`fetch sh601088 --scale 240 --adjust qfq` → `backtest --sim --tree examples/sim_tree.yaml --warmup 30` → 摘要含 Sharpe/Calmar 且量级合理（|Sharpe| < 5、Calmar 有限）；数字记入报告后清理。

- [ ] **Step 4: 全绿 + Commit**

```bash
git add tests/e2e.rs docs/cli-reference.md README.md
git commit -m "test+docs: risk metrics e2e, reference table, real-data smoke" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## 附录 A：Spec 覆盖对照（自检）
| Spec § | 实现于 |
|---|---|
| §3 公式（CAGR/vol/sharpe/sortino/calmar/VaR/t_stat + 30 天闸）| Task 1（黄金闭式逐条）|
| §4 挂载（SimReport/PortfolioReport/SignalStat/print/HTML）| Task 2 |
| §5 兼容旧 JSON + e2e + smoke + 文档 | Task 2/3 |

## 附录 B：明确不在范围（YAGNI）
- 打分模式 Sharpe；基准 RiskMetrics；β/月度表；rf≠0；分位参数化。
