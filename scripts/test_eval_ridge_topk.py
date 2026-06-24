"""TDD for eval_ridge_topk — top-K risk-weighted portfolio construction.

Replaces top-3 equal weight with top-K + inverse-volatility weighting, to test
whether intra-strategy diversification lifts Sharpe / cuts drawdown (E4.1).

Key invariant: top_n=3 + scheme="equal" must equal the vetted
eval_ridge.backtest_ridge bit-for-bit.
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import numpy as np
import pandas as pd

from build_factor_matrix import FACTOR_COLS
import eval_ridge as er
import eval_ridge_topk as tk

MOM_IDX = FACTOR_COLS.index("f_mom20")
D1, D2, D3 = "2021-01-04", "2021-01-11", "2021-01-18"
SYMS = ["sh600001", "sh600002", "sh600003", "sh600004", "sh600005", "sh600006"]


def make_panel():
    mom_by_date = {
        D1: {s: float(len(SYMS) - i) for i, s in enumerate(SYMS)},
        D2: {s: float(i + 1) for i, s in enumerate(SYMS)},
        D3: {s: float(i + 1) for i, s in enumerate(SYMS)},
    }
    rows = []
    for d, mom in mom_by_date.items():
        for i, s in enumerate(SYMS):
            row = {c: 0.0 for c in FACTOR_COLS}
            row.update({"date": d, "symbol": s})
            row["f_mom20"] = mom[s]
            row["fwd_ret_5d"] = 0.01 * mom[s]
            row["f_roe"] = 1.0
            row["f_bm"] = 1.0
            row["f_logamt"] = 20.0
            row["f_vol20"] = 0.1 + 0.02 * i      # distinct vols → invvol ≠ equal
            rows.append(row)
    return pd.DataFrame(rows)


def _w_onehot_mom():
    w = np.zeros(len(FACTOR_COLS))
    w[MOM_IDX] = 1.0
    return w


# ---------------------------------------------------------------------------
# inv_vol_weights(vols) -> normalized weights ∝ 1/vol
# ---------------------------------------------------------------------------

def test_inv_vol_equal_vols_gives_equal_weights():
    w = tk.inv_vol_weights(np.array([0.2, 0.2, 0.2]))
    assert np.allclose(w, [1 / 3, 1 / 3, 1 / 3])


def test_inv_vol_lower_vol_gets_more_weight():
    w = tk.inv_vol_weights(np.array([0.1, 0.2]))
    assert np.allclose(w, [2 / 3, 1 / 3])


def test_inv_vol_weights_sum_to_one():
    w = tk.inv_vol_weights(np.array([0.05, 0.13, 0.27, 0.4]))
    assert abs(w.sum() - 1.0) < 1e-12


def test_inv_vol_nan_filled_with_median():
    # NaN vol → treated as median of valid → here all equal → equal weights
    w = tk.inv_vol_weights(np.array([0.2, 0.2, np.nan]))
    assert np.allclose(w, [1 / 3, 1 / 3, 1 / 3])


def test_inv_vol_all_invalid_falls_back_to_equal():
    w = tk.inv_vol_weights(np.array([np.nan, 0.0]))
    assert np.allclose(w, [0.5, 0.5])


# ---------------------------------------------------------------------------
# one_sided_turnover(w_new, w_old) -> Σ positive part of (w_new - w_old)
# ---------------------------------------------------------------------------

def test_turnover_initial_build_is_one():
    assert abs(tk.one_sided_turnover({"a": 0.5, "b": 0.5}, {}) - 1.0) < 1e-12


def test_turnover_no_change_is_zero():
    assert tk.one_sided_turnover({"a": 0.5, "b": 0.5}, {"a": 0.5, "b": 0.5}) == 0.0


def test_turnover_full_rotation_is_one():
    assert abs(tk.one_sided_turnover({"a": 1.0}, {"b": 1.0}) - 1.0) < 1e-12


def test_turnover_single_name_swap_in_equal_triple():
    new = {"a": 1 / 3, "b": 1 / 3, "d": 1 / 3}
    old = {"a": 1 / 3, "b": 1 / 3, "c": 1 / 3}
    assert abs(tk.one_sided_turnover(new, old) - 1 / 3) < 1e-12


# ---------------------------------------------------------------------------
# backtest_ridge_weighted
# ---------------------------------------------------------------------------

def test_equal_top3_equals_vetted_baseline():
    panel = make_panel()
    w = _w_onehot_mom()
    base = er.backtest_ridge(panel, w, top_n=3, cost_bps=20.0, st_set=set(), delta=0.0)
    got = tk.backtest_ridge_weighted(
        panel, w, top_n=3, cost_bps=20.0, st_set=set(), delta=0.0, scheme="equal",
    )
    assert abs(got["total_return"] - base["total_return"]) < 1e-12
    assert abs(got["turnover"] - base["turnover"]) < 1e-12
    assert [set(h["picks"]) for h in got["holdings"]] == \
           [set(h["picks"]) for h in base["holdings"]]


def test_invvol_differs_from_equal_when_vols_differ():
    panel = make_panel()
    w = _w_onehot_mom()
    eq = tk.backtest_ridge_weighted(
        panel, w, top_n=3, cost_bps=20.0, st_set=set(), delta=0.0, scheme="equal",
    )
    iv = tk.backtest_ridge_weighted(
        panel, w, top_n=3, cost_bps=20.0, st_set=set(), delta=0.0, scheme="invvol",
    )
    assert abs(iv["total_return"] - eq["total_return"]) > 1e-9
