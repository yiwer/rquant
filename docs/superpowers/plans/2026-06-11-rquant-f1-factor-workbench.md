# rquant F-1 因子检验工作台（factor 子命令）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `rquant factor`：横截面单/多因子检验——IC/RankIC 汇总、IC 衰减阶梯、Q 分层回测（含 Top−Bottom 价差）、因子相关性矩阵，JSON + print + HTML。

**Architecture:** 在 master(HEAD `c94d77d`)上新增 `src/factor/{stats.rs, mod.rs}`。公式权威=spec §3（`docs/superpowers/specs/2026-06-11-rquant-f1-factor-workbench-design.md`，实现者先通读）。复用 universe 读取器、`backtest::portfolio::{build_timeline,is_fresh}`、`build_context`、DSL `eval_scalar`、`forward_return`(gross)、`report::risk::{risk_metrics,t_stat}`、viz 图元。纯量化无 LLM → 同步实现。

**Tech Stack:** Rust 2024 + 既有。

---

## 文件结构
```
新增: src/factor/stats.rs  # average_ranks/pearson/spearman/layer_sizes/decay_ladder（纯函数+黄金）
新增: src/factor/mod.rs    # 类型 + run_factor + print_factor_summary
改动: src/lib.rs           # + pub mod factor;
改动: src/report/viz.rs    # render_factor_html
改动: src/cli/mod.rs       # Cmd::Factor
改动: tests/e2e.rs、docs/cli-reference.md、README.md
```

---

## Task 1: factor/stats.rs 纯函数

**Files:**
- Create: `src/factor/stats.rs`、`src/factor/mod.rs`（暂只 `pub mod stats;`）；Modify: `src/lib.rs`（+ `pub mod factor;`）

- [ ] **Step 1: RED 黄金测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn average_ranks_ties_take_mean() {
        assert_eq!(average_ranks(&[10.0, 20.0, 20.0, 30.0]), vec![1.0, 2.5, 2.5, 4.0]);
        assert_eq!(average_ranks(&[3.0, 1.0, 2.0]), vec![3.0, 1.0, 2.0]);
    }

    #[test]
    fn pearson_closed_form() {
        assert_relative_eq!(pearson(&[1.0, 2.0, 3.0], &[2.0, 4.0, 6.0]).unwrap(), 1.0, epsilon = 1e-12);
        assert_relative_eq!(pearson(&[1.0, 2.0, 3.0], &[6.0, 4.0, 2.0]).unwrap(), -1.0, epsilon = 1e-12);
        assert!(pearson(&[1.0, 1.0, 1.0], &[1.0, 2.0, 3.0]).is_none()); // 零方差
        assert!(pearson(&[1.0], &[1.0]).is_none()); // n<2
    }

    #[test]
    fn spearman_monotone_nonlinear_is_one() {
        assert_relative_eq!(
            spearman(&[1.0, 2.0, 3.0, 4.0], &[1.0, 10.0, 100.0, 1000.0]).unwrap(),
            1.0, epsilon = 1e-12
        );
        assert_relative_eq!(
            spearman(&[1.0, 2.0, 3.0, 4.0], &[1000.0, 100.0, 10.0, 1.0]).unwrap(),
            -1.0, epsilon = 1e-12
        );
    }

    #[test]
    fn layer_sizes_distributes_remainder_to_front() {
        assert_eq!(layer_sizes(11, 5), vec![3, 2, 2, 2, 2]);
        assert_eq!(layer_sizes(10, 5), vec![2, 2, 2, 2, 2]);
    }

    #[test]
    fn decay_ladder_dedups_and_sorts() {
        assert_eq!(decay_ladder(4), vec![1, 2, 4, 8, 16]);
        assert_eq!(decay_ladder(1), vec![1, 2, 4]); // max(…,1) 去重
        assert_eq!(decay_ladder(16), vec![4, 8, 16, 32, 64]);
    }
}
```

- [ ] **Step 2: 实现**

```rust
/// 平均秩（并列取平均，1 起）。
pub fn average_ranks(values: &[f64]) -> Vec<f64> {
    let n = values.len();
    let mut idx: Vec<usize> = (0..n).collect();
    idx.sort_by(|&a, &b| values[a].partial_cmp(&values[b]).unwrap_or(std::cmp::Ordering::Equal));
    let mut ranks = vec![0.0; n];
    let mut i = 0;
    while i < n {
        let mut j = i;
        while j + 1 < n && values[idx[j + 1]] == values[idx[i]] {
            j += 1;
        }
        let avg = (i + 1 + j + 1) as f64 / 2.0;
        for k in i..=j {
            ranks[idx[k]] = avg;
        }
        i = j + 1;
    }
    ranks
}

