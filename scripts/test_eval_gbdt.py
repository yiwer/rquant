# scripts/test_eval_gbdt.py
import numpy as np
import pandas as pd
import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import eval_gbdt as eg
from build_factor_matrix import FACTOR_COLS


class _Stub:
    """Fake model: prediction = first column (f_bm)."""
    def predict(self, X):
        return np.asarray(X)[:, 0]


def _panel():
    rows = []
    for d, b in [("2024-01-02", 0.0), ("2024-01-09", 0.01)]:
        for s, v in enumerate([0.40, 0.41, 0.30]):
            x = [0.0] * len(FACTOR_COLS)
            x[0] = v
            rows.append([d, f"s{s}", *x, 0.05])
    p = pd.DataFrame(rows, columns=["date", "symbol", *FACTOR_COLS, "fwd_ret_5d"])
    p["f_roe"] = 10
    p["f_logamt"] = 20
    return p


def test_gbdt_hysteresis_reduces_turnover():
    p = _panel()
    m = [_Stub()]
    t0 = eg.backtest_gbdt(p, m, 1, 0.0, set(), delta=0.0)["turnover"]
    t1 = eg.backtest_gbdt(p, m, 1, 0.0, set(), delta=0.5)["turnover"]
    assert t1 <= t0


def test_gbdt_backtest_zero_cost_ge_net():
    p = _panel()
    m = [_Stub()]
    g = eg.backtest_gbdt(p, m, 2, 0.0, set(), 0.0)["total_return"]
    n = eg.backtest_gbdt(p, m, 2, 20.0, set(), 0.0)["total_return"]
    assert g >= n - 1e-9
