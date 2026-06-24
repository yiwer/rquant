"""Orthogonal incremental-IC gate for candidate factors.

Before adding ANY factor to the 72-factor panel + re-running the ridge gauntlet,
test whether it carries information ORTHOGONAL to the existing 72. For each OOS
rebalance date:
  (a) raw rank-IC of the candidate vs fwd_ret_5d;
  (b) residualise the candidate on the gauss-normed existing 72 (cross-sectional
      OLS) and take the rank-IC of the RESIDUAL — the incremental info a LINEAR
      ridge cannot already extract.
Aggregates mean IC + ICIR(=mean/std) + t + positive-rate over the OOS span,
plus per-fold incremental mean for stability. Gate (project F-1 standard):
keep iff |incremental RankIC| > 0.03 AND |incremental ICIR| > 0.3 with a
consistent sign across folds. Cheap pre-filter — only survivors get promoted
into build_factor_matrix.py + a full gauntlet re-run.

Usage: python scripts/eval_factor_orthogonal.py            # runs the built-in batch
"""
import sys
import os
sys.stdout.reconfigure(encoding="utf-8")
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import numpy as np
import pandas as pd

import factor_lib as fl
import eval_ridge as er
from build_factor_matrix import FACTOR_COLS, FUND
from test_norm_hysteresis import norm_gauss

OOS_LO = "2020-01-02"
FOLDS = [
    ("2020", "2020-01-02", "2020-12-31"), ("2021", "2021-01-02", "2021-12-31"),
    ("2022", "2022-01-02", "2022-12-31"), ("2023", "2023-01-02", "2023-12-31"),
    ("2024", "2024-01-02", "2024-12-31"), ("2025", "2025-01-02", "2026-06-30"),
]
IC_MIN = 0.03      # |incremental RankIC| floor (F-1 standard)
ICIR_MIN = 0.30    # |incremental ICIR| floor


def _load_sue(panel):
    """SUE (standardized earnings surprise) from fundamentals eps, PIT-merged.

    Per symbol: surprise = eps_t − eps_{t−4q} (YoY change in cumulative eps),
    standardized by its own rolling std (own earnings volatility) → cross-section
    non-monotone vs f_npyoy. merge_asof backward on disclosure date (same PIT
    convention as build_factor_matrix). Returns a Series aligned to panel rows.
    """
    out = pd.Series(np.nan, index=panel.index)
    pan = panel[["symbol", "date"]].copy()
    pan["date_dt"] = pd.to_datetime(pan["date"])
    for sym, grp in pan.groupby("symbol"):
        fp = os.path.join(FUND, f"{sym}.csv")
        if not os.path.exists(fp):
            continue
        f = pd.read_csv(fp)
        if "eps" not in f.columns:
            continue
        f = f.dropna(subset=["eps"]).copy()
        if len(f) < 6:
            continue
        f["time_dt"] = pd.to_datetime(f["time"])
        f = f.sort_values("time_dt")
        eps = f["eps"].astype(float)
        surp = eps - eps.shift(4)
        sig = surp.rolling(8, min_periods=4).std()
        f["sue"] = surp / (sig + 1e-12)
        f2 = f.dropna(subset=["sue"])[["time_dt", "sue"]]
        if f2.empty:
            continue
        g = grp.sort_values("date_dt")
        m = pd.merge_asof(g, f2, left_on="date_dt", right_on="time_dt", direction="backward")
        out.loc[g.index] = m["sue"].values
    return out


def build_candidates(panel):
    """Batch A — earnings/financials-derived (products + eps-surprise)."""
    p = panel
    return {
        "gpa":      p["f_gm"] * p["f_aturn"],            # gross profitability GP/A = gm × asset_turn
        "accruals": p["f_roa"] * (1.0 - p["f_cfonp"]),   # (NI−CFO)/TA proxy = roa × (1 − CFO/NI)
        "sue":      _load_sue(p),                        # standardized earnings surprise (PEAD axis)
    }


def _load_holder_factors(panel):
    """Batch C — holder-count %-change + net issuance, PIT-merged on 公告日期.

    From data/holders/<sym>.csv (fetch_holder_shares.py). holder_chg<0 = 筹码集中
    (accumulation); netissue = YoY total-share growth (issuance → supply)."""
    hdir = os.path.join(er.REPO, "data", "holders")
    holder = pd.Series(np.nan, index=panel.index)
    issue = pd.Series(np.nan, index=panel.index)
    pan = panel[["symbol", "date"]].copy()
    pan["date_dt"] = pd.to_datetime(pan["date"])
    for sym, grp in pan.groupby("symbol"):
        fp = os.path.join(hdir, f"{sym}.csv")
        if not os.path.exists(fp):
            continue
        h = pd.read_csv(fp, parse_dates=["announce"])
        h = h.dropna(subset=["announce"]).sort_values("announce")
        if h.empty:
            continue
        h["netissue"] = h["total_share"] / h["total_share"].shift(4) - 1.0
        feats = h[["announce", "holder_pct_chg", "netissue"]]
        g = grp.sort_values("date_dt")
        m = pd.merge_asof(g, feats, left_on="date_dt", right_on="announce", direction="backward")
        holder.loc[g.index] = m["holder_pct_chg"].values
        issue.loc[g.index] = m["netissue"].values
    return {"holder_chg": holder, "netissue": issue}


