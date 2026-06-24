"""Rank-bucket return profile of the ridge-on-gauss signal — diagnosing WHY
top-10 beats top-3 (objective-portfolio mismatch).

The training objective is a pooled cross-sectional ridge on CENTERED RANKS of
forward return: it minimises average ordering error over the WHOLE cross-section
(rank-IC maximisation), with no special calibration of the extreme right tail.
Hypothesis: the signal is monotone across deciles (real, broad) but flat/anti-
selected at the extreme top (rank 1-3 ≤ 4-10) → top-3 has no edge over 4-10, so a
broad harvest (top-10) dominates a concentrated one (top-3).

This is a pure SIGNAL-QUALITY analysis (no hysteresis / holding / cost): per OOS
rebalance date, score the eligible names with the fold's ridge w, demean returns
cross-sectionally (excess vs the equal-weight eligible universe), and profile mean
excess by score decile and by fine top-rank band. Pooled across the 6 WFO folds.

Reuses vetted fit_ridge / _eligible / norm_gauss; control: pooled OOS rank-IC ≈ 0.066.
"""
import sys
import os

sys.stdout.reconfigure(encoding="utf-8")
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import numpy as np
import pandas as pd

import eval_ridge as er
import factor_lib as fl
import train_nonlinear as tn
from build_factor_matrix import FACTOR_COLS
from test_norm_hysteresis import norm_gauss

FOLDS = [
    ("2018-01-02", "2019-12-31", "2020-01-02", "2020-12-31"),
    ("2018-01-02", "2020-12-31", "2021-01-02", "2021-12-31"),
] + list(tn.WFO_FOLDS)

N_BUCKETS = 10
TOP_BANDS = [(1, 3), (4, 10), (11, 20), (21, 50)]


def decile_means(scores, rets, n_buckets=N_BUCKETS):
    """Mean ret per score bucket, ascending (bucket 0 = lowest scores).

    Contiguous split with the remainder in the FRONT buckets (np.array_split:
    larger chunks first), mirroring the repo's factor-layer convention.
    """
    s = np.asarray(scores, float)
    r = np.asarray(rets, float)
    order = np.argsort(s, kind="mergesort")
    r_sorted = r[order]
    chunks = np.array_split(r_sorted, n_buckets)
    return np.array([float(np.mean(c)) if len(c) else float("nan") for c in chunks])


def top_rank_means(scores, rets, ranges):
    """Mean ret over 1-based rank ranges, rank 1 = highest score. Clips to n."""
    s = np.asarray(scores, float)
    r = np.asarray(rets, float)
    order = np.argsort(-s, kind="mergesort")
    r_ranked = r[order]
    n = len(r_ranked)
    out = []
    for lo, hi in ranges:
        seg = r_ranked[lo - 1:min(hi, n)]
        out.append(float(np.mean(seg)) if len(seg) else float("nan"))
    return out


def main():
    st_set = set(pd.read_csv(er.ST_PATH)["symbol"]) if os.path.exists(er.ST_PATH) else set()
    panel = pd.read_csv(er.PANEL_MEMBERSHIP, dtype={"symbol": str})
    print(f"Panel: {len(panel)} rows, dates {panel['date'].min()}..{panel['date'].max()}; ST {len(st_set)}")

    decile_acc = []     # one row of N_BUCKETS per (fold,date)
    band_acc = []       # one row of len(TOP_BANDS) per (fold,date)
    ic_acc = []

    for fold in FOLDS:
        tl, th, ol, oh = fold
        oos = panel[(panel["date"] >= ol) & (panel["date"] <= oh)].copy()
        if len(oos) == 0:
            continue
        w, _ = er.fit_ridge(panel, tl, th)
        for d, g in oos.groupby("date"):
            g = er._eligible(g.dropna(subset=["fwd_ret_5d"]), st_set)
            if len(g) < N_BUCKETS:
                continue
            score = norm_gauss(g[FACTOR_COLS].to_numpy(float)) @ w
            fwd = g["fwd_ret_5d"].to_numpy(float)
            xs_excess = fwd - fwd.mean()        # vs equal-weight eligible universe
            decile_acc.append(decile_means(score, xs_excess, N_BUCKETS))
            band_acc.append(top_rank_means(score, xs_excess, TOP_BANDS))
            ic_acc.append(fl.rank_ic(score, fwd))

    decile_mean = np.nanmean(np.array(decile_acc), axis=0)
    band_mean = np.nanmean(np.array(band_acc), axis=0)
    pooled_ic = float(np.nanmean(ic_acc))
    n_dates = len(decile_acc)

    print(f"\n{'='*64}\nSignal decile profile — mean 5d cross-sectional excess (per name)\n{'='*64}")
    print(f"pooled over {n_dates} OOS rebalance dates; D1=lowest score, D10=highest")
    for i, m in enumerate(decile_mean):
        bar = "#" * max(0, int(round(m * 2000)))   # 1 char ≈ 5bp
        print(f"  D{i+1:>2}: {m:+.4f}  {bar}")
    spread = decile_mean[-1] - decile_mean[0]
    mono = all(decile_mean[i] <= decile_mean[i + 1] for i in range(len(decile_mean) - 1))
    print(f"  D10-D1 spread = {spread:+.4f} (per 5d) | strictly monotone across deciles: {mono}")

    print(f"\n{'='*64}\nWithin-TOP fine bands — does the extreme tail add return?\n{'='*64}")
    for (lo, hi), m in zip(TOP_BANDS, band_mean):
        print(f"  rank {lo:>2}-{hi:<2}: mean 5d excess {m:+.4f}")
    t3, n410 = band_mean[0], band_mean[1]
    print(f"\n  top-3 ({t3:+.4f}) {'≤' if t3 <= n410 else '>'} ranks 4-10 ({n410:+.4f}) "
          f"→ {'NO edge over 4-10 (tail has no skill)' if t3 <= n410 else 'top-3 still leads'}")

    print(f"\nCONTROL: pooled OOS rank-IC = {pooled_ic:+.4f} (expect ≈ +0.066).")
    print("READ: monotone deciles = real broad signal; flat/inverted extreme tail = objective")
    print("      fit broad order not the tail → breadth (top-10) harvests it, top-3 wastes it.")


if __name__ == "__main__":
    main()
