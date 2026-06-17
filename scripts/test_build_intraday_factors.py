#!/usr/bin/env python3
"""build_intraday_factors 单测：钉死 6 因子公式 + 14:45 无前视截断 + 弃权边界。

跑：python -m pytest scripts/test_build_intraday_factors.py -q
或：python scripts/test_build_intraday_factors.py
"""
import math
import pandas as pd
from build_intraday_factors import intraday_day_factors


def _day(bars):
    """bars: list of (hhmm, o,h,l,c,v) on 2025-12-10 → DataFrame(time datetime,...)."""
    rows = [{"time": pd.Timestamp(f"2025-12-10 {hhmm}:00"),
             "open": o, "high": h, "low": l, "close": c, "volume": v}
            for (hhmm, o, h, l, c, v) in bars]
    return pd.DataFrame(rows)


# 三快照 bar（09:45/13:45/14:45）+ prev_close=9.9；手算见 spec
BASE = [("09:45", 10.0, 10.5, 9.8, 10.2, 100),
        ("13:45", 10.2, 10.3, 10.0, 10.1, 200),
        ("14:45", 10.1, 10.4, 10.0, 10.3, 300)]
PREV = 9.9


def test_factor_formulas():
    f = intraday_day_factors(_day(BASE), PREV)
    assert math.isclose(f["last_leg"], 10.3 / 10.1 - 1, rel_tol=1e-9)
    assert math.isclose(f["intraday_rev"], -(10.3 / 10.0 - 1), rel_tol=1e-9)
    # vwap: tp*(v) / Σv，tp=(h+l+c)/3
    vwap = ((10.5 + 9.8 + 10.2) / 3 * 100 + (10.3 + 10.0 + 10.1) / 3 * 200
            + (10.4 + 10.0 + 10.3) / 3 * 300) / 600
    assert math.isclose(f["close_vs_vwap"], 10.3 / vwap - 1, rel_tol=1e-9)
    assert math.isclose(f["intraday_range"], (10.5 - 9.8) / 9.9, rel_tol=1e-9)
    assert math.isclose(f["vol_tilt"], 500 / 600, rel_tol=1e-9)  # PM(13:45+14:45)=500
    assert math.isclose(f["overnight"], 10.0 / 9.9 - 1, rel_tol=1e-9)


def test_no_lookahead_1500_bar_excluded():
    """加一根极端 15:00 bar，因子必须与不加时完全一致（快照 ≤14:45 排除之）。"""
    base = intraday_day_factors(_day(BASE), PREV)
    withclose = intraday_day_factors(_day(BASE + [("15:00", 10.3, 12.0, 10.3, 11.5, 999)]), PREV)
    for k in base:
        assert (math.isnan(base[k]) and math.isnan(withclose[k])) or \
               math.isclose(base[k], withclose[k], rel_tol=1e-12), f"{k} leaked 15:00 info"


def test_halfday_abstain():
    """快照 <2 bar（仅 09:45）→ 全弃权 NaN。"""
    f = intraday_day_factors(_day([("09:45", 10.0, 10.5, 9.8, 10.2, 100)]), PREV)
    assert all(math.isnan(v) for v in f.values())


def test_missing_prevclose():
    """prev_close=NaN → overnight & intraday_range NaN，其余有限。"""
    f = intraday_day_factors(_day(BASE), float("nan"))
    assert math.isnan(f["overnight"]) and math.isnan(f["intraday_range"])
    for k in ("last_leg", "intraday_rev", "close_vs_vwap", "vol_tilt"):
        assert not math.isnan(f[k]), k


def test_last_leg_nan_when_no_1345_bar():
    """无 13:45 bar → last_leg NaN，其余快照因子仍算。"""
    bars = [("09:45", 10.0, 10.5, 9.8, 10.2, 100), ("14:45", 10.1, 10.4, 10.0, 10.3, 300)]
    f = intraday_day_factors(_day(bars), PREV)
    assert math.isnan(f["last_leg"])
    assert not math.isnan(f["intraday_rev"]) and not math.isnan(f["vol_tilt"])


if __name__ == "__main__":
    for name, fn in sorted(globals().items()):
        if name.startswith("test_") and callable(fn):
            fn(); print(f"PASS {name}")
    print("all passed")
