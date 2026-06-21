#!/usr/bin/env python3
"""build_sector_factors 单测：板块动量/趋势/广度公式 + 无前视。
跑：python -m pytest scripts/test_build_sector_factors.py -q"""
import numpy as np
import pandas as pd
from build_sector_factors import sector_factors


def test_mom20_and_breadth():
    # 单一板块 30 天，每天 +1% → index 复利上涨；mom20 = index[t]/index[t-20]-1
    n = 30
    panel = pd.DataFrame({
        "date": pd.date_range("2020-01-01", periods=n, freq="D"),
        "industry": ["A01农业"] * n,
        "ret": [0.01] * n,
        "breadth": [0.6] * n,
    })
    f = sector_factors(panel)
    idx = (1.01 ** 20) - 1.0
    assert np.isclose(f["sec_mom20"].iloc[-1], idx, rtol=1e-6)
    assert np.isclose(f["sec_breadth"].iloc[-1], 0.6, rtol=1e-9)   # 5日均=0.6


def test_two_sectors_independent():
    n = 25
    a = pd.DataFrame({"date": pd.date_range("2020-01-01", periods=n), "industry": "A",
                      "ret": 0.0, "breadth": 0.5})
    b = pd.DataFrame({"date": pd.date_range("2020-01-01", periods=n), "industry": "B",
                      "ret": 0.02, "breadth": 0.9})
    f = sector_factors(pd.concat([a, b]))
    fa = f[f["industry"] == "A"]["sec_mom20"].iloc[-1]
    fb = f[f["industry"] == "B"]["sec_mom20"].iloc[-1]
    assert fb > fa                                   # B 板块动量更强
