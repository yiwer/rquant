#!/usr/bin/env python3
"""漏斗桥接：读 current() 全市场快照 → 硬门槛(可交易/流动性/价带) + 粗排 → 短名单文件。

喂给 `fetch_gm_realtime.py --mode tail --shortlist <本文件输出>`，只对短名单逐只拉 15m。
门槛只做「通用、非 alpha」的剔除（停牌/低流动性/仙股/涨停买不进）；粗排键 --rank 可换成你的 alpha。
可选 --pool：你日线层隔夜筛出的候选集 → 取 pool ∩ snapshot（让日线 alpha 挑大梁,粗排只做日内收口）。

输入快照列见 fetch_gm_realtime.SNAP_COLS；输出每行一只 sh600000,默认 data/gm/shortlist.txt。
诚实：门槛是「能不能交易/够不够流动」,不替你定方向；粗排默认 liquidity(中性),换 intraday/range_pos/vwap_gap 即注入你的偏好。
"""
import argparse
import csv
import glob
import os
import sys

from fetch_gm_realtime import to_local  # 复用代码格式归一（导入不触发 gm,仅 main 内导入 gm）

try:
    sys.stdout.reconfigure(encoding="utf-8")
except Exception:
    pass

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
RANK_KEYS = ("liquidity", "intraday", "range_pos", "vwap_gap")


def to_float(x):
    try:
        return float(x)
    except (TypeError, ValueError):
        return None


def parse_row(d):
    r = {"symbol": (d.get("symbol") or "").strip()}
    for k in ("open", "high", "low", "price", "cum_volume", "cum_amount",
              "bid1", "bid1_v", "ask1", "ask1_v"):
        r[k] = to_float(d.get(k, ""))
    return r


def passes_gates(r, min_price, max_price, min_amount, drop_limit_up):
    """通用可交易/流动性门槛(非 alpha)。"""
    p = r["price"]
    if p is None or p <= 0:
        return False                                  # 无现价 → 停牌
    if r["cum_volume"] is None or r["cum_volume"] <= 0:
        return False                                  # 今日零成交 → 停牌/无量
    if p < min_price:
        return False                                  # 仙股/退市风险
    if max_price and p > max_price:
        return False
    if r["cum_amount"] is None or r["cum_amount"] < min_amount:
        return False                                  # 流动性不足
    if drop_limit_up and (r["ask1"] is None or (r["ask1_v"] or 0) <= 0):
        return False                                  # 无卖盘 = 涨停封板,尾盘买不进
    return True


def score(r, key):
    """粗排分,越大越优先；不可算 → None(被排除出排序)。"""
    p, o, h, l = r["price"], r["open"], r["high"], r["low"]
    if key == "liquidity":
        return r["cum_amount"]
    if key == "intraday":
        return (p / o - 1.0) if (o and o > 0) else None
    if key == "range_pos":
        return ((p - l) / (h - l)) if (h is not None and l is not None and h > l) else None
    if key == "vwap_gap":
        cv, ca = r["cum_volume"], r["cum_amount"]
        return (p / (ca / cv) - 1.0) if (cv and cv > 0 and ca) else None
    return None


def select_top(rows, key, top):
    scored = [(score(r, key), r) for r in rows]
    scored = [(s, r) for s, r in scored if s is not None]
    scored.sort(key=lambda x: x[0], reverse=True)
    out = [r for _, r in scored]
    return out[:top] if top and top > 0 else out


def load_pool(path):
    pool = set()
    with open(path, encoding="utf-8") as f:
        for line in f:
            x = line.strip().split(",")[0]
            if x and not x.lower().startswith("symbol"):
                pool.add(to_local(x))
    return pool


def latest_snapshot(snap_dir):
    fs = sorted(glob.glob(os.path.join(snap_dir, "snapshot_*.csv")))
    return fs[-1] if fs else None


def main():
    ap = argparse.ArgumentParser(description="快照 → 漏斗短名单")
    ap.add_argument("--snapshot", default=None, help="快照 CSV;默认取 data/gm/snapshot 最新一个")
    ap.add_argument("--snap-dir", default=os.path.join(REPO, "data", "gm", "snapshot"))
    ap.add_argument("--out", default=os.path.join(REPO, "data", "gm", "shortlist.txt"))
    ap.add_argument("--pool", default=None, help="日线层候选集文件 → 取交集(可选)")
    ap.add_argument("--rank", choices=RANK_KEYS, default="liquidity", help="粗排键(默认中性=流动性)")
    ap.add_argument("--top", type=int, default=300, help="取前 N(0=全部过门槛者)")
    ap.add_argument("--min-price", type=float, default=2.0)
    ap.add_argument("--max-price", type=float, default=0.0, help="0=不限")
    ap.add_argument("--min-amount", type=float, default=3e7, help="今日成交额下限(元),默认3000万")
    ap.add_argument("--drop-limit-up", action="store_true", help="剔除无卖盘(涨停封板,买不进)")
    a = ap.parse_args()

    snap = a.snapshot or latest_snapshot(a.snap_dir)
    if not snap or not os.path.exists(snap):
        print(f"[!] 找不到快照。先跑 fetch_gm_realtime.py --mode snapshot/tail 生成 {a.snap_dir}/snapshot_*.csv")
        sys.exit(2)

    with open(snap, encoding="utf-8") as f:
        rows = [parse_row(d) for d in csv.DictReader(f)]
    n0 = len(rows)

    gated = [r for r in rows if passes_gates(r, a.min_price, a.max_price, a.min_amount, a.drop_limit_up)]
    n1 = len(gated)

    if a.pool:
        pool = load_pool(a.pool)
        gated = [r for r in gated if r["symbol"] in pool]
    n2 = len(gated)

    picked = select_top(gated, a.rank, a.top)
    os.makedirs(os.path.dirname(a.out), exist_ok=True)
    with open(a.out, "w", encoding="utf-8") as f:
        for r in picked:
            f.write(r["symbol"] + "\n")

    print(f"快照 {os.path.relpath(snap, REPO)}: {n0} 行")
    print(f"  门槛后 {n1}（价≥{a.min_price} 额≥{a.min_amount:.0f}"
          f"{' 去涨停' if a.drop_limit_up else ''}）"
          + (f" → pool∩ {n2}" if a.pool else "")
          + f" → 粗排[{a.rank}] 取前 {a.top} = {len(picked)}")
    if picked:
        prev = ", ".join(f"{r['symbol']}({score(r, a.rank):.3g})" for r in picked[:5])
        print(f"  前5: {prev}")
    print(f"  → 写 {os.path.relpath(a.out, REPO)}（{len(picked)} 只）。喂: "
          f"fetch_gm_realtime.py --mode tail --shortlist {os.path.relpath(a.out, REPO)}")


if __name__ == "__main__":
    main()
