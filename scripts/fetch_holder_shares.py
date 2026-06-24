#!/usr/bin/env python3
"""Fetch 股东户数 (holder count) + 总股本 (total shares) per stock via akshare.

ONE endpoint (stock_zh_a_gdhs_detail_em) yields both Batch-C axes:
  * holder-count %-change (筹码集中度: declining holders = accumulation)
  * total-share series → net issuance rate
keyed by the ANNOUNCEMENT date (公告日期, PIT-correct — distinct from the period
end). Persists a minimal CSV per symbol to data/holders/ with resume (skip
existing), so a pilot run feeds a later full run.

Pilot:  python scripts/fetch_holder_shares.py --stride 7      # ~1/7 of universe
Full:   python scripts/fetch_holder_shares.py --stride 1
Gate is run separately by eval_factor_orthogonal.py (reads data/holders/).
"""
import sys
import os
sys.stdout.reconfigure(encoding="utf-8")
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import argparse
import time

import pandas as pd

import eval_ridge as er

OUT_DIR = os.path.join(er.REPO, "data", "holders")
# Positional columns in stock_zh_a_gdhs_detail_em (names are GBK; index is stable)
C_PERIOD, C_HOLDER_PCT, C_TOTAL_SHARE, C_ANNOUNCE = 0, 5, 9, 12


def universe(stride):
    panel = pd.read_csv(er.PANEL_MEMBERSHIP, dtype={"symbol": str}, usecols=["symbol"])
    syms = sorted(panel["symbol"].unique())
    return syms[::stride]


def fetch_one(sym):
    """sym like 'sh600000' → akshare 6-digit code. Returns minimal DataFrame or None."""
    import akshare as ak
    code = sym[-6:]
    df = ak.stock_zh_a_gdhs_detail_em(symbol=code)
    if df is None or len(df) == 0:
        return None
    out = pd.DataFrame({
        "announce": pd.to_datetime(df.iloc[:, C_ANNOUNCE], errors="coerce"),
        "period": pd.to_datetime(df.iloc[:, C_PERIOD], errors="coerce"),
        "holder_pct_chg": pd.to_numeric(df.iloc[:, C_HOLDER_PCT], errors="coerce"),
        "total_share": pd.to_numeric(df.iloc[:, C_TOTAL_SHARE], errors="coerce"),
    }).dropna(subset=["announce"]).sort_values("announce")
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--stride", type=int, default=7, help="take every Nth symbol (1=full)")
    ap.add_argument("--sleep", type=float, default=0.3)
    args = ap.parse_args()
    os.makedirs(OUT_DIR, exist_ok=True)

    syms = universe(args.stride)
    print(f"[fetch_holder_shares] {len(syms)} symbols (stride={args.stride}) → {OUT_DIR}")
    ok = skip = err = 0
    for i, sym in enumerate(syms):
        fp = os.path.join(OUT_DIR, f"{sym}.csv")
        if os.path.exists(fp):
            skip += 1
            continue
        try:
            out = fetch_one(sym)
            if out is None or out.empty:
                err += 1
            else:
                out.to_csv(fp, index=False, encoding="utf-8")
                ok += 1
            time.sleep(args.sleep)
        except Exception as e:
            err += 1
            if err <= 8:
                print(f"  ERR {sym}: {repr(e)[:120]}")
        if (i + 1) % 25 == 0:
            print(f"  {i+1}/{len(syms)}  ok={ok} skip={skip} err={err}")
    print(f"[fetch_holder_shares] done: ok={ok} skip={skip} err={err}")


if __name__ == "__main__":
    main()
