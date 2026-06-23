# scripts/test_train_factor_weights.py
import numpy as np, pandas as pd
import train_factor_weights as tw
from build_factor_matrix import FACTOR_COLS

def _toy_panel():
    rng = np.random.default_rng(0); rows = []
    for d in pd.bdate_range("2018-01-02", "2023-06-30", freq="5B").strftime("%Y-%m-%d"):
        for s in range(50):
            x = rng.normal(size=len(FACTOR_COLS))
            fwd = 0.8 * x[0] - 0.5 * x[3] + rng.normal(scale=0.3)   # f_bm 正、f_roe 负
            rows.append([d, f"s{s}", *x, fwd])
    return pd.DataFrame(rows, columns=["date", "symbol", *FACTOR_COLS, "fwd_ret_5d"])

def test_build_xy_shapes_and_rank_range():
    X, y, dates = tw.build_xy(_toy_panel(), "2018-01-01", "2023-12-31")
    assert X.shape[1] == len(FACTOR_COLS)
    assert X.min() >= 0 and X.max() <= 1                  # 排名归一∈[0,1]
    assert len(y) == X.shape[0]

def test_train_learns_expected_signs():
    panel = _toy_panel()
    X, y, _ = tw.build_xy(panel, "2018-01-01", "2023-12-31")
    import factor_lib as fl
    w = fl.elastic_net_fit(X, y, alpha=0.001, l1_ratio=0.5)
    assert w[0] > 0           # f_bm 正贡献
    assert w[3] < 0           # f_roe 负贡献（构造如此）
