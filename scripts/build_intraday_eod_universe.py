#!/usr/bin/env python3
"""多年「日内→日频」回测 universe：primary=kday(日线→前向收益)，fundamentals=features_15m 的
EOD(15:00) 31 指标快照，**滞后 1 交易日**(无前视)。

为每股取每日最后一根 15m bar(15:00) 的 31 个指标 → 戳到该股的「下一交易日」(kday) →
当日可据此交易(信号来自昨收)。滞后 1 日彻底消除同根前视(对应 2026-06-18 的 ≤14:45 截断)。

输出：
  data/baostock/features_15m_eod_lag1/<sym>.csv  (date-only time + 31 列)
  data/baostock/universe_intraday_day.csv         (symbol,primary,context,fundamentals;绝对路径正斜杠)

诚实文化：缺数据/warmup NaN 原样保留(引擎弃权)，绝不臆造。
"""
import os, glob, sys, csv
import numpy as np
import pandas as pd

try:
    sys.stdout.reconfigure(encoding="utf-8")
except Exception:
    pass

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BS = os.path.join(REPO, "data", "baostock")
FEAT = os.path.join(BS, "features_15m")
KDAY = os.path.join(BS, "kday")
LAG = os.path.join(BS, "features_15m_eod_lag1")
UNIV = os.path.join(BS, "universe_intraday_day.csv")


def kday_dates(sym):
    p = os.path.join(KDAY, f"{sym}.csv")
    if not os.path.exists(p):
        return None
    d = pd.read_csv(p, usecols=["time"])
    return np.array(sorted(pd.to_datetime(d["time"]).dt.strftime("%Y-%m-%d").unique()))


def build_one(sym):
    fp = os.path.join(FEAT, f"{sym}.csv")
    kd = kday_dates(sym)
    if kd is None or len(kd) == 0:
        return None
    df = pd.read_csv(fp)
    if len(df) == 0:
        return None
    df = df.sort_values("time")
    df["_date"] = pd.to_datetime(df["time"]).dt.strftime("%Y-%m-%d")
    eod = df.groupby("_date").tail(1).copy()          # 每日最后一根(15:00)的真实行，保留 NaN
    eod = eod.sort_values("_date")
    # 滞后 1 交易日：EOD date d → 下一 kday(strictly >) = 行动日
    idx = np.searchsorted(kd, eod["_date"].values, side="right")
    valid = idx < len(kd)
    eod = eod[valid].copy()
    eod["time"] = kd[idx[valid]]
    feat_cols = [c for c in df.columns if c not in ("time", "_date")]
    out = eod[["time"] + feat_cols]
    out.to_csv(os.path.join(LAG, f"{sym}.csv"), index=False)
    return sym


def main():
    os.makedirs(LAG, exist_ok=True)
    syms = sorted(os.path.basename(p)[:-4] for p in glob.glob(os.path.join(FEAT, "*.csv")))
    ok = []
    for i, s in enumerate(syms, 1):
        try:
            if build_one(s):
                ok.append(s)
        except Exception as e:
            print(f"  skip {s}: {e}")
        if i % 200 == 0:
            print(f"  {i}/{len(syms)}...")
    with open(UNIV, "w", newline="", encoding="utf-8") as f:
        w = csv.writer(f)
        w.writerow(["symbol", "primary", "context", "fundamentals"])
        for s in ok:
            w.writerow([s, os.path.join(KDAY, f"{s}.csv").replace("\\", "/"), "",
                        os.path.join(LAG, f"{s}.csv").replace("\\", "/")])
    print(f"wrote {UNIV}: {len(ok)} symbols; lagged EOD features -> {LAG}")


if __name__ == "__main__":
    main()
