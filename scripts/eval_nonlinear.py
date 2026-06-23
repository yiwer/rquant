# scripts/eval_nonlinear.py
"""非线性因子回测器：expand_features + 成本感知迟滞 + 多折 WFO。

对 membership 面板与 full 面板各跑逐折 OOS，对照等权基线，聚合裁决。

主接口：
  backtest_hysteresis(panel, w, expand_fn, top_n, cost_bps, st_set, delta) -> report
  select_delta(panel, fold, w, expand_fn, st_set) -> float
  main()
"""
import sys
import os
sys.stdout.reconfigure(encoding="utf-8")
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import json
import numpy as np
import pandas as pd

import factor_lib as fl
from build_factor_matrix import FACTOR_COLS, OUT as PANEL_MEMBERSHIP, OUT_DIR
import iterate as it
import train_nonlinear as tn

# ---------------------------------------------------------------------------
# Paths
# ---------------------------------------------------------------------------

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ST_PATH = os.path.join(REPO, "data", "baostock", "st_symbols.csv")
PANEL_FULL = os.path.join(OUT_DIR, "factors_full.csv")
WEIGHTS_NL = os.path.join(OUT_DIR, "weights_nonlinear.json")

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

DELTA_GRID = [0.0, 0.02, 0.05, 0.1]
TOP_N = 3
LIQ_FLOOR_LOG = float(np.log(5e7))


# ---------------------------------------------------------------------------
# Hard gate (mirrors eval_linear_score._eligible)
# ---------------------------------------------------------------------------

def _eligible(g, st_set):
    """硬闸：非 ST ∧ roe>0 ∧ f_bm>0 ∧ 流动性≥地板。"""
    ok = (~g["symbol"].isin(st_set)) & (g["f_roe"] > 0) & (g["f_bm"] > 0) & (g["f_logamt"] >= LIQ_FLOOR_LOG)
    return g[ok]


# ---------------------------------------------------------------------------
# Core backtest
# ---------------------------------------------------------------------------

def backtest_hysteresis(panel, w, expand_fn, top_n, cost_bps, st_set, delta):
    """Weekly backtest with non-linear scoring + hysteresis.

    Args:
        panel:     DataFrame with columns date, symbol, FACTOR_COLS..., fwd_ret_5d, f_roe, f_logamt.
        w:         Weight vector aligned to features produced by expand_fn.
        expand_fn: Callable(Xrank: ndarray) -> ndarray of expanded features.
                   Signature matches factor_lib.expand_features partially applied with interaction_pairs.
        top_n:     Number of stocks to pick each period.
        cost_bps:  Round-trip cost in basis points (half applied each side).
        st_set:    Set of ST symbol strings to exclude.
        delta:     Hysteresis advantage added to incumbent holdings' scores before ranking.

    Returns:
        dict matching eval_linear_score.backtest return contract:
            holdings, regime_slices, risk, total_return, max_drawdown, turnover,
            n_rebalances, excess_return.
    """
    panel = panel.sort_values(["date", "symbol"])
    nav = 1.0
    prev = set()  # incumbent holdings from previous period
    navs = []
    period_rets = []
    total_turn = 0.0

    # Regime slices mirrors eval_linear_score.backtest
    TRAIN = ("train", "2018-01-02", "2023-12-29")
    OOS = ("2024-26_OOS", "2024-01-02", "2026-06-30")

    dates = sorted(panel["date"].unique())

    for d in dates:
        g = _eligible(panel[panel["date"] == d].dropna(subset=["fwd_ret_5d"]), st_set)
        if len(g) < top_n:
            continue

        # 1. Rank factors cross-sectionally
        Xrank = fl.rank_columns(g[FACTOR_COLS].to_numpy(float))

        # 2. Expand features (non-linear: squareds + interactions)
        Xexp = expand_fn(Xrank)

        # 3. Linear score in expanded feature space
        score = Xexp @ np.asarray(w, float)

        # 4. Hysteresis: incumbents get +delta score boost before ranking
        if delta > 0.0 and prev:
            is_incumbent = g["symbol"].isin(prev).to_numpy()
            score = score + delta * is_incumbent.astype(float)

        gi = g.assign(_score=score).sort_values(["_score", "symbol"], ascending=[False, True])
        pick = gi.head(top_n)
        ret = float(pick["fwd_ret_5d"].mean())
        cur = set(pick["symbol"])

        # Symmetric turnover (fraction of positions changed)
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
        "excess_return": 0.0,  # placeholder; caller uses to_index_relative
    }


# ---------------------------------------------------------------------------
# Delta selection (train only — no lookahead)
# ---------------------------------------------------------------------------

