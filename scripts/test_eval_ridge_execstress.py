"""TDD for backtest_ridge_exec — ridge-on-gauss backtest with A-share
execution realism (limit-up entry exclusion + square-root impact cost).

Uses a tiny synthetic panel (no real data), per repo convention.
Key invariant: with realism knobs OFF the result must equal the vetted
eval_ridge.backtest_ridge bit-for-bit.
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import numpy as np
import pandas as pd

from build_factor_matrix import FACTOR_COLS
import eval_ridge as er
import eval_ridge_execstress as ex

MOM_IDX = FACTOR_COLS.index("f_mom20")
D1, D2, D3 = "2021-01-04", "2021-01-11", "2021-01-18"
SYMS = ["sh600001", "sh600002", "sh600003", "sh600004", "sh600005", "sh600006"]


def make_panel():
    """6 symbols × 3 dates. Score driven solely by f_mom20 (one-hot w).

    d1 momentum order s1>s2>...>s6 (top3 = s1,s2,s3)
    d2,d3 reversed     s6>s5>...>s1 (top3 = s6,s5,s4)  → full turnover d1→d2, none d2→d3
    """
    mom_by_date = {
        D1: {s: float(len(SYMS) - i) for i, s in enumerate(SYMS)},   # s1=6 .. s6=1
        D2: {s: float(i + 1) for i, s in enumerate(SYMS)},           # s1=1 .. s6=6
        D3: {s: float(i + 1) for i, s in enumerate(SYMS)},
    }
    rows = []
    for d, mom in mom_by_date.items():
        for s in SYMS:
            row = {c: 0.0 for c in FACTOR_COLS}
            row.update({"date": d, "symbol": s})
            row["f_mom20"] = mom[s]
            row["fwd_ret_5d"] = 0.01 * mom[s]      # distinct, ties to rank
            row["f_roe"] = 1.0                      # pass _eligible
            row["f_bm"] = 1.0
            row["f_logamt"] = 20.0                  # ADV = exp(20) ≈ 4.85e8
            row["lock_up"] = False
            rows.append(row)
    return pd.DataFrame(rows)


def _w_onehot_mom():
    w = np.zeros(len(FACTOR_COLS))
    w[MOM_IDX] = 1.0
    return w


def _picks_on(rep, date):
    for h in rep["holdings"]:
        if h["t"] == date:
            return set(h["picks"])
    return None


def test_knobs_off_equals_vetted_baseline():
    panel = make_panel()
    w = _w_onehot_mom()
    base = er.backtest_ridge(panel, w, top_n=3, cost_bps=20.0, st_set=set(), delta=0.0)
    exec_off = ex.backtest_ridge_exec(
        panel, w, top_n=3, cost_bps=20.0, st_set=set(), delta=0.0,
        lock_col="lock_up", aum=0.0, impact_k=0.0,
    )
    assert abs(exec_off["total_return"] - base["total_return"]) < 1e-12
    assert exec_off["turnover"] == base["turnover"]
    assert [set(h["picks"]) for h in exec_off["holdings"]] == \
           [set(h["picks"]) for h in base["holdings"]]


def test_limit_up_name_excluded_from_buys():
    panel = make_panel()
    # s1 is the top pick on d1 but closes limit-up locked → can't buy
    panel.loc[(panel["date"] == D1) & (panel["symbol"] == "sh600001"), "lock_up"] = True
    w = _w_onehot_mom()
    rep = ex.backtest_ridge_exec(
        panel, w, top_n=3, cost_bps=20.0, st_set=set(), delta=0.0,
        lock_col="lock_up", aum=0.0, impact_k=0.0,
    )
    picks_d1 = _picks_on(rep, D1)
    assert "sh600001" not in picks_d1
    assert picks_d1 == {"sh600002", "sh600003", "sh600004"}


def test_impact_cost_reduces_net_return_with_turnover():
    panel = make_panel()
    w = _w_onehot_mom()
    no_impact = ex.backtest_ridge_exec(
        panel, w, top_n=3, cost_bps=20.0, st_set=set(), delta=0.0,
        lock_col="lock_up", aum=0.0, impact_k=0.0,
    )
    with_impact = ex.backtest_ridge_exec(
        panel, w, top_n=3, cost_bps=20.0, st_set=set(), delta=0.0,
        lock_col="lock_up", aum=5e8, impact_k=100.0,
    )
    # turnover is positive (d1 build + d2 full rotation) → impact must bite
    assert with_impact["total_return"] < no_impact["total_return"]


# ---------------------------------------------------------------------------
# compute_lock_rows(kday_df, symbol, dates) -> [{date,symbol,lock_up,lock_down}]
# ---------------------------------------------------------------------------

def _kday(rows):
    # rows: list of (time, open, high, low, close, pctChg)
    return pd.DataFrame(rows, columns=["time", "open", "high", "low", "close", "pctChg"])


def test_compute_lock_rows_detects_limit_up_main_board():
    k = _kday([
        ("2021-03-01 15:00:00", 9.2, 10.0, 9.2, 10.0, 9.99),   # locked up (10% board)
        ("2021-03-02 15:00:00", 10.0, 10.5, 9.0, 9.0, -9.99),  # locked down
    ])
    out = ex.compute_lock_rows(k, "sh600001", {"2021-03-01", "2021-03-02"})
    by_date = {r["date"]: r for r in out}
    assert by_date["2021-03-01"]["lock_up"] is True
    assert by_date["2021-03-01"]["lock_down"] is False
    assert by_date["2021-03-02"]["lock_down"] is True
    assert by_date["2021-03-02"]["lock_up"] is False


def test_compute_lock_rows_chinext_uses_20pct_after_reform():
    k = _kday([
        ("2021-03-01 15:00:00", 9.0, 10.0, 9.0, 10.0, 10.0),   # +10% but board is 20% → NOT locked
        ("2021-03-02 15:00:00", 9.0, 10.0, 9.0, 10.0, 19.95),  # +20% → locked
    ])
    out = ex.compute_lock_rows(k, "sz300750", {"2021-03-01", "2021-03-02"})
    by_date = {r["date"]: r for r in out}
    assert by_date["2021-03-01"]["lock_up"] is False
    assert by_date["2021-03-02"]["lock_up"] is True


def test_compute_lock_rows_filters_to_requested_dates():
    k = _kday([
        ("2021-03-01 15:00:00", 9.2, 10.0, 9.2, 10.0, 9.99),
        ("2021-03-02 15:00:00", 9.2, 10.0, 9.2, 10.0, 9.99),
    ])
    out = ex.compute_lock_rows(k, "sh600001", {"2021-03-01"})
    assert [r["date"] for r in out] == ["2021-03-01"]
