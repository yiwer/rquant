# 选股器迭代 #1（优质驱动 + 动量倾斜，combine v2）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the screener's falsified strict-AND combine with a quality-driven + momentum-tilt model (select top-N by quality, tilt by the momentum setup), then re-validate on the deep-20 data.

**Architecture:** Pure-logic change in `src/screen/combine.rs` (`combined = quality × (1 + λ·tilt)`, eligibility = quality≥q_floor only, tilt = max strength among configured tilt-setups) + two new `MergeConfig` fields (`lambda`, `tilt_setups`). The orchestrator/backtest/CLI/seed-trees are reused unchanged except the two `MergeParams` construction sites. Validation reuses `screen --backtest`.

**Tech Stack:** Rust 2024, serde/serde_yaml, the existing `src/screen/` module. Spec: `docs/superpowers/specs/2026-06-15-screen-quality-tilt-design.md`.

---

## Current code reference (exact, as merged at master `7dea34c`)

`src/screen/combine.rs`:
```rust
#[derive(Debug, Clone, Copy)]
pub struct MergeParams {
    pub theta_fire: f64,
    pub vote_frac: f64,
    pub q_floor: f64,
}
// ...
pub fn combine(quality: &[f64], setups: &BTreeMap<String, Vec<f64>>, p: &MergeParams) -> CombineOutput {
    let q = mean_finite(quality);
    let mut tags = Vec::new();
    let mut setup_strength: BTreeMap<String, f64> = BTreeMap::new();
    for (tag, scores) in setups {
        let (fired, strength) = setup_vote(scores, p.theta_fire, p.vote_frac);
        if fired { tags.push(tag.clone()); setup_strength.insert(tag.clone(), strength); }
    }
    let spec = setup_strength.values().copied().fold(0.0_f64, f64::max);
    let eligible = !tags.is_empty() && q >= p.q_floor;
    let combined = if eligible { q * spec } else { 0.0 };
    CombineOutput { quality_score: q, speculative_score: spec, combined_score: combined, tags, setup_strength }
}
```
`src/screen/config.rs` `MergeConfig` has fields theta_fire/vote_frac/q_floor/top/quality_layers (each `#[serde(default="...")]`) + a `Default` impl + `fn default_*`. `ScreenConfig::validate(&self)` checks quality_trees non-empty, setup_trees non-empty, each setup non-empty, theta_fire/q_floor∈[0,1], vote_frac∈(0,1], top≥1.

`src/screen/mod.rs` `run_screen` builds: `let mp = MergeParams { theta_fire: sc.merge.theta_fire, vote_frac: sc.merge.vote_frac, q_floor: sc.merge.q_floor };`
`src/screen/backtest.rs` `run_screen_backtest` builds the same `MergeParams { ... }`.

---

## Task IT-1: Config — add `lambda` + `tilt_setups` + cross-validation

**Files:**
- Modify: `src/screen/config.rs`

- [ ] **Step 1: Add the two fields + defaults to `MergeConfig`**

In the `MergeConfig` struct, after the `quality_layers` field, add:
```rust
    /// 倾斜强度系数：combined = quality × (1 + lambda × tilt)。0 = 纯优质驱动。
    #[serde(default = "default_lambda")]
    pub lambda: f64,
    /// 参与选股倾斜的形态标签（其余形态仅标注不倾斜）。
    #[serde(default = "default_tilt_setups")]
    pub tilt_setups: Vec<String>,
```
Add the default fns next to the existing `fn default_*`:
```rust
fn default_lambda() -> f64 { 1.0 }
fn default_tilt_setups() -> Vec<String> { vec!["动量延续".to_string()] }
```
In `impl Default for MergeConfig`, add the two fields:
```rust
            lambda: default_lambda(),
            tilt_setups: default_tilt_setups(),
```

- [ ] **Step 2: Extend `ScreenConfig::validate` with the lambda + tilt_setups checks**

At the end of `validate` (before `Ok(())`), add:
```rust
        if m.lambda < 0.0 {
            return Err(crate::Error::Data("screen config: lambda must be >= 0".into()));
        }
        if m.tilt_setups.is_empty() {
            return Err(crate::Error::Data("screen config: tilt_setups must be non-empty".into()));
        }
        for s in &m.tilt_setups {
            if !self.setup_trees.contains_key(s) {
                return Err(crate::Error::Data(format!(
                    "screen config: tilt_setup '{s}' not found in setup_trees"
                )));
            }
        }
```
(`m` is the existing `let m = &self.merge;` binding already in `validate`.)

- [ ] **Step 3: Update existing tests + add new ones**

