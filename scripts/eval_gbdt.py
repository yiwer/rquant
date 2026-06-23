# scripts/eval_gbdt.py
"""GBDT 因子回测器：ensemble_predict 打分 + 成本感知迟滞 + 多折 WFO。

对 membership 面板与 full 面板各跑逐折 OOS，对照等权基线，聚合裁决。

主接口：
  backtest_gbdt(panel, models, top_n, cost_bps, st_set, delta) -> report
  select_delta_gbdt(panel, fold, models, st_set) -> float
  main()
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
import train_gbdt as tg

# ---------------------------------------------------------------------------
# Paths
# ---------------------------------------------------------------------------

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ST_PATH = os.path.join(REPO, "data", "baostock", "st_symbols.csv")
PANEL_FULL = os.path.join(OUT_DIR, "factors_full.csv")

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

DELTA_GRID = [0.0, 0.02, 0.05, 0.1]
TOP_N = 3
LIQ_FLOOR_LOG = float(np.log(5e7))


# ---------------------------------------------------------------------------
# Hard gate (mirrors eval_nonlinear._eligible)
# ---------------------------------------------------------------------------

def _eligible(g, st_set):
    """硬闸：非 ST ∧ roe>0 ∧ f_bm>0 ∧ 流动性≥地板。"""
    ok = (~g["symbol"].isin(st_set)) & (g["f_roe"] > 0) & (g["f_bm"] > 0) & (g["f_logamt"] >= LIQ_FLOOR_LOG)
    return g[ok]


# ---------------------------------------------------------------------------
# Core GBDT backtest
# ---------------------------------------------------------------------------

def backtest_gbdt(panel, models, top_n, cost_bps, st_set, delta):
    """Weekly backtest with GBDT ensemble scoring + hysteresis.

    Args:
        panel:     DataFrame with columns date, symbol, FACTOR_COLS..., fwd_ret_5d, f_roe, f_logamt.
        models:    List of lgb.Booster (or any object with .predict(X) -> array).
        top_n:     Number of stocks to pick each period.
        cost_bps:  Round-trip cost in basis points (half applied each side).
        st_set:    Set of ST symbol strings to exclude.
        delta:     Hysteresis advantage added to incumbent holdings' scores before ranking.

    Returns:
        dict matching eval_nonlinear.backtest_hysteresis return contract:
            holdings, regime_slices, risk, total_return, max_drawdown, turnover,
            n_rebalances, excess_return.
    """
    panel = panel.sort_values(["date", "symbol"])
    nav = 1.0
    prev = set()  # incumbent holdings from previous period
    navs = []
    period_rets = []
    total_turn = 0.0

    # Regime slices mirrors eval_nonlinear.backtest_hysteresis
    TRAIN = ("train", "2018-01-02", "2023-12-29")
    OOS = ("2024-26_OOS", "2024-01-02", "2026-06-30")

    dates = sorted(panel["date"].unique())

    for d in dates:
        g = _eligible(panel[panel["date"] == d].dropna(subset=["fwd_ret_5d"]), st_set)
        if len(g) < top_n:
            continue

        # 1. Rank factors cross-sectionally
        Xrank = fl.rank_columns(g[FACTOR_COLS].to_numpy(float))

        # 2. GBDT ensemble score
        score = tg.ensemble_predict(models, Xrank)

        # 3. Hysteresis: incumbents get +delta score boost before ranking
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

def select_delta_gbdt(panel, fold, models, st_set):
    """Choose hysteresis delta on the TRAIN slice of fold using GBDT ensemble.

    Maximises NET total return on the fold's train window only.
    Grid: {0, 0.02, 0.05, 0.1}.

    Args:
        panel:   Full panel DataFrame (includes OOS rows — ignored here).
        fold:    (train_lo, train_hi, oos_lo, oos_hi) tuple.
        models:  List of lgb.Booster objects.
        st_set:  ST exclusion set.

    Returns:
        Best delta from DELTA_GRID.
    """
    train_lo, train_hi, _oos_lo, _oos_hi = fold
    train_panel = panel[(panel["date"] >= train_lo) & (panel["date"] <= train_hi)].copy()

    best_delta = 0.0
    best_net_ret = -np.inf

    for d in DELTA_GRID:
        report = backtest_gbdt(
            train_panel, models, top_n=TOP_N, cost_bps=it.COST, st_set=st_set, delta=d
        )
        net_ret = report["total_return"]
        if net_ret > best_net_ret:
            best_net_ret = net_ret
            best_delta = d

    return best_delta


# ---------------------------------------------------------------------------
# Per-fold OOS evaluation
# ---------------------------------------------------------------------------

def _eval_fold_oos_gbdt(panel, fold, st_set, idx_data):
    """Run one WFO fold: train GBDT → select delta → OOS backtest (GBDT + equal-weight).

    Returns:
        dict with keys: fold, gbdt_oos, eq_oos, delta, n_oos_dates
    """
    train_lo, train_hi, oos_lo, oos_hi = fold
    oos_panel = panel[(panel["date"] >= oos_lo) & (panel["date"] <= oos_hi)].copy()

    if len(oos_panel) == 0:
        return {"fold": f"{oos_lo}..{oos_hi}", "gbdt_oos": None, "eq_oos": None}

    # --- Train GBDT ensemble on TRAIN slice ---
    print(f"    Training GBDT ensemble for fold train={train_lo}..{train_hi} ...")
    models = tg.train_fold_gbdt(panel, fold)

    # --- Select delta on TRAIN slice (no OOS peek) ---
    print(f"    Selecting delta on train slice ...")
    delta = select_delta_gbdt(panel, fold, models, st_set)

    # --- OOS backtest: GBDT with hysteresis ---
    print(f"    OOS backtest (GBDT, delta={delta:.2f}) ...")
    rep_gbdt = backtest_gbdt(
        oos_panel, models, top_n=TOP_N, cost_bps=it.COST, st_set=st_set, delta=delta
    )

    # --- OOS backtest: equal-weight baseline (f_bm=f_npyoy=1, identity, delta=0) ---
    # Mirrors eval_nonlinear's equal-weight: weights on first two factors, no expansion
    w_eq = np.zeros(len(FACTOR_COLS))
    w_eq[0] = 1.0   # f_bm
    w_eq[1] = 1.0   # f_npyoy

    # Use a two-element list as a "model" list compatible with ensemble_predict:
    # We implement equal-weight by wrapping linear scoring as pseudo-models.
    # Actually, equal-weight = direct rank scoring without GBDT.
    # Re-use backtest_gbdt but with a linear pseudo-ensemble.
    class _LinearEnsemble:
        """Wraps a weight vector as a .predict() method for use with ensemble_predict."""
        def __init__(self, w):
            self._w = np.asarray(w, float)

        def predict(self, X):
            return np.asarray(X, float) @ self._w

    eq_models = [_LinearEnsemble(w_eq)]
    rep_eq = backtest_gbdt(
        oos_panel, eq_models, top_n=TOP_N, cost_bps=it.COST, st_set=st_set, delta=0.0
    )

    # --- Convert to index-relative (vs csi300) ---
    idx_m, idx_dates = idx_data
    rel_gbdt = it.to_index_relative(rep_gbdt, idx_m, idx_dates)
    rel_eq = it.to_index_relative(rep_eq, idx_m, idx_dates)

    gbdt_oos = rel_gbdt["excess_return"] if rel_gbdt else None
    eq_oos = rel_eq["excess_return"] if rel_eq else None

    return {
        "fold": f"{oos_lo}..{oos_hi}",
        "gbdt_oos": gbdt_oos,
        "eq_oos": eq_oos,
        "delta": delta,
        "n_oos_dates": rep_gbdt["n_rebalances"],
    }


# ---------------------------------------------------------------------------
# Per-panel aggregation
# ---------------------------------------------------------------------------

def _eval_panel_gbdt(panel_path, panel_label, st_set, idx_data):
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
        fr = _eval_fold_oos_gbdt(panel, fold, st_set, idx_data)
        fold_results.append(fr)
        gbdt = fr.get("gbdt_oos")
        eq = fr.get("eq_oos")
        delta = fr.get("delta", "?")
        if gbdt is not None:
            print(f"    delta={delta:.2f}  gbdt_oos={gbdt:+.4f}", end="")
        else:
            print(f"    gbdt_oos=None", end="")
        if eq is not None:
            print(f"  eq_oos={eq:+.4f}")
        else:
            print(f"  eq_oos=None")

    # Aggregate across folds
    gbdt_vals = [f["gbdt_oos"] for f in fold_results if f.get("gbdt_oos") is not None]
    eq_vals = [f["eq_oos"] for f in fold_results if f.get("eq_oos") is not None]
    gbdt_mean = float(np.mean(gbdt_vals)) if gbdt_vals else None
    eq_mean = float(np.mean(eq_vals)) if eq_vals else None
    gbdt_pos = sum(1 for v in gbdt_vals if v > 0)
    eq_pos = sum(1 for v in eq_vals if v > 0)
    n_folds = len(gbdt_vals)

    print(f"\n  --- Aggregate ({panel_label}) ---")
    if gbdt_mean is not None:
        print(f"  GBDT:         mean OOS excess={gbdt_mean:+.4f}  positive_folds={gbdt_pos}/{n_folds}")
    else:
        print(f"  GBDT: no valid folds")
    if eq_mean is not None:
        print(f"  Equal-weight: mean OOS excess={eq_mean:+.4f}  positive_folds={eq_pos}/{n_folds}")
    else:
        print(f"  Equal-weight: no valid folds")

    verdict_gbdt = (gbdt_mean is not None and gbdt_mean > 0 and gbdt_pos > n_folds / 2)
    verdict_eq = (eq_mean is not None and eq_mean > 0 and eq_pos > n_folds / 2)
    gbdt_beats_eq = (gbdt_mean is not None and eq_mean is not None and gbdt_mean > eq_mean)

    print(f"  § 5.3-positive (GBDT):     {verdict_gbdt}")
    print(f"  § 5.3-positive (equal-wt): {verdict_eq}")
    print(f"  GBDT BEATS equal-weight:   {gbdt_beats_eq}")

    return {
        "panel": panel_label,
        "folds": fold_results,
        "gbdt_mean_oos": gbdt_mean,
        "eq_mean_oos": eq_mean,
        "gbdt_pos_folds": gbdt_pos,
        "eq_pos_folds": eq_pos,
        "n_folds": n_folds,
        "verdict_gbdt_53": verdict_gbdt,
        "gbdt_beats_eq": gbdt_beats_eq,
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
        r = _eval_panel_gbdt(PANEL_MEMBERSHIP, "membership", st_set, idx_data)
        results.append(r)
    else:
        print(f"[WARN] membership panel not found: {PANEL_MEMBERSHIP}")

    # --- Full (wide) panel ---
    if os.path.exists(PANEL_FULL):
        r = _eval_panel_gbdt(PANEL_FULL, "full (wide)", st_set, idx_data)
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
        gbdt = r["gbdt_mean_oos"]
        eq = r["eq_mean_oos"]
        beats = r["gbdt_beats_eq"]
        pos = r["verdict_gbdt_53"]
        if gbdt is not None:
            print(f"[{lbl}]  gbdt_mean={gbdt:+.4f}  eq_mean={eq:+.4f}  "
                  f"beats_eq={beats}  §5.3-pos={pos}")
        else:
            print(f"[{lbl}]  no data")

    all_beats = all(r["gbdt_beats_eq"] for r in results if r["gbdt_mean_oos"] is not None)
    any_pos = any(r["verdict_gbdt_53"] for r in results)
    print(f"\nGBDT beats equal-weight on ALL panels: {all_beats}")
    print(f"At least one panel §5.3-positive (GBDT): {any_pos}")

    return results


if __name__ == "__main__":
    main()
