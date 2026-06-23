# scripts/test_eval_nonlinear.py
"""Tests for eval_nonlinear.py -- TDD: write tests first, then implement.

Run from scripts/ dir:
    python -m pytest test_eval_nonlinear.py -v
"""
import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import numpy as np
import pandas as pd
import pytest

from build_factor_matrix import FACTOR_COLS

# FACTOR_COLS = ['f_bm', 'f_npyoy', 'f_revyoy', 'f_roe', 'f_gm', 'f_mom20',
#                'f_mom120', 'f_rev5', 'f_trend60', 'f_atr', 'f_rvol', 'f_logamt', 'f_secmom']
# Indices:         0         1          2            3        4       5        6        7        8       9       10        11        12
#
# _eligible hard gate needs:  f_roe (idx=3) > 0, f_bm (idx=0) > 0, f_logamt (idx=11) >= log(5e7)
import numpy as _np
LIQ_FLOOR_LOG = float(_np.log(5e7))   # ~17.73

# Sentinel values that pass the hard gate
_ROE_OK = 5.0       # f_roe > 0
_BM_OK = 0.5        # f_bm > 0
_LIQ_OK = 18.0      # f_logamt >= LIQ_FLOOR_LOG


def _make_row(date, symbol, score_val, fwd=0.05):
    """Build one panel row; hard gate passes by default.
    score_val goes into f_bm (index 0) but we also need f_bm>0 for gate.
    So we put the score in a neutral slot (f_mom20, idx=5) and keep f_bm fixed.
    """
    x = [0.0] * len(FACTOR_COLS)
    # Gate slots (must be positive / meet floor)
    x[0] = _BM_OK        # f_bm > 0
    x[3] = _ROE_OK        # f_roe > 0
    x[11] = _LIQ_OK       # f_logamt >= floor
    # Score slot
    x[5] = score_val      # f_mom20 — used for scoring in tests
    return [date, symbol] + x + [fwd]


def _make_panel_df(rows):
    cols = ["date", "symbol"] + FACTOR_COLS + ["fwd_ret_5d"]
    return pd.DataFrame(rows, columns=cols)


# ---------------------------------------------------------------------------
# Test 1 (from brief): hysteresis reduces turnover
# ---------------------------------------------------------------------------

def test_hysteresis_reduces_turnover():
    """The brief-mandated test: delta=0.5 should produce <= turnover of delta=0.0.

    Setup: two periods; s0 leads in period 1, s1 leads in period 2 by a tiny bump.
    Without hysteresis the model switches. With large delta the incumbent is kept.
    """
    import eval_nonlinear as en

    rows = []
    for d, bump in [("2024-01-02", 0.0), ("2024-01-09", 0.01)]:
        for s, b in enumerate([0.40, 0.41, 0.30]):   # s0/s1 score close, alternates lead
            x = [0.0] * len(FACTOR_COLS)
            x[0] = _BM_OK
            x[3] = _ROE_OK
            x[11] = _LIQ_OK
            # Use f_mom20 (idx=5) as the score carrier; s1 wins in period 2 by bump
            x[5] = b + (bump if s == 1 else 0)
            rows.append([d, f"s{s}"] + x + [0.05])

    p = _make_panel_df(rows)

    # Weight on f_mom20 (index 5)
    w = np.zeros(len(FACTOR_COLS))
    w[5] = 1.0

    t0 = en.backtest_hysteresis(p, w, lambda X: X, 1, 0.0, set(), delta=0.0)["turnover"]
    t1 = en.backtest_hysteresis(p, w, lambda X: X, 1, 0.0, set(), delta=0.5)["turnover"]
    assert t1 <= t0, f"Expected hysteresis to reduce turnover, got t0={t0} t1={t1}"


# ---------------------------------------------------------------------------
# Test 2: select_delta returns value from the allowed grid
# ---------------------------------------------------------------------------

def test_select_delta_returns_valid_grid_value():
    """select_delta must return a value from {0, 0.02, 0.05, 0.1}."""
    import eval_nonlinear as en

    rows = []
    for i, d in enumerate(["2022-01-03", "2022-01-10", "2022-01-17"]):
        for s in range(5):
            x = [0.0] * len(FACTOR_COLS)
            x[0] = _BM_OK
            x[3] = _ROE_OK
            x[11] = _LIQ_OK
            x[5] = float(s) / 4.0
            rows.append([d, f"s{s}"] + x + [float(s) * 0.01])

    p = _make_panel_df(rows)
    w = np.zeros(len(FACTOR_COLS))
    w[5] = 1.0

    fold = ("2022-01-01", "2022-01-17", "2022-01-24", "2022-12-31")
    delta = en.select_delta(p, fold, w, lambda X: X, set())
    assert delta in (0.0, 0.02, 0.05, 0.1), f"select_delta returned unexpected value: {delta}"


# ---------------------------------------------------------------------------
# Test 3: backtest_hysteresis with delta=0 picks highest-scoring stock
# ---------------------------------------------------------------------------

def test_backtest_hysteresis_delta_zero_normal_picks():
    """With delta=0 and a simple panel, picks the highest-scoring stock."""
    import eval_nonlinear as en

    rows = []
    # Single period, 4 stocks. sA has highest f_mom20 score.
    for sym, score in [("sA", 0.9), ("sB", 0.5), ("sC", 0.3), ("sD", 0.1)]:
        x = [0.0] * len(FACTOR_COLS)
        x[0] = _BM_OK
        x[3] = _ROE_OK
        x[11] = _LIQ_OK
        x[5] = score    # f_mom20
        rows.append(["2023-01-02", sym] + x + [0.01])

    p = _make_panel_df(rows)
    w = np.zeros(len(FACTOR_COLS))
    w[5] = 1.0   # weight on f_mom20

    result = en.backtest_hysteresis(p, w, lambda X: X, 1, 0.0, set(), delta=0.0)
    picks = result["holdings"][0]["picks"]
    assert "sA" in picks, f"Expected sA in picks (highest score), got {picks}"
