# A股日线选股器（优质+投机价值标注筛选）Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a multi-tree-ensemble daily stock screener that scores "quality" and tags speculative setups for A-share symbols, with a historical-backtest validation mechanism, on the deep 20-symbol data.

**Architecture:** A thin new `src/screen/` orchestrator runs N cross-sectional selection trees (strength-tree family) in parallel per symbol, then combines per-symbol tree scalars into a quality score + speculative tags (vote) + combined score (dual output). It REUSES the validated engine: `score_symbol` / `build_context` / `traverse` for per-symbol evaluation, and `build_timeline` / `last_close_at` / `select_top` / `accrue` / `turnover_between` / `risk_metrics` for the backtest. True cross-section (rank/select across the universe) lives in the orchestrator; within-tree `percentrank` is time-series self-normalization.

**Tech Stack:** Rust 2024, serde / serde_yaml / serde_json, chrono, tokio (async tree eval), clap (CLI). Spec: `docs/superpowers/specs/2026-06-15-screen-quality-speculative-design.md`.

---

## Reuse Reference (exact existing signatures — do NOT modify these)

All in crate `rquant` (root):

```rust
// src/data/bar.rs
pub struct Bar { pub time: NaiveDateTime, pub open: f64, pub high: f64, pub low: f64, pub close: f64, pub volume: f64 }
// src/data/reader.rs
pub fn read_bars_csv(path: &Path) -> Result<Vec<Bar>>            // CSV: time,open,high,low,close,volume
// src/data/universe.rs
pub struct UniverseEntry { pub symbol: String, pub primary: PathBuf, pub context: PathBuf }
pub fn read_universe_csv(path: &Path) -> Result<Vec<UniverseEntry>>   // CSV: symbol,primary[,context]; sorted by symbol
// src/tree/loader.rs
pub fn load_tree_file(path: &Path) -> Result<Tree>
pub struct Tree { pub meta: Meta, /* ... */ pub leaves: HashMap<String, Leaf> }   // tree.meta.name: String
pub struct Leaf { pub stance: Stance, /* ... */ }                                  // Leaf::weight_at(&Context) -> f64
// src/tree/schema.rs
pub enum Stance { Long, Flat, Short }   // #[serde(rename_all="lowercase")]
// src/features/context.rs
pub fn build_context(primary: &[Bar], context: &[Bar], news: &[NewsRecord], aux: &BTreeMap<String, AuxTable>, t: NaiveDateTime, window: usize) -> Context
// src/engine/traversal.rs
pub async fn traverse(tree: &Tree, ctx: &Context, llm: &LlmEvaluator) -> Result<Trace>   // Trace { leaf: String, ... }
// src/backtest/portfolio.rs  (ALL pub)
pub fn build_timeline(all: &[Vec<Bar>]) -> Vec<NaiveDateTime>
pub fn last_close_at(bars: &[Bar], t: NaiveDateTime) -> Option<f64>
pub fn is_fresh(bars: &[Bar], t: NaiveDateTime) -> bool
pub fn select_top(scores: &[(String, f64)], n: usize) -> Vec<(String, f64)>   // filters s>0, desc, tie symbol asc
pub fn accrue(weights: &BTreeMap<String,f64>, px_start: &BTreeMap<String,f64>, px_end: &BTreeMap<String,f64>) -> f64
pub fn turnover_between(old: &BTreeMap<String,f64>, new: &BTreeMap<String,f64>) -> f64
pub async fn score_symbol(primary: &[Bar], context: &[Bar], aux: &BTreeMap<String, AuxTable>, tree: &Tree, llm: &LlmEvaluator, soft: bool, t: NaiveDateTime, window: usize) -> Result<Option<f64>>
// src/report/risk.rs
pub fn risk_metrics(nav: &[(NaiveDateTime, f64)], max_drawdown: f64) -> Option<RiskMetrics>
// RiskMetrics fields: ann_return, ann_vol, sharpe, sortino, calmar, var95, cvar95 (all f64 / Option<f64>)
// src/eval/llm.rs
pub enum LlmEvaluator { Disabled, /* ... */ }   // screen uses Disabled (pure-quant trees)
```

**Module path note:** `AuxTable` = `crate::data::aux_table::AuxTable`. `NewsRecord` = `crate::data::news::NewsRecord` (pass `&[]`). `LlmEvaluator` = `crate::eval::llm::LlmEvaluator`.

**Window/warmup constraint:** the seed quality tree uses `ema(close,200)` → evaluation `window` must be ≥ ~220 and backtest `warmup` ≥ ~260. Screen CLI defaults: `window=260`, `warmup=260`. Unit tests use SIMPLE inline trees with small windows; the seed trees are validated by the real-data smoke (SCR-10), not unit tests.

---

## Task SCR-1: Module skeleton + screen config types + loader

**Files:**
- Create: `src/screen/mod.rs`
- Create: `src/screen/config.rs`
- Modify: `src/lib.rs` (add `pub mod screen;` near the other `pub mod` lines)

- [ ] **Step 1: Register the module**

In `src/lib.rs`, add alongside the existing `pub mod` declarations (e.g. after `pub mod report;` or near `pub mod optimize;`):

```rust
pub mod screen;
```

In `src/screen/mod.rs` (initial content):

```rust
//! 日线选股器：多树并行集成 → 优质分 + 投机形态标注（双输出）+ 历史回测验证。

pub mod config;
```

- [ ] **Step 2: Write failing test for config parse**

Create `src/screen/config.rs`:

```rust
//! 选股集成配置（数据驱动：加/裁树 = 改配置非改码）。

use crate::Result;
use chrono::NaiveDate;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// 集成合并参数（起始值经 spec §5 迭代定）。
#[derive(Debug, Clone, Deserialize)]
pub struct MergeConfig {
    #[serde(default = "default_theta_fire")]
    pub theta_fire: f64,
    #[serde(default = "default_vote_frac")]
    pub vote_frac: f64,
    #[serde(default = "default_q_floor")]
    pub q_floor: f64,
    #[serde(default = "default_top")]
    pub top: usize,
    /// 优质分分层数（回测质量分层用）。
    #[serde(default = "default_layers")]
    pub quality_layers: usize,
}

fn default_theta_fire() -> f64 { 0.5 }
fn default_vote_frac() -> f64 { 0.5 }
fn default_q_floor() -> f64 { 0.5 }
fn default_top() -> usize { 10 }
fn default_layers() -> usize { 3 }

impl Default for MergeConfig {
    fn default() -> Self {
        MergeConfig {
            theta_fire: default_theta_fire(),
            vote_frac: default_vote_frac(),
            q_floor: default_q_floor(),
            top: default_top(),
            quality_layers: default_layers(),
        }
    }
}

/// 命名 regime 窗口（回测跨牛熊切片用）。
#[derive(Debug, Clone, Deserialize)]
pub struct RegimeWindow {
    pub label: String,
    pub from: NaiveDate,
    pub to: NaiveDate,
}

/// 选股集成配置。树路径相对 cwd（同 portfolio 约定）。
#[derive(Debug, Clone, Deserialize)]
pub struct ScreenConfig {
    pub quality_trees: Vec<PathBuf>,
    /// 形态标签 -> 该形态的树集（可多树投票）。
    pub setup_trees: BTreeMap<String, Vec<PathBuf>>,
    #[serde(default)]
    pub merge: MergeConfig,
    #[serde(default)]
    pub regimes: Vec<RegimeWindow>,
}

impl ScreenConfig {
    /// 校验：至少 1 棵优质树、至少 1 个形态、各形态非空、参数范围合法。
    pub fn validate(&self) -> Result<()> {
        if self.quality_trees.is_empty() {
            return Err(crate::Error::Data("screen config: quality_trees must be non-empty".into()));
        }
        if self.setup_trees.is_empty() {
            return Err(crate::Error::Data("screen config: setup_trees must be non-empty".into()));
        }
        for (tag, trees) in &self.setup_trees {
            if trees.is_empty() {
                return Err(crate::Error::Data(format!("screen config: setup '{tag}' has no trees")));
            }
        }
        let m = &self.merge;
        if !(0.0..=1.0).contains(&m.theta_fire) || !(0.0..=1.0).contains(&m.q_floor) {
            return Err(crate::Error::Data("screen config: theta_fire/q_floor must be in [0,1]".into()));
        }
        if !(m.vote_frac > 0.0 && m.vote_frac <= 1.0) {
            return Err(crate::Error::Data("screen config: vote_frac must be in (0,1]".into()));
        }
        if m.top == 0 {
            return Err(crate::Error::Data("screen config: top must be >= 1".into()));
        }
        Ok(())
    }
}

/// 从 YAML 文件加载并校验。
pub fn load_screen_config(path: &Path) -> Result<ScreenConfig> {
    let src = std::fs::read_to_string(path)?;
    let cfg: ScreenConfig = serde_yaml::from_str(&src)
        .map_err(|e| crate::Error::Data(format!("screen config parse error: {e}")))?;
    cfg.validate()?;
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_config_with_defaults() {
        let yaml = r#"
quality_trees: [examples/trees/screen/quality_v1.yaml]
setup_trees:
  动量延续: [examples/trees/screen/momentum_v1.yaml]
"#;
        let cfg: ScreenConfig = serde_yaml::from_str(yaml).unwrap();
        cfg.validate().unwrap();
        assert_eq!(cfg.quality_trees.len(), 1);
        assert_eq!(cfg.setup_trees.len(), 1);
        assert!((cfg.merge.theta_fire - 0.5).abs() < 1e-12);
        assert_eq!(cfg.merge.top, 10);
        assert_eq!(cfg.merge.quality_layers, 3);
        assert!(cfg.regimes.is_empty());
    }

    #[test]
    fn validate_rejects_empty_quality_trees() {
        let yaml = r#"
quality_trees: []
setup_trees:
  x: [a.yaml]
"#;
        let cfg: ScreenConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_rejects_bad_vote_frac() {
        let yaml = r#"
quality_trees: [q.yaml]
setup_trees:
  x: [a.yaml]
merge: { vote_frac: 0.0 }
"#;
        let cfg: ScreenConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn parses_regimes() {
        let yaml = r#"
quality_trees: [q.yaml]
setup_trees:
  x: [a.yaml]
regimes:
  - { label: "2018熊", from: 2018-01-02, to: 2018-12-28 }
"#;
        let cfg: ScreenConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.regimes.len(), 1);
        assert_eq!(cfg.regimes[0].label, "2018熊");
    }
}
```

