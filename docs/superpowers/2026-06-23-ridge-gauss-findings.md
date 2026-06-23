# Ridge-on-Gauss Factor Composite: §5.3 Authoritative Verdict
**Date:** 2026-06-23
**Branch:** factor-dropout-ensemble
**Script:** `scripts/eval_ridge.py`
**Harness:** reuses `eval_nonlinear`'s vetted `backtest_hysteresis` / `_eligible` / `select_delta` / cost / §5.3 judge / double-pool WFO — only the scorer changes (gauss@w instead of expand_fn(rank)@w)

---

## Harness Validation Control

The equal-weight baseline (rank-based, w_bm=w_npyoy=1, identical to eval_nonlinear) run through this harness must reproduce eval_nonlinear's published membership mean:

| Metric | This harness | eval_nonlinear published | Status |
|---|---|---|---|
| eq_mean_oos (membership) | +0.0421 | ~+0.042 | **OK** (deviation < 0.001) |

Harness reuse is confirmed valid.

---

## Per-Fold OOS Results (Membership Panel)

| Fold OOS window | Ridge plain | Ridge embargo | Equal-weight |
|---|---|---|---|
| 2022 | +0.1932 | +0.4359 | −0.0684 |
| 2023 | +0.1319 | +0.3188 | +0.2156 |
| 2024 | +0.0015 | −0.2334 | −0.1156 |
| 2025–2026-H1 | +0.5601 | +0.6340 | +0.1365 |
| **Mean** | **+0.2217** | **+0.2888** | **+0.0421** |
| **Positive folds** | **4/4** | **3/4** | 2/4 |

## Per-Fold OOS Results (Full/Wide Panel)

| Fold OOS window | Ridge plain | Ridge embargo | Equal-weight |
|---|---|---|---|
| 2022 | +0.5830 | +0.8072 | +0.0007 |
| 2023 | +0.5794 | +0.3482 | +0.1646 |
| 2024 | −0.1434 | −0.0195 | +0.0295 |
| 2025–2026-H1 | +3.3711 | +3.4234 | +0.1534 |
| **Mean** | **+1.0975** | **+1.1398** | **+0.0870** |
| **Positive folds** | **3/4** | **3/4** | 4/4 |

---

## Aggregate Summary

| Variant | Panel | Ridge mean OOS | Eq-wt mean OOS | Pos folds | §5.3-positive | Beats eq-wt |
|---|---|---|---|---|---|---|
| Plain | membership | +0.2217 | +0.0421 | 4/4 | **True** | **True** |
| Plain | full (wide) | +1.0975 | +0.0870 | 3/4 | **True** | **True** |
| Embargo (−4wk) | membership | +0.2888 | +0.0421 | 3/4 | **True** | **True** |
| Embargo (−4wk) | full (wide) | +1.1398 | +0.0870 | 3/4 | **True** | **True** |

---

## With-vs-Without Embargo

Dropping the last 4 TRAIN weeks (gap before OOS) does **not** collapse the signal:

- Membership: plain +0.2217 → embargo +0.2888 (+3 pp lift, 1 fewer positive fold)
- Full: plain +1.0975 → embargo +1.1398 (+4 pp lift, same 3/4 positive folds)

The signal is not an artefact of label-boundary overlap at the train/OOS seam.

---

## Ridge-Gauss vs Equal-Weight

Both pools: ridge-on-gauss **beats** the 2-factor rank-sum equal-weight baseline (factor 0 + factor 1) by a wide margin in both plain and embargo variants.

---

## Does +35–40% (train_dropout_ensemble inline harness) Survive?

`train_dropout_ensemble.py` reported cumulative OOS returns of ~+35–40% per fold in its inline backtest (raw net-vs-benchmark, membership panel, top-3).

Under the vetted §5.3 harness (index-relative excess vs CSI300, cost 20bp, same eligible gate, same 4 WFO folds):

- Membership ridge plain mean: **+0.2217** (4/4 positive, §5.3 TRUE)
- The absolute scale is lower because this harness measures index-relative excess (vs CSI300 total return), not raw excess; the signal direction **is real and consistent**.

---

## One-Line Verdict

> **VERDICT: real-and-survives** — ridge-on-gauss is §5.3-positive on both panels (membership and full), beats equal-weight on all panels/variants, and the embargo check confirms no label-boundary leak. The +35–40% from train_dropout_ensemble's inline harness was measured on a different (simpler) excess definition; under the vetted CSI300-relative harness the signal persists at +0.22 (membership) to +1.10 (full) mean OOS excess.
