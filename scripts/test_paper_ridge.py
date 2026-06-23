#!/usr/bin/env python3
"""paper_ridge 纯逻辑单测（合成数据，无网络/无真数据）。

跑：python -m pytest scripts/test_paper_ridge.py -q
或：python scripts/test_paper_ridge.py

覆盖 select_picks（确定性/迟滞/并列序）、realize_position（毛/换手/净/未到期 None）、
advance_journal（开新仓/结算到期/prev 链接/不碰 ≤train_hi/闭仓不可变）、running_nav。
"""
import numpy as np
import pandas as pd

import paper_ridge as pr
from build_factor_matrix import FACTOR_COLS

MOM = FACTOR_COLS.index("f_mom20")          # active scoring factor
W = np.zeros(len(FACTOR_COLS)); W[MOM] = 1.0   # score == gauss-rank of f_mom20


def _rows(date, specs):
    """specs: list of (symbol, mom, fwd). Eligibility cols set to pass _eligible."""
    out = []
    for sym, mom, fwd in specs:
        d = {c: 0.0 for c in FACTOR_COLS}
        d.update({"f_roe": 1.0, "f_bm": 1.0, "f_logamt": 18.0, "f_mom20": mom})
        d.update({"date": date, "symbol": sym, "fwd_ret_5d": fwd})
        out.append(d)
    return out


def _elig(date, specs):
    return pd.DataFrame(_rows(date, specs))


# --------------------------------------------------------------------------
# select_picks
# --------------------------------------------------------------------------

def test_select_picks_orders_by_factor():
    g = _elig("d", [("S1", 5, 0), ("S2", 4, 0), ("S3", 3, 0), ("S4", 2, 0), ("S5", 1, 0)])
    assert pr.select_picks(g, W, [], delta=0.0, top_n=3) == ["S1", "S2", "S3"]


def test_select_picks_empty():
    g = pd.DataFrame({c: [] for c in (["date", "symbol", "fwd_ret_5d"] + FACTOR_COLS)})
    assert pr.select_picks(g, W, [], delta=0.0, top_n=3) == []


def test_select_picks_tiebreak_symbol_asc():
    # B,A tie at the top → symbol-ascending breaks it → A wins the single slot
    g = _elig("d", [("B", 5, 0), ("A", 5, 0), ("C", 1, 0)])
    assert pr.select_picks(g, W, [], delta=0.0, top_n=1) == ["A"]


def test_select_picks_hysteresis_pulls_incumbent():
    g = _elig("d", [("S1", 5, 0), ("S2", 4, 0), ("S3", 3, 0), ("S4", 2, 0), ("S5", 1, 0)])
    base = pr.select_picks(g, W, [], delta=0.0, top_n=3)
    assert "S5" not in base                       # worst factor, normally excluded
    boosted = pr.select_picks(g, W, ["S5"], delta=10.0, top_n=3)
    assert "S5" in boosted and boosted[0] == "S1"  # incumbent pulled in; leader unchanged


# --------------------------------------------------------------------------
# realize_position
# --------------------------------------------------------------------------

def test_realize_gross_net_no_turnover():
    rz = pr.realize_position(["A", "B", "C"], ["A", "B", "C"],
                             {"A": 0.06, "B": 0.02, "C": -0.02}, cost_bps=20.0)
    assert abs(rz["gross_ret"] - 0.02) < 1e-12
    assert rz["turnover"] == 0.0
    assert abs(rz["net_ret"] - 0.02) < 1e-12        # no turnover → no cost


def test_realize_turnover_and_cost():
    # picks ABC vs prev ABD → symdiff {C,D}=2 over 6 → 1/3 turnover
    rz = pr.realize_position(["A", "B", "C"], ["A", "B", "D"],
                             {"A": 0.0, "B": 0.0, "C": 0.06}, cost_bps=30.0)
    assert abs(rz["gross_ret"] - 0.02) < 1e-12
    assert abs(rz["turnover"] - 1.0 / 3.0) < 1e-12
    assert abs(rz["net_ret"] - (0.02 - 30.0 / 1e4 * (1.0 / 3.0))) < 1e-12


def test_realize_pending_when_fwd_missing_or_nan():
    assert pr.realize_position(["A", "B"], [], {"A": 0.01}, cost_bps=20.0) is None       # B missing
    assert pr.realize_position(["A", "B"], [], {"A": 0.01, "B": np.nan}) is None         # B NaN
    assert pr.realize_position([], [], {}) is None                                       # empty


