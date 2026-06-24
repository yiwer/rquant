"""TDD for eval_ridge_label — objective/label experiments on ridge-on-gauss.

Two questions:
  1. risk-adjusted label: rank(fwd/vol) instead of rank(fwd) — does ranking by
     forward Sharpe (not raw return) lift OOS Sharpe / shrink the top3↔top10 gap?
  2. long-side power: a one-sided "longtail" label that flattens the bottom half
     and only orders the top — does it steepen the long-side decile profile, or is
     the short-side power structural (un-movable to the long side)?

Pure cores under test:
  - make_label(fwd, vol, mode): centered training label for {raw, riskadj, longtail}
  - fit_ridge_label(..., mode="raw") == eval_ridge.fit_ridge bit-for-bit (anchor)
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import numpy as np
import pandas as pd

from build_factor_matrix import FACTOR_COLS
import eval_ridge as er
import eval_ridge_label as lb

SYMS = ["sh600001", "sh600002", "sh600003", "sh600004", "sh600005", "sh600006"]


# ---------------------------------------------------------------------------
# make_label(fwd, vol, mode) -> centered label
# ---------------------------------------------------------------------------

def test_raw_label_is_centered_rank():
    y = lb.make_label([0.1, 0.3, 0.2], [1.0, 1.0, 1.0], "raw")
    assert np.allclose(y, [-0.5, 0.5, 0.0])


def test_riskadj_promotes_low_vol_name():
    # fwd/vol = [1,2,1]: the middle name (0.2 ret / 0.1 vol) becomes the top
    y = lb.make_label([0.1, 0.2, 0.3], [0.1, 0.1, 0.3], "riskadj")
    assert np.allclose(y, [-0.25, 0.5, -0.25])
    assert np.argmax(y) == 1


def test_longtail_flattens_bottom_half():
    # bottom half gets an equal (most-negative) label; only the top is ordered
    y = lb.make_label([0.1, 0.2, 0.3, 0.4], [1, 1, 1, 1], "longtail")
    assert abs(y[0] - y[1]) < 1e-12           # losers not distinguished
    assert y[3] > y[2] > y[1]                  # winners ordered, top steepest


def test_label_is_centered():
    for mode in ("raw", "riskadj", "longtail"):
        y = lb.make_label([0.1, 0.2, 0.3, 0.4, 0.5], [0.1, 0.2, 0.3, 0.2, 0.1], mode)
        assert abs(float(np.sum(y))) < 1e-9


def test_riskadj_nan_vol_filled_with_median():
    # NaN vol must not crash; filled with median → finite label
    y = lb.make_label([0.1, 0.2, 0.3], [0.1, np.nan, 0.3], "riskadj")
    assert np.all(np.isfinite(y))


# ---------------------------------------------------------------------------
# fit_ridge_label(mode="raw") == eval_ridge.fit_ridge  (the anchor invariant)
# ---------------------------------------------------------------------------

def _make_panel():
    rng = np.random.default_rng(0)
    rows = []
    for d in ["2021-01-04", "2021-01-11"]:
        for i, s in enumerate(SYMS):
            row = {c: float(rng.normal()) for c in FACTOR_COLS}
            row["date"] = d
            row["symbol"] = s
            row["fwd_ret_5d"] = float(rng.normal() * 0.05)
            row["f_vol20"] = 0.1 + 0.02 * i
            rows.append(row)
    return pd.DataFrame(rows)


def test_fit_raw_equals_vetted_fit_ridge():
    panel = _make_panel()
    w_ref, n_ref = er.fit_ridge(panel, "2021-01-01", "2021-12-31")
    w_got, n_got = lb.fit_ridge_label(panel, "2021-01-01", "2021-12-31", mode="raw")
    assert n_got == n_ref
    assert np.allclose(w_got, w_ref)


def test_fit_riskadj_differs_from_raw():
    panel = _make_panel()
    w_raw, _ = lb.fit_ridge_label(panel, "2021-01-01", "2021-12-31", mode="raw")
    w_ra, _ = lb.fit_ridge_label(panel, "2021-01-01", "2021-12-31", mode="riskadj")
    assert not np.allclose(w_raw, w_ra)
