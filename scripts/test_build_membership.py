"""build_membership 纯逻辑自测（无 pytest）：python scripts/test_build_membership.py → exit 0=pass。"""
import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import pandas as pd
import build_membership as bm

def test_rank_top_n():
    t = {"A": 100.0, "B": 300.0, "C": 200.0, "D": float("nan"), "E": -1.0}
    assert bm.rank_top_n(t, 2) == ["B", "C"]
    assert bm.rank_top_n(t, 10) == ["B", "C", "A"]   # NaN/负剔除
    print("ok rank_top_n")

def test_point_in_time_survivorship():
    # A 仅活到 2018-02-28("退市")，B 全程到 2018-03-31
    idx1 = pd.date_range("2018-01-01", "2018-02-28", freq="D")
    idx2 = pd.date_range("2018-01-01", "2018-03-31", freq="D")
    panel = {
        "A": pd.DataFrame({"close":[10.0]*len(idx1), "volume":[100.0]*len(idx1)}, index=idx1),
        "B": pd.DataFrame({"close":[10.0]*len(idx2), "volume":[ 50.0]*len(idx2)}, index=idx2),
    }
    mem = {d.strftime("%Y-%m"): set(s) for d, s in
           bm.compute_membership(panel, top=10, lookback=20, start="2018-01-01")}
    assert mem["2018-02"] == {"A", "B"}, mem   # A 活跃期入选(survivorship-free)
    assert mem["2018-03"] == {"B"}, mem        # A 退市后无 bar 自动出
    print("ok point_in_time_survivorship")

if __name__ == "__main__":
    test_rank_top_n(); test_point_in_time_survivorship(); print("ALL PASS")