/// Pearson 相关：n≥2 且两侧方差 > 1e-12，否则 None。
pub fn pearson(x: &[f64], y: &[f64]) -> Option<f64> {
    if x.len() != y.len() || x.len() < 2 {
        return None;
    }
    let n = x.len() as f64;
    let mx = x.iter().sum::<f64>() / n;
    let my = y.iter().sum::<f64>() / n;
    let (mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0);
    for i in 0..x.len() {
        let (dx, dy) = (x[i] - mx, y[i] - my);
        sxx += dx * dx;
        syy += dy * dy;
        sxy += dx * dy;
    }
    if sxx <= 1e-12 || syy <= 1e-12 {
        return None;
    }
    Some(sxy / (sxx.sqrt() * syy.sqrt()))
}

/// Spearman = Pearson(平均秩, 平均秩)。
pub fn spearman(x: &[f64], y: &[f64]) -> Option<f64> {
    if x.len() != y.len() || x.len() < 2 {
        return None;
    }
    pearson(&average_ranks(x), &average_ranks(y))
}

/// 分层大小：基础 n/q，前 n%q 层 +1（升序因子值连续切层）。
pub fn layer_sizes(n: usize, q: usize) -> Vec<usize> {
    let base = n / q;
    let rem = n % q;
    (0..q).map(|i| base + usize::from(i < rem)).collect()
}

/// IC 衰减阶梯：dedup{max(h/4,1), max(h/2,1), h, 2h, 4h} 升序。
pub fn decay_ladder(h: usize) -> Vec<usize> {
    let mut v = vec![(h / 4).max(1), (h / 2).max(1), h, 2 * h, 4 * h];
    v.sort_unstable();
    v.dedup();
    v
}
```

- [ ] **Step 3: GREEN + clippy + Commit**

```bash
git add src/factor src/lib.rs
git commit -m "feat(factor): rank/correlation/layer/ladder pure fns with closed-form goldens" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: 采样循环（factor/mod.rs）

**Files:**
- Modify: `src/factor/mod.rs`

- [ ] **Step 1: 类型 + 采样（READ `backtest/portfolio.rs` 的加载/timeline/is_fresh 段与 `backtest/runner.rs`(或所在处) 的 `forward_return` 签名先）**

