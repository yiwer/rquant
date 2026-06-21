#!/usr/bin/env python3
"""板块轮动因子（动量/趋势/广度/成交额热度）→ 逐股(其所属板块)，滞后1日无前视。
输出 data/baostock/sector_factors/<sym>.csv，供 pa_overlay 树经 fund.<col> 取用。"""
import os, glob, sys
import numpy as np
import pandas as pd

try:
    sys.stdout.reconfigure(encoding="utf-8")
except Exception:
    pass

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BS = os.path.join(REPO, "data", "baostock")
KDAY = os.path.join(BS, "kday")
PANEL = os.path.join(BS, "sector_daily_panel.csv")
MEMB = os.path.join(BS, "sector_membership.csv")
OUT = os.path.join(BS, "sector_factors")


def sector_factors(panel):
    """panel: date,industry,ret,breadth → 每(date,industry) 因子。"""
    p = panel.copy()
    p["date"] = pd.to_datetime(p["date"])
    p = p.sort_values(["industry", "date"])
    g = p.groupby("industry", group_keys=False)
    p["index"] = g["ret"].apply(lambda r: (1.0 + r).cumprod())
    p["sec_mom20"] = g["index"].apply(lambda x: x / x.shift(20) - 1.0)
    p["sec_trend"] = g["index"].apply(lambda x: x / x.rolling(20).mean() - 1.0)
    p["sec_breadth"] = g["breadth"].apply(lambda x: x.rolling(5).mean())
    return p[["date", "industry", "sec_mom20", "sec_trend", "sec_breadth"]]


def main():
    os.makedirs(OUT, exist_ok=True)
    panel = pd.read_csv(PANEL)
    panel["date"] = pd.to_datetime(panel["date"]).dt.strftime("%Y-%m-%d")
    sf = sector_factors(panel)
    sf["date"] = pd.to_datetime(sf["date"]).dt.strftime("%Y-%m-%d")
    memb = pd.read_csv(MEMB, encoding="utf-8")[["symbol", "industry"]]
    s2i = dict(zip(memb["symbol"], memb["industry"]))
    # 板块成交额热度：聚合各股 amount → 板块日成交额 / 其 MA20
    amt = {}
    for p in glob.glob(os.path.join(KDAY, "*.csv")):
        s = os.path.basename(p)[:-4]
        ind = s2i.get(s)
        if ind is None:
            continue
        d = pd.read_csv(p, usecols=["time", "amount"])
        d["date"] = pd.to_datetime(d["time"]).dt.strftime("%Y-%m-%d")
        amt.setdefault(ind, []).append(d[["date", "amount"]])
    heat_rows = []
    for ind, dfs in amt.items():
        a = pd.concat(dfs).groupby("date", as_index=False)["amount"].sum().sort_values("date")
        a["sec_heat"] = a["amount"] / a["amount"].rolling(20).mean()
        a["industry"] = ind
        heat_rows.append(a[["date", "industry", "sec_heat"]])
    heat = pd.concat(heat_rows) if heat_rows else pd.DataFrame(columns=["date", "industry", "sec_heat"])
    sf = sf.merge(heat, on=["date", "industry"], how="left")
    cols = ["sec_mom20", "sec_trend", "sec_breadth", "sec_heat"]
    ok = 0
    for s, ind in s2i.items():
        sub = sf[sf["industry"] == ind].sort_values("date")
        if len(sub) < 25:
            continue
        out = sub[["date"] + cols].copy()
        out[cols] = out[cols].shift(1)                 # 滞后1日
        out = out.rename(columns={"date": "time"}).iloc[1:]
        out.to_csv(os.path.join(OUT, f"{s}.csv"), index=False)
        ok += 1
    print(f"built {ok} sector-factor CSVs -> {OUT}")


if __name__ == "__main__":
    main()
