#!/usr/bin/env python3
"""build_indicators 单测：钉关键公式 + 因果性(无前视，前缀稳定) + 输出形态。

跑：python scripts/test_build_indicators.py
"""
import math
import numpy as np
import pandas as pd
from build_indicators import compute_indicators


def _bars(n=80):
    c = np.array([10.0 + i for i in range(n)])          # 10,11,...
    return pd.DataFrame({
        "time": pd.date_range("2021-01-04 09:45:00", periods=n, freq="15min").strftime("%Y-%m-%d %H:%M:%S"),
        "open": c, "high": c + 0.5, "low": c - 0.5, "close": c,
        "volume": np.array([1000.0 + 10 * i for i in range(n)]),
    })


def test_ret_ma_ema_formulas():
    f = compute_indicators(_bars())
    assert math.isclose(f["ret"].iloc[1], 11/10 - 1, rel_tol=1e-12)
    assert math.isclose(f["ma5"].iloc[4], (10+11+12+13+14)/5, rel_tol=1e-12)
    assert math.isnan(f["ma5"].iloc[3])  # 不足窗 → NaN
    assert math.isclose(f["ema12"].iloc[0], 10.0, rel_tol=1e-12)  # adjust=False 以首值播种
    a = 2/13
    assert math.isclose(f["ema12"].iloc[1], a*11 + (1-a)*10, rel_tol=1e-12)


def test_boll_and_macd():
    f = compute_indicators(_bars())
    c = np.array([10.0+i for i in range(80)])
    assert math.isclose(f["boll_mid"].iloc[19], c[:20].mean(), rel_tol=1e-12)
    # 线性序列：dif=ema12-ema26 应为正且有限；hist=2*(dif-dea)
    assert np.isfinite(f["macd_dif"].iloc[-1])
    assert math.isclose(f["macd_hist"].iloc[-1], 2*(f["macd_dif"].iloc[-1]-f["macd_dea"].iloc[-1]), rel_tol=1e-9)


def test_rsi_all_gains_is_100():
    # 单调上升 → 无下跌 → avg_loss=0 → RSI=100
    f = compute_indicators(_bars())
    assert f["rsi14"].iloc[-1] > 99.9


def test_causality_prefix_stable():
    """因果性：compute(full).iloc[:k] 必须等于 compute(prefix_k)（rolling/ewm 只用 ≤t）。"""
    full = compute_indicators(_bars(80))
    for k in (40, 60, 75):
        pref = compute_indicators(_bars(80).iloc[:k].copy())
        a = full.iloc[:k].reset_index(drop=True)
        b = pref.reset_index(drop=True)
        for col in b.columns:
            if col == "time":
                continue
            x, y = a[col].to_numpy(), b[col].to_numpy()
            both_nan = np.isnan(x) & np.isnan(y)
            assert np.allclose(x[~both_nan], y[~both_nan], rtol=1e-9, atol=1e-9), f"{col} not prefix-stable at k={k}"


def test_shape_and_no_crash():
    f = compute_indicators(_bars())
    assert len(f) == 80
    assert f.columns[0] == "time"
    for must in ("ret","ma20","macd_hist","rsi14","boll_pctb","atr14","kdj_j","cci14","wr14","obv","vwap20","roc12","rvol20","corr_pv20"):
        assert must in f.columns, must


if __name__ == "__main__":
    for name, fn in sorted(globals().items()):
        if name.startswith("test_") and callable(fn):
            fn(); print(f"PASS {name}")
    print("all passed")