- [ ] **Step 3: Run tests to verify they fail then pass**

Run: `cargo test --lib screen::config`
Expected: compiles and 4 tests PASS. (If `crate::Error::Data` variant name differs, check `src/lib.rs` / `src/error.rs` for the actual data-error variant and adjust — grep `enum Error`.)

- [ ] **Step 4: Commit**

```bash
git add src/lib.rs src/screen/mod.rs src/screen/config.rs
git commit -m "feat(screen): module skeleton + ensemble config types + loader"
```

---

## Task SCR-2: Combine pure logic (quality mean / setup vote / dual output)

**Files:**
- Create: `src/screen/combine.rs`
- Modify: `src/screen/mod.rs` (add `pub mod combine;`)

- [ ] **Step 1: Add module declaration**

In `src/screen/mod.rs` add:

```rust
pub mod combine;
```

- [ ] **Step 2: Write the failing tests + implementation**

Create `src/screen/combine.rs`:

```rust
//! 纯合并逻辑：每股的「逐树标量」→ 优质分 + 形态投票标签 + 综合分（双输出）。
//! 真横截面（跨标的排名/选股）不在这里——由编排器用 portfolio::select_top 做。

use std::collections::BTreeMap;

/// 合并参数。
#[derive(Debug, Clone, Copy)]
pub struct MergeParams {
    pub theta_fire: f64,
    pub vote_frac: f64,
    pub q_floor: f64,
}

/// 单股合并输出。
#[derive(Debug, Clone, PartialEq)]
pub struct CombineOutput {
    pub quality_score: f64,
    pub speculative_score: f64,
    pub combined_score: f64,
    /// 命中（投票通过）的形态标签，按标签名升序（BTreeMap 保序）。
    pub tags: Vec<String>,
    /// 命中形态 -> 强度（用于回测归因）。
    pub setup_strength: BTreeMap<String, f64>,
}

/// 有限值均值；无有限值 → 0。
fn mean_finite(xs: &[f64]) -> f64 {
    let v: Vec<f64> = xs.iter().copied().filter(|x| x.is_finite()).collect();
    if v.is_empty() { 0.0 } else { v.iter().sum::<f64>() / v.len() as f64 }
}

/// 单形态投票：命中当 count(s >= theta_fire) >= ceil(n*vote_frac)（下限 1）；
/// 强度 = 命中树得分均值（未命中 → (false, 0)）。
pub fn setup_vote(scores: &[f64], theta_fire: f64, vote_frac: f64) -> (bool, f64) {
    let finite: Vec<f64> = scores.iter().copied().filter(|x| x.is_finite()).collect();
    let n = finite.len();
    if n == 0 {
        return (false, 0.0);
    }
    let need = ((n as f64 * vote_frac).ceil() as usize).max(1);
    let firing: Vec<f64> = finite.into_iter().filter(|s| *s >= theta_fire).collect();
    if firing.len() >= need {
        let strength = firing.iter().sum::<f64>() / firing.len() as f64;
        (true, strength)
    } else {
        (false, 0.0)
    }
}

/// 合并：优质分 = 优质树得分均值；形态 = 投票；投机分 = 命中形态最大强度；
/// 综合分 = 优质×投机，但不合格（无标签 或 优质<q_floor）→ 0。
pub fn combine(
    quality: &[f64],
    setups: &BTreeMap<String, Vec<f64>>,
    p: &MergeParams,
) -> CombineOutput {
    let q = mean_finite(quality);
    let mut tags = Vec::new();
    let mut setup_strength: BTreeMap<String, f64> = BTreeMap::new();
    for (tag, scores) in setups {
        let (fired, strength) = setup_vote(scores, p.theta_fire, p.vote_frac);
        if fired {
            tags.push(tag.clone());
            setup_strength.insert(tag.clone(), strength);
        }
    }
    let spec = setup_strength.values().copied().fold(0.0_f64, f64::max);
    let eligible = !tags.is_empty() && q >= p.q_floor;
    let combined = if eligible { q * spec } else { 0.0 };
    CombineOutput {
        quality_score: q,
        speculative_score: spec,
        combined_score: combined,
        tags,
        setup_strength,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p() -> MergeParams { MergeParams { theta_fire: 0.5, vote_frac: 0.5, q_floor: 0.5 } }

    #[test]
    fn vote_single_tree_fires_when_above_theta() {
        assert_eq!(setup_vote(&[0.7], 0.5, 0.5), (true, 0.7));
        assert_eq!(setup_vote(&[0.3], 0.5, 0.5), (false, 0.0));
    }

    #[test]
    fn vote_majority_of_three() {
        // need = ceil(3*0.5) = 2
        assert_eq!(setup_vote(&[0.6, 0.8, 0.1], 0.5, 0.5).0, true);  // 2 fire
        assert_eq!(setup_vote(&[0.6, 0.1, 0.1], 0.5, 0.5).0, false); // 1 fires < 2
        let (fired, strength) = setup_vote(&[0.6, 0.8, 0.1], 0.5, 0.5);
        assert!(fired);
        assert!((strength - 0.7).abs() < 1e-12); // mean of firing {0.6,0.8}
    }

    #[test]
    fn vote_empty_or_nan() {
        assert_eq!(setup_vote(&[], 0.5, 0.5), (false, 0.0));
        assert_eq!(setup_vote(&[f64::NAN], 0.5, 0.5), (false, 0.0));
    }

    #[test]
    fn combine_quality_is_mean() {
        let setups = BTreeMap::new();
        let out = combine(&[1.0, 0.5], &setups, &p());
        assert!((out.quality_score - 0.75).abs() < 1e-12);
    }

    #[test]
    fn combine_tags_and_combined_score() {
        let mut setups = BTreeMap::new();
        setups.insert("动量延续".to_string(), vec![0.8]);
        setups.insert("超跌反弹".to_string(), vec![0.2]); // below theta → not fired
        let out = combine(&[0.9], &setups, &p());
        assert_eq!(out.tags, vec!["动量延续".to_string()]);
        assert!((out.speculative_score - 0.8).abs() < 1e-12);
        assert!((out.combined_score - 0.9 * 0.8).abs() < 1e-12);
    }

    #[test]
    fn combine_ineligible_when_no_tags() {
        let setups = BTreeMap::new();
        let out = combine(&[1.0], &setups, &p());
        assert!(out.tags.is_empty());
        assert_eq!(out.combined_score, 0.0);
    }

    #[test]
    fn combine_ineligible_when_quality_below_floor() {
        let mut setups = BTreeMap::new();
        setups.insert("动量延续".to_string(), vec![0.9]);
        let out = combine(&[0.3], &setups, &p()); // quality 0.3 < q_floor 0.5
        assert_eq!(out.tags, vec!["动量延续".to_string()]); // tag still reported
        assert_eq!(out.combined_score, 0.0);               // but not eligible
    }

    #[test]
    fn combine_speculative_is_max_over_setups() {
        let mut setups = BTreeMap::new();
        setups.insert("动量延续".to_string(), vec![0.6]);
        setups.insert("突破临界".to_string(), vec![0.9]);
        let out = combine(&[1.0], &setups, &p());
        assert_eq!(out.tags.len(), 2);
        assert!((out.speculative_score - 0.9).abs() < 1e-12);
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib screen::combine`
Expected: all 8 tests PASS.

- [ ] **Step 4: Commit**

```bash
git add src/screen/mod.rs src/screen/combine.rs
git commit -m "feat(screen): pure combine logic — quality mean, setup vote, dual output"
```

---

## Task SCR-3: Seed trees (quality + 3 setups) + load/lint test

**Files:**
- Create: `examples/trees/screen/quality_v1.yaml`
- Create: `examples/trees/screen/momentum_v1.yaml`
- Create: `examples/trees/screen/breakout_v1.yaml`
- Create: `examples/trees/screen/pullback_v1.yaml`
- Create: `tests/screen_seed_trees.rs` (integration test asserting all seed trees load)

- [ ] **Step 1: Write the seed trees**

`examples/trees/screen/quality_v1.yaml`:

```yaml
# 优质·技术稳健 v1 — 横截面选择树（long graded = 优质分；flat = 不合格）
# 闸：均线多头排列 → 流动性下限 → 按距高点(回撤健康)+趋势噪声比分级。
meta:
  name: "优质·技术稳健 v1"
  forward_window: 60
  stances: [long, flat]
params:
  n_ema_long: 200
  n_ema_mid: 50
  n_dd: 120
  n_std: 20
  n_slope: 20
  n_liq: 20
  liq_floor: 50000000      # 日均成交额下限（元）— 起始值，迭代定
  dd_good: 0.90            # 距120日高点 >=90% → 优
  dd_ok: 0.80
factors:
  ema_l: "ema(close, n_ema_long)"
  ema_m: "ema(close, n_ema_mid)"
  liq:   "sma(close * volume, n_liq)"
  dd:    "close / highest(close, n_dd)"
  tns:   "slope(ema(close, n_slope), n_slope) / (std(close, n_std) + 0.000000001)"
root: gate_trend
nodes:
  gate_trend:
    type: quant
    branches:
      - when: "close > ema_l and ema_m > ema_l"
        goto: gate_liq
        label: trend_ok
    default: { goto: leaf_fail, label: no_trend }
  gate_liq:
    type: quant
    branches:
      - when: "liq >= liq_floor"
        goto: grade
        label: liquid
    default: { goto: leaf_fail, label: illiquid }
  grade:
    type: quant
    branches:
      - when: "dd >= dd_good and tns > 0"
        goto: leaf_hi
        label: excellent
      - when: "dd >= dd_ok"
        goto: leaf_mid
        label: good
    default: { goto: leaf_lo, label: fair }
leaves:
  leaf_fail: { stance: flat }
  leaf_hi:   { stance: long, weight: 1.0,  horizon: 60 }
  leaf_mid:  { stance: long, weight: 0.66, horizon: 60 }
  leaf_lo:   { stance: long, weight: 0.33, horizon: 60 }
```

