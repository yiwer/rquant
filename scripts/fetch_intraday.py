#!/usr/bin/env python3
"""逐股拉 sina 15m → data/intraday_15m/<sym>.csv（time,open,high,low,close,volume）。

源：sina stock_zh_a_minute(period=15, adjust=qfq)——实测仅 ~6 个月历史、~5s/股、不供退市股。
eastmoney 分钟限频不可用（探针 ConnectionError），故只用 sina。

Windows requests 回退系统/注册表代理 → patch getproxies。resume=跳过已存在非空文件；
每股重试退避；无数据/失败记录跳过（不臆造）。仅在收盘后/数据稳定时联网跑。
"""
import argparse, os, sys, time
import requests
requests.utils.getproxies = lambda: {}
try:
    requests.sessions.getproxies = lambda: {}
except Exception:
    pass
import akshare as ak
import pandas as pd

try:
    sys.stdout.reconfigure(encoding="utf-8")
except Exception:
    pass

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def fetch_one(sym, retries=3):
    last = None
    for k in range(retries):
        try:
            df = ak.stock_zh_a_minute(symbol=sym, period="15", adjust="qfq")
            if df is None or len(df) == 0:
                return None, "empty"
            df = df.rename(columns={"day": "time"})
            keep = ["time", "open", "high", "low", "close", "volume"]
            df = df[keep].copy()
            for c in ["open", "high", "low", "close", "volume"]:
                df[c] = pd.to_numeric(df[c], errors="coerce")
            df = df.dropna(subset=["open", "high", "low", "close"])
            return df, f"{len(df)} bars"
        except Exception as e:
            last = f"{type(e).__name__}: {str(e)[:80]}"
            time.sleep(2.0 * (k + 1))  # 退避
    return None, last or "fail"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--symbols", default=os.path.join(REPO, "data", "universe_intraday_symbols.txt"))
    ap.add_argument("--out-dir", default=os.path.join(REPO, "data", "intraday_15m"))
    ap.add_argument("--sleep", type=float, default=0.3, help="股间隔秒")
    a = ap.parse_args()
    os.makedirs(a.out_dir, exist_ok=True)
    syms = [s.strip() for s in open(a.symbols, encoding="utf-8") if s.strip()]
    print(f"fetching 15m for {len(syms)} symbols → {a.out_dir}")
    ok = skip = fail = 0
    failed = []
    for i, sym in enumerate(syms, 1):
        out = os.path.join(a.out_dir, f"{sym}.csv")
        if os.path.exists(out) and os.path.getsize(out) > 100:
            skip += 1
            continue
        df, status = fetch_one(sym)
        if df is not None:
            df.to_csv(out, index=False)
            ok += 1
        else:
            fail += 1
            failed.append((sym, status))
        if i % 20 == 0 or i == len(syms):
            print(f"  [{i}/{len(syms)}] ok={ok} skip={skip} fail={fail}", flush=True)
        time.sleep(a.sleep)
    if failed:
        print("failed:", ", ".join(f"{s}({m})" for s, m in failed[:20]))
    print(f"DONE ok={ok} skip={skip} fail={fail}")


if __name__ == "__main__":
    main()
