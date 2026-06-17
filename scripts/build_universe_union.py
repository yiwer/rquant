#!/usr/bin/env python3
"""5年内 top-2000 月度并集（survivorship-free）→ 按近期成交额排序的标的清单。

并集(~5115) 是真·无幸存者偏差覆盖；按流动性排序使"可用 top-2000"先落地，长尾(含退市/低流动)后补。
输出 data/baostock/universe_5yr_symbols.txt（engine 格式 shXXXXXX，每行一只，流动性降序）。
"""
import argparse, os
import pandas as pd

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--membership", default=os.path.join(REPO, "data", "membership_top2000.csv"))
    ap.add_argument("--data-dir", default=os.path.join(REPO, "data"))
    ap.add_argument("--from-date", default="2021-01-01", help="并集起始月（5年内）")
    ap.add_argument("--out", default=os.path.join(REPO, "data", "baostock", "universe_5yr_symbols.txt"))
    a = ap.parse_args()

    mem = pd.read_csv(a.membership); mem["date"] = pd.to_datetime(mem["date"])
    union = sorted(mem[mem["date"] >= a.from_date]["symbol"].unique())
    print(f"top-2000 并集 since {a.from_date}: {len(union)} symbols")

    liq = {}
    for s in union:
        p = os.path.join(a.data_dir, f"{s}.csv")
        if not os.path.exists(p):
            liq[s] = -1.0; continue   # 无现成日线 → 排最后（多为退市/早期）
        try:
            df = pd.read_csv(p, usecols=["close", "volume"])
            tail = df.tail(20)
            liq[s] = float((tail["close"] * tail["volume"]).mean()) if len(tail) else 0.0
        except Exception:
            liq[s] = 0.0
    ordered = sorted(union, key=lambda s: liq[s], reverse=True)
    have = sum(1 for s in union if liq[s] > 0)
    print(f"有现成日线可定序: {have}/{len(union)}；其余排尾")

    os.makedirs(os.path.dirname(a.out), exist_ok=True)
    with open(a.out, "w", encoding="utf-8") as f:
        f.write("\n".join(ordered) + "\n")
    print(f"wrote {a.out} ({len(ordered)} symbols, liquidity-desc)")
    print(f"前5: {ordered[:5]}  尾5: {ordered[-5:]}")


if __name__ == "__main__":
    main()