def select_delta(panel, fold, w, expand_fn, st_set):
    """Choose hysteresis delta on the TRAIN slice of fold.

    Maximises NET mean excess vs universe EW on the fold's train window only.
    Grid: {0, 0.02, 0.05, 0.1}.

    Args:
        panel:     Full panel DataFrame (includes OOS rows — ignored here).
        fold:      (train_lo, train_hi, oos_lo, oos_hi) tuple.
        w:         Weight vector for scoring.
        expand_fn: Feature expansion callable.
        st_set:    ST exclusion set.

    Returns:
        Best delta from DELTA_GRID.
    """
    train_lo, train_hi, _oos_lo, _oos_hi = fold
    train_panel = panel[(panel["date"] >= train_lo) & (panel["date"] <= train_hi)].copy()

    best_delta = 0.0
    best_net_excess = -np.inf

    for d in DELTA_GRID:
        report = backtest_hysteresis(
            train_panel, w, expand_fn, top_n=TOP_N, cost_bps=it.COST, st_set=st_set, delta=d
        )
        # Net excess vs universe equal-weight
        # Simple proxy: net total return (index not available for synthetic panels;
        # real data: could compare vs EW of train panel, but EW of cross-section is 0
        # in expectation, so total net return IS the relevant signal here)
        net_ret = report["total_return"]
        if net_ret > best_net_excess:
            best_net_excess = net_ret
            best_delta = d

    return best_delta


# ---------------------------------------------------------------------------
# Per-fold OOS evaluation
# ---------------------------------------------------------------------------

def _make_expand_fn(interaction_pairs):
    """Return a closure that calls fl.expand_features with given pairs."""
    pairs = [tuple(p) for p in interaction_pairs]  # ensure tuples

    def _expand(Xrank):
        return fl.expand_features(Xrank, pairs)

    return _expand


def _eval_fold_oos(panel, fold, st_set, idx_data):
    """Run one WFO fold: train nonlinear → select delta → OOS backtest (nonlinear + equal-weight).

    Returns:
        dict with keys: fold, nl_oos, eq_oos (all floats or None)
    """
    train_lo, train_hi, oos_lo, oos_hi = fold
    oos_panel = panel[(panel["date"] >= oos_lo) & (panel["date"] <= oos_hi)].copy()

    if len(oos_panel) == 0:
        return {"fold": f"{oos_lo}..{oos_hi}", "nl_oos": None, "eq_oos": None}

    # --- Train nonlinear weights on TRAIN slice ---
    res = tn.train_fold(panel, fold)
    w_nl = np.array(res["weights"])
    interaction_pairs = res["interaction_pairs"]
    expand_fn = _make_expand_fn(interaction_pairs)

    # --- Select delta on TRAIN slice (no OOS peek) ---
    delta = select_delta(panel, fold, w_nl, expand_fn, st_set)

    # --- OOS backtest: non-linear with hysteresis ---
    rep_nl = backtest_hysteresis(
        oos_panel, w_nl, expand_fn, top_n=TOP_N, cost_bps=it.COST, st_set=st_set, delta=delta
    )

    # --- OOS backtest: equal-weight baseline (f_bm=f_npyoy=1, no expansion, delta=0) ---
    # Equal-weight baseline: weights on first two factors (f_bm, f_npyoy) = 1, rest 0
    # Matches eval_linear_score.main's w_equal but uses factor indices 0 and 1
    w_eq = np.zeros(len(FACTOR_COLS))
    w_eq[0] = 1.0   # f_bm
    w_eq[1] = 1.0   # f_npyoy
    rep_eq = backtest_hysteresis(
        oos_panel, w_eq, lambda X: X, top_n=TOP_N, cost_bps=it.COST, st_set=st_set, delta=0.0
    )

    # --- Convert to index-relative (vs csi300) ---
    idx_m, idx_dates = idx_data
    rel_nl = it.to_index_relative(rep_nl, idx_m, idx_dates)
    rel_eq = it.to_index_relative(rep_eq, idx_m, idx_dates)

    nl_oos = rel_nl["excess_return"] if rel_nl else None
    eq_oos = rel_eq["excess_return"] if rel_eq else None

    return {
        "fold": f"{oos_lo}..{oos_hi}",
        "nl_oos": nl_oos,
        "eq_oos": eq_oos,
        "delta": delta,
        "n_oos_dates": rep_nl["n_rebalances"],
    }


# ---------------------------------------------------------------------------
# Per-panel aggregation
# ---------------------------------------------------------------------------