In `config.rs` `mod tests`:
- `parses_minimal_config_with_defaults`: add assertions after the existing ones:
```rust
        assert!((cfg.merge.lambda - 1.0).abs() < 1e-12);
        assert_eq!(cfg.merge.tilt_setups, vec!["动量延续".to_string()]);
```
- Add three new tests:
```rust
    #[test]
    fn validate_rejects_negative_lambda() {
        let yaml = r#"
quality_trees: [q.yaml]
setup_trees:
  动量延续: [a.yaml]
merge: { lambda: -0.5 }
"#;
        let cfg: ScreenConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_rejects_tilt_setup_not_in_setups() {
        let yaml = r#"
quality_trees: [q.yaml]
setup_trees:
  突破临界: [a.yaml]
merge: { tilt_setups: ["动量延续"] }
"#;
        let cfg: ScreenConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(cfg.validate().is_err(), "tilt_setup not in setup_trees should fail");
    }

    #[test]
    fn validate_accepts_tilt_setup_in_setups() {
        let yaml = r#"
quality_trees: [q.yaml]
setup_trees:
  动量延续: [a.yaml]
  突破临界: [b.yaml]
merge: { tilt_setups: ["动量延续"] }
"#;
        let cfg: ScreenConfig = serde_yaml::from_str(yaml).unwrap();
        cfg.validate().unwrap();
    }
```
**Note:** existing `validate_rejects_*` tests use `setup_trees: { x: [a.yaml] }` and now also trip the tilt-setup check (default tilt `[动量延续]` ⊄ `{x}`); they still `is_err()`, so they keep passing — no change needed there. `parses_regimes` doesn't call `validate()`, unaffected.

- [ ] **Step 4: Run tests**

