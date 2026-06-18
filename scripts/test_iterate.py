#!/usr/bin/env python3
"""iterate 纯函数单测：break-even / regime / 符号翻转 / 裁决+旗标。
跑：python scripts/test_iterate.py"""
import math
from iterate import break_even, regime_excess, detect_sign_flip, judge, to_index_relative


def _rep(excess, sharpe, regimes):
    return {"excess_return": excess, "max_drawdown": 0.2, "turnover": 100.0, "n_rebalances": 100,
            "risk": {"sharpe": sharpe},
            "regime_slices": [{"label": k, "excess": v} for k, v in regimes]}


def test_break_even():
    assert math.isclose(break_even(0.20, 0.10, 20.0), 40.0, rel_tol=1e-9)
    assert break_even(-0.1, -0.3, 20.0) is None
    assert break_even(0.0, 0.0, 20.0) is None


def test_regime_excess():
    n = _rep(-0.1, 0.2, [("train", 0.05), ("2024-26_OOS", -0.03)])
    assert math.isclose(regime_excess(n, True), -0.03)
    assert math.isclose(regime_excess(n, False), 0.05)


def test_sign_flip():
    assert detect_sign_flip([0.1, -0.02, 0.05]) is True
    assert detect_sign_flip([0.1, 0.05, 0.2]) is False


def test_judge_pass():
    g = _rep(0.30, None, [("train", 0.2), ("OOS", 0.12)])
    n = _rep(0.18, 1.1, [("train", 0.15), ("OOS", 0.09)])
    v, flags, _ = judge(g, n, sweep=[0.09, 0.08, 0.11])
    assert v == "PASS" and flags == [], (v, flags)


def test_judge_falsified_oos():
    g = _rep(0.30, None, [("train", 0.2), ("OOS", 0.12)])
    n = _rep(-0.02, 0.8, [("train", 0.1), ("OOS", -0.04)])
    v, flags, _ = judge(g, n, sweep=None)
    assert v == "FALSIFIED" and "net-OOS<=0" in flags and "in-sample-only" in flags, (v, flags)


def test_judge_signflip_falsifies():
    g = _rep(0.30, None, [("train", 0.2), ("OOS", 0.12)])
    n = _rep(0.18, 1.1, [("train", 0.15), ("OOS", 0.09)])
    v, flags, _ = judge(g, n, sweep=[0.09, -0.03, 0.11])
    assert v == "FALSIFIED" and "sign-flip" in flags, (v, flags)


def test_to_index_relative():
    rep = {"holdings": [{"t": "2018-01-02T15:00:00", "nav": 1.0},
                        {"t": "2020-01-02T15:00:00", "nav": 1.5},
                        {"t": "2024-01-02T15:00:00", "nav": 2.0},
                        {"t": "2025-01-02T15:00:00", "nav": 2.2}],
           "risk": {"sharpe": 1.0}, "max_drawdown": 0.2, "turnover": 10, "n_rebalances": 4,
           "regime_slices": [{"label": "train", "from": "2018-01-01", "to": "2021-12-31", "excess": 0.0},
                             {"label": "2024-26_OOS", "from": "2024-01-01", "to": "2026-01-01", "excess": 0.0}]}
    m = {"2018-01-02": 100.0, "2020-01-02": 110.0, "2024-01-02": 120.0, "2025-01-02": 120.0}
    dates = sorted(m)
    out = to_index_relative(rep, m, dates)
    # full: strat 1.0->2.2 = +1.2 ; idx 100->120 = +0.2 ; excess +1.0
    assert math.isclose(out["excess_return"], 1.0, abs_tol=1e-9), out["excess_return"]
    tr = [s for s in out["regime_slices"] if s["label"] == "train"][0]
    # train: nav 1.0->1.5 = +0.5 ; idx 100->110 = +0.1 ; excess +0.4
    assert math.isclose(tr["excess"], 0.4, abs_tol=1e-9), tr
    oos = [s for s in out["regime_slices"] if "OOS" in s["label"]][0]
    # OOS: nav 2.0->2.2 = +0.1 ; idx 120->120 = 0 ; excess +0.1
    assert math.isclose(oos["excess"], 0.1, abs_tol=1e-9), oos
    # risk passthrough (absolute sharpe unchanged)
    assert out["risk"]["sharpe"] == 1.0


if __name__ == "__main__":
    for name, fn in sorted(globals().items()):
        if name.startswith("test_") and callable(fn):
            fn(); print(f"PASS {name}")
    print("all passed")
