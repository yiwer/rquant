import numpy as np, pandas as pd
import build_factor_matrix as bm


def test_atr14_first_values():
    high = np.array([10.0, 11, 12]); low = np.array([9.0, 9.5, 11]); close = np.array([9.5, 10.5, 11.5])
    a = bm.atr14(high, low, close)
    assert np.isnan(a[0])                      # 首根无前收→NaN
    assert a[-1] > 0 and not np.isnan(a[-1])


def test_factor_cols_count_and_order():
    assert bm.FACTOR_COLS[0] == "f_bm" and bm.FACTOR_COLS[-1] == "f_secmom"
    assert len(bm.FACTOR_COLS) == 13


def test_compute_symbol_factors_pit_and_label():
    n = 140
    dates = pd.bdate_range("2020-01-01", periods=n).strftime("%Y-%m-%d")
    close = pd.Series(np.linspace(10, 24, n))   # 单调上行
    kday = pd.DataFrame({"time": dates, "open": close, "high": close*1.01,
                         "low": close*0.99, "close": close, "volume": 1e6, "amount": close*1e6})
    fund = pd.DataFrame({"time": [dates[0]], "roe": [12.0], "np_yoy": [30.0], "rev_yoy": [10.0],
                         "gross_margin": [40.0], "eps": [1.0], "bps": [5.0]})
    out = bm.compute_symbol_factors(kday, fund, None)
    row = out.loc[dates[130]]                   # 中段某日
    assert np.isclose(row["f_bm"], 5.0 / close.iloc[130])     # bps/close
    assert np.isclose(row["f_npyoy"], 30.0)                   # 时点财务前向填充
    assert row["f_mom20"] > 0                                 # 上行→正动量
    # 标签=未来5日收益，单调上行应为正
    assert row["fwd_ret_5d"] > 0
    assert np.isnan(out.iloc[-1]["fwd_ret_5d"])              # 末尾无未来→NaN
    assert np.isnan(row["f_secmom"])                          # sec=None→NaN
