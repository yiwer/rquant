# scripts/test_train_ablation.py
import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np, pandas as pd
import eval_ridge as er
import train_ablation as ta
from test_norm_hysteresis import norm_gauss
from build_factor_matrix import FACTOR_COLS

PANEL = er.PANEL_MEMBERSHIP
def _panel():
    return pd.read_csv(PANEL, dtype={"symbol": str})

def test_fit_variant_defaults_reproduce_fit_ridge():
    p = _panel(); lo, hi = "2018-01-02", "2021-12-31"
    w_ref, n_ref = er.fit_ridge(p, lo, hi)
    w, n = ta.fit_variant(p, lo, hi)          # defaults = norm_gauss, clip90, no dropout
    assert n == n_ref
    assert np.allclose(w, w_ref, atol=1e-9), np.abs(w - w_ref).max()

def test_backtest_score_baseline_reproduces_backtest_ridge():
    p = _panel(); st = set()
    oos = p[(p["date"] >= "2022-01-02") & (p["date"] <= "2022-12-31")]
    w, _ = er.fit_ridge(p, "2018-01-02", "2021-12-31")
    ref = er.backtest_ridge(oos, w, top_n=er.TOP_N, cost_bps=20.0, st_set=st, delta=0.05)
    sf = lambda g: norm_gauss(g[FACTOR_COLS].to_numpy(float)) @ w
    got = ta.backtest_score(oos, sf, top_n=er.TOP_N, cost_bps=20.0, st_set=st, delta=0.05)
    assert abs(got["total_return"] - ref["total_return"]) < 1e-9
    assert [h["picks"] for h in got["holdings"]] == [h["picks"] for h in ref["holdings"]]
