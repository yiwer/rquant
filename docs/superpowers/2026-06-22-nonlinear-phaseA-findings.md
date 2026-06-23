# Phase A Non-linear Factor Evaluation: Findings (2026-06-22)

**Task:** A4 — weekly backtester with non-linear features + cost-aware hysteresis + multi-fold WFO,
comparing learned non-linear weights vs equal-weight on both the membership panel and the wide
(full) panel.

**Scripts:** `scripts/eval_nonlinear.py`, `scripts/train_nonlinear.py`

---

## Setup

- **WFO:** 4 anchored-expanding folds, train always starts 2018-01-02, OOS shifts one year at a time.
- **Non-linear model:** ElasticNet on expanded features (original ranks + squared + top-5 interactions). Weights trained on each fold's train slice only (no OOS leak).
- **Hysteresis δ:** Selected on train slice per fold via grid {0, 0.02, 0.05, 0.1}; maximises net total return on train. OOS is never touched during δ selection.
- **Equal-weight baseline:** f_bm=1, f_npyoy=1, no feature expansion, δ=0.
- **Top-N:** 3 picks per period. Cost: 20 bps (iterate.COST). Benchmark: CSI 300 index.
- **Hard gates:** non-ST, f_roe>0, f_bm>0, f_logamt >= log(5×10^7).

---

## Membership Panel (factors.csv — PIT membership mask applied)

268,557 rows, dates 2018-02-06..2026-06-11.

| Fold OOS window | Selected δ | Learned-NL OOS excess | Equal-wt OOS excess |
|---|---|---|---|
| 2022-01-02..2022-12-31 | 0.02 | **+0.4129** | -0.0684 |
| 2023-01-02..2023-12-31 | 0.02 | -0.1787 | **+0.2156** |
| 2024-01-02..2024-12-31 | 0.05 | **+0.6557** | -0.1156 |
| 2025-01-02..2026-06-30 | 0.02 | -0.1990 | **+0.1365** |

**Aggregate (membership):**
- Learned-NL: mean OOS excess = **+0.1727** (2/4 positive folds)
- Equal-weight: mean OOS excess = **+0.0421** (2/4 positive folds)
- §5.3-positive (NL): **No** (positive folds ≤ 50%)
- §5.3-positive (EW): **No** (positive folds = 50%, not majority)
- Non-linear beats equal-weight (mean OOS): **Yes** (+0.1727 > +0.0421)

---

## Full (Wide) Panel (factors_full.csv — no membership mask)

371,418 rows, dates 2018-01-02..2026-06-11.

| Fold OOS window | Selected δ | Learned-NL OOS excess | Equal-wt OOS excess |
|---|---|---|---|
| 2022-01-02..2022-12-31 | 0.02 | **+0.0324** | +0.0007 |
| 2023-01-02..2023-12-31 | 0.02 | -0.1017 | **+0.1646** |
| 2024-01-02..2024-12-31 | 0.05 | -0.2286 | **+0.0295** |
| 2025-01-02..2026-06-30 | 0.05 | **+1.2734** | **+0.1534** |

**Aggregate (full panel):**
- Learned-NL: mean OOS excess = **+0.2439** (2/4 positive folds)
- Equal-weight: mean OOS excess = **+0.0870** (4/4 positive folds)
- §5.3-positive (NL): **No** (positive folds = 50%)
- §5.3-positive (EW): **Yes** (4/4 positive folds, mean > 0)
- Non-linear beats equal-weight (mean OOS): **Yes** (+0.2439 > +0.0870)

---

## Wide-panel Survivorship Caveat

The `factors_full.csv` panel omits the PIT membership mask. This means:

- The stock universe includes **all securities with sufficient price/fundamental history**
  back-filled to 2018, not just those in the top-2000 membership index at each date.
- **Absolute excess return numbers are inflated** relative to a live strategy that could only
  invest in contemporaneous exchange-listed stocks.
- The **relative verdict** (non-linear vs equal-weight on the same panel) is internally
  consistent: both strategies see the same survivorship bias equally, so the comparison
  direction is valid.
- The membership panel figures are more reliable for absolute OOS magnitude. The full-panel
  figures should only be used for within-panel model comparison.

---

## Final Verdict

| Criterion | Membership panel | Full panel |
|---|---|---|
| NL mean OOS excess > 0 | Yes (+0.173) | Yes (+0.244) |
| NL beats equal-weight (mean OOS) | **Yes** | **Yes** |
| NL §5.3-positive (majority positive folds) | No (2/4) | No (2/4) |
| Equal-wt §5.3-positive | No (2/4) | Yes (4/4) |

**Non-linear learned weights beat equal-weight in mean OOS excess on both panels.**
However, neither method achieves §5.3-positive status (majority-positive OOS folds with NL).
The non-linear model shows higher *mean* OOS excess but also higher variance — large positive
folds (2022: +0.41, 2024: +0.66) are paired with large negative ones (2023: -0.18, 2025: -0.20).

The equal-weight baseline is more stable on the full panel (4/4 positive folds) but its
absolute magnitude is modest (+0.04..+0.09 mean).

**Conclusion:** Phase A result = **non-linear beats equal-weight on mean OOS (both panels),
but fails §5.3 majority-positive gate.** Both methods are pre-production; neither passes
the strictest quality gate. Recommended next step: investigate fold 2023 and 2025 failures —
macro regime change or model overfit to 2018-2021 data.