# --------------------------------------------------------------------------
# advance_journal + running_nav
# --------------------------------------------------------------------------

def _panel(d4_fwd):
    """d1 (<=train_hi, ignored), d2/d3 matured, d4 fwd controlled by arg."""
    specs = [("S1", 5, None), ("S2", 4, None), ("S3", 3, None), ("S4", 2, None), ("S5", 1, None)]
    rows = []
    rows += _rows("d1", [(s, m, 0.0) for s, m, _ in specs])
    rows += _rows("d2", [("S1", 5, 0.06), ("S2", 4, 0.02), ("S3", 3, -0.02), ("S4", 2, 0.0), ("S5", 1, 0.0)])
    rows += _rows("d3", [("S1", 5, 0.03), ("S2", 4, 0.01), ("S3", 3, 0.02), ("S4", 2, 0.0), ("S5", 1, 0.0)])
    rows += _rows("d4", [("S1", 5, d4_fwd), ("S2", 4, d4_fwd), ("S3", 3, d4_fwd), ("S4", 2, d4_fwd), ("S5", 1, d4_fwd)])
    return pd.DataFrame(rows)


def test_advance_opens_and_closes():
    panel = _panel(np.nan)        # d4 not matured yet
    rows = pr.advance_journal(panel, W, delta=0.0, train_hi="d1", st_set=set(), journal=[],
                              cost_bps=20.0, top_n=3)
    by = {r["date"]: r for r in rows}
    assert "d1" not in by                         # train_hi boundary not traded
    assert set(by) == {"d2", "d3", "d4"}
    assert by["d2"]["status"] == "closed" and by["d3"]["status"] == "closed"
    assert by["d4"]["status"] == "open"
    # picks deterministic
    assert by["d2"]["picks"] == "S1;S2;S3"
    # d2 prev empty (cold) → turnover 1.0 ; d3 prev == d2 picks → turnover 0
    assert float(by["d2"]["turnover"]) == 1.0
    assert float(by["d3"]["turnover"]) == 0.0
    # d2 gross = mean(0.06,0.02,-0.02)=0.02 ; net = 0.02 - 0.002*1.0
    assert abs(float(by["d2"]["gross_ret"]) - 0.02) < 1e-12
    assert abs(float(by["d2"]["net_ret"]) - (0.02 - 0.002)) < 1e-12
    # open row has prev linkage to d3 and no realised P&L
    assert by["d4"]["prev_picks"] == "S1;S2;S3"
    assert by["d4"]["gross_ret"] == ""


def test_advance_idempotent_then_matures():
    panel = _panel(np.nan)
    j1 = pr.advance_journal(panel, W, 0.0, "d1", set(), [], 20.0, 3)
    j2 = pr.advance_journal(panel, W, 0.0, "d1", set(), j1, 20.0, 3)
    assert j1 == j2                               # re-run with same data is a no-op

    matured = _panel(0.04)                        # d4 forward returns now known
    j3 = pr.advance_journal(matured, W, 0.0, "d1", set(), j2, 20.0, 3)
    by = {r["date"]: r for r in j3}
    assert by["d4"]["status"] == "closed"
    assert abs(float(by["d4"]["gross_ret"]) - 0.04) < 1e-12   # mean of equal fwds
    # closed d2/d3 untouched by the later run
    assert by["d2"] == {r["date"]: r for r in j2}["d2"]


def test_running_nav_compounds():
    panel = _panel(np.nan)
    rows = pr.advance_journal(panel, W, 0.0, "d1", set(), [], 20.0, 3)
    navs = {n["date"]: n["nav"] for n in pr.running_nav(rows)}
    nav_d2 = 1.0 * (1 + (0.02 - 0.002))           # 1.018
    nav_d3 = nav_d2 * (1 + 0.02)                   # d3 turnover 0 → net = gross = 0.02
    assert abs(navs["d2"] - nav_d2) < 1e-12
    assert abs(navs["d3"] - nav_d3) < 1e-12
    assert abs(navs["d4"] - nav_d3) < 1e-12        # open row carries last nav


if __name__ == "__main__":
    for name, fn in sorted(globals().items()):
        if name.startswith("test_") and callable(fn):
            fn(); print(f"PASS {name}")
    print("all passed")
