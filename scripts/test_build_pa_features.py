#!/usr/bin/env python3
"""build_pa_features 单测：钉死 PA 特征公式 + 无前视(滚动结构/不看未来)。
跑：python -m pytest scripts/test_build_pa_features.py -q"""
import numpy as np
import pandas as pd
from build_pa_features import pa_features


def _series(closes, highs=None, lows=None, opens=None):
    n = len(closes)
    highs = highs or [c * 1.01 for c in closes]
    lows = lows or [c * 0.99 for c in closes]
    opens = opens or [c for c in closes]
    return pd.DataFrame({
        "time": pd.date_range("2020-01-01", periods=n, freq="D"),
        "open": opens, "high": highs, "low": lows, "close": closes,
    })


def test_ema20_and_dir_uptrend():
    # 单调上涨 60 天 → EMA20 上方、方向为 +1
    df = _series([100 + i for i in range(60)])
    f = pa_features(df)
    assert f["pa_ema20"].iloc[-1] > 0          # 收盘在 EMA20 上方
    assert f["pa_dir"].iloc[-1] == 1           # EMA20 斜率向上


def test_regime_trend_vs_range():
    # 平滑趋势 ER 高；锯齿震荡 ER 低
    trend = pa_features(_series([100 + i for i in range(40)]))["pa_regime"].iloc[-1]
    chop = pa_features(_series([100 + (i % 2) for i in range(40)]))["pa_regime"].iloc[-1]
    assert trend > 0.8 and chop < 0.2


def test_struct_hh_hl_uptrend():
    # 阶梯上涨：高点/低点都抬高 → pa_struct = +2
    closes = [100 + i for i in range(40)]
    f = pa_features(_series(closes))
    assert f["pa_struct"].iloc[-1] == 2


def test_no_lookahead_shape_only_past():
    # 在 t 处的特征只依赖 ≤t；篡改未来某天不应改变更早的特征值
    base = [100 + np.sin(i / 3) for i in range(60)]
    f0 = pa_features(_series(base))
    bumped = list(base); bumped[55] += 10        # 改第 55 天
    f1 = pa_features(_series(bumped))
    # 第 40 天的特征不受未来(第55天)影响
    for col in ["pa_ema20", "pa_struct", "pa_regime", "pa_pullback"]:
        assert f0[col].iloc[40] == f1[col].iloc[40] or (
            pd.isna(f0[col].iloc[40]) and pd.isna(f1[col].iloc[40]))


def test_h1_h2_pullback_entry():
    # 上涨→回调3天→连续2根创新高：H1 在首根突破、H2 在次根
    closes = [100, 102, 104, 106, 108, 110,  # 上涨
              108, 106, 104,                  # 回调
              107, 109]                       # 连续2根反抽(高点抬升)
    highs = [c + 1 for c in closes]
    f = pa_features(_series(closes, highs=highs))
    assert f["pa_h1"].iloc[-2] == 1           # 倒数第二根 = H1
    assert f["pa_h2"].iloc[-1] == 1           # 最后一根 = H2