def _eval_panel(panel_path, panel_label, st_set, idx_data):
    """Run all WFO folds on one panel. Returns list of per-fold dicts + aggregate stats."""
    print(f"\n{'='*60}")
    print(f"Panel: {panel_label}  ({panel_path})")
    print(f"{'='*60}")

    panel = pd.read_csv(panel_path, dtype={"symbol": str})
    print(f"  Loaded {len(panel)} rows, dates {panel['date'].min()}..{panel['date'].max()}")

    fold_results = []
    for fold in tn.WFO_FOLDS:
        train_lo, train_hi, oos_lo, oos_hi = fold
        print(f"\n  [fold] train={train_lo}..{train_hi}  OOS={oos_lo}..{oos_hi}")
        fr = _eval_fold_oos(panel, fold, st_set, idx_data)
        fold_results.append(fr)
        nl = fr["nl_oos"]
        eq = fr["eq_oos"]
        delta = fr.get("delta", "?")
        print(f"    delta={delta:.2f}  nl_oos={nl:+.4f}" if nl is not None else f"    nl_oos=None", end="")
        print(f"  eq_oos={eq:+.4f}" if eq is not None else "  eq_oos=None")

    # Aggregate across folds
    nl_vals = [f["nl_oos"] for f in fold_results if f["nl_oos"] is not None]
    eq_vals = [f["eq_oos"] for f in fold_results if f["eq_oos"] is not None]
    nl_mean = float(np.mean(nl_vals)) if nl_vals else None
    eq_mean = float(np.mean(eq_vals)) if eq_vals else None
    nl_pos = sum(1 for v in nl_vals if v > 0)
    eq_pos = sum(1 for v in eq_vals if v > 0)
    n_folds = len(nl_vals)

    print(f"\n  --- Aggregate ({panel_label}) ---")
    print(f"  Non-linear:  mean OOS excess={nl_mean:+.4f}  positive_folds={nl_pos}/{n_folds}" if nl_mean is not None else "  Non-linear: no valid folds")
    print(f"  Equal-weight: mean OOS excess={eq_mean:+.4f}  positive_folds={eq_pos}/{n_folds}" if eq_mean is not None else "  Equal-weight: no valid folds")

    verdict_nl = (nl_mean is not None and nl_mean > 0 and nl_pos > n_folds / 2)
    verdict_eq = (eq_mean is not None and eq_mean > 0 and eq_pos > n_folds / 2)
    nl_beats_eq = (nl_mean is not None and eq_mean is not None and nl_mean > eq_mean)

    print(f"  § 5.3-positive (nonlinear): {verdict_nl}")
    print(f"  § 5.3-positive (equal-wt): {verdict_eq}")
    print(f"  Non-linear BEATS equal-weight: {nl_beats_eq}")

    return {
        "panel": panel_label,
        "folds": fold_results,
        "nl_mean_oos": nl_mean,
        "eq_mean_oos": eq_mean,
        "nl_pos_folds": nl_pos,
        "eq_pos_folds": eq_pos,
        "n_folds": n_folds,
        "verdict_nl_53": verdict_nl,
        "nl_beats_eq": nl_beats_eq,
    }


# ---------------------------------------------------------------------------
# Main orchestration
# ---------------------------------------------------------------------------

def main():
    # Load ST exclusion set
    st_set = set(pd.read_csv(ST_PATH)["symbol"]) if os.path.exists(ST_PATH) else set()
    print(f"ST set: {len(st_set)} symbols")

    # Load CSI 300 index for OOS excess computation
    idx_data = it.load_index("csi300")

    results = []

    # --- Membership panel ---
    if os.path.exists(PANEL_MEMBERSHIP):
        r = _eval_panel(PANEL_MEMBERSHIP, "membership", st_set, idx_data)
        results.append(r)
    else:
        print(f"[WARN] membership panel not found: {PANEL_MEMBERSHIP}")

    # --- Full (wide) panel ---
    if os.path.exists(PANEL_FULL):
        r = _eval_panel(PANEL_FULL, "full (wide)", st_set, idx_data)
        results.append(r)
    else:
        print(f"[WARN] full panel not found: {PANEL_FULL}")
        print("  Run: python scripts/build_factor_matrix.py --no-membership")

    # --- Final verdict ---
    print(f"\n{'='*60}")
    print("=== FINAL VERDICT ===")
    print(f"{'='*60}")
    for r in results:
        lbl = r["panel"]
        nl = r["nl_mean_oos"]
        eq = r["eq_mean_oos"]
        beats = r["nl_beats_eq"]
        pos = r["verdict_nl_53"]
        print(f"[{lbl}]  nl_mean={nl:+.4f}  eq_mean={eq:+.4f}  "
              f"beats_eq={beats}  §5.3-pos={pos}" if nl is not None else f"[{lbl}]  no data")

    all_beats = all(r["nl_beats_eq"] for r in results if r["nl_mean_oos"] is not None)
    any_pos = any(r["verdict_nl_53"] for r in results)
    print(f"\nNon-linear beats equal-weight on ALL panels: {all_beats}")
    print(f"At least one panel §5.3-positive: {any_pos}")

    return results


if __name__ == "__main__":
    main()
