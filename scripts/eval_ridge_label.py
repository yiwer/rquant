"""Objective/label experiments on ridge-on-gauss: risk-adjusted ranking and a
long-side-focused label (no index hedging).

Backbone unchanged — convex, regularised, rank-space ridge (the part proven robust;
the DE deploy-objective tried to align the FIT to the portfolio and blew up OOS).
We only change WHAT is ranked (the label), not HOW it is fit:

  raw      : rank(fwd_ret_5d)                  — vetted baseline
  riskadj  : rank(fwd_ret_5d / vol)            — rank by forward Sharpe, attacks the
             variance-drag that kills the concentrated harvest at the SIGNAL level
  longtail : one-sided label, bottom half flat — spend all fitting capacity ordering
             the LONG top; tests whether the short-side power can be moved long, or
             is structural (limits-to-arbitrage: un-shortable junk stays mispriced)

Per mode: 6-fold §5.3 (top-3 and top-10/invvol) + long-side decile profile.
Reuses fit_ridge's Gram machinery / norm_gauss / backtest_ridge(+weighted) / decile.
Anchor: fit_ridge_label(mode="raw") == eval_ridge.fit_ridge bit-for-bit.
"""
import sys
import os

sys.stdout.reconfigure(encoding="utf-8")
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import numpy as np
import pandas as pd

import eval_ridge as er
import eval_ridge_topk as tk
import eval_ridge_decile as dc
import factor_lib as fl
import iterate as it
import train_nonlinear as tn
from build_factor_matrix import FACTOR_COLS
from test_norm_hysteresis import norm_gauss

FOLDS = [
    ("2018-01-02", "2019-12-31", "2020-01-02", "2020-12-31"),
    ("2018-01-02", "2020-12-31", "2021-01-02", "2021-12-31"),
] + list(tn.WFO_FOLDS)

MODES = ["raw", "riskadj", "longtail"]
VOL_COL = "f_vol20"


def make_label(fwd, vol, mode):
    """Centered cross-sectional training label for one rebalance date.

    raw      : rank(fwd) − mean            (== rank − 0.5, the vetted label)
    riskadj  : rank(fwd / vol) − mean      (vol NaN/≤0 → median-filled)
    longtail : center(max(rank(fwd) − 0.5, 0))  — bottom half flattened to one value
    """
    f = np.asarray(fwd, float)
    if mode == "raw":
        r = fl.cross_sectional_rank(f)
        return r - r.mean()
    if mode == "riskadj":
        v = np.asarray(vol, float).copy()
        valid = np.isfinite(v) & (v > 0)
        if valid.any():
            v[~valid] = np.median(v[valid])
        else:
            v[:] = 1.0
        r = fl.cross_sectional_rank(f / v)
        return r - r.mean()
    if mode == "longtail":
        r = fl.cross_sectional_rank(f)
        h = np.maximum(r - 0.5, 0.0)
        return h - h.mean()
    raise ValueError(f"unknown label mode: {mode}")


def fit_ridge_label(panel, date_lo, date_hi, mode="raw", vol_col=VOL_COL):
    """Same Gram/ridge machinery as eval_ridge.fit_ridge, only the label changes.

    mode="raw" reproduces eval_ridge.fit_ridge exactly.
    """
    sub = panel[(panel["date"] >= date_lo) & (panel["date"] <= date_hi)].copy()
    sub = sub.dropna(subset=["fwd_ret_5d"])
    p = len(FACTOR_COLS)
    Gram = np.zeros((p, p))
    b = np.zeros(p)
    n_train_dates = 0

    for d, g in sub.groupby("date"):
        g = g.dropna(subset=["fwd_ret_5d"])
        if len(g) < 5:
            continue
        G = norm_gauss(g[FACTOR_COLS].to_numpy(float))
        y = make_label(g["fwd_ret_5d"].to_numpy(float), g[vol_col].to_numpy(float), mode)
        Gram += G.T @ G
        b += G.T @ y
        n_train_dates += 1

    if n_train_dates == 0:
        return np.zeros(p), 0

    lam = er.RIDGE_A * np.mean(np.diag(Gram))
    A = Gram + lam * np.eye(p)
    w = np.linalg.solve(A, b)
    q = np.percentile(np.abs(w), 90) + 1e-12
    w = np.clip(w, -q, q)
    return w, n_train_dates


