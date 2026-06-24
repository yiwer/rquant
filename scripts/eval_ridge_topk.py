"""E4.1: top-K risk-weighted portfolio construction for ridge-on-gauss.

The deployed candidate selects top-3 EQUAL weight — a 3-name punt whose Sharpe
(0.68) is carried by concentration variance. This script tests whether spreading
the SAME signal across top-K (10/15/20) with inverse-volatility weighting buys a
higher Sharpe / lower drawdown (intra-strategy diversification = the free lunch the
"岭值双引擎" blend found at the strategy level). Pure construction change — no signal
re-fit, so low overfitting risk.

Reuses vetted fit_ridge / select_delta_ridge / _eligible / norm_gauss /
to_index_relative; only the position-sizing inside the backtest loop changes.
Invariant: top_n=3 + scheme="equal" == eval_ridge.backtest_ridge bit-for-bit.
"""
import sys
import os

sys.stdout.reconfigure(encoding="utf-8")
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import numpy as np
import pandas as pd

import eval_ridge as er
import iterate as it
import train_nonlinear as tn
from build_factor_matrix import FACTOR_COLS
from test_norm_hysteresis import norm_gauss

FOLDS = [
    ("2018-01-02", "2019-12-31", "2020-01-02", "2020-12-31"),
    ("2018-01-02", "2020-12-31", "2021-01-02", "2021-12-31"),
] + list(tn.WFO_FOLDS)

TOP_N_GRID = [3, 10, 15, 20]
SCHEMES = ["equal", "invvol"]
VOL_COL = "f_vol20"


def inv_vol_weights(vols):
    """Normalized inverse-volatility weights (diagonal risk parity).

    Invalid vols (NaN / ≤0) are filled with the median of the valid ones; if none
    are valid, falls back to equal weights. Returns weights summing to 1.
    """
    v = np.asarray(vols, float).copy()
    valid = np.isfinite(v) & (v > 0)
    if not valid.any():
        n = len(v)
        return np.full(n, 1.0 / n)
    v[~valid] = np.median(v[valid])
    inv = 1.0 / v
    return inv / inv.sum()


def one_sided_turnover(w_new, w_old):
    """Fraction of capital bought = Σ positive part of (w_new − w_old) over the
    union of symbols. Equals the vetted set-based turnover for equal weights
    (initial build → 1.0, full rotation → 1.0, no change → 0)."""
    syms = set(w_new) | set(w_old)
    return float(sum(max(w_new.get(s, 0.0) - w_old.get(s, 0.0), 0.0) for s in syms))


def backtest_ridge_weighted(panel, w, top_n, cost_bps, st_set, delta,
                            scheme="equal", vol_col=VOL_COL):
    """Mirror of eval_ridge.backtest_ridge with top-K + position weighting.

    scheme="equal" + top_n=3 is bit-for-bit identical to backtest_ridge.
    scheme="invvol" weights picks by inverse f_vol20 (diagonal risk parity).
    """
    panel = panel.sort_values(["date", "symbol"])
    nav = 1.0
    prev_w = {}
    navs = []
    period_rets = []
    total_turn = 0.0

    TRAIN = ("train", "2018-01-02", "2023-12-29")
    OOS = ("2024-26_OOS", "2024-01-02", "2026-06-30")

    dates = sorted(panel["date"].unique())

    for d in dates:
        g = er._eligible(panel[panel["date"] == d].dropna(subset=["fwd_ret_5d"]), st_set)
        if len(g) < top_n:
            continue

        G = norm_gauss(g[FACTOR_COLS].to_numpy(float))
        score = G @ np.asarray(w, float)

        if delta > 0.0 and prev_w:
            is_incumbent = g["symbol"].isin(set(prev_w)).to_numpy()
            score = score + delta * is_incumbent.astype(float)

        gi = g.assign(_score=score).sort_values(["_score", "symbol"], ascending=[False, True])
        pick = gi.head(top_n)
        syms = list(pick["symbol"])
        frs = pick["fwd_ret_5d"].to_numpy(float)

        if scheme == "invvol":
            wts = inv_vol_weights(pick[vol_col].to_numpy(float))
        else:
            wts = np.full(len(syms), 1.0 / len(syms))

        ret = float(np.sum(wts * frs))
        cur_w = {s: float(x) for s, x in zip(syms, wts)}

        turn = one_sided_turnover(cur_w, prev_w)
        total_turn += turn
        ret_net = ret - cost_bps / 1e4 * turn
        period_rets.append(ret_net)
        nav *= (1.0 + ret_net)
        navs.append({"t": d, "nav": nav, "picks": syms})
        prev_w = cur_w

    total = navs[-1]["nav"] - 1.0 if navs else 0.0
    peak = -1e9
    mdd = 0.0
    for h in navs:
        peak = max(peak, h["nav"])
        mdd = max(mdd, 1.0 - h["nav"] / peak)

    pr = np.array(period_rets)
    sharpe = float(np.mean(pr) / np.std(pr) * np.sqrt(48)) if len(pr) > 1 and np.std(pr) > 0 else None

    return {
        "holdings": navs,
        "regime_slices": [{"label": L, "from": a, "to": b} for L, a, b in [TRAIN, OOS]],
        "risk": {"sharpe": sharpe},
        "total_return": total,
        "max_drawdown": mdd,
        "turnover": total_turn,
        "n_rebalances": len(navs),
        "excess_return": 0.0,
    }


