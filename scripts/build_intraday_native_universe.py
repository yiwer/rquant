#!/usr/bin/env python3
"""15m-NATIVE 路径/时点/结构 因子（非 TA 指标）+ 财务 → merged universe（滞后1日，无前视）。

从 k15m 原始 bar 算「只有日内数据能看到」的行为型因子（每日 16 根 09:45..15:00）：
  er             路径效率比 = |close_last-close_first| / Σ|Δclose|（趋势平滑/信念 vs 噪声震荡）
  late_ret       尾盘1h 收益 close_1500/close_1345-1（机构尾盘驱动 / "聪明钱"）
  morn_ret       早盘 close_1130/open-1（散户开盘驱动）
  aft_ret        午盘 close_1500/open_1315-1
  ampm           午盘-早盘 不对称（派发：早强午弱→负）
  vol_late_frac  尾盘量占比 Σvol(≥14:00)/Σvol（机构后置）
  range_pos      收盘在日内振幅中位置 (close-low)/(high-low)（1=收在最高=强）
  close_vwap_gap close/日内VWAP-1（资金 vs 均价；持续>0=吸筹）
  overnight      open/prev_close-1（隔夜跳空/漂移）
  intraday       close/open-1（日内收益）
再 merge_asof 季度点时财务（roe/np_yoy/rev_yoy/gross_margin/eps/bps）。

输出 data/baostock/merged_native/<sym>.csv + data/baostock/universe_intraday_native.csv（primary=kday）。
诚实：因子滞后1交易日(昨日盘后算→今日用)；财务用公告日 as-of。皆无前视。缺数据→NaN(弃权)。
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
K15M = os.path.join(BS, "k15m")
KDAY = os.path.join(BS, "kday")
FUND = os.path.join(REPO, "data", "fundamentals")
OUT = os.path.join(BS, "merged_native")
UNIV = os.path.join(BS, "universe_intraday_native.csv")
NATIVE = ["er", "late_ret", "morn_ret", "aft_ret", "ampm", "vol_late_frac",
          "range_pos", "close_vwap_gap", "overnight", "intraday"]
FIN_COLS = ["roe", "np_yoy", "rev_yoy", "gross_margin", "eps", "bps"]


def native_daily(sym):
    df = pd.read_csv(os.path.join(K15M, f"{sym}.csv"))
    if len(df) == 0:
        return None
    t = pd.to_datetime(df["time"])
    df["date"] = t.dt.strftime("%Y-%m-%d")
    df["tod"] = t.dt.strftime("%H:%M")
    df = df.sort_values("time")
    g = df.groupby("date", sort=True)
    a = g.agg(open=("open", "first"), close=("close", "last"), cfirst=("close", "first"),
              high=("high", "max"), low=("low", "min"), vol=("volume", "sum"))
    df["tp"] = (df["high"] + df["low"] + df["close"]) / 3.0 * df["volume"]
    a["vwap"] = g["tp"].sum() / a["vol"].replace(0, np.nan)
    a["absmove"] = g["close"].apply(lambda s: s.diff().abs().sum())
    a["er"] = (a["close"] - a["cfirst"]).abs() / a["absmove"].replace(0, np.nan)

    def at_close(tod):
        return df.loc[df["tod"] == tod].drop_duplicates("date").set_index("date")["close"]

    def at_open(tod):
        return df.loc[df["tod"] == tod].drop_duplicates("date").set_index("date")["open"]

    a["c1130"] = at_close("11:30"); a["c1345"] = at_close("13:45"); a["o1315"] = at_open("13:15")
    vl = df.loc[df["tod"] >= "14:00"].groupby("date")["volume"].sum()
    a["vol_late_frac"] = vl / a["vol"].replace(0, np.nan)
    a["late_ret"] = a["close"] / a["c1345"] - 1.0
    a["morn_ret"] = a["c1130"] / a["open"] - 1.0
    a["aft_ret"] = a["close"] / a["o1315"] - 1.0
    a["ampm"] = a["aft_ret"] - a["morn_ret"]
    rng = (a["high"] - a["low"]).replace(0, np.nan)
    a["range_pos"] = (a["close"] - a["low"]) / rng
    a["close_vwap_gap"] = a["close"] / a["vwap"] - 1.0
    a["intraday"] = a["close"] / a["open"] - 1.0
    a["overnight"] = a["open"] / a["close"].shift(1) - 1.0
    a = a.reset_index()  # date column
    out = a[["date"] + NATIVE].copy()
    # 滞后1交易日：第 i 天因子戳到第 i+1 天（行动日）
    out["time"] = out["date"].shift(-1)
    out = out.iloc[:-1].drop(columns="date")
    return out[["time"] + NATIVE]


def build_one(sym):
    if not os.path.exists(os.path.join(KDAY, f"{sym}.csv")):
        return None
    nat = native_daily(sym)
    if nat is None or len(nat) == 0:
        return None
    nat["time"] = pd.to_datetime(nat["time"])
    fp = os.path.join(FUND, f"{sym}.csv")
    if os.path.exists(fp):
        fin = pd.read_csv(fp)
        fin["time"] = pd.to_datetime(fin["time"])
        fin = fin.sort_values("time")
        keep = ["time"] + [c for c in FIN_COLS if c in fin.columns]
        merged = pd.merge_asof(nat.sort_values("time"), fin[keep], on="time", direction="backward")
    else:
        merged = nat.copy()
        for c in FIN_COLS:
            merged[c] = float("nan")
    merged["time"] = merged["time"].dt.strftime("%Y-%m-%d")
    merged.to_csv(os.path.join(OUT, f"{sym}.csv"), index=False)
    return sym


def main():
    os.makedirs(OUT, exist_ok=True)
    syms = sorted(os.path.basename(p)[:-4] for p in glob.glob(os.path.join(K15M, "*.csv"))
                  if os.path.exists(os.path.join(KDAY, os.path.basename(p))))
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
                        os.path.join(OUT, f"{s}.csv").replace("\\", "/")])
    print(f"wrote {UNIV}: {len(ok)} symbols; native+fin -> {OUT}")


if __name__ == "__main__":
    main()