`examples/trees/screen/momentum_v1.yaml`:

```yaml
# 动量延续 v1 — 强者恒强：自归一动量分位高 + 趋势在位。
meta:
  name: "动量延续 v1"
  forward_window: 20
  stances: [long, flat]
params:
  n_mom: 20
  n_rank: 60
  n_ema: 50
  n_slope: 10
  thr_hi: 0.85
  thr_mid: 0.65
factors:
  mom:       "close / ref(close, n_mom) - 1"
  mom_pct:   "percentrank(mom, n_rank)"
  ema_t:     "ema(close, n_ema)"
  ema_slope: "slope(ema(close, n_ema), n_slope)"
root: gate_trend
nodes:
  gate_trend:
    type: quant
    branches:
      - when: "close > ema_t and ema_slope > 0"
        goto: bands
        label: trend_ok
    default: { goto: leaf_flat, label: no_trend }
  bands:
    type: quant
    branches:
      - when: "mom_pct >= thr_hi"
        goto: leaf_hi
        label: strong
      - when: "mom_pct >= thr_mid"
        goto: leaf_mid
        label: moderate
    default: { goto: leaf_flat, label: weak }
leaves:
  leaf_flat: { stance: flat }
  leaf_hi:   { stance: long, weight: 1.0, horizon: 20 }
  leaf_mid:  { stance: long, weight: 0.6, horizon: 20 }
```

`examples/trees/screen/breakout_v1.yaml`:

```yaml
# 突破临界 v1 — 距60日前高近 + 量起 + 缩量蓄势（缩量盘整→放量突破前夜）。
meta:
  name: "突破临界 v1"
  forward_window: 10
  stances: [long, flat]
params:
  n_high: 60
  prox: 0.97
  n_vol: 20
  n_std_fast: 10
  n_std_slow: 40
factors:
  prior_high: "highest(ref(high, 1), n_high)"
  vol_ma:     "sma(volume, n_vol)"
  std_fast:   "std(close, n_std_fast)"
  std_slow:   "std(close, n_std_slow)"
root: gate
nodes:
  gate:
    type: quant
    branches:
      - when: "close >= 0.99 * prior_high and volume > vol_ma and std_fast < std_slow"
        goto: leaf_hi
        label: at_breakout
      - when: "close >= prox * prior_high and volume > vol_ma and std_fast < std_slow"
        goto: leaf_mid
        label: near_breakout
    default: { goto: leaf_flat, label: none }
leaves:
  leaf_flat: { stance: flat }
  leaf_hi:   { stance: long, weight: 1.0, horizon: 10 }
  leaf_mid:  { stance: long, weight: 0.7, horizon: 10 }
```

`examples/trees/screen/pullback_v1.yaml`:

```yaml
# 超跌反弹 v1 — 长趋势在位但短期深跌近支撑（上升趋势中的回调买点）。
meta:
  name: "超跌反弹 v1"
  forward_window: 10
  stances: [long, flat]
params:
  n_ema_long: 120
  n_rsi: 14
  n_ema_short: 20
  rsi_os: 35
  pb: 0.95
factors:
  ema_l: "ema(close, n_ema_long)"
  rsi_v: "rsi(close, n_rsi)"
  ema_s: "ema(close, n_ema_short)"
root: gate_trend
nodes:
  gate_trend:
    type: quant
    branches:
      - when: "close > ema_l"
        goto: gate_pull
        label: uptrend
    default: { goto: leaf_flat, label: no_trend }
  gate_pull:
    type: quant
    branches:
      - when: "rsi_v < rsi_os"
        goto: leaf_hi
        label: oversold
      - when: "close < ema_s * pb"
        goto: leaf_mid
        label: below_ma
    default: { goto: leaf_flat, label: none }
leaves:
  leaf_flat: { stance: flat }
  leaf_hi:   { stance: long, weight: 1.0, horizon: 10 }
  leaf_mid:  { stance: long, weight: 0.6, horizon: 10 }
```

- [ ] **Step 2: Write the load test**

Create `tests/screen_seed_trees.rs`:

```rust
//! 种子树须能加载且通过加载期 lint（构造正确性闸）。
use std::path::Path;

#[test]
fn all_seed_trees_load() {
    let paths = [
        "examples/trees/screen/quality_v1.yaml",
        "examples/trees/screen/momentum_v1.yaml",
        "examples/trees/screen/breakout_v1.yaml",
        "examples/trees/screen/pullback_v1.yaml",
    ];
    for p in paths {
        let tree = rquant::tree::loader::load_tree_file(Path::new(p))
            .unwrap_or_else(|e| panic!("seed tree {p} failed to load/lint: {e}"));
        assert!(!tree.meta.name.is_empty(), "tree {p} has empty name");
        assert!(!tree.leaves.is_empty(), "tree {p} has no leaves");
    }
}
```

- [ ] **Step 3: Run the test**

Run: `cargo test --test screen_seed_trees`
Expected: PASS. If a tree fails load-time lint (e.g. L1 const-false / L2 single-length, or an unknown DSL identifier like `high`/`volume`), read the error and fix the YAML. Confirm `high`/`volume`/`rsi`/`slope`/`percentrank`/`highest`/`ref`/`std`/`sma`/`ema` are all valid by grepping `src/dsl/` and `src/features/` — adjust any factor that the lint rejects.

- [ ] **Step 4: Commit**

```bash
git add examples/trees/screen/quality_v1.yaml examples/trees/screen/momentum_v1.yaml examples/trees/screen/breakout_v1.yaml examples/trees/screen/pullback_v1.yaml tests/screen_seed_trees.rs
git commit -m "feat(screen): seed trees (quality + 3 speculative setups) + load/lint test"
```

---

## Task SCR-4: As-of orchestrator (run_screen) + types + print + JSON

**Files:**
- Modify: `src/screen/mod.rs` (add result types, `score_and_leaf` helper, `run_screen`, `print_screen`)

- [ ] **Step 1: Write the orchestrator + types + tests**

Append to `src/screen/mod.rs` (after the `pub mod` lines):

