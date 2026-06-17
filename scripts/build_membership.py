"""构建 survivorship-free top-N membership（月末按近 lookback 日均成交额，≤d 排名 + 在市窗）。
读 data/<sym>.csv 面板 → data/membership_top2000.csv (date,symbol) + data/universe_membership.csv (成员并集 roster)。
point-in-time: 排名只用 ≤d 数据；在市=近 active_days 日内有 bar；退市股活跃期入、退市后出。
用法: python scripts/build_membership.py [--data-dir data] [--universe data/universe_full.csv]
      [--top 2000] [--lookback 20] [--start 2018-01-01] [--active-days 14]"""
import argparse, os, sys
import numpy as np
import pandas as pd

def rank_top_n(turnover, n):
    """turnover: sym->float；降序 top-n symbol；NaN/<=0 剔除。"""
    items = [(s, v) for s, v in turnover.items()
             if v is not None and np.isfinite(v) and v > 0]
    items.sort(key=lambda kv: kv[1], reverse=True)
    return [s for s, _ in items[:n]]

def month_end_dates(all_dates, start):
    s = pd.Timestamp(start)
    idx = all_dates[all_dates >= s]
    if len(idx) == 0: return []
    grp = pd.Series(idx, index=idx).groupby([idx.year, idx.month]).max()
    return list(grp.values)

def load_panel(data_dir, symbols):
    panel = {}
    for s in symbols:
        p = os.path.join(data_dir, f"{s}.csv")
        if not os.path.exists(p): continue
        try:
            df = pd.read_csv(p, usecols=["time", "close", "volume"])
        except Exception:
            continue
        if df.empty: continue
        df["date"] = pd.to_datetime(df["time"]).dt.normalize()
        panel[s] = df.set_index("date")[["close", "volume"]].sort_index()
    return panel

def compute_membership(panel, top, lookback, start, active_days=14):
    """→ list[(Timestamp, [symbols])]，每月末 top-N。"""
    if not panel: return []
    all_dates = pd.DatetimeIndex(sorted(set().union(*[df.index for df in panel.values()])))
    out = []
    for d in month_end_dates(all_dates, start):
        d = pd.Timestamp(d)
        lo = d - pd.Timedelta(days=active_days)
        turnover = {}
        for s, df in panel.items():
            if df.loc[lo:d].empty:            # 在市窗内无 bar → 不在市(退市/长停)
                continue
            win = df.loc[:d].tail(lookback)   # ≤d 近 lookback 交易日
            if win.empty: continue
            turnover[s] = float((win["close"] * win["volume"]).mean())  # 成交额近似；排名 scale-invariant
        members = rank_top_n(turnover, top)
        if members: out.append((d, members))
    return out

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--data-dir", default="data")
    ap.add_argument("--universe", default="data/universe_full.csv")
    ap.add_argument("--out-membership", default="data/membership_top2000.csv")
    ap.add_argument("--out-union", default="data/universe_membership.csv")
    ap.add_argument("--fund-dir", default="data/fundamentals")
    ap.add_argument("--top", type=int, default=2000)
    ap.add_argument("--lookback", type=int, default=20)
    ap.add_argument("--start", default="2018-01-01")
    ap.add_argument("--active-days", type=int, default=14)
    args = ap.parse_args()
    symbols = list(pd.read_csv(args.universe)["symbol"])
    panel = load_panel(args.data_dir, symbols)
    print(f"  loaded {len(panel)}/{len(symbols)} symbols with data", file=sys.stderr)
    mem = compute_membership(panel, args.top, args.lookback, args.start, args.active_days)
    union = set()
    with open(args.out_membership, "w", encoding="utf-8", newline="") as f:
        f.write("date,symbol\n")
        for d, members in mem:
            ds = d.strftime("%Y-%m-%d")
            for s in sorted(members):
                f.write(f"{ds},{s}\n"); union.add(s)
    with open(args.out_union, "w", encoding="utf-8", newline="") as f:
        f.write("symbol,primary,context,fundamentals\n")
        for s in sorted(union):
            fund = f"{args.fund_dir}/{s}.csv"
            fund_col = fund if os.path.exists(fund) else ""
            f.write(f"{s},{args.data_dir}/{s}.csv,,{fund_col}\n")
    print(f"wrote {len(mem)} rebalances, {len(union)} union symbols")

if __name__ == "__main__":
    main()
