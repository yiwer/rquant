#!/usr/bin/env python3
"""日线 PA 特征（趋势/结构/回调/H1H2/通道/信号K强度），滞后1交易日无前视。
输出 data/baostock/pa_features/<sym>.csv，供 pa_overlay 树经 fund.<col> 取用。"""
import os, glob, sys
import numpy as np
import pandas as pd

try:
    sys.stdout.reconfigure(encoding="utf-8")
except Exception:
    pass

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
KDAY = os.path.join(REPO, "data", "baostock", "kday")
OUT = os.path.join(REPO, "data", "baostock", "pa_features")
W = 10   # 结构/回调窗口
COLS = ["pa_ema20", "pa_dir", "pa_struct", "pa_regime", "pa_pullback",
        "pa_h1", "pa_h2", "pa_chan", "pa_sig_with", "pa_sig_cnt", "pa_ext"]


def _h1h2(close, high):
    """上涨回调后首/次根创新高(high>前一根high)。仅用过去 → 无前视。返回 (h1[], h2[])。"""
    n = len(close); h1 = np.zeros(n); h2 = np.zeros(n)
    ema = pd.Series(close).ewm(span=20, adjust=False).mean().values
    pulled = False; up_count = 0
    for i in range(1, n):
        uptrend = close[i] > ema[i]
        if not uptrend:
            pulled = False; up_count = 0; continue
        if close[i] < close[i - 1]:               # 回调中
            pulled = True; up_count = 0
        elif pulled and high[i] > high[i - 1]:    # 回调后向上突破前一根高
            up_count += 1
            if up_count == 1:
                h1[i] = 1
            elif up_count == 2:
                h2[i] = 1; pulled = False
    return h1, h2


def pa_features(df):
    df = df.reset_index(drop=True)
    c = df["close"].astype(float); h = df["high"].astype(float)
    l = df["low"].astype(float); o = df["open"].astype(float)
    out = pd.DataFrame({"time": df["time"]})
    ema20 = c.ewm(span=20, adjust=False).mean()
    out["pa_ema20"] = c / ema20 - 1.0
    out["pa_dir"] = np.sign(ema20.diff(5)).fillna(0.0)
    # 效率比 ER(20)：方向位移 / 路径长度
    chg = (c - c.shift(20)).abs()
    path = c.diff().abs().rolling(20).sum()
    out["pa_regime"] = (chg / path.replace(0, np.nan)).clip(0, 1)
    # 结构：两段滚动高/低比较（无前视）
    rh = h.rolling(W).max(); rl = l.rolling(W).min()
    HH = (rh > rh.shift(W)).astype(int); HL = (rl > rl.shift(W)).astype(int)
    LL = (rl < rl.shift(W)).astype(int); LH = (rh < rh.shift(W)).astype(int)
    out["pa_struct"] = (HH + HL - LL - LH).astype(float)
    # 回调深度：上升趋势中从近 W 高回撤、且收盘仍在 EMA20 上方
    recent_high = h.rolling(W).max()
    pull = (recent_high - c) / recent_high.replace(0, np.nan)
    out["pa_pullback"] = np.where((c > ema20) & (out["pa_dir"] > 0), pull.clip(lower=0), 0.0)
    # H1/H2
    h1, h2 = _h1h2(c.values, h.values)
    out["pa_h1"] = h1; out["pa_h2"] = h2
    # 通道宽窄：ATR(14)/价（窄=低）
    pc = c.shift(1)
    tr = pd.concat([(h - l), (h - pc).abs(), (l - pc).abs()], axis=1).max(axis=1)
    atr = tr.ewm(alpha=1 / 14, adjust=False).mean()
    out["pa_chan"] = atr / c
    # 信号K强度：实体占比 × 收盘位置；顺势(上涨K)/逆势(下跌K)分列
    rng = (h - l).replace(0, np.nan)
    body = (c - o).abs() / rng
    close_pos = (c - l) / rng
    up_bar = (c >= o)
    out["pa_sig_with"] = np.where(up_bar, body * close_pos, 0.0)
    out["pa_sig_cnt"] = np.where(~up_bar, body * (1 - close_pos), 0.0)
    # 过度延展：EMA20 上方超过 1 个 ATR 的部分
    out["pa_ext"] = ((c - ema20) / atr.replace(0, np.nan)).clip(lower=0)
    return out[["time"] + COLS]


def main():
    os.makedirs(OUT, exist_ok=True)
    files = sorted(glob.glob(os.path.join(KDAY, "*.csv")))
    ok = 0
    for i, p in enumerate(files, 1):
        s = os.path.basename(p)[:-4]
        df = pd.read_csv(p, usecols=["time", "open", "high", "low", "close"])
        if len(df) < 60:
            continue
        df["time"] = pd.to_datetime(df["time"])
        df = df.sort_values("time").reset_index(drop=True)
        feat = pa_features(df)
        feat[COLS] = feat[COLS].shift(1)              # 滞后1交易日(无前视)
        feat["time"] = feat["time"].dt.strftime("%Y-%m-%d")
        feat.iloc[1:].to_csv(os.path.join(OUT, f"{s}.csv"), index=False)
        ok += 1
        if i % 400 == 0:
            print(f"  {i}/{len(files)}...")
    print(f"built {ok} PA-feature CSVs -> {OUT}")


if __name__ == "__main__":
    main()
