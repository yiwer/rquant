# scripts/eval_ridge.py
"""Authoritative §5.3 verdict for multivariate ridge-on-gauss factor composite.

HARNESS: directly reuses eval_nonlinear's backtest_hysteresis / _eligible /
select_delta / cost / §5.3 judge / double-pool (membership + full) and WFO folds.
The ONLY change vs eval_nonlinear is the *scoring*: norm_gauss(factor_matrix) @ w
instead of expand_features(rank_columns(...)) @ w.

VALIDATION CONTROL: equal-weight run through THIS harness must reproduce eval_nonlinear's
published equal-weight membership mean (~+0.042).  Printed side by side for comparison.

EMBARGO VARIANT: also run with the last 4 weeks of each TRAIN window dropped
(kills label-boundary overlap between train end and OOS start).

VERDICT: does +35-40% (train_dropout_ensemble inline harness) survive the vetted harness?
"""
import sys
import os
sys.stdout.reconfigure(encoding="utf-8")
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import numpy as np
import pandas as pd

import factor_lib as fl
from build_factor_matrix import FACTOR_COLS, OUT as PANEL_MEMBERSHIP, OUT_DIR
import iterate as it
import train_nonlinear as tn

# Import norm_gauss from test_norm_hysteresis (the vetted implementation)
from test_norm_hysteresis import norm_gauss

# ---------------------------------------------------------------------------
# Paths
# ---------------------------------------------------------------------------

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ST_PATH = os.path.join(REPO, "data", "baostock", "st_symbols.csv")
PANEL_FULL = os.path.join(OUT_DIR, "factors_full.csv")

# ---------------------------------------------------------------------------
# Constants (identical to eval_nonlinear)
# ---------------------------------------------------------------------------

DELTA_GRID = [0.0, 0.02, 0.05, 0.1]
TOP_N = 3
LIQ_FLOOR_LOG = float(np.log(5e7))
RIDGE_A = 0.10          # λ = RIDGE_A * mean(diag(Gram))
EMBARGO_WEEKS = 4       # drop last N weeks of TRAIN before fitting (embargo variant)


# ---------------------------------------------------------------------------
# Hard gate — verbatim copy from eval_nonlinear._eligible
# ---------------------------------------------------------------------------

def _eligible(g, st_set):
    """Hard gate: non-ST ∧ roe>0 ∧ f_bm>0 ∧ liquidity≥floor."""
    ok = (~g["symbol"].isin(st_set)) & (g["f_roe"] > 0) & (g["f_bm"] > 0) & (g["f_logamt"] >= LIQ_FLOOR_LOG)
    return g[ok]


# ---------------------------------------------------------------------------
# Ridge-on-gauss fit (train-only, no leak)
# ---------------------------------------------------------------------------

