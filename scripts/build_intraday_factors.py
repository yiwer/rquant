#!/usr/bin/env python3
"""15m 日内 bar + 日线 prev_close → 每股「每日 6 因子」CSV（fund.* 通道格式）。

因子用 ≤14:45「预收盘快照」计算（排除 15:00 bar）→ 决策严格早于 15:00 成交价、无自我前视。
戳在当日 15:00:00（匹配日线 bar，screen as_of(T 15:00) 取 T 行）。

输出 data/intraday_factors/<sym>.csv:
  time,last_leg,intraday_rev,close_vs_vwap,intraday_range,vol_tilt,overnight

诚实文化：缺数据/半日/首日 → 该因子 NaN（引擎弃权），绝不臆造。
"""
import argparse, glob, os, sys
import pandas as pd

try:
    sys.stdout.reconfigure(encoding="utf-8")
except Exception:
    pass

import datetime as _dt
PRECLOSE = _dt.time(14, 45)   # 快照截止（含）：≤14:45
PM_START = _dt.time(13, 0)    # 下午段起
T1345    = _dt.time(13, 45)   # 尾盘动量基准 bar

FACTORS = ["last_leg", "intraday_rev", "close_vs_vwap", "intraday_range", "vol_tilt", "overnight"]


def intraday_day_factors(day_df, prev_close):
    """day_df: 单日全部 15m bar（列 time[datetime],open,high,low,close,volume）；prev_close: 前一交易日日线收盘。
    返回 6 因子 dict（NaN=弃权）。仅用 ≤14:45 快照。"""
    nan = float("nan")
    out = {k: nan for k in FACTORS}
    if day_df is None or len(day_df) == 0:
        return out
    tod = day_df["time"].dt.time
    snap = day_df[tod <= PRECLOSE]
    if len(snap) < 2:
        return out  # 半日市/数据稀 → 弃权
    snap_tod = snap["time"].dt.time
    last_close = float(snap["close"].iloc[-1])
    day_open = float(snap["open"].iloc[0])
    has_pc = prev_close is not None and prev_close == prev_close and prev_close != 0  # 非 None/NaN/0

    # 1 尾盘动量 last_leg = close@14:45 / close@13:45 − 1
    row1345 = snap[snap_tod == T1345]
    if len(row1345) >= 1:
        c1345 = float(row1345["close"].iloc[0])
        if c1345 != 0:
            out["last_leg"] = last_close / c1345 - 1.0

    # 2 日内反转 intraday_rev = −(close@14:45 / day_open − 1)（高=日内跌者=反转候选）
    if day_open != 0:
        out["intraday_rev"] = -(last_close / day_open - 1.0)

    # 3 收盘强度 close_vs_vwap = close@14:45 / VWAP(快照) − 1，VWAP 用典型价(H+L+C)/3 量加权
    volsum = float(snap["volume"].sum())
    if volsum > 0:
        tp = (snap["high"] + snap["low"] + snap["close"]) / 3.0
        vwap = float((tp * snap["volume"]).sum() / volsum)
        if vwap != 0:
            out["close_vs_vwap"] = last_close / vwap - 1.0
        # 5 量能后移 vol_tilt = Σvol(13:00..14:45) / Σvol(快照)
        pm = snap[snap_tod >= PM_START]
        out["vol_tilt"] = float(pm["volume"].sum()) / volsum

    # 4 日内波幅 intraday_range = (max high − min low)[快照] / prev_close
    if has_pc:
        out["intraday_range"] = (float(snap["high"].max()) - float(snap["low"].min())) / prev_close
        # 6 隔夜跳空 overnight = day_open / prev_close − 1
        out["overnight"] = day_open / prev_close - 1.0
    return out


def build_factors(intraday_df, daily_df):
    """intraday_df: 一股 15m bars；daily_df: 同股日线 bars。→ 每日 6 因子 DataFrame（time=date 15:00）。"""
    idf = intraday_df.copy()
    idf["time"] = pd.to_datetime(idf["time"])
    idf["date"] = idf["time"].dt.normalize()

    d = daily_df.copy()
    d["time"] = pd.to_datetime(d["time"])
    d["date"] = d["time"].dt.normalize()
    d = d.sort_values("date")
    d["prev_close"] = d["close"].shift(1)
    prev_map = dict(zip(d["date"], d["prev_close"]))

    rows = []
    for date, grp in idf.sort_values("time").groupby("date"):
        pc = prev_map.get(date, float("nan"))
        f = intraday_day_factors(grp, pc)
        f["time"] = date.strftime("%Y-%m-%d")  # fund 载入器要求 date-only；无前视由 ≤14:45 截断保证（非戳）
        rows.append(f)
    cols = ["time"] + FACTORS
    return pd.DataFrame(rows, columns=cols)


def _process_one(intraday_path, daily_dir, out_dir):
    sym = os.path.splitext(os.path.basename(intraday_path))[0]
    daily_path = os.path.join(daily_dir, f"{sym}.csv")
    if not os.path.exists(daily_path):
        return sym, "no-daily"
    idf = pd.read_csv(intraday_path)
    ddf = pd.read_csv(daily_path)
    if len(idf) == 0 or len(ddf) == 0:
        return sym, "empty"
    out = build_factors(idf, ddf)
    out.to_csv(os.path.join(out_dir, f"{sym}.csv"), index=False)
    return sym, f"{len(out)} days"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--intraday-dir", default="data/intraday_15m")
    ap.add_argument("--daily-dir", default="data")
    ap.add_argument("--out-dir", default="data/intraday_factors")
    a = ap.parse_args()
    os.makedirs(a.out_dir, exist_ok=True)
    files = sorted(glob.glob(os.path.join(a.intraday_dir, "*.csv")))
    ok = 0
    for p in files:
        sym, status = _process_one(p, a.daily_dir, a.out_dir)
        if "days" in status:
            ok += 1
        else:
            print(f"  skip {sym}: {status}")
    print(f"built {ok}/{len(files)} intraday-factor CSVs → {a.out_dir}")


if __name__ == "__main__":
    main()
