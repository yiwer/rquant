# Phase B GBDT Factor Evaluation: Findings (2026-06-22)

**Task:** B2 — GBDT ensemble backtester with cost-aware hysteresis + multi-fold WFO,
comparing GBDT ensemble vs equal-weight on both the membership panel and the wide (full)
panel. Phase B builds on Phase A (eval_nonlinear.py) by replacing the ElasticNet linear
scorer with a LightGBM multi-seed ensemble.

**Scripts:** `scripts/eval_gbdt.py`, `scripts/train_gbdt.py`
**Dependency:** `lightgbm` (pip install lightgbm)

---

## Setup

- **WFO:** 4 anchored-expanding folds, train always starts 2018-01-02, OOS shifts one year at a time.
- **GBDT model:** LightGBM ensemble of 5 seeds per fold. Inner early-stopping on last year of train. Regularised (num_leaves=31, max_depth=5, lr=0.03, min_child_samples=200, lambda_l1=l2=1.0). Features = per-date cross-sectional rank of 13 factors.
- **Hysteresis δ:** Selected on train slice per fold via grid {0, 0.02, 0.05, 0.1}; maximises net total return on train. OOS is never touched during δ selection.
- **Equal-weight baseline:** f_bm=1, f_npyoy=1, no GBDT, δ=0. Identical to Phase-A baseline.
- **Top-N:** 3 picks per period. Cost: 20 bps (iterate.COST). Benchmark: CSI 300 index.
- **Hard gates:** non-ST, f_roe>0, f_bm>0, f_logamt >= log(5×10^7).

---

## Membership Panel (factors.csv — PIT membership mask applied)

268,557 rows, dates 2018-02-06..2026-06-11.

| Fold OOS window | Selected δ | GBDT OOS excess | Equal-wt OOS excess |
|---|---|---|---|
| 2022-01-02..2022-12-31 | 0.00 | -0.0014 | -0.0684 |
| 2023-01-02..2023-12-31 | 0.02 | **+0.0252** | **+0.2156** |
| 2024-01-02..2024-12-31 | 0.00 | **+0.0207** | -0.1156 |
| 2025-01-02..2026-06-30 | 0.00 | -0.1492 | **+0.1365** |

**Aggregate (membership):**
- GBDT:         mean OOS excess = **-0.0262** (2/4 positive folds)
- Equal-weight: mean OOS excess = **+0.0421** (2/4 positive folds)
- §5.3-positive (GBDT): **No** (mean < 0 and positive folds ≤ 50%)
- §5.3-positive (EW):   **No** (positive folds = 50%, not majority)
- GBDT beats equal-weight (mean OOS): **No** (-0.0262 < +0.0421)

---

## Full (Wide) Panel (factors_full.csv — no membership mask)

371,418 rows, dates 2018-01-02..2026-06-11.

| Fold OOS window | Selected δ | GBDT OOS excess | Equal-wt OOS excess |
|---|---|---|---|
| 2022-01-02..2022-12-31 | 0.05 | **+0.2312** | +0.0007 |
| 2023-01-02..2023-12-31 | 0.00 | **+0.3323** | **+0.1646** |
| 2024-01-02..2024-12-31 | 0.00 | **+0.6044** | +0.0295 |
| 2025-01-02..2026-06-30 | 0.00 | **+5.8206** | **+0.1534** |

**Aggregate (full panel):**
- GBDT:         mean OOS excess = **+1.7471** (4/4 positive folds)
- Equal-weight: mean OOS excess = **+0.0870** (4/4 positive folds)
- §5.3-positive (GBDT): **Yes** (4/4 positive folds, mean > 0)
- §5.3-positive (EW):   **Yes** (4/4 positive folds, mean > 0)
- GBDT beats equal-weight (mean OOS): **Yes** (+1.7471 >> +0.0870)

---

## GBDT vs Equal-weight vs Phase-A Non-linear

| Panel | Method | Mean OOS excess | Positive folds | §5.3-positive |
|---|---|---|---|---|
| Membership | GBDT (Phase B) | **-0.0262** | 2/4 | No |
| Membership | Equal-weight | +0.0421 | 2/4 | No |
| Membership | Learned-NL (Phase A) | +0.1727 | 2/4 | No |
| Full (wide) | GBDT (Phase B) | **+1.7471** | 4/4 | **Yes** |
| Full (wide) | Equal-weight | +0.0870 | 4/4 | Yes |
| Full (wide) | Learned-NL (Phase A) | +0.2439 | 2/4 | No |

---

## Wide-panel Survivorship Caveat

The `factors_full.csv` panel omits the PIT membership mask, meaning:

- The stock universe includes **all securities with sufficient price/fundamental history**
  back-filled to 2018, not just those contemporaneously exchange-listed in the top-2000.
- **The GBDT full-panel figures are severely inflated** by survivorship bias. Fold 4 alone
  shows +5.82 OOS excess — a number that almost certainly reflects the GBDT finding patterns
  in the full back-filled universe that would be inaccessible to a live strategy.
- The **relative direction** (GBDT vs equal-weight on the same panel) is internally
  consistent: both see the same survivorship. The within-panel comparison is valid.
- The membership panel figures (GBDT mean = -0.03) are the more reliable signal for
  absolute OOS magnitude; the full-panel results should only be used for model comparisons,
  not for absolute performance claims.

---

## Final Verdict

| Criterion | Membership panel | Full panel |
|---|---|---|
| GBDT mean OOS excess > 0 | **No (-0.026)** | Yes (+1.747, but inflated) |
| GBDT beats equal-weight (mean OOS) | **No** | Yes |
| GBDT §5.3-positive (majority positive folds) | No (2/4) | **Yes (4/4)** |
| Equal-wt §5.3-positive | No (2/4) | Yes (4/4) |
| GBDT beats Phase-A NL (mean OOS) | **No** (-0.026 vs +0.173) | Yes (+1.747 vs +0.244) |

**Honest assessment:**

The GBDT results are **mixed and panel-dependent**:

1. **Membership panel (more reliable):** GBDT underperforms equal-weight (mean OOS -0.03 vs
   +0.04). It also underperforms Phase-A non-linear (+0.17). GBDT fails §5.3 on the
   membership panel. This is the authoritative signal.

2. **Full panel (survivorship-inflated):** GBDT achieves 4/4 positive folds and massively
   outperforms equal-weight. However, fold 4 (+5.82) dominates the mean and almost certainly
   reflects survivorship in the back-filled universe. This result cannot be taken at face value.

3. **The GBDT does NOT beat equal-weight on the membership panel**, which is the panel
   relevant to a live strategy. Phase-A non-linear (ElasticNet) also beats GBDT on membership
   (+0.17 vs -0.03 mean OOS).

4. **§5.3 stability gate:** GBDT passes on the full panel (4/4 positive folds) but fails on
   the membership panel (2/4). Since the full-panel result is contaminated by survivorship,
   the stability conclusion is ambiguous at best.

**Conclusion: GBDT Phase-B = NOT RECOMMENDED for production.** On the membership panel
(the right benchmark), GBDT has negative mean OOS excess, underperforms the equal-weight
baseline, and underperforms Phase-A non-linear. The full-panel §5.3-pass is driven by
survivorship and fold-4 leverage in an inflated universe. Neither Phase A nor Phase B
achieves §5.3-positive status on the membership panel. Recommended next step: investigate
the fold-4 full-panel outlier and consider whether the feature set needs richer financial
signal before adopting GBDT.
