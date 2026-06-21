#!/usr/bin/env python3
"""生成 15m 选股 universe：symbol→primary=k15m, fundamentals=features_15m(31 个 15m 指标)。
取同时有 k15m/<sym>.csv 且 features_15m/<sym>.csv 的 symbol；绝对路径+正斜杠(同 universe_baostock_day.csv)。"""
import os, csv, glob
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
K15M = os.path.join(REPO, "data", "baostock", "k15m")
FEAT = os.path.join(REPO, "data", "baostock", "features_15m")
OUT  = os.path.join(REPO, "data", "baostock", "universe_baostock_15m_feat.csv")

def main():
    syms = sorted(
        os.path.basename(p)[:-4] for p in glob.glob(os.path.join(K15M, "*.csv"))
        if os.path.exists(os.path.join(FEAT, os.path.basename(p)))
    )
    if not syms:
        raise SystemExit("no symbols with both k15m and features_15m")
    with open(OUT, "w", newline="", encoding="utf-8") as f:
        w = csv.writer(f); w.writerow(["symbol", "primary", "context", "fundamentals"])
        for s in syms:
            w.writerow([s, os.path.join(K15M, f"{s}.csv").replace("\\", "/"),
                        "", os.path.join(FEAT, f"{s}.csv").replace("\\", "/")])
    print(f"wrote {OUT}: {len(syms)} symbols")

if __name__ == "__main__":
    main()