```rust
use crate::data::aux_table::AuxTable;
use crate::data::bar::Bar;
use crate::eval::llm::LlmEvaluator;
use crate::tree::loader::Tree;
use crate::tree::schema::Stance;
use crate::Result;
use chrono::{NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::backtest::portfolio::{build_timeline, is_fresh, select_top};
use crate::screen::combine::{combine, CombineOutput, MergeParams};
use crate::screen::config::{load_screen_config, ScreenConfig};

/// 单棵树命中理由。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenReason {
    pub tree: String,
    pub leaf: String,
    pub score: f64,
}

/// 单股选股记录（双输出：tags 标注 + combined_score 排名）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenRow {
    pub symbol: String,
    pub rank: usize,
    pub quality_score: f64,
    pub speculative_score: f64,
    pub combined_score: f64,
    pub tags: Vec<String>,
    pub selected: bool,
    pub reasons: Vec<ScreenReason>,
}

/// 选股结果（as-of 某根 K）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenResult {
    pub as_of: NaiveDateTime,
    pub n_universe: usize,
    pub top: usize,
    pub rows: Vec<ScreenRow>,
}

/// as-of 选股运行配置。
pub struct ScreenRunConfig {
    pub config_path: PathBuf,
    pub universe_path: PathBuf,
    pub as_of: Option<NaiveDate>,
    pub top: Option<usize>,
    pub window: usize,
    pub out_path: Option<PathBuf>,
}

fn dir(s: Stance) -> f64 {
    match s {
        Stance::Long => 1.0,
        Stance::Short => -1.0,
        Stance::Flat => 0.0,
    }
}

/// 硬模式：返回 (得分, 叶名)；不新鲜 → None。用于 as-of 的可解释路径。
async fn score_and_leaf(
    primary: &[Bar],
    context: &[Bar],
    aux: &BTreeMap<String, AuxTable>,
    tree: &Tree,
    llm: &LlmEvaluator,
    t: NaiveDateTime,
    window: usize,
) -> Result<Option<(f64, String)>> {
    if !is_fresh(primary, t) {
        return Ok(None);
    }
    let ctx = crate::features::context::build_context(primary, context, &[], aux, t, window);
    let tr = crate::engine::traversal::traverse(tree, &ctx, llm).await?;
    let score = tree.leaves.get(&tr.leaf).map_or(0.0, |l| l.weight_at(&ctx) * dir(l.stance));
    Ok(Some((score, tr.leaf.clone())))
}

/// 加载配置声明的所有树：(name, Tree)。
fn load_trees(paths: &[PathBuf]) -> Result<Vec<(String, Tree)>> {
    let mut out = Vec::with_capacity(paths.len());
    for p in paths {
        let t = crate::tree::loader::load_tree_file(p)?;
        out.push((t.meta.name.clone(), t));
    }
    Ok(out)
}

/// 选取 as-of 时间戳：给定日期 → ≤该日期的最大时间线点；否则 → 末点。
fn pick_as_of(timeline: &[NaiveDateTime], as_of: Option<NaiveDate>) -> Result<NaiveDateTime> {
    if timeline.is_empty() {
        return Err(crate::Error::Data("screen: empty timeline".into()));
    }
    match as_of {
        None => Ok(*timeline.last().unwrap()),
        Some(d) => timeline
            .iter()
            .rev()
            .find(|t| t.date() <= d)
            .copied()
            .ok_or_else(|| crate::Error::Data(format!("screen: no bar on/before {d}"))),
    }
}

/// as-of 选股：并行跑树集成 → 合并 → 排名 → ScreenResult。
pub async fn run_screen(cfg: &ScreenRunConfig, llm: &LlmEvaluator) -> Result<ScreenResult> {
    let sc: ScreenConfig = load_screen_config(&cfg.config_path)?;
    let quality = load_trees(&sc.quality_trees)?;
    let mut setups: BTreeMap<String, Vec<(String, Tree)>> = BTreeMap::new();
    for (tag, paths) in &sc.setup_trees {
        setups.insert(tag.clone(), load_trees(paths)?);
    }

    let universe = crate::data::universe::read_universe_csv(&cfg.universe_path)?;
    let mut primaries: Vec<Vec<Bar>> = Vec::with_capacity(universe.len());
    let mut contexts: Vec<Vec<Bar>> = Vec::with_capacity(universe.len());
    for e in &universe {
        primaries.push(crate::data::reader::read_bars_csv(&e.primary)?);
        contexts.push(crate::data::reader::read_bars_csv(&e.context)?);
    }

    let timeline = build_timeline(&primaries);
    let t = pick_as_of(&timeline, cfg.as_of)?;
    let aux: BTreeMap<String, AuxTable> = BTreeMap::new();
    let mp = MergeParams {
        theta_fire: sc.merge.theta_fire,
        vote_frac: sc.merge.vote_frac,
        q_floor: sc.merge.q_floor,
    };
    let top = cfg.top.unwrap_or(sc.merge.top);

    let mut rows: Vec<ScreenRow> = Vec::new();
    for (i, e) in universe.iter().enumerate() {
        if !is_fresh(&primaries[i], t) {
            continue; // 停牌/无当期 K → 不参与
        }
        let mut reasons: Vec<ScreenReason> = Vec::new();
        // 优质树
        let mut q_scores: Vec<f64> = Vec::new();
        for (name, tree) in &quality {
            if let Some((s, leaf)) = score_and_leaf(&primaries[i], &contexts[i], &aux, tree, llm, t, cfg.window).await? {
                q_scores.push(s);
                reasons.push(ScreenReason { tree: name.clone(), leaf, score: s });
            }
        }
        // 形态树
        let mut setup_scores: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        let mut fired_reasons: Vec<ScreenReason> = Vec::new();
        for (tag, trees) in &setups {
            let mut v = Vec::new();
            for (name, tree) in trees {
                if let Some((s, leaf)) = score_and_leaf(&primaries[i], &contexts[i], &aux, tree, llm, t, cfg.window).await? {
                    v.push(s);
                    if s >= mp.theta_fire {
                        fired_reasons.push(ScreenReason { tree: name.clone(), leaf, score: s });
                    }
                }
            }
            setup_scores.insert(tag.clone(), v);
        }
        reasons.extend(fired_reasons);

        let out: CombineOutput = combine(&q_scores, &setup_scores, &mp);
        rows.push(ScreenRow {
            symbol: e.symbol.clone(),
            rank: 0,
            quality_score: out.quality_score,
            speculative_score: out.speculative_score,
            combined_score: out.combined_score,
            tags: out.tags,
            selected: false,
            reasons,
        });
    }

    // 排名：combined_score 降序、并列 symbol 升序
    rows.sort_by(|a, b| {
        b.combined_score
            .partial_cmp(&a.combined_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.symbol.cmp(&b.symbol))
    });
    for (idx, r) in rows.iter_mut().enumerate() {
        r.rank = idx + 1;
    }
    // selected：用 select_top 在合格股（combined>0）里取 top-N，标记
    let scores: Vec<(String, f64)> = rows.iter().map(|r| (r.symbol.clone(), r.combined_score)).collect();
    let chosen: std::collections::BTreeSet<String> =
        select_top(&scores, top).into_iter().map(|(s, _)| s).collect();
    for r in rows.iter_mut() {
        r.selected = chosen.contains(&r.symbol);
    }

    let result = ScreenResult {
        as_of: t,
        n_universe: universe.len(),
        top,
        rows,
    };
    if let Some(p) = &cfg.out_path {
        let json = serde_json::to_string_pretty(&result)?;
        std::fs::write(p, json)?;
    }
    Ok(result)
}

/// 打印选股摘要（选出的标的 + 标签 + 分数）。
pub fn print_screen(r: &ScreenResult) {
    println!("=== rquant SCREEN @ {} （universe {}，top {}）===", r.as_of, r.n_universe, r.top);
    println!("{:<10} {:>4} {:>7} {:>7} {:>7}  标签", "标的", "排名", "优质", "投机", "综合");
    for row in r.rows.iter().filter(|x| x.selected) {
        println!(
            "{:<10} {:>4} {:>7.3} {:>7.3} {:>7.3}  {}",
            row.symbol, row.rank, row.quality_score, row.speculative_score, row.combined_score,
            row.tags.join("/")
        );
    }
    let n_sel = r.rows.iter().filter(|x| x.selected).count();
    println!("入选 {n_sel} 只（共 {} 只评估）", r.rows.len());
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use std::io::Write;

    fn daily(y: i32, m: u32, d: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(0, 0, 0).unwrap()
    }

    // 简单树（小窗口，避免 ema200）：优质 = close>sma(close,3)；动量 = close>ref(close,2)。
    const QUALITY_SIMPLE: &str = r#"
meta: { name: q_simple, forward_window: 1, stances: [long, flat] }
root: g
nodes:
  g:
    type: quant
    branches:
      - when: "close > sma(close, 3)"
        goto: leaf_long
        label: up
    default: { goto: leaf_flat, label: flat }
leaves:
  leaf_long: { stance: long, weight: 1.0 }
  leaf_flat: { stance: flat }
"#;
    const MOM_SIMPLE: &str = r#"
meta: { name: m_simple, forward_window: 1, stances: [long, flat] }
root: g
nodes:
  g:
    type: quant
    branches:
      - when: "close > ref(close, 2)"
        goto: leaf_long
        label: up
    default: { goto: leaf_flat, label: flat }
leaves:
  leaf_long: { stance: long, weight: 1.0 }
  leaf_flat: { stance: flat }
"#;

    fn write_tmp(suffix: &str, content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::Builder::new().suffix(suffix).tempfile().unwrap();
        write!(f, "{content}").unwrap();
        f.flush().unwrap();
        f
    }

    fn write_bars(rising: bool) -> tempfile::NamedTempFile {
        let mut f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        writeln!(f, "time,open,high,low,close,volume").unwrap();
        let mut price = 100.0;
        for d in 1..=10u32 {
            writeln!(f, "{},{p},{p},{p},{p},1000", daily(2024, 1, d).format("%Y-%m-%d %H:%M:%S"), p = price).unwrap();
            price *= if rising { 1.02 } else { 0.98 };
        }
        f.flush().unwrap();
        f
    }

    #[tokio::test]
    async fn screen_selects_rising_symbol_with_tag() {
        let q = write_tmp(".yaml", QUALITY_SIMPLE);
        let m = write_tmp(".yaml", MOM_SIMPLE);
        let cfg_yaml = format!(
            "quality_trees: [{}]\nsetup_trees:\n  动量延续: [{}]\nmerge: {{ q_floor: 0.5, top: 1 }}\n",
            q.path().to_str().unwrap().replace('\\', "/"),
            m.path().to_str().unwrap().replace('\\', "/"),
        );
        let cfg_f = write_tmp(".yaml", &cfg_yaml);

        let f_up = write_bars(true);
        let f_dn = write_bars(false);
        let mut univ = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        writeln!(univ, "symbol,primary\nUP,{}\nDN,{}",
            f_up.path().to_str().unwrap(), f_dn.path().to_str().unwrap()).unwrap();
        univ.flush().unwrap();

        let run = ScreenRunConfig {
            config_path: cfg_f.path().to_path_buf(),
            universe_path: univ.path().to_path_buf(),
            as_of: None,
            top: None,
            window: 10,
            out_path: None,
        };
        let res = run_screen(&run, &LlmEvaluator::Disabled).await.unwrap();
        assert_eq!(res.n_universe, 2);
        let up = res.rows.iter().find(|r| r.symbol == "UP").unwrap();
        let dn = res.rows.iter().find(|r| r.symbol == "DN").unwrap();
        assert!(up.selected, "rising symbol should be selected");
        assert!(up.tags.contains(&"动量延续".to_string()));
        assert!(!dn.selected, "falling symbol should not be selected");
        assert_eq!(up.rank, 1);
    }
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test --lib screen::tests::screen_selects_rising_symbol_with_tag`
Expected: PASS.

- [ ] **Step 3: Run the full screen module tests + clippy**

Run: `cargo test --lib screen::` then `cargo clippy --lib`
Expected: all PASS, no clippy warnings on the screen module.

- [ ] **Step 4: Commit**

```bash
git add src/screen/mod.rs
git commit -m "feat(screen): as-of orchestrator (run_screen) + result types + print + JSON"
```

---

## Task SCR-5: Screen backtest core loop (nav / benchmark / select via combine)

**Files:**
- Create: `src/screen/backtest.rs`
- Modify: `src/screen/mod.rs` (add `pub mod backtest;`)

- [ ] **Step 1: Add module declaration**

In `src/screen/mod.rs` add (with the other `pub mod` lines):