```rust
pub struct FactorSpecItem { pub name: String, pub expr: String }

pub struct FactorConfig {
    pub universe_path: PathBuf,
    pub factors: Vec<FactorSpecItem>,
    pub sample: usize,    // 采样间隔 K
    pub horizon: usize,   // 主前瞻 H
    pub layers: usize,    // Q
    pub warmup: usize,
    pub window: usize,
    pub out_path: PathBuf,
    pub html_path: Option<PathBuf>,
}

/// 一个采样期的原始观测：每标的（因子值按因子序、收益按阶梯序对齐）。
pub(crate) struct SymbolPoint {
    pub symbol: String,
    pub factors: Vec<Option<f64>>, // 非有限 → None
    pub rets: Vec<Option<f64>>,    // forward_return gross；尾部不足 → None
}
pub(crate) struct PeriodData {
    pub t: chrono::NaiveDateTime,
    pub points: Vec<SymbolPoint>,
}

pub(crate) fn collect_periods(cfg: &FactorConfig) -> crate::Result<(Vec<PeriodData>, Vec<usize> /*ladder*/, usize /*n_symbols*/)>
```
`collect_periods` 逻辑：
1. 校验：factors 非空、name 唯一非空、`sample/layers/horizon ≥ 1`；每个 expr `dsl::parser` 解析（失败 → 加载错，含因子名）。
2. universe 加载（primary/context bars，mirror portfolio）；`timeline = build_timeline`；采样索引 `warmup..len step K`；< 2 → Error::Data。
3. `ladder = stats::decay_ladder(cfg.horizon)`。
4. 每采样点 t × 每标的：`bars.binary_search_by_key(&t, |b| b.time)` 命中才参与（即 is_fresh 的索引版）；`build_context`（news 空、aux 空）逐因子 `eval_scalar`（Err 或非有限 → None）；逐阶梯 h `forward_return(bars, i, h, Stance::Long, 零成本)` 取 `.gross`（None → None；零成本构造按实际 CostModel 形态，cost_bps=0）。
5. 返回 PeriodData 序列（即使某期全 None 也保留，聚合层负责跳过计数）。

- [ ] **Step 2: 合成黄金测试（tempfile universe，6 标的恒定增长率 g_k 升序 → 动量因子与未来收益同序）**

```rust
    // fixture：标的 k 价格 p_t = 10·(1+g_k)^t，g_k ∈ {0.001,0.002,...,0.006}；
    // 同一时间网格（跨多日，>40 bar）；factor "mom=close/ref(close,4)-1"。
    // 断言：collect_periods 后任一采样期，有效 points==6；按 factors[0] 排序的 symbol 顺序
    // 与按 rets[主H档] 排序一致（同序 ⇒ 后续 RankIC=1 的前提成立）。
```
（测试写为可执行断言：对每期收集 (factor, ret) 有效对，`spearman(factors, rets).unwrap() ≈ 1.0`——直接用 T1 的 spearman 当测试器。）

- [ ] **Step 3: GREEN + clippy + Commit**

