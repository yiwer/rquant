#!/usr/bin/env python3
"""合并「财务 + 15m EOD(滞后1日)」fundamentals universe，用于测试 15m 作为价值核心的正交 tilt。

每股：features_15m_eod_lag1/<sym>.csv(日频 31 个 15m 指标，已滞后1日) 左连 merge_asof
data/fundamentals/<sym>.csv(季度点时财务 roe/np_yoy/rev_yoy/gross_margin/eps/bps，按公告日 as-of)。
→ data/baostock/merged_intraday/<sym>.csv（time + 31 15m 列 + 6 财务列；财务前向 as-of 填充）。
→ data/baostock/universe_intraday_merged.csv（primary=kday，fundamentals=merged）。

诚实：财务用公告日 as-of(point-in-time，无前视)；15m 已滞后1日。两者都不前视。
"""
import os, glob, sys, csv
import pandas as pd

try:
    sys.stdout.reconfigure(encoding="utf-8")
except Exception:
    pass

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BS = os.path.join(REPO, "data", "baostock")
LAG = os.path.join(BS, "features_15m_eod_lag1")
KDAY = os.path.join(BS, "kday")
FUND = os.path.join(REPO, "data", "fundamentals")
OUT = os.path.join(BS, "merged_intraday")
UNIV = os.path.join(BS, "universe_intraday_merged.csv")
FIN_COLS = ["roe", "np_yoy", "rev_yoy", "gross_margin", "eps", "bps"]


def build_one(sym):
    lp = os.path.join(LAG, f"{sym}.csv")
    if not os.path.exists(os.path.join(KDAY, f"{sym}.csv")):
        return None
    feat = pd.read_csv(lp)
    if len(feat) == 0:
        return None
    feat["time"] = pd.to_datetime(feat["time"])
    feat = feat.sort_values("time")
    fp = os.path.join(FUND, f"{sym}.csv")
    if os.path.exists(fp):
        fin = pd.read_csv(fp)
        fin["time"] = pd.to_datetime(fin["time"])
        fin = fin.sort_values("time")
        keep = ["time"] + [c for c in FIN_COLS if c in fin.columns]
        merged = pd.merge_asof(feat, fin[keep], on="time", direction="backward")
    else:
        merged = feat.copy()
        for c in FIN_COLS:
            merged[c] = float("nan")
    merged["time"] = merged["time"].dt.strftime("%Y-%m-%d")
    merged.to_csv(os.path.join(OUT, f"{sym}.csv"), index=False)
    return sym


def main():
    os.makedirs(OUT, exist_ok=True)
    syms = sorted(os.path.basename(p)[:-4] for p in glob.glob(os.path.join(LAG, "*.csv")))
    ok = []
    for i, s in enumerate(syms, 1):
        try:
            if build_one(s):
                ok.append(s)
        except Exception as e:
            print(f"  skip {s}: {e}")
        if i % 300 == 0:
            print(f"  {i}/{len(syms)}...")
    with open(UNIV, "w", newline="", encoding="utf-8") as f:
        w = csv.writer(f)
        w.writerow(["symbol", "primary", "context", "fundamentals"])
        for s in ok:
            w.writerow([s, os.path.join(KDAY, f"{s}.csv").replace("\\", "/"), "",
                        os.path.join(OUT, f"{s}.csv").replace("\\", "/")])
    print(f"wrote {UNIV}: {len(ok)} symbols; merged fundamentals -> {OUT}")


if __name__ == "__main__":
    main()