def fit_ridge(panel, date_lo, date_hi):
    """Accumulate Gram = Σ GᵀG and b = Σ Gᵀy over TRAIN weeks, then solve ridge.

    G = norm_gauss(factor_matrix_of_that_date)
    y = cross_sectional_rank(fwd_ret_5d) − 0.5   (centred rank)

    λ = RIDGE_A * mean(diag(Gram))
    w clipped to ±(90th pct of |w|)

    Returns:
        w: ndarray of shape (n_factors,)
        n_train_dates: int
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
        G = norm_gauss(g[FACTOR_COLS].to_numpy(float))          # shape (n, p)
        y = fl.cross_sectional_rank(g["fwd_ret_5d"].to_numpy(float)) - 0.5  # centred rank
        Gram += G.T @ G
        b += G.T @ y
        n_train_dates += 1

    if n_train_dates == 0:
        return np.zeros(p), 0

    lam = RIDGE_A * np.mean(np.diag(Gram))
    A = Gram + lam * np.eye(p)
    w = np.linalg.solve(A, b)
    # Clip to ±90th pct of |w|
    q = np.percentile(np.abs(w), 90) + 1e-12
    w = np.clip(w, -q, q)
    return w, n_train_dates


def fit_ridge_with_embargo(panel, date_lo, date_hi, embargo_weeks=EMBARGO_WEEKS):
    """Same as fit_ridge but drop the last embargo_weeks rebalance dates from TRAIN.

    Identifies weekly rebalance dates in [date_lo, date_hi], then truncates
    at len(dates) - embargo_weeks.
    """
    sub = panel[(panel["date"] >= date_lo) & (panel["date"] <= date_hi)].copy()
    rebal_dates = sorted(sub["date"].unique())
    if len(rebal_dates) <= embargo_weeks:
        # Can't embargo — use all data
        return fit_ridge(panel, date_lo, date_hi)
    cutoff_date = rebal_dates[-(embargo_weeks + 1)]  # inclusive upper bound
    return fit_ridge(panel, date_lo, cutoff_date)


# ---------------------------------------------------------------------------
# Backtest — verbatim structure from eval_nonlinear.backtest_hysteresis
# but scores via norm_gauss(factor_matrix) @ w instead of expand_fn(rank) @ w
# ---------------------------------------------------------------------------

def backtest_ridge(panel, w, top_n, cost_bps, st_set, delta):
    """Weekly backtest with ridge-on-gauss scoring + hysteresis.

    Mirrors eval_nonlinear.backtest_hysteresis exactly:
      - same _eligible gate
      - same hysteresis mechanics
      - same regime slices (TRAIN 2018/OOS 2024)
      - same NAV/return/drawdown accounting
      - same excess_return placeholder (0.0; caller converts to index-relative)

    ONLY difference: score = norm_gauss(factor_matrix) @ w
    """
    panel = panel.sort_values(["date", "symbol"])
    nav = 1.0
    prev = set()
    navs = []
    period_rets = []
    total_turn = 0.0

    TRAIN = ("train", "2018-01-02", "2023-12-29")
    OOS = ("2024-26_OOS", "2024-01-02", "2026-06-30")

    dates = sorted(panel["date"].unique())

    for d in dates:
        g = _eligible(panel[panel["date"] == d].dropna(subset=["fwd_ret_5d"]), st_set)
        if len(g) < top_n:
            continue

        # Score: gauss-normalise raw factor matrix, then dot with w
        G = norm_gauss(g[FACTOR_COLS].to_numpy(float))
        score = G @ np.asarray(w, float)

        # Hysteresis: incumbents get +delta score boost
        if delta > 0.0 and prev:
            is_incumbent = g["symbol"].isin(prev).to_numpy()
            score = score + delta * is_incumbent.astype(float)

        gi = g.assign(_score=score).sort_values(["_score", "symbol"], ascending=[False, True])
        pick = gi.head(top_n)
        ret = float(pick["fwd_ret_5d"].mean())
        cur = set(pick["symbol"])

        turn = len(cur ^ prev) / max(len(cur) + len(prev), 1)
        total_turn += turn
        ret_net = ret - cost_bps / 1e4 * turn
        period_rets.append(ret_net)
        nav *= (1.0 + ret_net)
        navs.append({"t": d, "nav": nav, "picks": list(cur)})
        prev = cur

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
# Delta selection (train only — no lookahead)
# Mirrors eval_nonlinear.select_delta but uses backtest_ridge
# ---------------------------------------------------------------------------

def select_delta_ridge(panel, fold, w, st_set):
    """Choose hysteresis delta on TRAIN slice of fold using ridge scoring.

    No OOS peek.  Grid: {0, 0.02, 0.05, 0.1}.
    """
    train_lo, train_hi, _oos_lo, _oos_hi = fold
    train_panel = panel[(panel["date"] >= train_lo) & (panel["date"] <= train_hi)].copy()

    best_delta = 0.0
    best_net = -np.inf

    for d in DELTA_GRID:
        rep = backtest_ridge(train_panel, w, top_n=TOP_N, cost_bps=it.COST, st_set=st_set, delta=d)
        if rep["total_return"] > best_net:
            best_net = rep["total_return"]
            best_delta = d

    return best_delta


# ---------------------------------------------------------------------------
# Rank-based backtest for validation control
# Mirrors eval_nonlinear.backtest_hysteresis verbatim (rank_columns + linear score)
# Used ONLY for the harness validation control, not for ridge evaluation
# ---------------------------------------------------------------------------

def backtest_rank_linear(panel, w, top_n, cost_bps, st_set, delta):
    """Verbatim clone of eval_nonlinear.backtest_hysteresis with expand_fn = identity.

    Scores via rank_columns(factor_matrix) @ w.
    Used only for harness validation control (must reproduce eval_nonlinear ~+0.042).
    """
    panel = panel.sort_values(["date", "symbol"])
    nav = 1.0
    prev = set()
    navs = []
    period_rets = []
    total_turn = 0.0

    TRAIN = ("train", "2018-01-02", "2023-12-29")
    OOS = ("2024-26_OOS", "2024-01-02", "2026-06-30")

    dates = sorted(panel["date"].unique())

    for d in dates:
        g = _eligible(panel[panel["date"] == d].dropna(subset=["fwd_ret_5d"]), st_set)
        if len(g) < top_n:
            continue

        # Score: rank-normalize columns then linear dot — identical to eval_nonlinear
        Xrank = fl.rank_columns(g[FACTOR_COLS].to_numpy(float))
        score = Xrank @ np.asarray(w, float)

        if delta > 0.0 and prev:
            is_incumbent = g["symbol"].isin(prev).to_numpy()
            score = score + delta * is_incumbent.astype(float)

        gi = g.assign(_score=score).sort_values(["_score", "symbol"], ascending=[False, True])
        pick = gi.head(top_n)
        ret = float(pick["fwd_ret_5d"].mean())
        cur = set(pick["symbol"])

        turn = len(cur ^ prev) / max(len(cur) + len(prev), 1)
        total_turn += turn
        ret_net = ret - cost_bps / 1e4 * turn
        period_rets.append(ret_net)
        nav *= (1.0 + ret_net)
        navs.append({"t": d, "nav": nav, "picks": list(cur)})
        prev = cur

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
# Per-fold evaluation (ridge + embargo + equal-weight)
# ---------------------------------------------------------------------------

def _eval_fold_oos(panel, fold, st_set, idx_data, embargo=False):
    """Run one WFO fold: fit ridge → select delta → OOS backtest.

    Returns dict: fold, ridge_oos, eq_oos, delta, n_oos_dates
    """
    train_lo, train_hi, oos_lo, oos_hi = fold
    oos_panel = panel[(panel["date"] >= oos_lo) & (panel["date"] <= oos_hi)].copy()

    if len(oos_panel) == 0:
        return {"fold": f"{oos_lo}..{oos_hi}", "ridge_oos": None, "eq_oos": None}

    # --- Fit ridge weights on TRAIN slice (no OOS data) ---
    if embargo:
        w_ridge, n_train = fit_ridge_with_embargo(panel, train_lo, train_hi)
    else:
        w_ridge, n_train = fit_ridge(panel, train_lo, train_hi)

    # --- Select delta on TRAIN slice ---
    delta = select_delta_ridge(panel, fold, w_ridge, st_set)

    # --- OOS backtest: ridge-on-gauss with hysteresis ---
    rep_ridge = backtest_ridge(
        oos_panel, w_ridge, top_n=TOP_N, cost_bps=it.COST, st_set=st_set, delta=delta
    )

    # --- OOS backtest: equal-weight baseline ---
    # Mirror eval_nonlinear exactly: w_eq[0]=1 (f_bm), w_eq[1]=1 (f_npyoy), scored via rank
    p = len(FACTOR_COLS)
    w_eq = np.zeros(p)
    w_eq[0] = 1.0   # f_bm
    w_eq[1] = 1.0   # f_npyoy
    rep_eq = backtest_rank_linear(
        oos_panel, w_eq, top_n=TOP_N, cost_bps=it.COST, st_set=st_set, delta=0.0
    )

    # --- Convert to index-relative (vs csi300) ---
    idx_m, idx_dates = idx_data
    rel_ridge = it.to_index_relative(rep_ridge, idx_m, idx_dates)
    rel_eq = it.to_index_relative(rep_eq, idx_m, idx_dates)

    ridge_oos = rel_ridge["excess_return"] if rel_ridge else None
    eq_oos = rel_eq["excess_return"] if rel_eq else None

    return {
        "fold": f"{oos_lo}..{oos_hi}",
        "ridge_oos": ridge_oos,
        "eq_oos": eq_oos,
        "delta": delta,
        "n_oos_dates": rep_ridge["n_rebalances"],
        "n_train_dates": n_train,
    }


# ---------------------------------------------------------------------------
# Per-panel aggregation
# ---------------------------------------------------------------------------

def _eval_panel(panel_path, panel_label, st_set, idx_data):
    """Run all WFO folds on one panel.  Returns aggregate stats dict."""
    print(f"\n{'='*60}")
    print(f"Panel: {panel_label}  ({panel_path})")
    print(f"{'='*60}")

    panel = pd.read_csv(panel_path, dtype={"symbol": str})
    print(f"  Loaded {len(panel)} rows, dates {panel['date'].min()}..{panel['date'].max()}")

    fold_results_plain = []
    fold_results_embargo = []

    for fold in tn.WFO_FOLDS:
        train_lo, train_hi, oos_lo, oos_hi = fold
        print(f"\n  [fold] train={train_lo}..{train_hi}  OOS={oos_lo}..{oos_hi}")

        # --- Plain ridge (no embargo) ---
        fr_plain = _eval_fold_oos(panel, fold, st_set, idx_data, embargo=False)
        fold_results_plain.append(fr_plain)
        r = fr_plain["ridge_oos"]
        e = fr_plain["eq_oos"]
        d = fr_plain.get("delta", "?")
        print(f"    [plain]   delta={d:.2f}  ridge_oos={r:+.4f}" if r is not None else "    [plain] ridge_oos=None", end="")
        print(f"  eq_oos={e:+.4f}" if e is not None else "  eq_oos=None")

        # --- Embargo ridge ---
        fr_emb = _eval_fold_oos(panel, fold, st_set, idx_data, embargo=True)
        fold_results_embargo.append(fr_emb)
        r2 = fr_emb["ridge_oos"]
        e2 = fr_emb["eq_oos"]
        d2 = fr_emb.get("delta", "?")
        print(f"    [embargo] delta={d2:.2f}  ridge_oos={r2:+.4f}" if r2 is not None else "    [embargo] ridge_oos=None", end="")
        print(f"  eq_oos={e2:+.4f}" if e2 is not None else "  eq_oos=None")

    def aggregate(fold_results, tag):
        ridge_vals = [f["ridge_oos"] for f in fold_results if f["ridge_oos"] is not None]
        eq_vals = [f["eq_oos"] for f in fold_results if f["eq_oos"] is not None]
        ridge_mean = float(np.mean(ridge_vals)) if ridge_vals else None
        eq_mean = float(np.mean(eq_vals)) if eq_vals else None
        ridge_pos = sum(1 for v in ridge_vals if v > 0)
        n_folds = len(ridge_vals)

        print(f"\n  --- Aggregate [{tag}] ({panel_label}) ---")
        if ridge_mean is not None:
            print(f"  Ridge-on-gauss: mean OOS excess={ridge_mean:+.4f}  positive_folds={ridge_pos}/{n_folds}")
        else:
            print("  Ridge-on-gauss: no valid folds")
        if eq_mean is not None:
            print(f"  Equal-weight:   mean OOS excess={eq_mean:+.4f}")
        else:
            print("  Equal-weight: no valid folds")

        verdict_ridge = (ridge_mean is not None and ridge_mean > 0 and ridge_pos > n_folds / 2)
        beats_eq = (ridge_mean is not None and eq_mean is not None and ridge_mean > eq_mean)
        print(f"  §5.3-positive (ridge): {verdict_ridge}")
        print(f"  Ridge BEATS equal-weight: {beats_eq}")

        return {
            "tag": tag,
            "panel": panel_label,
            "folds": fold_results,
            "ridge_mean_oos": ridge_mean,
            "eq_mean_oos": eq_mean,
            "ridge_pos_folds": ridge_pos,
            "n_folds": n_folds,
            "verdict_ridge_53": verdict_ridge,
            "ridge_beats_eq": beats_eq,
        }

    r_plain = aggregate(fold_results_plain, "plain")
    r_emb = aggregate(fold_results_embargo, "embargo-4wk")
    return r_plain, r_emb


# ---------------------------------------------------------------------------
# Main orchestration
# ---------------------------------------------------------------------------

def main():
    # Load ST exclusion set
    st_set = set(pd.read_csv(ST_PATH)["symbol"]) if os.path.exists(ST_PATH) else set()
    print(f"ST set: {len(st_set)} symbols")

    # Load CSI 300 index for OOS excess computation
    idx_data = it.load_index("csi300")

    results_plain = []
    results_emb = []

    # --- Membership panel ---
    if os.path.exists(PANEL_MEMBERSHIP):
        rp, re = _eval_panel(PANEL_MEMBERSHIP, "membership", st_set, idx_data)
        results_plain.append(rp)
        results_emb.append(re)
    else:
        print(f"[WARN] membership panel not found: {PANEL_MEMBERSHIP}")

    # --- Full (wide) panel ---
    if os.path.exists(PANEL_FULL):
        rp, re = _eval_panel(PANEL_FULL, "full (wide)", st_set, idx_data)
        results_plain.append(rp)
        results_emb.append(re)
    else:
        print(f"[WARN] full panel not found: {PANEL_FULL}")

    # ---------------------------------------------------------------------------
    # HARNESS VALIDATION CONTROL
    # Equal-weight uses rank_columns scoring (w_eq[0]=1, w_eq[1]=1), exactly as
    # eval_nonlinear does.  The membership mean should match ~+0.042.
    # If it does not, the harness reuse is wrong.
    # ---------------------------------------------------------------------------
    print(f"\n{'='*60}")
    print("=== HARNESS VALIDATION CONTROL ===")
    print(f"{'='*60}")
    print("Equal-weight membership OOS excess (this harness, rank-based w_eq[0,1]=1)")
    print("  should match eval_nonlinear published value ~+0.042:")
    for r in results_plain:
        if r["panel"] == "membership":
            eq = r["eq_mean_oos"]
            fold_eqs = [f["eq_oos"] for f in r["folds"] if f.get("eq_oos") is not None]
            print(f"  per-fold eq_oos: {[f'{v:+.4f}' for v in fold_eqs]}")
            print(f"  eq_mean_oos = {eq:+.4f}" if eq is not None else "  eq_mean_oos = None")
            diff = abs(eq - 0.042) if eq is not None else None
            if diff is not None:
                status = "OK (within 0.01)" if diff < 0.01 else f"DEVIATION={diff:.4f} (>0.01)"
                print(f"  vs expected ~+0.042: {status}")
    print("  NOTE: equal-weight here uses exact eval_nonlinear scoring (rank, w_bm=w_npyoy=1)")
    print("  If eval_nonlinear was run on a different panel date range, numbers may differ.")

    # ---------------------------------------------------------------------------
    # Final verdict
    # ---------------------------------------------------------------------------
    print(f"\n{'='*60}")
    print("=== FINAL VERDICT ===")
    print(f"{'='*60}")

    print("\n[Plain ridge — no embargo]")
    for r in results_plain:
        lbl = r["panel"]
        rm = r["ridge_mean_oos"]
        em = r["eq_mean_oos"]
        beats = r["ridge_beats_eq"]
        pos = r["verdict_ridge_53"]
        if rm is not None:
            print(f"  [{lbl}]  ridge_mean={rm:+.4f}  eq_mean={em:+.4f}  "
                  f"beats_eq={beats}  §5.3-pos={pos}")
        else:
            print(f"  [{lbl}]  no data")

    print("\n[Embargo ridge — last 4 TRAIN weeks dropped]")
    for r in results_emb:
        lbl = r["panel"]
        rm = r["ridge_mean_oos"]
        em = r["eq_mean_oos"]
        beats = r["ridge_beats_eq"]
        pos = r["verdict_ridge_53"]
        if rm is not None:
            print(f"  [{lbl}]  ridge_mean={rm:+.4f}  eq_mean={em:+.4f}  "
                  f"beats_eq={beats}  §5.3-pos={pos}")
        else:
            print(f"  [{lbl}]  no data")

    # Summary of ridge vs equal-weight across all panels/variants
    print(f"\n{'='*60}")
    print("=== SUMMARY (ridge vs equal-weight, both pools) ===")
    print(f"{'='*60}")
    for tag, results in [("plain", results_plain), ("embargo", results_emb)]:
        for r in results:
            rm = r["ridge_mean_oos"]
            em = r["eq_mean_oos"]
            n = r["n_folds"]
            pos = r["ridge_pos_folds"]
            lbl = r["panel"]
            if rm is not None:
                inline_claim = 0.35  # train_dropout_ensemble reported ~+35%
                survives = rm > 0 and rm > (em if em else 0)
                print(f"  [{tag}][{lbl}] ridge={rm:+.4f}  eq={em:+.4f}  "
                      f"pos={pos}/{n}  §5.3={r['verdict_ridge_53']}  "
                      f"survives-inline-claim={'YES' if survives else 'NO'}")

    # One-line verdict
    any_membership_positive = any(
        r["verdict_ridge_53"] for r in results_plain if r["panel"] == "membership"
    )
    any_full_positive = any(
        r["verdict_ridge_53"] for r in results_plain if r["panel"] != "membership"
    )
    ridge_beats_eq_all = all(r["ridge_beats_eq"] for r in results_plain if r["ridge_mean_oos"] is not None)

    print(f"\n{'='*60}")
    print("=== ONE-LINE VERDICT ===")
    print(f"{'='*60}")
    if any_membership_positive or any_full_positive:
        print("VERDICT: ridge-on-gauss is real-and-survives the vetted harness "
              f"(§5.3-positive: membership={any_membership_positive}, full={any_full_positive}, "
              f"beats-eq-all-panels={ridge_beats_eq_all})")
    else:
        print("VERDICT: inline-harness-was-inflating — ridge-on-gauss does NOT survive "
              "the vetted §5.3 harness (no panel is §5.3-positive in OOS)")

    return results_plain, results_emb


if __name__ == "__main__":
    main()