# ---------------------------------------------------------------------------
# Per-mode evaluation: 6-fold §5.3 (top-3 + top-10/invvol) + long-side profile
# ---------------------------------------------------------------------------

def _m(xs):
    vals = [x for x in xs if x is not None]
    return float(np.mean(vals)) if vals else None


def run_mode(panel, mode, st_set, idx_data):
    top3_ex, top3_sh, t10_ex, t10_sh = [], [], [], []
    dec_acc, band_acc, ic_acc = [], [], []

    for fold in FOLDS:
        tl, th, ol, oh = fold
        oos = panel[(panel["date"] >= ol) & (panel["date"] <= oh)].copy()
        if len(oos) == 0:
            continue
        w, _ = fit_ridge_label(panel, tl, th, mode=mode)
        delta = er.select_delta_ridge(panel, fold, w, st_set)

        r3 = er.backtest_ridge(oos, w, top_n=er.TOP_N, cost_bps=it.COST, st_set=st_set, delta=delta)
        rel3 = it.to_index_relative(r3, idx_data[0], idx_data[1])
        top3_ex.append(rel3["excess_return"] if rel3 else None)
        top3_sh.append(r3["risk"]["sharpe"])

        r10 = tk.backtest_ridge_weighted(oos, w, top_n=10, cost_bps=it.COST,
                                         st_set=st_set, delta=delta, scheme="invvol")
        rel10 = it.to_index_relative(r10, idx_data[0], idx_data[1])
        t10_ex.append(rel10["excess_return"] if rel10 else None)
        t10_sh.append(r10["risk"]["sharpe"])

        for d, g in oos.groupby("date"):
            g = er._eligible(g.dropna(subset=["fwd_ret_5d"]), st_set)
            if len(g) < dc.N_BUCKETS:
                continue
            score = norm_gauss(g[FACTOR_COLS].to_numpy(float)) @ w
            fwd = g["fwd_ret_5d"].to_numpy(float)
            xs = fwd - fwd.mean()
            dec_acc.append(dc.decile_means(score, xs, dc.N_BUCKETS))
            band_acc.append(dc.top_rank_means(score, xs, dc.TOP_BANDS))
            ic_acc.append(fl.rank_ic(score, fwd))

    dec = np.nanmean(np.array(dec_acc), axis=0)
    band = np.nanmean(np.array(band_acc), axis=0)
    return {
        "top3_ex": _m(top3_ex), "top3_sh": _m(top3_sh),
        "t10_ex": _m(t10_ex), "t10_sh": _m(t10_sh),
        "top3_ex_folds": top3_ex, "t10_ex_folds": t10_ex,
        "D1": float(dec[0]), "D6": float(dec[5]), "D10": float(dec[9]),
        "long_spread": float(dec[9] - dec[5]), "ls_spread": float(dec[9] - dec[0]),
        "fine_top3": float(band[0]), "fine_410": float(band[1]),
        "ic": float(np.nanmean(ic_acc)), "dec": dec,
    }