```rust
pub mod backtest;
```

- [ ] **Step 2: Write the backtest core + a synthetic integration test**

Create `src/screen/backtest.rs`:

```rust
//! 选股器历史回测：镜像 portfolio 主循环，把单树打分换成多树合并选股。
//! 复用 portfolio 的 timeline/last_close_at/select_top/accrue/turnover_between + risk_metrics。

use crate::data::aux_table::AuxTable;
use crate::data::bar::Bar;
use crate::eval::llm::LlmEvaluator;
use crate::tree::loader::Tree;
use crate::Result;
use chrono::{NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::backtest::portfolio::{
    accrue, build_timeline, last_close_at, score_symbol, select_top, turnover_between,
};
use crate::screen::combine::{combine, MergeParams};
use crate::screen::config::load_screen_config;

/// 调仓快照。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenHolding {
    pub t: NaiveDateTime,
    pub nav: f64,
    pub benchmark_nav: f64,
    /// (symbol, combined_score)
    pub selected: Vec<(String, f64)>,
}

/// 回测报告（核心字段；归因/regime/质量分层在后续任务补）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenBacktestReport {
    pub n_rebalances: usize,
    pub top: usize,
    pub rebalance: usize,
    pub total_return: f64,
    pub benchmark_return: f64,
    pub excess_return: f64,
    pub max_drawdown: f64,
    pub turnover: f64,
    pub avg_members: f64,
    pub holdings: Vec<ScreenHolding>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub risk: Option<crate::report::risk::RiskMetrics>,
    // 后续任务追加（带 #[serde(default)] 以保兼容）：
    #[serde(default)]
    pub tag_attribution: Vec<TagAttribution>,
    #[serde(default)]
    pub regime_slices: Vec<RegimeSlice>,
    #[serde(default)]
    pub quality_layers: Vec<QualityLayer>,
}

/// 标签归因（SCR-6 填充）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TagAttribution {
    pub tag: String,
    pub n_picks: usize,
    pub hit_rate: f64,
    pub mean_fwd_return: f64,
}

/// regime 切片（SCR-7 填充）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RegimeSlice {
    pub label: String,
    pub from: String,
    pub to: String,
    pub picks_return: f64,
    pub benchmark_return: f64,
    pub excess: f64,
}

/// 优质分分层（SCR-8 填充）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QualityLayer {
    pub layer: usize,
    pub n: usize,
    pub mean_quality: f64,
    pub mean_fwd_return: f64,
}

/// 回测运行配置。
pub struct ScreenBacktestConfig {
    pub config_path: PathBuf,
    pub universe_path: PathBuf,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub rebalance: usize,
    pub top: Option<usize>,
    pub warmup: usize,
    pub window: usize,
    pub cost_bps: f64,
    pub soft: bool,
    pub out_path: Option<PathBuf>,
}

/// 单标的、单调仓点：跑所有树 → 返回 (combined_score, tags, quality_score, per-setup-strength)。
/// 内部 helper，供主循环与归因复用。
struct SymbolEval {
    combined: f64,
    quality: f64,
    tags: Vec<String>,
}

#[allow(clippy::too_many_arguments)]
async fn eval_symbol(
    primary: &[Bar],
    context: &[Bar],
    aux: &BTreeMap<String, AuxTable>,
    quality: &[Tree],
    setups: &BTreeMap<String, Vec<Tree>>,
    llm: &LlmEvaluator,
    soft: bool,
    t: NaiveDateTime,
    window: usize,
    mp: &MergeParams,
) -> Result<Option<SymbolEval>> {
    // 不新鲜 → None（score_symbol 已含 is_fresh 检查，但要统一短路）
    let mut q_scores: Vec<f64> = Vec::new();
    let mut any = false;
    for tree in quality {
        if let Some(s) = score_symbol(primary, context, aux, tree, llm, soft, t, window).await? {
            q_scores.push(s);
            any = true;
        }
    }
    if !any {
        return Ok(None); // 停牌
    }
    let mut setup_scores: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for (tag, trees) in setups {
        let mut v = Vec::new();
        for tree in trees {
            if let Some(s) = score_symbol(primary, context, aux, tree, llm, soft, t, window).await? {
                v.push(s);
            }
        }
        setup_scores.insert(tag.clone(), v);
    }
    let out = combine(&q_scores, &setup_scores, mp);
    Ok(Some(SymbolEval { combined: out.combined_score, quality: out.quality_score, tags: out.tags }))
}

fn load_trees(paths: &[PathBuf]) -> Result<Vec<Tree>> {
    paths.iter().map(|p| crate::tree::loader::load_tree_file(p)).collect()
}

/// 端到端选股回测。
pub async fn run_screen_backtest(
    cfg: &ScreenBacktestConfig,
    llm: &LlmEvaluator,
) -> Result<ScreenBacktestReport> {
    if cfg.rebalance == 0 {
        return Err(crate::Error::Data("rebalance must be >= 1".into()));
    }
    let sc = load_screen_config(&cfg.config_path)?;
    let quality = load_trees(&sc.quality_trees)?;
    let mut setups: BTreeMap<String, Vec<Tree>> = BTreeMap::new();
    for (tag, paths) in &sc.setup_trees {
        setups.insert(tag.clone(), load_trees(paths)?);
    }
    let mp = MergeParams {
        theta_fire: sc.merge.theta_fire,
        vote_frac: sc.merge.vote_frac,
        q_floor: sc.merge.q_floor,
    };
    let top = cfg.top.unwrap_or(sc.merge.top);

    let universe = crate::data::universe::read_universe_csv(&cfg.universe_path)?;
    let mut primaries: Vec<Vec<Bar>> = Vec::with_capacity(universe.len());
    let mut contexts: Vec<Vec<Bar>> = Vec::with_capacity(universe.len());
    for e in &universe {
        primaries.push(crate::data::reader::read_bars_csv(&e.primary)?);
        contexts.push(crate::data::reader::read_bars_csv(&e.context)?);
    }
    let aux: BTreeMap<String, AuxTable> = BTreeMap::new();

    // 时间线（按 from/to 过滤）
    let full = build_timeline(&primaries);
    let timeline: Vec<NaiveDateTime> = full
        .into_iter()
        .filter(|t| cfg.from.is_none_or(|f| t.date() >= f) && cfg.to.is_none_or(|to| t.date() <= to))
        .collect();
    let n = timeline.len();
    let rb_indices: Vec<usize> = (cfg.warmup..n).step_by(cfg.rebalance).collect();
    if rb_indices.len() < 2 {
        return Err(crate::Error::Data("timeline too short for warmup/rebalance".into()));
    }
    let mut segments: Vec<(usize, usize)> = Vec::new();
    for w in rb_indices.windows(2) {
        segments.push((w[0], w[1]));
    }
    let last_rb = *rb_indices.last().unwrap();
    if last_rb != n - 1 {
        segments.push((last_rb, n - 1));
    }

    let rate = cfg.cost_bps / 2.0 / 10_000.0;
    let mut nav = 1.0_f64;
    let mut bnav = 1.0_f64;
    let mut peak = 1.0_f64;
    let mut max_dd = 0.0_f64;
    let mut total_turnover = 0.0_f64;
    let mut total_members = 0usize;
    let mut holdings: Vec<ScreenHolding> = Vec::new();
    let mut w_old: BTreeMap<String, f64> = BTreeMap::new();

    for (rb_idx, end_idx) in &segments {
        let t_rb = timeline[*rb_idx];
        let t_end = timeline[*end_idx];

        // 逐标的多树合并打分
        let mut scores: Vec<(String, f64)> = Vec::new();
        for (i, e) in universe.iter().enumerate() {
            if let Some(ev) = eval_symbol(
                &primaries[i], &contexts[i], &aux, &quality, &setups, llm, cfg.soft, t_rb, cfg.window, &mp,
            ).await? {
                scores.push((e.symbol.clone(), ev.combined));
            }
        }
        let selected = select_top(&scores, top);
        total_members += selected.len();
        let w_new: BTreeMap<String, f64> = if !selected.is_empty() {
            let eq = 1.0 / selected.len() as f64;
            selected.iter().map(|(s, _)| (s.clone(), eq)).collect()
        } else {
            BTreeMap::new()
        };

        let tv = turnover_between(&w_old, &w_new);
        nav *= 1.0 - rate * tv;
        total_turnover += tv;

        holdings.push(ScreenHolding { t: t_rb, nav, benchmark_nav: bnav, selected: selected.clone() });
        peak = peak.max(nav);
        max_dd = max_dd.max(1.0 - nav / peak);

        // 价格映射
        let px_start: BTreeMap<String, f64> = universe.iter().enumerate()
            .filter_map(|(i, e)| last_close_at(&primaries[i], t_rb).map(|p| (e.symbol.clone(), p)))
            .collect();
        let px_end: BTreeMap<String, f64> = universe.iter().enumerate()
            .filter_map(|(i, e)| last_close_at(&primaries[i], t_end).map(|p| (e.symbol.clone(), p)))
            .collect();

        let r = accrue(&w_new, &px_start, &px_end);
        nav *= 1.0 + r;

        // 基准：所有有价标的等权
        let bw: BTreeMap<String, f64> = {
            let syms: Vec<String> = px_start.keys().cloned().collect();
            let neq = syms.len();
            if neq > 0 { let eq = 1.0 / neq as f64; syms.into_iter().map(|s| (s, eq)).collect() } else { BTreeMap::new() }
        };
        let br = accrue(&bw, &px_start, &px_end);
        bnav *= 1.0 + br;

        peak = peak.max(nav);
        max_dd = max_dd.max(1.0 - nav / peak);
        w_old = w_new;
    }

    let n_rebalances = holdings.len();
    let total_return = nav - 1.0;
    let benchmark_return = bnav - 1.0;
    let nav_series: Vec<(NaiveDateTime, f64)> = holdings.iter().map(|h| (h.t, h.nav)).collect();
    let risk = crate::report::risk::risk_metrics(&nav_series, max_dd);

    let report = ScreenBacktestReport {
        n_rebalances,
        top,
        rebalance: cfg.rebalance,
        total_return,
        benchmark_return,
        excess_return: total_return - benchmark_return,
        max_drawdown: max_dd,
        turnover: total_turnover,
        avg_members: if n_rebalances > 0 { total_members as f64 / n_rebalances as f64 } else { 0.0 },
        holdings,
        risk,
        tag_attribution: Vec::new(),
        regime_slices: Vec::new(),
        quality_layers: Vec::new(),
    };

    if let Some(p) = &cfg.out_path {
        let json = serde_json::to_string_pretty(&report)?;
        std::fs::write(p, json)?;
    }
    Ok(report)
}

/// 打印回测摘要。
pub fn print_screen_backtest(r: &ScreenBacktestReport) {
    println!("=== rquant SCREEN BACKTEST （top {}，rebalance {}）===", r.top, r.rebalance);
    println!("调仓次数    : {}", r.n_rebalances);
    println!("总收益率    : {:.4}", r.total_return);
    println!("基准收益率  : {:.4}", r.benchmark_return);
    println!("超额收益    : {:.4}", r.excess_return);
    println!("最大回撤    : {:.4}", r.max_drawdown);
    println!("换手率      : {:.4}", r.turnover);
    println!("平均成员数  : {:.2}", r.avg_members);
    if let Some(rk) = &r.risk {
        let f = |v: Option<f64>| v.map_or("—".to_string(), |x| format!("{x:.2}"));
        println!("Sharpe      : {}", f(rk.sharpe));
        println!("Calmar      : {}", f(rk.calmar));
    }
    for ta in &r.tag_attribution {
        println!("标签 {:<10} picks={:<4} 胜率={:.2} 均前瞻收益={:+.4}", ta.tag, ta.n_picks, ta.hit_rate, ta.mean_fwd_return);
    }
    for rs in &r.regime_slices {
        println!("regime {:<10} [{}~{}] 组合={:+.4} 基准={:+.4} 超额={:+.4}", rs.label, rs.from, rs.to, rs.picks_return, rs.benchmark_return, rs.excess);
    }
    for ql in &r.quality_layers {
        println!("优质层 Q{} n={:<4} 均优质={:.3} 均前瞻收益={:+.4}", ql.layer, ql.n, ql.mean_quality, ql.mean_fwd_return);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use std::io::Write;

    fn daily(d: u32) -> NaiveDateTime {
        // 20 个连续日（2024-01 + 2024-02 跨足够长）
        let base = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        (base + chrono::Duration::days(d as i64)).and_hms_opt(0, 0, 0).unwrap()
    }

    const Q_SIMPLE: &str = r#"
meta: { name: q, forward_window: 1, stances: [long, flat] }
root: g
nodes:
  g: { type: quant, branches: [ { when: "close > sma(close, 3)", goto: l, label: up } ], default: { goto: f, label: flat } }
leaves: { l: { stance: long, weight: 1.0 }, f: { stance: flat } }
"#;
    const M_SIMPLE: &str = r#"
meta: { name: m, forward_window: 1, stances: [long, flat] }
root: g
nodes:
  g: { type: quant, branches: [ { when: "close > ref(close, 2)", goto: l, label: up } ], default: { goto: f, label: flat } }
leaves: { l: { stance: long, weight: 1.0 }, f: { stance: flat } }
"#;

    fn wf(suffix: &str, content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::Builder::new().suffix(suffix).tempfile().unwrap();
        write!(f, "{content}").unwrap();
        f.flush().unwrap();
        f
    }

    fn bars(pct: f64) -> tempfile::NamedTempFile {
        let mut f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        writeln!(f, "time,open,high,low,close,volume").unwrap();
        let mut price = 100.0;
        for d in 0..30u32 {
            writeln!(f, "{},{p},{p},{p},{p},1000", daily(d).format("%Y-%m-%d %H:%M:%S"), p = price).unwrap();
            price *= 1.0 + pct;
        }
        f.flush().unwrap();
        f
    }

    #[tokio::test]
    async fn backtest_picks_beat_benchmark() {
        let q = wf(".yaml", Q_SIMPLE);
        let m = wf(".yaml", M_SIMPLE);
        let cfg_yaml = format!(
            "quality_trees: [{}]\nsetup_trees:\n  动量延续: [{}]\nmerge: {{ q_floor: 0.5, top: 1 }}\n",
            q.path().to_str().unwrap().replace('\\', "/"),
            m.path().to_str().unwrap().replace('\\', "/"),
        );
        let cfg_f = wf(".yaml", &cfg_yaml);
        let up = bars(0.01);
        let dn = bars(-0.01);
        let mut univ = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        writeln!(univ, "symbol,primary\nUP,{}\nDN,{}", up.path().to_str().unwrap(), dn.path().to_str().unwrap()).unwrap();
        univ.flush().unwrap();

        let cfg = ScreenBacktestConfig {
            config_path: cfg_f.path().to_path_buf(),
            universe_path: univ.path().to_path_buf(),
            from: None, to: None,
            rebalance: 4, top: None, warmup: 5, window: 10, cost_bps: 10.0, soft: false,
            out_path: None,
        };
        let r = run_screen_backtest(&cfg, &LlmEvaluator::Disabled).await.unwrap();
        assert!(r.n_rebalances >= 2);
        // UP 每期入选 → 组合跑赢等权基准（含 DN 拖累）
        assert!(r.total_return > r.benchmark_return, "picks {} should beat benchmark {}", r.total_return, r.benchmark_return);
        for h in &r.holdings {
            if !h.selected.is_empty() {
                assert_eq!(h.selected[0].0, "UP");
            }
        }
    }
}
```

