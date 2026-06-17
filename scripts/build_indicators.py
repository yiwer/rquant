#!/usr/bin/env python3
"""在 bars CSV 上计算扩展常规技术指标 → 特征 CSV（time + ~30 指标列），time 对齐输入 bar。

全部因果（rolling/ewm/shift 只用 ≤t 数据，无前视）。日线与 15m 通用。
输入列需含 time,open,high,low,close,volume（可选 amount,turn）。
扩展集：MA/EMA/RSI/MACD/BOLL/ATR/KDJ/volMA/ret/振幅 + CCI/WR/OBV/VWAP/ROC/已实现波动/量价corr。
"""
import argparse, glob, os, sys
import numpy as np
import pandas as pd

try:
    sys.stdout.reconfigure(encoding="utf-8")
except Exception:
    pass
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def compute_indicators(df):
    """df: bars (time,open,high,low,close,volume[,amount,turn])，按 time 升序。返回 time+指标 DataFrame。"""
    d = df.sort_values("time").reset_index(drop=True)
    o, h, l, c, v = d["open"], d["high"], d["low"], d["close"], d["volume"]
    pc = c.shift(1)
    out = pd.DataFrame({"time": d["time"]})

    # 收益/振幅
    out["ret"] = c / pc - 1.0
    out["amplitude"] = (h - l) / pc

    # 均线族
    for n in (5, 10, 20, 60):
        out[f"ma{n}"] = c.rolling(n).mean()
    out["ema12"] = c.ewm(span=12, adjust=False).mean()
    out["ema26"] = c.ewm(span=26, adjust=False).mean()
    for n in (5, 20):
        out[f"volma{n}"] = v.rolling(n).mean()

    # MACD (12,26,9)；A股惯例 hist=2*(dif-dea)
    dif = out["ema12"] - out["ema26"]
    dea = dif.ewm(span=9, adjust=False).mean()
    out["macd_dif"] = dif
    out["macd_dea"] = dea
    out["macd_hist"] = 2.0 * (dif - dea)

    # RSI(14) Wilder
    delta = c.diff()
    gain = delta.clip(lower=0.0)
    loss = (-delta).clip(lower=0.0)
    ag = gain.ewm(alpha=1 / 14, adjust=False).mean()
    al = loss.ewm(alpha=1 / 14, adjust=False).mean()
    with np.errstate(divide="ignore", invalid="ignore"):
        rs = ag / al  # al=0 且 ag>0 → inf → RSI=100（标准约定）；al=0 且 ag=0(全平) → nan
    out["rsi14"] = 100.0 - 100.0 / (1.0 + rs)

    # BOLL(20,2)
    mid = c.rolling(20).mean()
    sd = c.rolling(20).std(ddof=0)
    up, dn = mid + 2 * sd, mid - 2 * sd
    out["boll_mid"], out["boll_up"], out["boll_dn"] = mid, up, dn
    out["boll_pctb"] = (c - dn) / (up - dn)
    out["boll_bw"] = (up - dn) / mid

    # ATR(14) Wilder
    tr = pd.concat([(h - l), (h - pc).abs(), (l - pc).abs()], axis=1).max(axis=1)
    out["atr14"] = tr.ewm(alpha=1 / 14, adjust=False).mean()

    # KDJ(9,3,3)
    low9 = l.rolling(9).min()
    high9 = h.rolling(9).max()
    rsv = (c - low9) / (high9 - low9) * 100.0
    k = rsv.ewm(alpha=1 / 3, adjust=False).mean()
    dd = k.ewm(alpha=1 / 3, adjust=False).mean()
    out["kdj_k"], out["kdj_d"], out["kdj_j"] = k, dd, 3 * k - 2 * dd

    # CCI(14)
    tp = (h + l + c) / 3.0
    tpma = tp.rolling(14).mean()
    md = (tp - tpma).abs().rolling(14).mean()
    out["cci14"] = (tp - tpma) / (0.015 * md)

    # Williams %R(14)
    h14, l14 = h.rolling(14).max(), l.rolling(14).min()
    out["wr14"] = (h14 - c) / (h14 - l14) * -100.0

    # OBV
    out["obv"] = (np.sign(out["ret"].fillna(0.0)) * v).cumsum()

    # VWAP(20 滚动，典型价加权)
    out["vwap20"] = (tp * v).rolling(20).sum() / v.rolling(20).sum()

    # ROC(12) 动量
    out["roc12"] = c / c.shift(12) - 1.0

    # 已实现波动率(20，对数收益标准差)
    out["rvol20"] = np.log(c / pc).rolling(20).std(ddof=0)

    # 量价相关(20)
    out["corr_pv20"] = c.rolling(20).corr(v)
    return out


def _process(bars_path, out_dir):
    sym = os.path.splitext(os.path.basename(bars_path))[0]
    df = pd.read_csv(bars_path)
    if len(df) < 2:
        return sym, "empty"
    feat = compute_indicators(df)
    feat.to_csv(os.path.join(out_dir, f"{sym}.csv"), index=False)
    return sym, f"{len(feat)}r×{len(feat.columns)-1}c"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bars-dir", required=True, help="如 data/baostock/kday 或 .../k15m")
    ap.add_argument("--out-dir", required=True, help="如 data/baostock/features_day")
    ap.add_argument("--limit", type=int, default=0)
    a = ap.parse_args()
    os.makedirs(a.out_dir, exist_ok=True)
    files = sorted(glob.glob(os.path.join(a.bars_dir, "*.csv")))
    if a.limit > 0:
        files = files[:a.limit]
    ok = 0
    for i, p in enumerate(files, 1):
        sym, st = _process(p, a.out_dir)
        if "r×" in st:
            ok += 1
        else:
            print(f"  skip {sym}: {st}")
        if i % 200 == 0:
            print(f"  [{i}/{len(files)}] ok={ok}", flush=True)
    print(f"built {ok}/{len(files)} feature CSVs → {a.out_dir}")


if __name__ == "__main__":
    main()