CANDIDATE_BATCHES = {"A": build_candidates, "holders": _load_holder_factors}


def _fold_of(d):
    for tag, lo, hi in FOLDS:
        if lo <= d <= hi:
            return tag
    return None


def incremental_ic(panel, cand):
    """Per OOS date: raw IC + residual-on-72 IC. Returns aggregates dict."""
    df = panel[panel["date"] >= OOS_LO].copy()
    df["_cand"] = cand[df.index].values
    raw_ics, inc_ics = [], []
    fold_inc = {tag: [] for tag, _, _ in FOLDS}

    for d, g in df.groupby("date"):
        g = g.dropna(subset=["fwd_ret_5d", "_cand"])
        if len(g) < 20:
            continue
        fwd = g["fwd_ret_5d"].to_numpy(float)
        c = g["_cand"].to_numpy(float)
        if np.nanstd(c) == 0:
            continue
        raw = fl.rank_ic(c, fwd)
        # residualise candidate on gauss(72) + intercept (cross-sectional OLS)
        X = norm_gauss(g[FACTOR_COLS].to_numpy(float))     # n×72, finite
        A = np.column_stack([np.ones(len(g)), X])
        beta, *_ = np.linalg.lstsq(A, c, rcond=None)
        resid = c - A @ beta
        inc = fl.rank_ic(resid, fwd)
        if not np.isnan(raw):
            raw_ics.append(raw)
        if not np.isnan(inc):
            inc_ics.append(inc)
            ft = _fold_of(d)
            if ft:
                fold_inc[ft].append(inc)

    def agg(xs):
        a = np.array(xs)
        if len(a) == 0:
            return (np.nan, np.nan, np.nan, np.nan)
        icir = a.mean() / a.std() if a.std() > 0 else np.nan
        t = icir * np.sqrt(len(a)) if np.isfinite(icir) else np.nan
        return (a.mean(), icir, t, (a > 0).mean())

    rmean, ricir, rt, rpos = agg(raw_ics)
    imean, iicir, it, ipos = agg(inc_ics)
    fold_means = {tag: (float(np.mean(v)) if v else np.nan) for tag, v in fold_inc.items()}
    sign = np.sign(imean) if np.isfinite(imean) else 0
    consistent = sum(1 for v in fold_means.values() if np.isfinite(v) and np.sign(v) == sign)
    passed = (abs(imean) > IC_MIN and abs(iicir) > ICIR_MIN and consistent >= 4)
    return {
        "n": len(inc_ics), "raw_mean": rmean, "raw_icir": ricir,
        "inc_mean": imean, "inc_icir": iicir, "inc_t": it, "inc_pos": ipos,
        "fold_means": fold_means, "consistent_folds": consistent, "passed": passed,
    }


def main():
    import argparse
    ap = argparse.ArgumentParser()
    ap.add_argument("--batch", choices=list(CANDIDATE_BATCHES), default="A")
    args = ap.parse_args()
    panel = pd.read_csv(er.PANEL_MEMBERSHIP, dtype={"symbol": str}).reset_index(drop=True)
    cands = CANDIDATE_BATCHES[args.batch](panel)
    print(f"orthogonal incremental-IC gate (OOS {OOS_LO}+, membership; existing {len(FACTOR_COLS)} factors)")
    print(f"gate: |inc RankIC|>{IC_MIN} AND |inc ICIR|>{ICIR_MIN} AND sign-consistent ≥4/6 folds\n")
    print(f"{'factor':>12}{'rawIC':>9}{'incIC':>9}{'incICIR':>9}{'inc_t':>8}{'inc+%':>7}{'cons':>6}  verdict")
    results = {}
    for name, c in cands.items():
        r = incremental_ic(panel, c)
        results[name] = r
        print(f"{name:>12}{r['raw_mean']:>+9.4f}{r['inc_mean']:>+9.4f}{r['inc_icir']:>+9.3f}"
              f"{r['inc_t']:>+8.2f}{r['inc_pos']*100:>6.0f}%{r['consistent_folds']:>5}/6  "
              f"{'KEEP' if r['passed'] else 'drop'}")
    print(f"\nper-fold incremental IC:")
    print(f"{'factor':>12}" + "".join(f"{tag:>9}" for tag, _, _ in FOLDS))
    for name, r in results.items():
        print(f"{name:>12}" + "".join(f"{r['fold_means'][tag]:>+9.4f}" if np.isfinite(r['fold_means'][tag]) else f"{'·':>9}" for tag, _, _ in FOLDS))
    keep = [n for n, r in results.items() if r["passed"]]
    print(f"\nsurvivors → promote to panel + gauntlet: {keep if keep else '(none)'}")


if __name__ == "__main__":
    main()