**Note on `is_none_or`:** if the Rust toolchain rejects `Option::is_none_or` (stabilized 1.82), replace with `cfg.from.map_or(true, |f| t.date() >= f)`.

- [ ] **Step 3: Run the test**

Run: `cargo test --lib screen::backtest`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/screen/mod.rs src/screen/backtest.rs
git commit -m "feat(screen): backtest core loop — multi-tree select, nav vs benchmark, risk"
```

---

## Task SCR-6: Tag attribution (per-setup forward-return)

**Files:**
- Modify: `src/screen/backtest.rs` (populate `tag_attribution`; extend `eval_symbol` to also return tags; collect per-pick segment returns)

- [ ] **Step 1: Write a failing test**

Add to `src/screen/backtest.rs` `mod tests`:

```rust
    #[tokio::test]
    async fn backtest_tag_attribution_populated() {
        let q = wf(".yaml", Q_SIMPLE);
        let m = wf(".yaml", M_SIMPLE);
        let cfg_yaml = format!(
            "quality_trees: [{}]\nsetup_trees:\n  动量延续: [{}]\nmerge: {{ q_floor: 0.5, top: 1 }}\n",
            q.path().to_str().unwrap().replace('\\', "/"),
            m.path().to_str().unwrap().replace('\\', "/"),
        );
        let cfg_f = wf(".yaml", &cfg_yaml);
        let up = bars(0.01);
        let dn = bars(-0.01);
        let mut univ = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        writeln!(univ, "symbol,primary\nUP,{}\nDN,{}", up.path().to_str().unwrap(), dn.path().to_str().unwrap()).unwrap();
        univ.flush().unwrap();
        let cfg = ScreenBacktestConfig {
            config_path: cfg_f.path().to_path_buf(), universe_path: univ.path().to_path_buf(),
            from: None, to: None, rebalance: 4, top: None, warmup: 5, window: 10, cost_bps: 10.0, soft: false, out_path: None,
        };
        let r = run_screen_backtest(&cfg, &LlmEvaluator::Disabled).await.unwrap();
        let mom = r.tag_attribution.iter().find(|a| a.tag == "动量延续").expect("动量延续 attribution present");
        assert!(mom.n_picks >= 2, "should have picks tagged 动量延续");
        assert!(mom.mean_fwd_return > 0.0, "rising picks → positive forward return");
        assert!(mom.hit_rate > 0.5);
    }
```

- [ ] **Step 2: Implement attribution**

In `eval_symbol`, the returned `SymbolEval` already has `tags`. In the main loop, after `let selected = select_top(...)`, compute each selected symbol's segment forward return and record per-tag. Add BEFORE the loop:

```rust
    // 标签归因累加器：tag -> (picks 段收益列表)
    let mut tag_rets: BTreeMap<String, Vec<f64>> = BTreeMap::new();
```

We need each selected symbol's tags. Re-evaluate is wasteful; instead capture tags during scoring. Replace the scoring block in the loop with one that keeps a `BTreeMap<symbol, SymbolEval>`:

```rust
        let mut evals: BTreeMap<String, SymbolEval> = BTreeMap::new();
        let mut scores: Vec<(String, f64)> = Vec::new();
        for (i, e) in universe.iter().enumerate() {
            if let Some(ev) = eval_symbol(
                &primaries[i], &contexts[i], &aux, &quality, &setups, llm, cfg.soft, t_rb, cfg.window, &mp,
            ).await? {
                scores.push((e.symbol.clone(), ev.combined));
                evals.insert(e.symbol.clone(), ev);
            }
        }
        let selected = select_top(&scores, top);
```

Then AFTER `px_start`/`px_end` are built, accumulate per-tag returns of the SELECTED picks:

```rust
        for (sym, _) in &selected {
            let seg_ret = match (px_start.get(sym), px_end.get(sym)) {
                (Some(a), Some(b)) if *a > 0.0 => b / a - 1.0,
                _ => 0.0,
            };
            if let Some(ev) = evals.get(sym) {
                for tag in &ev.tags {
                    tag_rets.entry(tag.clone()).or_default().push(seg_ret);
                }
            }
        }
