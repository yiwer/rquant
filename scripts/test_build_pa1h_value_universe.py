#!/usr/bin/env python3
"""build_pa1h_value_universe 单测：1h 重采样(每4根15m合1) + 财务 as-of merge。
跑：python -m pytest scripts/test_build_pa1h_value_universe.py -q"""
import pandas as pd
from build_pa1h_value_universe import resample_1h, merge_frames, eod_lag1


def test_resample_4x15m_to_1h():
    # 一日 8 根 15m → 2 根 1h；每 4 根：open=首 high=max low=min close=末 vol=和
    t = pd.date_range("2021-01-04 09:45", periods=8, freq="15min")
    df = pd.DataFrame({"time": t,
                       "open":  [10, 11, 12, 13, 20, 21, 22, 23],
                       "high":  [10.5, 11.5, 12.5, 13.5, 20.5, 21.5, 22.5, 23.5],
                       "low":   [9.5, 10.5, 11.5, 12.5, 19.5, 20.5, 21.5, 22.5],
                       "close": [11, 12, 13, 14, 21, 22, 23, 24],
                       "volume":[1, 2, 3, 4, 5, 6, 7, 8]})
    h = resample_1h(df).reset_index(drop=True)
    assert len(h) == 2
    assert h.loc[0, "open"] == 10 and h.loc[0, "close"] == 14
    assert h.loc[0, "high"] == 13.5 and h.loc[0, "low"] == 9.5
    assert h.loc[0, "volume"] == 10            # 1+2+3+4
    assert h.loc[1, "open"] == 20 and h.loc[1, "close"] == 24 and h.loc[1, "volume"] == 26


def test_merge_asof_financials_no_lookahead():
    pa = pd.DataFrame({"time": pd.to_datetime(["2020-03-02", "2020-03-03"]),
                       "pa_ema20": [0.01, 0.02]})
    fin = pd.DataFrame({"time": pd.to_datetime(["2019-10-31", "2020-04-30"]),
                        "bps": [5.0, 6.0]})
    m = merge_frames(pa, fin)
    assert list(m["pa_ema20"]) == [0.01, 0.02]
    assert list(m["bps"]) == [5.0, 5.0]        # 3月用2019Q3(≤t)，不前视2020-04-30


def test_eod_lag1_prior_trading_day():
    # 2 trading days, 2 1h bars each, distinct pa_ema20 per bar
    # Day1: bar1=0.10, bar2(EOD)=0.20; Day2: bar1=0.30, bar2(EOD)=0.40
    times = pd.to_datetime([
        "2021-01-04 10:30", "2021-01-04 11:30",  # day 1 bars
        "2021-01-05 10:30", "2021-01-05 11:30",  # day 2 bars
    ])
    feat = pd.DataFrame({"time": times, "pa_ema20": [0.10, 0.20, 0.30, 0.40]})
    result = eod_lag1(feat)
    # (a) exactly 1 row (day-2; day-1 is dropped after shift)
    assert len(result) == 1, f"expected 1 row, got {len(result)}"
    # (b) day-2's pa_ema20 == day-1's EOD bar value (0.20)
    assert result.iloc[0]["pa_ema20"] == 0.20, f"expected 0.20, got {result.iloc[0]['pa_ema20']}"
    # (c) output time for that row is day-2's date
    expected_date = pd.Timestamp("2021-01-05")
    assert result.iloc[0]["time"] == expected_date, f"expected {expected_date}, got {result.iloc[0]['time']}"
