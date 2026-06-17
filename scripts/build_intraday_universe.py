#!/usr/bin/env python3
"""日内试点 universe：从 membership 最新月成员里取流动性最高、当前在市的 ~N 只。

出 data/universe_intraday.csv（symbol,primary,context,fundamentals；绝对路径/正斜杠）：
  primary      → 既有日线 data/<sym>.csv（驱动时间线 + 持有 close[T]→close[T+1]）
  fundamentals → data/intraday_factors/<sym>.csv（日内因子，走 fund.* 通道；fetch+build 后生成）

流动性 = 近 recent-days 日均 close×volume（同源同单位，仅用于排名）。
当前在市 = 日线末根在数据末日附近（剔除样本中途退市，sina 也不供退市 15m）。
诚实：幸存者偏差（仅当前在市）声明在 spec。
"""
import argparse, os, sys
import pandas as pd

try:
    sys.stdout.reconfigure(encoding="utf-8")
except Exception:
    pass

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def abspath_fwd(*parts):
    return os.path.abspath(os.path.join(REPO, *parts)).replace("\\", "/")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--membership", default=os.path.join(REPO, "data", "membership_top2000.csv"))
    ap.add_argument("--data-dir", default=os.path.join(REPO, "data"))
    ap.add_argument("--out", default=os.path.join(REPO, "data", "universe_intraday.csv"))
    ap.add_argument("--n", type=int, default=180)
    ap.add_argument("--recent-days", type=int, default=20)
    ap.add_argument("--max-stale-days", type=int, default=10, help="日线末根距参考日 > 此 → 视为退市/停牌出局")
    ap.add_argument("--asof", default=None, help="点时选 universe（流动性截至此日，剔除窗口内前视）；如 2025-12-09")
    a = ap.parse_args()

    ref = pd.Timestamp(a.asof) if a.asof else None
    mem = pd.read_csv(a.membership)
    mem["date"] = pd.to_datetime(mem["date"])
    snap = mem[mem["date"] <= ref]["date"].max() if ref is not None else mem["date"].max()
    members = sorted(mem[mem["date"] == snap]["symbol"].unique())
    print(f"membership snapshot {'≤'+str(ref.date()) if ref is not None else 'latest'} = {snap.date()} → {len(members)} members")

    # 流动性 = 截至 ref（或全数据末）的近 recent-days 日均 close×volume；live = 末根近 ref（剔窗口前退市）
    last_dates, liq = {}, {}
    for sym in members:
        p = os.path.join(a.data_dir, f"{sym}.csv")
        if not os.path.exists(p):
            continue
        try:
            df = pd.read_csv(p, usecols=["time", "close", "volume"])
        except Exception:
            continue
        df["time"] = pd.to_datetime(df["time"])
        if ref is not None:
            df = df[df["time"] <= ref]
        if len(df) < a.recent_days:
            continue
        last_dates[sym] = df["time"].max()
        tail = df.tail(a.recent_days)
        liq[sym] = float((tail["close"] * tail["volume"]).mean())

    if not last_dates:
        raise SystemExit("no member daily data found")
    data_end = ref if ref is not None else max(last_dates.values())
    cutoff = data_end - pd.Timedelta(days=a.max_stale_days)
    live = [s for s in liq if last_dates[s] >= cutoff]
    print(f"ref={data_end.date()}  live(≤{a.max_stale_days}d stale)={len(live)} / {len(liq)} with data")

    ranked = sorted(live, key=lambda s: liq[s], reverse=True)[:a.n]
    print(f"selected top {len(ranked)} by {a.recent_days}d mean turnover "
          f"(max={liq[ranked[0]]:.3e}, min={liq[ranked[-1]]:.3e})")

    with open(a.out, "w", encoding="utf-8", newline="") as f:
        f.write("symbol,primary,context,fundamentals\n")
        for sym in sorted(ranked):
            f.write(f"{sym},{abspath_fwd('data', sym + '.csv')},,"
                    f"{abspath_fwd('data', 'intraday_factors', sym + '.csv')}\n")
    print(f"wrote {a.out}")
    # 同时输出纯标的列表供 fetch 用
    listing = os.path.join(os.path.dirname(a.out), "universe_intraday_symbols.txt")
    with open(listing, "w", encoding="utf-8") as f:
        f.write("\n".join(sorted(ranked)) + "\n")
    print(f"wrote {listing}")


if __name__ == "__main__":
    main()