```

After the loop, build `tag_attribution`:

```rust
    let tag_attribution: Vec<TagAttribution> = tag_rets.iter().map(|(tag, rets)| {
        let n = rets.len();
        let hit = rets.iter().filter(|r| **r > 0.0).count();
        let mean = if n > 0 { rets.iter().sum::<f64>() / n as f64 } else { 0.0 };
        TagAttribution {
            tag: tag.clone(),
            n_picks: n,
            hit_rate: if n > 0 { hit as f64 / n as f64 } else { 0.0 },
            mean_fwd_return: mean,
        }
    }).collect();
```

Set `tag_attribution` in the report (replace `tag_attribution: Vec::new()`).

- [ ] **Step 3: Run the test**

Run: `cargo test --lib screen::backtest`
Expected: all backtest tests PASS (including the new attribution test).

- [ ] **Step 4: Commit**

```bash
git add src/screen/backtest.rs
git commit -m "feat(screen): backtest per-tag forward-return attribution"
```

---

## Task SCR-7: Regime slices (cross-bull/bear sub-metrics)

**Files:**
- Modify: `src/screen/backtest.rs` (populate `regime_slices` from `ScreenConfig.regimes`)

- [ ] **Step 1: Write a failing test**

Add to `mod tests`:

```rust
    #[tokio::test]
    async fn backtest_regime_slices_populated() {
        let q = wf(".yaml", Q_SIMPLE);
        let m = wf(".yaml", M_SIMPLE);
        // regime covers the synthetic 30-day window (2024-01-01 .. 2024-01-30)
        let cfg_yaml = format!(
            "quality_trees: [{}]\nsetup_trees:\n  动量延续: [{}]\nmerge: {{ q_floor: 0.5, top: 1 }}\nregimes:\n  - {{ label: full, from: 2024-01-01, to: 2024-02-01 }}\n",
            q.path().to_str().unwrap().replace('\\', "/"),
            m.path().to_str().unwrap().replace('\\', "/"),
        );
        let cfg_f = wf(".yaml", &cfg_yaml);
        let up = bars(0.01);
        let dn = bars(-0.01);
        let mut univ = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        writeln!(univ, "symbol,primary\nUP,{}\nDN,{}", up.path().to_str().unwrap(), dn.path().to_str().unwrap()).unwrap();
        univ.flush().unwrap();
        let cfg = ScreenBacktestConfig {
            config_path: cfg_f.path().to_path_buf(), universe_path: univ.path().to_path_buf(),
            from: None, to: None, rebalance: 4, top: None, warmup: 5, window: 10, cost_bps: 10.0, soft: false, out_path: None,
        };
        let r = run_screen_backtest(&cfg, &LlmEvaluator::Disabled).await.unwrap();
        let slice = r.regime_slices.iter().find(|s| s.label == "full").expect("regime slice present");
        assert!((slice.excess - (slice.picks_return - slice.benchmark_return)).abs() < 1e-9);
        assert!(slice.picks_return > slice.benchmark_return);
    }
```

- [ ] **Step 2: Implement regime slicing**

`run_screen_backtest` needs the config's regimes; capture them: after `let sc = load_screen_config(...)?;` add `let regimes = sc.regimes.clone();`.

After computing `holdings`, compute slices. A regime slice's return is the nav ratio across holdings whose `t.date()` falls in `[from,to]`:

```rust
    let regime_slices: Vec<RegimeSlice> = regimes.iter().filter_map(|rw| {
        let inside: Vec<&ScreenHolding> = holdings.iter()
            .filter(|h| h.t.date() >= rw.from && h.t.date() <= rw.to)
            .collect();
        if inside.len() < 2 {
            return None; // 不足以算区间收益
        }
        let p0 = inside.first().unwrap();
        let p1 = inside.last().unwrap();
        let picks = if p0.nav > 0.0 { p1.nav / p0.nav - 1.0 } else { 0.0 };
        let bench = if p0.benchmark_nav > 0.0 { p1.benchmark_nav / p0.benchmark_nav - 1.0 } else { 0.0 };
        Some(RegimeSlice {
            label: rw.label.clone(),
            from: rw.from.to_string(),
            to: rw.to.to_string(),
            picks_return: picks,
            benchmark_return: bench,
            excess: picks - bench,
        })
    }).collect();
```

Set `regime_slices` in the report.

- [ ] **Step 3: Run the test**

Run: `cargo test --lib screen::backtest`
Expected: all PASS.

- [ ] **Step 4: Commit**

```bash
git add src/screen/backtest.rs
git commit -m "feat(screen): backtest regime slices (cross bull/bear sub-metrics)"
```

---

## Task SCR-8: Quality layering (forward-return by quality quantile)

**Files:**
- Modify: `src/screen/backtest.rs` (populate `quality_layers`)

- [ ] **Step 1: Write a failing test**

Add to `mod tests`:

```rust
    #[tokio::test]
    async fn backtest_quality_layers_populated() {
        let q = wf(".yaml", Q_SIMPLE);
        let m = wf(".yaml", M_SIMPLE);
        let cfg_yaml = format!(
            "quality_trees: [{}]\nsetup_trees:\n  动量延续: [{}]\nmerge: {{ q_floor: 0.0, top: 2, quality_layers: 2 }}\n",
            q.path().to_str().unwrap().replace('\\', "/"),
            m.path().to_str().unwrap().replace('\\', "/"),
        );
        let cfg_f = wf(".yaml", &cfg_yaml);
        let up = bars(0.01);
        let dn = bars(-0.01);
        let mut univ = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        writeln!(univ, "symbol,primary\nUP,{}\nDN,{}", up.path().to_str().unwrap(), dn.path().to_str().unwrap()).unwrap();
        univ.flush().unwrap();
        let cfg = ScreenBacktestConfig {
            config_path: cfg_f.path().to_path_buf(), universe_path: univ.path().to_path_buf(),
            from: None, to: None, rebalance: 4, top: None, warmup: 5, window: 10, cost_bps: 10.0, soft: false, out_path: None,
        };
        let r = run_screen_backtest(&cfg, &LlmEvaluator::Disabled).await.unwrap();
        assert!(!r.quality_layers.is_empty(), "quality layers should be computed");
        let total_n: usize = r.quality_layers.iter().map(|l| l.n).sum();
        assert!(total_n > 0);
    }
```

- [ ] **Step 2: Implement quality layering**

Need quality_layers count from config: after loading `sc`, capture `let n_layers = sc.merge.quality_layers.max(1);`.

Accumulate (quality, seg_return) for all ELIGIBLE symbols (tagged AND quality>=q_floor — i.e. combined>0) at each rebalance. Add accumulator before the loop:

```rust
    let mut layer_pairs: Vec<(f64, f64)> = Vec::new(); // (quality_score, segment_fwd_return)
```

Inside the loop, after `px_start`/`px_end` are built, collect eligible symbols' pairs (eligible = combined>0):

```rust
        for (sym, ev) in &evals {
            if ev.combined > 0.0 {
                let seg_ret = match (px_start.get(sym), px_end.get(sym)) {
                    (Some(a), Some(b)) if *a > 0.0 => b / a - 1.0,
                    _ => 0.0,
                };
                layer_pairs.push((ev.quality, seg_ret));
            }
        }
```

After the loop, split into `n_layers` quantile buckets by quality (ascending), so layer 1 = lowest quality, layer `n_layers` = highest:

```rust
    let quality_layers: Vec<QualityLayer> = {
        let mut pairs = layer_pairs.clone();
        pairs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let total = pairs.len();
        if total == 0 {
            Vec::new()
        } else {
            (0..n_layers).filter_map(|q| {
                let lo = q * total / n_layers;
                let hi = (q + 1) * total / n_layers;
                if hi <= lo { return None; }
                let slice = &pairs[lo..hi];
                let n = slice.len();
                let mean_q = slice.iter().map(|p| p.0).sum::<f64>() / n as f64;
                let mean_r = slice.iter().map(|p| p.1).sum::<f64>() / n as f64;
                Some(QualityLayer { layer: q + 1, n, mean_quality: mean_q, mean_fwd_return: mean_r })
            }).collect()
        }
    };
```

Set `quality_layers` in the report.

- [ ] **Step 3: Run the test**

Run: `cargo test --lib screen::backtest`
Expected: all PASS.

- [ ] **Step 4: Commit**

```bash
git add src/screen/backtest.rs
git commit -m "feat(screen): backtest quality layering (forward-return by quality quantile)"
```

---

## Task SCR-9: CLI `rquant screen` subcommand (as-of + backtest)

**Files:**
- Modify: `src/cli/mod.rs` (add `Cmd::Screen` variant + dispatch arm + a small date-parse helper)

- [ ] **Step 1: Add the Cmd variant**

In `src/cli/mod.rs`, add to the `enum Cmd` (after `Optimize { ... }` or near `Factor`):

```rust
    /// 日线选股器：多树集成 → 优质+投机形态标注（as-of），或历史回测验证（--backtest）。
    Screen {
        #[arg(long)]
        universe: PathBuf,
        #[arg(long, default_value = "examples/screen_v1.yaml")]
        config: PathBuf,
        /// 历史回测模式（回放集成、出净值/归因/regime/质量分层）
        #[arg(long, default_value_t = false)]
        backtest: bool,
        /// as-of 日期（选股模式；默认最新 K）YYYY-MM-DD
        #[arg(long)]
        as_of: Option<String>,
        /// 回测起始日 YYYY-MM-DD
        #[arg(long)]
        from: Option<String>,
        /// 回测结束日 YYYY-MM-DD
        #[arg(long)]
        to: Option<String>,
        #[arg(long)]
        top: Option<usize>,
        #[arg(long, default_value_t = 5)]
        rebalance: usize,
        #[arg(long, default_value_t = 260)]
        warmup: usize,
        #[arg(long, default_value_t = 260)]
        window: usize,
        #[arg(long, default_value_t = 10.0)]
        cost_bps: f64,
        #[arg(long, default_value_t = false)]
        soft: bool,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long, default_value = "")]
        llm_model: String,
        #[arg(long, default_value = "")]
        llm_base_url: String,
        #[arg(long, default_value = ".rquant-cache/llm")]
        llm_cache_dir: PathBuf,
    },
