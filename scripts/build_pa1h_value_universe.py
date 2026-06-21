#!/usr/bin/env python3
"""S2 用：k15m 重采样 1h → 复用 pa_features 算 PA(滞后1日) → merge 财务(as-of) → universe_pa1h_value.csv。"""
import os, glob, sys, csv
import pandas as pd

try:
    sys.stdout.reconfigure(encoding="utf-8")
except Exception:
    pass
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from build_pa_features import pa_features, COLS as PA_COLS

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BS = os.path.join(REPO, "data", "baostock")
K15M = os.path.join(BS, "k15m")
KDAY = os.path.join(BS, "kday")
FUND = os.path.join(REPO, "data", "fundamentals")
OUT = os.path.join(BS, "pa1h_value_merged")
UNIV = os.path.join(BS, "universe_pa1h_value.csv")
FIN_COLS = ["roe", "np_yoy", "rev_yoy", "gross_margin", "eps", "bps"]


def resample_1h(df15):
    """每交易日按顺序每 4 根 15m 合 1 根 1h（open首/high max/low min/close末/volume和）。"""
    d = df15.copy()
    d["time"] = pd.to_datetime(d["time"])
    d = d.sort_values("time").reset_index(drop=True)
    d["date"] = d["time"].dt.normalize()
    out = []
    for _, g in d.groupby("date", sort=True):
        g = g.reset_index(drop=True)
        for i in range(0, len(g), 4):
            blk = g.iloc[i:i + 4]
            out.append({"time": blk["time"].iloc[-1], "open": blk["open"].iloc[0],
                        "high": blk["high"].max(), "low": blk["low"].min(),
                        "close": blk["close"].iloc[-1], "volume": blk["volume"].sum()})
    return pd.DataFrame(out, columns=["time", "open", "high", "low", "close", "volume"])


def merge_frames(pa, fin):
    """pa: time + pa_*(已滞后)；fin: time + 财务(公告日)。→ pa 为基准 as-of-backward 并财务。"""
    fin_cols = [c for c in FIN_COLS if c in fin.columns]
    return pd.merge_asof(pa.sort_values("time"), fin[["time"] + fin_cols].sort_values("time"),
                         on="time", direction="backward")


def merge_one(sym):
    kp = os.path.join(K15M, f"{sym}.csv"); fp = os.path.join(FUND, f"{sym}.csv")
    if not (os.path.exists(kp) and os.path.exists(fp) and os.path.exists(os.path.join(KDAY, f"{sym}.csv"))):
        return False
    os.makedirs(OUT, exist_ok=True)
    h = resample_1h(pd.read_csv(kp, usecols=["time", "open", "high", "low", "close", "volume"]))
    if len(h) < 60:
        return False
    feat = pa_features(h)                       # time + pa_*
    feat[PA_COLS] = feat[PA_COLS].shift(1)      # 滞后1根(无前视)
    feat = feat.iloc[1:].copy()
    feat["time"] = pd.to_datetime(feat["time"]).dt.normalize()  # 1h 戳 → 当日(date) 供日频 as-of
    feat = feat.groupby("time").tail(1)        # 每日最后一根 1h 的 PA = 当日 EOD 1h-PA
    fin = pd.read_csv(fp); fin["time"] = pd.to_datetime(fin["time"])
    m = merge_frames(feat, fin)
    m["time"] = pd.to_datetime(m["time"]).dt.strftime("%Y-%m-%d")
    m.to_csv(os.path.join(OUT, f"{sym}.csv"), index=False)
    return True


def main():
    os.makedirs(OUT, exist_ok=True)
    syms = sorted(os.path.basename(p)[:-4] for p in glob.glob(os.path.join(K15M, "*.csv")))
    ok = []
    for i, s in enumerate(syms, 1):
        try:
            if merge_one(s):
                ok.append(s)
        except Exception as e:
            print(f"  skip {s}: {e}")
        if i % 300 == 0:
            print(f"  {i}/{len(syms)}...")
    with open(UNIV, "w", newline="", encoding="utf-8") as f:
        w = csv.writer(f); w.writerow(["symbol", "primary", "context", "fundamentals"])
        for s in ok:
            w.writerow([s, os.path.join(KDAY, f"{s}.csv").replace("\\", "/"), "",
                        os.path.join(OUT, f"{s}.csv").replace("\\", "/")])
    print(f"wrote {UNIV}: {len(ok)} syms")


if __name__ == "__main__":
    main()
