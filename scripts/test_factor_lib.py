import numpy as np
import factor_lib as fl

def test_cross_sectional_rank_basic():
    r = fl.cross_sectional_rank(np.array([10.0, 20.0, 30.0]))
    assert np.allclose(r, [0.0, 0.5, 1.0])

def test_cross_sectional_rank_ties_and_nan():
    r = fl.cross_sectional_rank(np.array([5.0, 5.0, 9.0, np.nan]))
    assert np.isclose(r[0], r[1])          # 并列同分
    assert np.isclose(r[3], 0.5)           # NaN→中位
    assert r[2] == max(r[:3])              # 最大值排名最高

def test_rank_ic_monotonic():
    x = np.array([1.0, 2, 3, 4, 5]); y = np.array([2.0, 4, 6, 8, 10])
    assert np.isclose(fl.rank_ic(x, y), 1.0)
    assert np.isclose(fl.rank_ic(x, -y), -1.0)

def test_elastic_net_recovers_sparse_weights():
    rng = np.random.default_rng(0)
    X = rng.normal(size=(2000, 5))
    w_true = np.array([2.0, 0.0, -1.5, 0.0, 0.0])
    y = X @ w_true + rng.normal(scale=0.05, size=2000)
    w = fl.elastic_net_fit(X, y, alpha=0.01, l1_ratio=0.5)
    assert abs(w[0] - 2.0) < 0.3 and abs(w[2] + 1.5) < 0.3
    assert abs(w[1]) < 0.2 and abs(w[3]) < 0.2 and abs(w[4]) < 0.2  # L1 压无关项

def test_elastic_net_l1ratio0_matches_ridge():
    rng = np.random.default_rng(1)
    X = rng.normal(size=(500, 4)); y = rng.normal(size=500)
    w = fl.elastic_net_fit(X, y, alpha=0.1, l1_ratio=0.0, max_iter=5000)
    Xc = X - X.mean(0); yc = y - y.mean()
    n = len(y); ridge = np.linalg.solve(Xc.T @ Xc / n + 0.1*np.eye(4), Xc.T @ yc / n)
    assert np.allclose(w, ridge, atol=1e-3)