```

- [ ] **Step 2: Add the dispatch arm**

In the `match cmd { ... }` block (alongside the other arms), add:

```rust
        Cmd::Screen {
            universe, config, backtest, as_of, from, to, top, rebalance,
            warmup, window, cost_bps, soft, out, llm_model, llm_base_url, llm_cache_dir,
        } => {
            let llm = build_llm(llm_model, llm_base_url, llm_cache_dir)?;
            let parse_date = |o: Option<String>| -> Result<Option<chrono::NaiveDate>> {
                match o {
                    None => Ok(None),
                    Some(s) => chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d")
                        .map(Some)
                        .map_err(|e| crate::Error::Data(format!("bad date '{s}': {e}"))),
                }
            };
            if backtest {
                let bcfg = crate::screen::backtest::ScreenBacktestConfig {
                    config_path: config,
                    universe_path: universe,
                    from: parse_date(from)?,
                    to: parse_date(to)?,
                    rebalance,
                    top,
                    warmup,
                    window,
                    cost_bps,
                    soft,
                    out_path: out,
                };
                let report = crate::screen::backtest::run_screen_backtest(&bcfg, &llm).await?;
                crate::screen::backtest::print_screen_backtest(&report);
            } else {
                let rcfg = crate::screen::ScreenRunConfig {
                    config_path: config,
                    universe_path: universe,
                    as_of: parse_date(as_of)?,
                    top,
                    window,
                    out_path: out,
                };
                let result = crate::screen::run_screen(&rcfg, &llm).await?;
                crate::screen::print_screen(&result);
            }
        }
```

**Check:** confirm `build_llm` is in scope in `src/cli/mod.rs` (it is — used by Portfolio/Signal/Optimize arms). Confirm the dispatch fn is `async` (it is — other arms `.await`). Confirm `Result`/`crate::Error` are imported at the top of `src/cli/mod.rs`; if `Error` isn't imported, use the fully-qualified `crate::Error`.

- [ ] **Step 3: Build + verify CLI parses**

Run: `cargo build`
Then: `cargo run -- screen --help`
Expected: builds; help shows the `screen` subcommand with all flags.

- [ ] **Step 4: Commit**

```bash
git add src/cli/mod.rs
git commit -m "feat(screen): rquant screen CLI subcommand (as-of + --backtest)"
```

---

## Task SCR-10: Ensemble config example + docs + real-data smoke

**Files:**
- Create: `examples/screen_v1.yaml`
- Modify: `docs/cli-reference.md` (add a `rquant screen` section — match the file's existing format; if the file doesn't exist, grep `docs/` for the CLI doc and add there)

- [ ] **Step 1: Write the ensemble config**

`examples/screen_v1.yaml`:

```yaml
# 选股集成 v1 — 优质树 + 3 形态树（每形态单树种子）。
# 路径相对仓库根（cwd）。regime 窗口为已知 A 股牛熊（与 RV-5 切片一致，起始值）。
quality_trees:
  - examples/trees/screen/quality_v1.yaml
setup_trees:
  动量延续:
    - examples/trees/screen/momentum_v1.yaml
  突破临界:
    - examples/trees/screen/breakout_v1.yaml
  超跌反弹:
    - examples/trees/screen/pullback_v1.yaml
merge:
  theta_fire: 0.5
  vote_frac: 0.5
  q_floor: 0.5
  top: 10
  quality_layers: 3
regimes:
  - { label: "2018熊", from: 2018-01-02, to: 2018-12-28 }
  - { label: "2019-21牛", from: 2019-01-02, to: 2021-12-31 }
  - { label: "2022熊", from: 2022-01-04, to: 2022-10-31 }
  - { label: "2023-24", from: 2023-01-03, to: 2024-12-31 }
```

- [ ] **Step 2: Document the command**

Add to `docs/cli-reference.md` a `## rquant screen` section describing:
- Purpose: daily multi-tree-ensemble screener — quality score + speculative setup tags (dual output).
- As-of: `rquant screen --universe data/universe_20.csv --config examples/screen_v1.yaml [--as-of YYYY-MM-DD] [--top N] [--out screen.json]`
- Backtest: `rquant screen --backtest --universe data/universe_20.csv --config examples/screen_v1.yaml --from 2018-01-01 --to 2026-06-01 [--rebalance 5] [--out screen_bt.json]`
- Note window/warmup default 260 (quality tree uses ema200); paths in config are cwd-relative.
- Note: pure-quant trees (no LLM); Phase 1 validated on the deep 20 universe only.

Write the actual section text (no placeholder) matching the surrounding doc style.

- [ ] **Step 3: Real-data smoke (requires deep 20 data present)**

Run the as-of screen:
```bash
cargo run --release -- screen --universe data/universe_20.csv --config examples/screen_v1.yaml --out tmps/screen_asof.json
```
Expected: prints a table of selected symbols with tags; writes `tmps/screen_asof.json`. Sanity-check: selected symbols have non-empty tags and quality_score ≥ 0.5.

Run the backtest:
```bash
cargo run --release -- screen --backtest --universe data/universe_20.csv --config examples/screen_v1.yaml --from 2018-06-01 --to 2026-06-01 --rebalance 5 --out tmps/screen_bt.json
```
Expected: prints total/benchmark/excess + per-tag attribution + regime slices + quality layers; writes `tmps/screen_bt.json`.

**This is the honest validation gate, not just a smoke:** read the numbers. If excess_return is negative across regimes, tag attribution shows no tag with positive mean forward return, and quality layers are non-monotonic → the seed ensemble has NO edge. That is a valid Phase-1 finding — record it; do NOT tune to make it look good (that's the overfitting trap §5.3 guards against). If `data/universe_20.csv` is absent, regenerate via `data/fetch_deep.cmd` (documented) or skip with a note.

- [ ] **Step 4: Commit (config + docs only; tmps/ is gitignored)**

```bash
git add examples/screen_v1.yaml docs/cli-reference.md
git commit -m "feat(screen): ensemble config example + CLI docs + real-data smoke"
```

---

## Task SCR-11: Final gate + finishing the branch

**Files:** none (verification + finishing)

- [ ] **Step 1: Format + full workspace gate**

```bash
cargo fmt
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```
Expected: fmt clean; ALL tests pass (root + desktop bridge crate); zero clippy warnings. **`--workspace` is mandatory** — the screen module adds root-crate public API; the desktop bridge crate (`rquant-desktop`) must still compile (lesson from the prior bridge-crate miss).

- [ ] **Step 2: Confirm no accidental files / check parallel commits**

```bash
git status --porcelain
git log --oneline -12
```
Expected: clean tree; the SCR-1..10 commits present. (User runs parallel sessions — verify no unrelated staged files.)

- [ ] **Step 3: Finish the development branch**

Invoke the **superpowers:finishing-a-development-branch** skill to verify tests, present merge options, and (on approval) merge `--no-ff` to master + delete the branch. Do NOT push unless the user explicitly asks.

- [ ] **Step 4: Update project memory**

Update `C:\Users\Administrator\.claude\projects\E--rust-app-rquant\memory\rquant-project.md` with the Phase-1 screener outcome (module added, seed ensemble, smoke/validation verdict — including an honest negative if found) and note Phase 2 (breadth fetch + daily run + desktop page) remains gated on Phase-1 validation.

---

## Self-Review (completed by plan author)

**Spec coverage:** §3 signals → SCR-3 (seed trees) + SCR-2 (combine); §4 dual output → SCR-2/SCR-4; §5.1 backtest (nav/benchmark, tag attribution, regime, quality layers) → SCR-5/6/7/8; §5.2 dual validation → SCR-10 smoke (factor IC is the existing `rquant factor`, invoked manually in the validation phase, no new code); §5.3 iteration → process (SCR-10 honest-read + memory note), config is data-driven for tree add/prune; §6 CLI → SCR-9; §6 config → SCR-1/SCR-10; §7 files → all tasks; §8 boundaries → SCR-10 honest-read note. Cross-section correction (self-normalization vs orchestrator) → reflected in SCR-4/SCR-5 (orchestrator does select_top).

**Type consistency:** `ScreenConfig`/`MergeConfig`/`RegimeWindow` (config.rs) used consistently in mod.rs + backtest.rs; `MergeParams`/`CombineOutput`/`combine`/`setup_vote` (combine.rs) signatures match call sites; `ScreenRow`/`ScreenResult`/`ScreenReason` (mod.rs); `ScreenBacktestReport`/`ScreenHolding`/`TagAttribution`/`RegimeSlice`/`QualityLayer` (backtest.rs) — fields populated incrementally across SCR-5..8 with `#[serde(default)]` for forward-compat. Reuse signatures verified against source (`score_symbol`, `select_top`, `accrue`, `turnover_between`, `build_timeline`, `last_close_at`, `risk_metrics`, `build_context`, `traverse`, `load_tree_file`, `read_universe_csv`, `read_bars_csv`).

**Placeholders:** none — all steps contain real code, real test assertions, exact commands.

**Known toolchain check-points flagged inline:** `crate::Error::Data` variant name (SCR-1), `Option::is_none_or` availability (SCR-5), `build_llm`/`Error` imports in cli/mod.rs (SCR-9), DSL identifier validity for `high`/`volume` (SCR-3).