```bash
git add src/factor/mod.rs
git commit -m "feat(factor): cross-sectional sampling loop (factor values x horizon-ladder gross returns)" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: 聚合（IC 汇总/衰减/分层/相关性）

**Files:**
- Modify: `src/factor/mod.rs`

- [ ] **Step 1: 报告类型 + 聚合实现（公式 = spec §3 逐条）**

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct LayerStats {
    pub q: usize,
    pub ann_returns: Vec<Option<f64>>,  // 低→高因子层
    pub spread_total: f64,              // top−bottom 连乘净值 −1
    pub spread_ann: Option<f64>,
    pub spread_sharpe: Option<f64>,
    pub monotonicity: Option<f64>,      // spearman(层序号, 层期均收益)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FactorStats {
    pub name: String,
    pub expr: String,
    pub n_periods: usize,   // 进入 IC 统计的有效期数
    pub n_skipped: usize,   // 有效对 < max(Q,5) 被跳过的期数
    pub ic_mean: Option<f64>, pub ic_std: Option<f64>, pub icir: Option<f64>,
    pub ic_t: Option<f64>, pub ic_pos_share: Option<f64>,
    pub rank_ic_mean: Option<f64>, pub rank_ic_std: Option<f64>, pub rank_icir: Option<f64>,
    pub rank_ic_t: Option<f64>, pub rank_ic_pos_share: Option<f64>,
    pub ic_decay: Vec<(usize, Option<f64>)>,  // (horizon, mean RankIC)
    pub layers: Option<LayerStats>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CorrMatrix { pub names: Vec<String>, pub values: Vec<Vec<Option<f64>>> }

#[derive(Debug, Serialize, Deserialize)]
pub struct FactorReport {
    pub n_symbols: usize, pub n_sample_points: usize,
    pub sample: usize, pub horizon: usize, pub layers_q: usize,
    pub factors: Vec<FactorStats>,
    pub corr: Option<CorrMatrix>,
}

pub fn run_factor(cfg: &FactorConfig) -> crate::Result<FactorReport>
```
聚合逻辑（每因子 f_idx，主档 = ladder 中 horizon 的位置）：
1. **逐期主档有效对**（factor Some 且 ret Some）：count < `max(Q,5)` → n_skipped+=1 跳过；否则该期算 `pearson`/`spearman` 入 IC/RankIC 序列，并做**分层**：按因子升序、`layer_sizes` 切层、层收益=成员 ret 均值、各层 nav ×=(1+r)、spread nav ×=(1+r_top−r_bottom)（nav 点记 (t, nav)，spread 同步记峰值算 max_dd）。
2. IC 汇总：mean/sample_std/ICIR（std>1e-12 否则 None）/`risk::t_stat`/正占比；序列空 → 全 None。
3. 衰减：每阶梯 h 独立——逐期有效对 ≥ 5 才计该期 RankIC，均值（无有效期 → None）。
4. 层年化：每层 nav 点列 → `risk_metrics(点列, 0.0)` 取 `ann_return`；spread → `risk_metrics(点列, spread_max_dd)` 取 ann/sharpe，total = nav 末值 −1。单调性 = `spearman(0..Q 序号, 各层期均收益)`。
5. **相关性**（factors.len() ≥ 2）：逐期每因子对在共同 Some 标的上 `spearman`（共同点 ≥ 5 才计）→ 各期平均（无 → None）；对角 Some(1.0)。
6. 组装 FactorReport，写 `cfg.out_path` JSON pretty；返回。

- [ ] **Step 2: 测试（沿用 Task 2 fixture）**

- `run_factor`（mom 单因子）：`rank_ic_mean ≈ 1.0`、`rank_icir` Some、layers 单调性 = Some(≈1.0)、`spread_total > 0`、`ann_returns` 低层 < 高层（首尾比较）。
- 反向因子 `rev=ref(close,4)/close-1`：`rank_ic_mean ≈ −1.0`、`spread_total < 0`、单调性 ≈ −1。
- 双因子 mom+rev：`corr.values[0][1] ≈ Some(−1.0)`、对角 Some(1.0)。
- 阈值跳过：universe 截到 3 标的（< max(5,Q)）→ 全期 skipped、IC 全 None、n_periods=0（不 panic）。

- [ ] **Step 3: GREEN + clippy + Commit**

```bash
git add src/factor/mod.rs
git commit -m "feat(factor): IC summary, decay ladder, quantile layers with spread, correlation matrix" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: CLI + print + HTML

**Files:**
- Modify: `src/cli/mod.rs`、`src/factor/mod.rs`（print）、`src/report/viz.rs`

- [ ] **Step 1: CLI（READ Portfolio 变体风格）**

```rust
    /// Cross-sectional factor workbench: IC/RankIC, decay, quantile layers, correlation
    Factor {
        #[arg(long)]
        universe: PathBuf,
        /// Repeatable: --factor "name=DSL expr"
        #[arg(long = "factor", value_name = "NAME=EXPR")]
        factor: Vec<String>,
        #[arg(long, default_value_t = 16)]
        sample: usize,
        #[arg(long, default_value_t = 16)]
        horizon: usize,
        #[arg(long, default_value_t = 5)]
        layers: usize,
        #[arg(long, default_value_t = 100)]
        warmup: usize,
        #[arg(long, default_value_t = 100)]
        window: usize,
        #[arg(long, default_value = "factor_report.json")]
        out: PathBuf,
        #[arg(long)]
        html: Option<PathBuf>,
    },