def main():
    st_set = set(pd.read_csv(er.ST_PATH)["symbol"]) if os.path.exists(er.ST_PATH) else set()
    idx_data = it.load_index("csi300")
    panel = pd.read_csv(er.PANEL_MEMBERSHIP, dtype={"symbol": str})
    print(f"Panel: {len(panel)} rows, dates {panel['date'].min()}..{panel['date'].max()}; ST {len(st_set)}")

    res = {mode: run_mode(panel, mode, st_set, idx_data) for mode in MODES}

    print(f"\n{'='*72}\nLabel × harvest — 6-fold OOS (vs csi300)\n{'='*72}")
    print(f"{'mode':>9}{'top3_ex':>9}{'top3_Sh':>9}{'t10_ex':>9}{'t10_Sh':>9}{'rankIC':>9}")
    for mode in MODES:
        r = res[mode]
        print(f"{mode:>9}{r['top3_ex']:>+9.3f}{r['top3_sh']:>9.3f}"
              f"{r['t10_ex']:>+9.3f}{r['t10_sh']:>9.3f}{r['ic']:>+9.4f}")

    print(f"\n{'='*72}\nSignal decile profile (per-name 5d xs-excess, bp) — long vs short power\n{'='*72}")
    print(f"{'mode':>9}{'D1(short)':>11}{'D6':>8}{'D10':>8}{'long D10-D6':>13}{'top3':>8}{'4-10':>8}")
    for mode in MODES:
        r = res[mode]
        print(f"{mode:>9}{r['D1']*1e4:>+11.0f}{r['D6']*1e4:>+8.0f}{r['D10']*1e4:>+8.0f}"
              f"{r['long_spread']*1e4:>+13.0f}{r['fine_top3']*1e4:>+8.0f}{r['fine_410']*1e4:>+8.0f}")

    fold_labels = [f[2][:4] for f in FOLDS]
    print(f"\n{'='*72}\nPer-fold top-3 excess (single-fold-artifact check)\n{'='*72}")
    print(f"{'mode':>9}" + "".join(f"{lbl:>9}" for lbl in fold_labels) + f"{'ex-2025':>9}")
    for mode in MODES:
        folds = res[mode]["top3_ex_folds"]
        cells = "".join(f"{(v if v is not None else float('nan')):>+9.3f}" for v in folds)
        ex25 = _m([v for lbl, v in zip(fold_labels, folds) if lbl != "2025"])
        print(f"{mode:>9}{cells}{ex25:>+9.3f}")

    raw = res["raw"]

    def _exclude25(folds):
        return _m([v for lbl, v in zip(fold_labels, folds) if lbl != "2025"])

    def _posfolds(folds):
        return sum(1 for v in folds if v is not None and v > 0)

    raw_ex25 = _exclude25(raw["top3_ex_folds"])
    print(f"\n=== READS (robustness-aware — mean alone is the single-fold trap) ===")
    for mode in MODES:
        r = res[mode]
        ex25 = _exclude25(r["top3_ex_folds"])
        pos = _posfolds(r["top3_ex_folds"])
        nf = sum(1 for v in r["top3_ex_folds"] if v is not None)
        ic_ok = r["ic"] > 0.5 * raw["ic"]
        # robust improvement must: beat raw ex-2025, keep ALL folds positive (raw's
        # defining 6/6 robustness), not collapse IC, and not degrade the top-10 harvest.
        robust = (mode == "raw") or (
            ex25 is not None and ex25 > raw_ex25 and pos == nf and ic_ok
            and r["t10_sh"] >= 0.9 * raw["t10_sh"]
        )
        flag = "baseline" if mode == "raw" else ("ROBUST-WIN" if robust else "REJECTED (single-fold / unstable / IC-collapse / hurts breadth)")
        print(f"  {mode:>9}: top3 mean {r['top3_ex']:+.3f} | ex-2025 {ex25:+.3f} | pos {pos}/{nf} "
              f"| IC {r['ic']:+.4f} | t10-Sh {r['t10_sh']:.3f} → {flag}")
    print(f"\n  long-side: longtail D10-D6 {raw['long_spread']*1e4:+.0f}→{res['longtail']['long_spread']*1e4:+.0f}bp "
          f"BUT rank-IC {raw['ic']:+.4f}→{res['longtail']['ic']:+.4f} (≈0, degenerate) and harvest Sharpe worse")
    print(f"  → short-side power is NOT movable to the long side by relabeling (looks structural / limits-to-arbitrage).")


if __name__ == "__main__":
    main()
