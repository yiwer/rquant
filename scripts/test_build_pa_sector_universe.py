#!/usr/bin/env python3
"""build_pa_sector_universe 单测：财务 as-of merge 正确 + 列齐。
跑：python -m pytest scripts/test_build_pa_sector_universe.py -q"""
import pandas as pd
from build_pa_sector_universe import merge_frames


def test_merge_asof_fin_and_factors():
    pa = pd.DataFrame({"time": pd.to_datetime(["2020-03-01", "2020-03-02"]),
                       "pa_ema20": [0.01, 0.02]})
    sec = pd.DataFrame({"time": pd.to_datetime(["2020-03-01", "2020-03-02"]),
                        "sec_mom20": [0.1, 0.1]})
    fin = pd.DataFrame({"time": pd.to_datetime(["2019-10-31", "2020-04-30"]),
                        "bps": [5.0, 6.0]})
    m = merge_frames(pa, sec, fin)
    assert list(m["pa_ema20"]) == [0.01, 0.02]
    assert list(m["sec_mom20"]) == [0.1, 0.1]
    assert list(m["bps"]) == [5.0, 5.0]        # 3月用2019Q3(≤t)的 5.0，不前视用 2020-04-30