```
分流：`factor` 每项按首个 `=` 切 name/expr（缺 `=`/空 name/空 expr/重复 name → anyhow 错误）；空列表 → 错误；构造 FactorConfig → `run_factor`（同步，直接调用）→ `print_factor_summary(&report)`；`html` Some → `render_factor_html` 写出。

- [ ] **Step 2: print_factor_summary**

每因子一块：name/expr、n_periods(+skipped)、`RankIC均值/ICIR/t/正占比`、IC 同行、衰减一行（`h=1:0.04 h=2:0.03 …`）、层年化一行（低→高 + spread total/ann/Sharpe）、单调性；多因子加相关性矩阵块（None → "—"，风格同既有 print）。

- [ ] **Step 3: render_factor_html（READ render_portfolio_html 外壳）**

headline 表（参数回显 + 每因子 RankIC/ICIR/单调性/spread）→ **IC 衰减** `multi_line_chart`（每因子一线，x=阶梯序，y=mean RankIC，None 档跳过点）→ 每因子**分层年化** `bar_chart`（低→高，None → 0 并在标题注明）→ 每因子 **spread 净值** `line_chart` → 相关性 HTML 表。确定性测试 + 关键子串（"RankIC"、polyline 数 ≥ 因子数）。

- [ ] **Step 4: 全绿 + clippy + `factor --help` + Commit**

```bash
git add src/cli/mod.rs src/factor/mod.rs src/report/viz.rs
git commit -m "feat(cli,report): factor subcommand with summary print and HTML (decay/layers/spread/corr)" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 5: e2e + 文档 + 真数据 smoke

**Files:**
- Modify: `tests/e2e.rs`、`docs/cli-reference.md`、`README.md`

- [ ] **Step 1: e2e**

`factor_full_chain`：6 标的合成 universe（Task 2 fixture 形态，tempfile）+ 双因子（mom/rev）经 `run_factor` → JSON 写出可反序列化、`rank_ic_mean` 符号正确（mom>0.9、rev<−0.9）、corr[0][1]≈−1、HTML 含 "RankIC"。

- [ ] **Step 2: 文档**

- cli-reference：factor 子命令全旗标表 + 输出字段表 + **判读标准**（spec §5 三条：|RankIC|>0.03 且 |ICIR|>0.3 入树线；|单调性|>0.8 且 |spread Sharpe|>1 强因子；相关>0.7 留 ICIR 高者；负值=反向）+ **gross 提醒**（入树后必须 backtest/sim 含成本复检）。
- README：factor 一节（命令示例 + 研究循环定位：检验 → 入树 → 回测）。

- [ ] **Step 3: 真数据 smoke（手动不入库）**

4 真股 qfq 60m（sh600000/sh600036/sz000001/sz000002）写 universe → `factor --factor "mom20=close/ref(close,20)-1" --factor "rsi14=rsi(close,14)" --sample 8 --horizon 8 --warmup 60 --html tmp/factor.html` → 记录 RankIC/ICIR/单调性/相关性数字（判读一句话）→ 清理。

- [ ] **Step 4: 全绿 + Commit**

```bash
git add tests/e2e.rs docs/cli-reference.md README.md
git commit -m "test+docs: factor workbench e2e, interpretation guide, real-data smoke" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## 附录 A：Spec 覆盖对照（自检）
| Spec § | 实现于 |
|---|---|
| §3 公式（秩/相关/阈值跳过/阶梯/分层/spread/单调性/corr）| T1 黄金 + T2/T3 |
| §4 架构（factor 模块/复用清单/同步）| T1-T3 |
| §4 HTML/print/CLI | T4 |
| §5 判读标准 + gross 提醒 | T5 文档 |
| §6 测试（黄金/反号/corr/跳过/e2e/smoke）| T1-T5 |

## 附录 B：明确不在范围（YAGNI）
- LLM 因子；中性化/去极值/z-score；时序 IC；分层成本；--aux 挂载（后续可加）；bootstrap。
