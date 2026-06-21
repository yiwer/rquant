#!/usr/bin/env python3
"""Fama-French 三因子 alpha 因子构造 → fund.<col> 通道（用户策略#2，价值族）。

思路（用户给定）：以 FF3(市场/SMB/HML) 对每股滚动回归，截距=alpha。
alpha<0 = 给定因子暴露下"该涨没涨"=被低估 → 买入候选（alpha 反转）。

构造（全程 ≤t 数据，无前视）：
- 市值：流通市值 = close × 流通股本，流通股本 = 100×volume/turn（turn=换手率%，来自 kday）。
- B/M：每股净资产 bps（季报 as-of）/ 价。
- 每日 2×3 排序(size 中位 × B/M 30/70) → 6 组合市值加权收益 → SMB=小−大, HML=高−低；
  市场 MKT = 全市场市值加权收益（rf≈0 略）。
- 每股滚动 WIN=120 日 OLS：ret ~ a + b1·MKT + b2·SMB + b3·HML，取 a=alpha（向量化：共享 X，一次 matmul 出全市场 alpha）。

输出 data/baostock/ff3_alpha/<sym>.csv(time, ff3_alpha + 财务列) + universe_ff3.csv(primary=kday)。
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
KDAY = os.path.join(BS, "kday")
FUND = os.path.join(REPO, "data", "fundamentals")
OUT = os.path.join(BS, "ff3_alpha")
UNIV = os.path.join(BS, "universe_ff3.csv")
WIN = 120
FIN_COLS = ["roe", "np_yoy", "rev_yoy", "gross_margin", "eps", "bps"]


def load_panel():
    """→ (dates, syms, RET[T×N], MCAP[T×N], BM[T×N])。merge_asof 在 datetime 空间，跨股对齐用字符串日期索引。"""
    rets, mcaps, bms, syms = {}, {}, {}, []
    for p in sorted(glob.glob(os.path.join(KDAY, "*.csv"))):
        s = os.path.basename(p)[:-4]
        fp = os.path.join(FUND, f"{s}.csv")
        if not os.path.exists(fp):
            continue
        d = pd.read_csv(p, usecols=["time", "close", "volume", "turn"])
        if len(d) < WIN + 20:
            continue
        d["dt"] = pd.to_datetime(d["time"])
        d = d.sort_values("dt").reset_index(drop=True)
        d["date"] = d["dt"].dt.strftime("%Y-%m-%d")
        ret = d["close"].pct_change()
        turn = d["turn"].where(d["turn"] > 0)
        mcap = d["close"] * (100.0 * d["volume"] / turn)            # 流通市值
        fin = pd.read_csv(fp)[["time", "bps"]].dropna()
        if fin.empty:
            continue
        fin["dt"] = pd.to_datetime(fin["time"])
        fin = fin.sort_values("dt")
        bps = pd.merge_asof(d[["dt"]], fin[["dt", "bps"]], on="dt", direction="backward")["bps"]
        bm = bps.values / d["close"].values                          # 账面市值比
        di = d["date"].values
        rets[s] = pd.Series(ret.values, index=di)
        mcaps[s] = pd.Series(mcap.values, index=di)
        bms[s] = pd.Series(bm, index=di)
        syms.append(s)
    RET = pd.DataFrame(rets).sort_index()
    MCAP = pd.DataFrame(mcaps).reindex(index=RET.index)
    BM = pd.DataFrame(bms).reindex(index=RET.index)
    return RET.index, list(RET.columns), RET, MCAP, BM


def daily_factors(RET, MCAP, BM):
    """每日 2×3 → SMB/HML/MKT 序列（T×1）。"""
    T = len(RET); mkt, smb, hml = np.full(T, np.nan), np.full(T, np.nan), np.full(T, np.nan)
    r = RET.values; mc = MCAP.values; bm = BM.values
    for t in range(T):
        rt, mct, bmt = r[t], mc[t], bm[t]
        ok = np.isfinite(rt) & np.isfinite(mct) & (mct > 0) & np.isfinite(bmt)
        if ok.sum() < 30:
            continue
        idx = np.where(ok)[0]
        mkt[t] = np.sum(mct[idx] * rt[idx]) / np.sum(mct[idx])       # 市值加权市场
        msz = np.median(mct[idx])
        bl, bh = np.quantile(bmt[idx], 0.3), np.quantile(bmt[idx], 0.7)

        def vw(sel):
            if sel.sum() == 0:
                return np.nan
            w = mct[idx][sel]
            return np.sum(w * rt[idx][sel]) / np.sum(w)
        small = mct[idx] <= msz; big = ~small
        lo = bmt[idx] <= bl; hi = bmt[idx] >= bh; mid = ~lo & ~hi
        s_ret = np.nanmean([vw(small & lo), vw(small & mid), vw(small & hi)])
        b_ret = np.nanmean([vw(big & lo), vw(big & mid), vw(big & hi)])
        h_ret = np.nanmean([vw(small & hi), vw(big & hi)])
        l_ret = np.nanmean([vw(small & lo), vw(big & lo)])
        smb[t] = s_ret - b_ret; hml[t] = h_ret - l_ret
    return mkt, smb, hml


def rolling_alpha(RET, mkt, smb, hml):
    """每股滚动 WIN 日 OLS 截距(alpha)。共享 X → 每窗一次 matmul。"""
    r = RET.values; T, N = r.shape
    F = np.column_stack([np.ones(T), mkt, smb, hml])               # T×4
    A = np.full((T, N), np.nan)
    for t in range(WIN, T):
        Xw = F[t - WIN:t]; yw = r[t - WIN:t]                       # WIN×4, WIN×N
        if not np.isfinite(Xw).all():
            continue
        valid = np.isfinite(yw).all(axis=0)                       # 整窗无缺的股
        if valid.sum() == 0:
            continue
        try:
            P = np.linalg.inv(Xw.T @ Xw) @ Xw.T                   # 4×WIN
        except np.linalg.LinAlgError:
            continue
        coef = P @ yw[:, valid]                                   # 4×nvalid
        A[t, valid] = coef[0]                                     # 截距=alpha
    return A


def main():
    os.makedirs(OUT, exist_ok=True)
    print("loading panel...")
    idx, syms, RET, MCAP, BM = load_panel()
    print(f"  {len(syms)} syms × {len(idx)} days")
    print("daily FF3 factors...")
    mkt, smb, hml = daily_factors(RET, MCAP, BM)
    print("rolling per-stock alpha...")
    A = rolling_alpha(RET, mkt, smb, hml)
    alpha = pd.DataFrame(A, index=idx, columns=syms)
    ok = []
    for s in syms:
        a = alpha[s].dropna()
        if len(a) < 60:
            continue
        fp = os.path.join(FUND, f"{s}.csv")
        fin = pd.read_csv(fp)
        fin["dt"] = pd.to_datetime(fin["time"])
        fin = fin.sort_values("dt")
        cols = [c for c in FIN_COLS if c in fin.columns]
        df = a.reset_index(); df.columns = ["time", "ff3_alpha"]
        df["dt"] = pd.to_datetime(df["time"]); df = df.sort_values("dt")
        merged = pd.merge_asof(df, fin[["dt"] + cols], on="dt", direction="backward")
        merged[["time", "ff3_alpha"] + cols].to_csv(os.path.join(OUT, f"{s}.csv"), index=False)
        ok.append(s)
    with open(UNIV, "w", newline="", encoding="utf-8") as f:
        w = csv.writer(f); w.writerow(["symbol", "primary", "context", "fundamentals"])
        for s in ok:
            w.writerow([s, os.path.join(KDAY, f"{s}.csv").replace("\\", "/"), "",
                        os.path.join(OUT, f"{s}.csv").replace("\\", "/")])
    print(f"wrote {UNIV}: {len(ok)} syms; ff3_alpha + fin -> {OUT}")


if __name__ == "__main__":
    main()