# ---------------------------------------------------------------------------
# Main orchestration — sweep top_n × scheme, report Sharpe / drawdown / excess
# ---------------------------------------------------------------------------

def _mean(xs):
    vals = [x for x in xs if x is not None]
    return float(np.mean(vals)) if vals else None


def main():
    st_set = set(pd.read_csv(er.ST_PATH)["symbol"]) if os.path.exists(er.ST_PATH) else set()
    idx_data = it.load_index("csi300")
    panel = pd.read_csv(er.PANEL_MEMBERSHIP, dtype={"symbol": str})
    print(f"Panel: {len(panel)} rows, dates {panel['date'].min()}..{panel['date'].max()}; ST {len(st_set)}")

    combos = [(n, s) for n in TOP_N_GRID for s in SCHEMES]
    agg = {c: {"ex": [], "sh": [], "dd": [], "turn": []} for c in combos}

    for fold in FOLDS:
        tl, th, ol, oh = fold
        oos = panel[(panel["date"] >= ol) & (panel["date"] <= oh)].copy()
        if len(oos) == 0:
            continue
        w, _n = er.fit_ridge(panel, tl, th)
        delta = er.select_delta_ridge(panel, fold, w, st_set)
        for (n, s) in combos:
            rep = backtest_ridge_weighted(
                oos, w, top_n=n, cost_bps=it.COST, st_set=st_set, delta=delta, scheme=s,
            )
            rel = it.to_index_relative(rep, idx_data[0], idx_data[1])
            agg[(n, s)]["ex"].append(rel["excess_return"] if rel else None)
            agg[(n, s)]["sh"].append(rep["risk"]["sharpe"])
            agg[(n, s)]["dd"].append(rep["max_drawdown"])
            agg[(n, s)]["turn"].append(rep["turnover"] / max(rep["n_rebalances"], 1))

    print(f"\n{'='*72}")
    print("top-K × weighting — 6-fold OOS (vs csi300). Baseline = top3/equal (gauntlet ①).")
    print(f"{'='*72}")
    print(f"{'topN':>5}{'scheme':>9}{'meanEx':>9}{'pos':>6}{'meanSharpe':>12}{'meanMaxDD':>11}{'turn/reb':>10}")
    base_ex = _mean(agg[(3, "equal")]["ex"])
    base_sh = _mean(agg[(3, "equal")]["sh"])
    base_dd = _mean(agg[(3, "equal")]["dd"])
    for (n, s) in combos:
        a = agg[(n, s)]
        ex = _mean(a["ex"])
        sh = _mean(a["sh"])
        dd = _mean(a["dd"])
        turn = _mean(a["turn"])
        pos = sum(1 for v in a["ex"] if v is not None and v > 0)
        nf = sum(1 for v in a["ex"] if v is not None)
        tag = "  <- baseline" if (n, s) == (3, "equal") else ""
        print(f"{n:>5}{s:>9}{ex:>+9.3f}{f'{pos}/{nf}':>6}{sh:>12.3f}{dd:>11.3f}{turn:>10.3f}{tag}")

    print(f"\nBaseline top3/equal: meanEx={base_ex:+.3f}  Sharpe={base_sh:.3f}  maxDD={base_dd:.3f}")
    print("Read: higher Sharpe + lower maxDD at modest meanEx cost = diversification works (E4.1).")

    # Per-fold excess distribution (single-fold-artifact check — the project's iron law)
    fold_labels = [f[2][:4] for f in FOLDS]
    print(f"\n{'='*72}\nPer-fold OOS excess (single-fold check)\n{'='*72}")
    print(f"{'topN/scheme':>14}" + "".join(f"{lbl:>9}" for lbl in fold_labels))
    for (n, s) in combos:
        cells = "".join(f"{(v if v is not None else float('nan')):>+9.3f}" for v in agg[(n, s)]["ex"])
        print(f"{f'{n}/{s}':>14}{cells}")


if __name__ == "__main__":
    main()