Run: `cargo test --lib screen::config`
Expected: all pass (existing + 3 new). If `crate::Error::Data` differs, match the real variant (it's used elsewhere in this file already).

- [ ] **Step 5: Commit**

```bash
git add src/screen/config.rs
git commit  (message: "feat(screen): config lambda + tilt_setups for quality-tilt combine" + footer: Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>)
```

---

## Task IT-2: Combine v2 + wire call sites + example config

**Files:**
- Modify: `src/screen/combine.rs`
- Modify: `src/screen/mod.rs` (run_screen MergeParams construction)
- Modify: `src/screen/backtest.rs` (run_screen_backtest MergeParams construction)
- Modify: `examples/screen/screen_v1.yaml`

- [ ] **Step 1: Change `MergeParams` (add fields, drop Copy)**

Replace the `MergeParams` definition in `combine.rs`:
```rust
/// 合并参数。
#[derive(Debug, Clone)]
pub struct MergeParams {
    pub theta_fire: f64,
    pub vote_frac: f64,
    pub q_floor: f64,
    /// 倾斜系数：combined = quality × (1 + lambda × tilt)。
    pub lambda: f64,
    /// 参与倾斜的形态标签（其余仅标注）。
    pub tilt_setups: Vec<String>,
}
```
(`Copy` removed because `Vec<String>` isn't `Copy`; all call sites pass `&MergeParams`, so this is safe.)

- [ ] **Step 2: Rewrite `combine` to the quality-tilt model**

Replace the body of `combine` (keep the signature `pub fn combine(quality: &[f64], setups: &BTreeMap<String, Vec<f64>>, p: &MergeParams) -> CombineOutput`):
```rust
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
    // 投机分 = 全部命中形态最大强度（仅信息）
    let spec = setup_strength.values().copied().fold(0.0_f64, f64::max);
    // 倾斜量 = 仅 tilt_setups 中命中形态的最大强度（未命中 → 0）
    let tilt = p
        .tilt_setups
        .iter()
        .filter_map(|s| setup_strength.get(s).copied())
        .fold(0.0_f64, f64::max);
    // 合格门 = 仅优质（去掉 AND tags 要求）；综合分 = 优质 × (1 + λ·倾斜)
    let eligible = q >= p.q_floor;
    let combined = if eligible { q * (1.0 + p.lambda * tilt) } else { 0.0 };
    CombineOutput {
        quality_score: q,
        speculative_score: spec,
        combined_score: combined,
        tags,
        setup_strength,
    }
}
```

- [ ] **Step 3: Rewrite the combine unit tests for the new semantics**

In `combine.rs` `mod tests`: the `setup_vote` tests (vote_single/majority/empty) are UNCHANGED. Replace the `p()` helper and the `combine_*` tests:
```rust
    fn p() -> MergeParams {
        MergeParams {
            theta_fire: 0.5,
            vote_frac: 0.5,
            q_floor: 0.5,
            lambda: 1.0,
            tilt_setups: vec!["动量延续".to_string()],
        }
    }

    #[test]
    fn combine_quality_is_mean() {
        let setups = BTreeMap::new();
        let out = combine(&[1.0, 0.5], &setups, &p());
        assert!((out.quality_score - 0.75).abs() < 1e-12);
    }

    #[test]
    fn combine_pure_quality_is_selectable() {
        // 无形态命中、但优质≥q_floor → 合格、combined = quality（tilt=0）。根治空仓的核心。
        let setups = BTreeMap::new();
        let out = combine(&[0.8], &setups, &p());
        assert!(out.tags.is_empty());
        assert!((out.combined_score - 0.8).abs() < 1e-12);
    }

    #[test]
    fn combine_momentum_tilts_combined() {
        let mut setups = BTreeMap::new();
        setups.insert("动量延续".to_string(), vec![0.8]);
        let out = combine(&[0.9], &setups, &p());
        assert_eq!(out.tags, vec!["动量延续".to_string()]);
        // combined = 0.9 × (1 + 1.0 × 0.8) = 1.62
        assert!((out.combined_score - 1.62).abs() < 1e-12);
    }

    #[test]
    fn combine_ineligible_when_quality_below_floor() {
        let mut setups = BTreeMap::new();
        setups.insert("动量延续".to_string(), vec![0.9]);
        let out = combine(&[0.3], &setups, &p()); // quality 0.3 < q_floor 0.5
        assert_eq!(out.combined_score, 0.0);
    }

    #[test]
    fn combine_tilt_only_from_tilt_setups() {
        // 突破临界命中但不在 tilt_setups → 不进倾斜；动量延续未命中 → tilt=0。
        let mut setups = BTreeMap::new();
        setups.insert("突破临界".to_string(), vec![0.9]); // fires, but NOT a tilt setup
        let out = combine(&[1.0], &setups, &p());
        assert_eq!(out.tags, vec!["突破临界".to_string()]); // still tagged
        assert!((out.speculative_score - 0.9).abs() < 1e-12); // info reflects it
        assert!((out.combined_score - 1.0).abs() < 1e-12); // but combined = q×(1+0) = 1.0 (no tilt)
    }

    #[test]
    fn combine_lambda_zero_is_pure_quality() {
        let mut pp = p();
        pp.lambda = 0.0;
        let mut setups = BTreeMap::new();
        setups.insert("动量延续".to_string(), vec![1.0]);
        let out = combine(&[0.7], &setups, &pp);
        assert!((out.combined_score - 0.7).abs() < 1e-12); // λ=0 → combined = quality
    }
```

- [ ] **Step 4: Wire the two `MergeParams` construction sites**

In `src/screen/mod.rs` `run_screen`, replace the `MergeParams { ... }` construction with:
```rust
    let mp = MergeParams {
        theta_fire: sc.merge.theta_fire,
        vote_frac: sc.merge.vote_frac,
        q_floor: sc.merge.q_floor,
        lambda: sc.merge.lambda,
        tilt_setups: sc.merge.tilt_setups.clone(),
    };
```
In `src/screen/backtest.rs` `run_screen_backtest`, replace its `MergeParams { ... }` the same way (identical five fields).

- [ ] **Step 5: Build + run the screen tests; verify integration tests still pass under new semantics**

Run: `cargo test --lib screen::`
Expected: compiles; all pass. The synthetic integration tests (`screen_selects_rising_symbol_with_tag`, `backtest_*`) use stark UP(rising)/DN(falling) data where UP has quality≥floor and DN has quality 0 — under the new model UP stays selected (combined = q×(1+λ·mom) > 0) and DN stays out (quality 0 < floor → combined 0), so their assertions still hold. If any assertion breaks, READ it and adjust to the new semantics (do NOT weaken a meaningful assertion — confirm the new behavior is correct first).

- [ ] **Step 6: Update the example ensemble config**

In `examples/screen/screen_v1.yaml`, under `merge:`, add two keys (after `quality_layers: 3`):
```yaml
  lambda: 1.0
  tilt_setups: [动量延续]
```

- [ ] **Step 7: Commit**

```bash
git add src/screen/combine.rs src/screen/mod.rs src/screen/backtest.rs examples/screen/screen_v1.yaml
git commit  (message: "feat(screen): combine v2 — quality-driven selection + momentum tilt" + Co-Authored-By footer)
```

---

## Task IT-3: Full gate + λ-sweep validation + finish + memory

**Files:**
- Create: `docs/superpowers/2026-06-15-screen-tilt-validation.md`
- (verification + finishing + memory; no further src changes unless a gate fails)

- [ ] **Step 1: Full workspace gate**

```bash
cargo test --workspace 2>&1 | grep -E "test result:|error|FAILED" ; echo "EXIT=${PIPESTATUS[0]}"
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | grep -E "error|warning|Finished" ; echo "EXIT=${PIPESTATUS[0]}"
```
Expected: both EXIT=0, 0 failed, clippy clean. **`--workspace` mandatory** (combine MergeParams is root public API; the desktop bridge must still compile). Do NOT pipe to plain `tail` (its exit code masks cargo's — use the PIPESTATUS form above).

- [ ] **Step 2: Ensure the deep-20 data is present in the working dir**

The backtest needs `data/universe_20.csv` + `data/*.csv` (gitignored). If running in a fresh worktree, copy from the main checkout:
```bash
ls data/universe_20.csv 2>/dev/null || cp E:/rust-app/rquant/data/*.csv data/
```

- [ ] **Step 3: λ-sweep backtest (the honest validation)**

Run all three and capture clean numbers (parse JSON with `python -c` using `encoding='utf-8'` — Chinese tags break Windows-default GBK):
```bash
# λ=1 (quality + momentum tilt) — uses the example config default
./target/release/rquant.exe screen --backtest --universe data/universe_20.csv --config examples/screen/screen_v1.yaml --from 2018-01-01 --to 2026-06-01 --rebalance 5 --out tmps/screen_tilt_l1.json
```
For λ=0, make a temp config copy with `lambda: 0.0` (or a second config file `tmps/screen_l0.yaml` with the same content but lambda 0.0) and run it to `tmps/screen_tilt_l0.json`. (Need `cargo build --release` first if the binary is stale.)

Parse each JSON for: `total_return`, `benchmark_return`, `excess_return`, `avg_members`, `max_drawdown`, `risk.sharpe`, `tag_attribution`, `regime_slices`, `quality_layers`.

- [ ] **Step 4: Write the honest validation report**

Create `docs/superpowers/2026-06-15-screen-tilt-validation.md` capturing:
- The three-way comparison (λ=0 vs λ=1 vs benchmark): total/excess/avg_members/sharpe/maxDD.
- **Did it fix cash-drag?** avg_members vs the 0.38 baseline.
- **Does the momentum tilt add value?** λ=1 vs λ=0 excess/sharpe — if λ=1 ≤ λ=0, the tilt adds nothing (honest negative; recommend λ=0 / pure quality).
- Regime slices (bull capture improved? 2022 bear still defensive?) + tag attribution (momentum still positive?).
- **Honest verdict** per §5.3: do NOT tune to look good. Distinguish "fixed cash-drag" (mechanism) from "beats benchmark" (likely still not — these large-caps are in the benchmark). State both plainly.

- [ ] **Step 5: Commit the report**

```bash
git add docs/superpowers/2026-06-15-screen-tilt-validation.md
git commit  (message: "docs(screen): iteration #1 quality-tilt validation findings" + Co-Authored-By footer)
```

- [ ] **Step 6: Finish the branch**

Invoke **superpowers:finishing-a-development-branch** (verify tests → options → on user's choice merge `--no-ff` to master → cleanup). Do NOT push unless the user asks.

- [ ] **Step 7: Update memory**

Update `C:\Users\Administrator\.claude\projects\E--rust-app-rquant\memory\rquant-project.md` with the iteration-#1 outcome (combine v2 shipped; the λ-sweep verdict — including honest negatives if found; whether the screener now stays invested; next direction).

---

## Self-Review (completed by plan author)

**Spec coverage:** §3 combine v2 → IT-2 (logic + tests); §4 config (lambda/tilt_setups + validate cross-check) → IT-1; §5 validation (λ=0 vs λ=1 vs benchmark + avg_members + regime + attribution + honest verdict) → IT-3 steps 3-4; §6 files → IT-1/IT-2/IT-3; §7 boundaries (Phase-1, reuse, may falsify) → IT-3 step 4. All covered.

**Placeholder scan:** none — exact code for every edit, exact combine formula, concrete test assertions with computed expected values (1.62, 0.8, etc.), exact commands.

**Type consistency:** `MergeParams` gains `lambda: f64` + `tilt_setups: Vec<String>` (IT-2 step 1), constructed identically in config defaults (IT-1), run_screen + run_screen_backtest (IT-2 step 4); `combine` signature unchanged (still `&MergeParams`); `MergeConfig.lambda`/`tilt_setups` (IT-1) read at both construction sites (IT-2 step 4). Copy→Clone on MergeParams is safe (all call sites use `&mp`). Consistent.

**Known check-points flagged inline:** `crate::Error::Data` variant (IT-1 step 4); integration-test assertions under new semantics (IT-2 step 5); PIPESTATUS exit capture (IT-3 step 1); gitignored data copy + UTF-8 JSON parse (IT-3 steps 2-3).
