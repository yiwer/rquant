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


# ── Task 2: Axis 1 + Axis 3 tests ─────────────────────────────────────────────

def test_argmax_norm_per_factor_picks_highest_ic():
    """Pure helper: picks the norm with highest summed |IC| per factor."""
    acc = {
        "gauss": np.array([0.10, 0.20]),
        "rank":  np.array([0.30, 0.05]),
        "winz":  np.array([0.02, 0.02]),
    }
    assert ta._argmax_norm_per_factor(acc, 2) == ["rank", "gauss"]


def test_per_factor_norm_picks_max_train_ic():
    """Smoke test: per_factor_norms returns a norm name per factor, no crash on real panel."""
    p = _panel()
    ch = ta.per_factor_norms(p, "2018-01-02", "2019-12-31")
    assert len(ch) == len(ta.FC)
    assert all(nm in ta.NORMS for nm in ch)


def test_weight_hhi_dispersion():
    """HHI = 1 for single-weight concentration, 0.25 for uniform 4-weight."""
    h1, m1 = ta.weight_hhi(np.array([1.0, 0, 0, 0]))   # concentrated
    h2, m2 = ta.weight_hhi(np.array([1.0, 1, 1, 1]))   # uniform
    assert h1 == 1.0 and m1 == 1.0
    assert abs(h2 - 0.25) < 1e-9 and abs(m2 - 0.25) < 1e-9


def test_clip_pct_changes_dispersion():
    """Tighter clip (p50) should produce HHI <= looser clip (p99)."""
    p = _panel()
    w99, _ = ta.fit_variant(p, "2018-01-02", "2021-12-31", clip_pct=99)
    w50, _ = ta.fit_variant(p, "2018-01-02", "2021-12-31", clip_pct=50)
    assert ta.weight_hhi(w50)[0] <= ta.weight_hhi(w99)[0] + 1e-9


# ── Task 3: Axis 2 dropout-bagging tests ──────────────────────────────────────

def test_dropout_p0_reproduces_baseline_weights():
    p = _panel()
    w0, _ = ta.fit_variant(p, "2018-01-02", "2021-12-31", drop_p=0.0, n_bags=1)
    wb, _ = er.fit_ridge(p, "2018-01-02", "2021-12-31")
    assert np.allclose(w0, wb, atol=1e-9)


def test_dropout_masks_columns():
    # drop_p=1.0 但保至少一列 → 权重几乎全 0(只一列非零方向)
    p = _panel()
    w, _ = ta.fit_variant(p, "2018-01-02", "2019-12-31", drop_p=1.0, n_bags=1, seed=1)
    assert int((np.abs(w) > 1e-9).sum()) <= 1
