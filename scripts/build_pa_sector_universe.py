#!/usr/bin/env python3
"""合并 财务(as-of) + PA(滞后1日) + 板块(滞后1日) → 一份 fundamentals → universe_pa_sector.csv。"""
import os, glob, sys, csv
import pandas as pd

try:
    sys.stdout.reconfigure(encoding="utf-8")
except Exception:
    pass

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BS = os.path.join(REPO, "data", "baostock")
KDAY = os.path.join(BS, "kday")
PA = os.path.join(BS, "pa_features")
SEC = os.path.join(BS, "sector_factors")
FUND = os.path.join(REPO, "data", "fundamentals")
OUT = os.path.join(BS, "pa_sector_merged")
UNIV = os.path.join(BS, "universe_pa_sector.csv")
FIN_COLS = ["roe", "np_yoy", "rev_yoy", "gross_margin", "eps", "bps"]


def merge_frames(pa, sec, fin):
    """pa/sec: time + 因子列(已滞后)；fin: time + 财务列(公告日)。→ 以 pa 的日期为基准 outer-merge sec + as-of fin。"""
    m = pd.merge(pa.sort_values("time"), sec.sort_values("time"), on="time", how="left")
    fin_cols = [c for c in FIN_COLS if c in fin.columns]
    m = pd.merge_asof(m.sort_values("time"), fin[["time"] + fin_cols].sort_values("time"),
                      on="time", direction="backward")
    return m


def merge_one(sym):
    os.makedirs(OUT, exist_ok=True)
    pp = os.path.join(PA, f"{sym}.csv"); sp = os.path.join(SEC, f"{sym}.csv")
    fp = os.path.join(FUND, f"{sym}.csv")
    if not (os.path.exists(pp) and os.path.exists(fp)):
        return False
    pa = pd.read_csv(pp); pa["time"] = pd.to_datetime(pa["time"])
    if os.path.exists(sp):
        sec = pd.read_csv(sp); sec["time"] = pd.to_datetime(sec["time"])
    else:
        sec = pd.DataFrame({"time": pa["time"], "sec_mom20": float("nan"),
                            "sec_trend": float("nan"), "sec_breadth": float("nan"), "sec_heat": float("nan")})
    fin = pd.read_csv(fp); fin["time"] = pd.to_datetime(fin["time"])
    m = merge_frames(pa, sec, fin)
    m["time"] = m["time"].dt.strftime("%Y-%m-%d")
    m.to_csv(os.path.join(OUT, f"{sym}.csv"), index=False)
    return True


def main():
    os.makedirs(OUT, exist_ok=True)
    syms = sorted(os.path.basename(p)[:-4] for p in glob.glob(os.path.join(PA, "*.csv")))
    ok = []
    for i, s in enumerate(syms, 1):
        try:
            if os.path.exists(os.path.join(KDAY, f"{s}.csv")) and merge_one(s):
                ok.append(s)
        except Exception as e:
            print(f"  skip {s}: {e}")
        if i % 400 == 0:
            print(f"  {i}/{len(syms)}...")
    with open(UNIV, "w", newline="", encoding="utf-8") as f:
        w = csv.writer(f); w.writerow(["symbol", "primary", "context", "fundamentals"])
        for s in ok:
            w.writerow([s, os.path.join(KDAY, f"{s}.csv").replace("\\", "/"), "",
                        os.path.join(OUT, f"{s}.csv").replace("\\", "/")])
    print(f"wrote {UNIV}: {len(ok)} syms")


if __name__ == "__main__":
    main()
