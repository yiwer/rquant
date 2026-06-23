# scripts/test_eval_linear_score.py
import numpy as np, pandas as pd
import eval_linear_score as ev
from build_factor_matrix import FACTOR_COLS

def _panel_two_dates():
    rows = []
    # 两个调仓日，每日 4 只票；f_bm 越大未来收益越高（单因子可分）
    for d, base in [("2024-01-02", 0.10), ("2024-01-09", 0.05)]:
        for s, bm in enumerate([0.1, 0.2, 0.3, 0.4]):
            x = [0.0]*len(FACTOR_COLS); x[0] = bm                       # 仅 f_bm 变化
            fwd = bm + base                                             # 越便宜未来越高
            rows.append([d, f"s{s}", *x, fwd])
    p = pd.DataFrame(rows, columns=["date","symbol",*FACTOR_COLS,"fwd_ret_5d"])
    p["f_roe"] = 10.0; p["f_logamt"] = 20.0                            # 过硬闸：roe>0、流动性高
    return p

def test_backtest_top1_picks_highest_score():
    p = _panel_two_dates()
    w = np.zeros(len(FACTOR_COLS)); w[0] = 1.0                          # 只用 f_bm
    rep = ev.backtest(p, w, top_n=1, cost_bps=0.0, st_set=set())
    # top-1 每期选 f_bm 最大(s3)，收益=其 fwd；两期复利
    navs = [h["nav"] for h in rep["holdings"]]
    assert navs[-1] > 1.0
    assert rep["n_rebalances"] == 2

def test_zero_cost_gross_ge_net():
    p = _panel_two_dates(); w = np.zeros(len(FACTOR_COLS)); w[0] = 1.0
    g = ev.backtest(p, w, 2, 0.0, set()); n = ev.backtest(p, w, 2, 20.0, set())
    assert g["total_return"] >= n["total_return"] - 1e-9

def test_st_excluded():
    p = _panel_two_dates(); w = np.zeros(len(FACTOR_COLS)); w[0] = 1.0
    rep = ev.backtest(p, w, 1, 0.0, st_set={"s3"})                      # 剔最高分 s3
    # s3 被剔 → top-1 应回补 s2，不应出现 s3
    assert all("s3" not in h.get("picks", []) for h in rep["holdings"])
