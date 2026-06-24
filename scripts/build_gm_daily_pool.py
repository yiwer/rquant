#!/usr/bin/env python3
"""日线层 → 候选池 daily_pool.txt（喂尾盘漏斗的 --pool）。

隔夜从 baostock 日线(data/baostock/kday/<sym>.csv)+ 可选财务(data/fundamentals/<sym>.csv)
做「通用门槛 + 粗排取前 K」→ 写 data/gm/daily_pool.txt。然后 tail.config.json 的 pool 指向它,
14:46 漏斗取 snapshot ∩ pool（让日线 alpha 主筛,日内只做收口）。

门槛(通用,非 alpha)：近 window 日 均成交额≥下限、价∈[min,max]、足够历史、近期停牌天数≤上限。
粗排键 --rank：liquidity(均额,默认中性) | momentum(window 日收益) | turnover(均换手)。换它=注入你的偏好。
可选财务门槛 --min-roe / --min-np-yoy(默认关；开了则缺财务数据的股票被剔，保守不臆造)。

诚实：无 kday/历史不足/缺数据→跳过,不臆造。REPO 相对路径,移植即用。仅在日线数据稳定(收盘后)跑。
"""
import argparse
import csv
import glob
import os
import sys

try:
    sys.stdout.reconfigure(encoding="utf-8")
except Exception:
    pass

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
KDAY = os.path.join(REPO, "data", "baostock", "kday")
FUND = os.path.join(REPO, "data", "fundamentals")
RANK_KEYS = ("liquidity", "momentum", "turnover")


def to_float(x):
    try:
        return float(x)
    except (TypeError, ValueError):
        return None


def daily_metrics(closes, amounts, turns, window):
    """末 window 日指标。历史不足/末值非法 → None。closes/amounts/turns 同序(时间升序)。"""
    n = len(closes)
    if n < window or window < 2:
        return None
    price = closes[-1]
    if price is None or price <= 0:
        return None
    a = [x for x in amounts[-window:] if x]  # 剔停牌日(0/None);均额只看真实交易日
    avg_amount = sum(a) / len(a) if a else 0.0
    base = closes[-window]
    mom = (price / base - 1.0) if (base and base > 0) else None
    t = [x for x in turns[-window:] if x is not None]
    avg_turn = (sum(t) / len(t)) if t else None
    susp_recent = sum(1 for x in amounts[-window:] if not x)  # 0/None 成交额 = 停牌
    return {"price": price, "avg_amount": avg_amount, "mom": mom,
            "avg_turn": avg_turn, "n_bars": n, "susp_recent": susp_recent}


def passes_gates(m, min_price, max_price, min_amount, max_susp):
    if m is None:
        return False
    if m["price"] < min_price:
        return False
    if max_price and m["price"] > max_price:
        return False
    if m["avg_amount"] < min_amount:
        return False
    if m["susp_recent"] > max_susp:
        return False
    return True


def fund_ok(roe, np_yoy, min_roe, min_np_yoy):
    """财务门槛;阈值为 None=不卡。开了门槛而值缺失→不通过(保守)。"""
    if min_roe is not None and (roe is None or roe < min_roe):
        return False
    if min_np_yoy is not None and (np_yoy is None or np_yoy < min_np_yoy):
        return False
    return True


def score(m, rank):
    if rank == "liquidity":
        return m["avg_amount"]
    if rank == "momentum":
        return m["mom"]
    if rank == "turnover":
        return m["avg_turn"]
    return None


def select_top(items, rank, top):
    """items: [(sym, metrics)] → 按 score 降序取前 top;不可算分者剔除。"""
    scored = [(score(m, rank), sym) for sym, m in items]
    scored = [(s, sym) for s, sym in scored if s is not None]
    scored.sort(key=lambda x: x[0], reverse=True)
    out = [sym for _, sym in scored]
    return out[:top] if top and top > 0 else out


def read_kday_series(path):
    """读 kday CSV → (closes, amounts, turns) 三列(时间升序,丢弃 close 非法行)。"""
    closes, amounts, turns = [], [], []
    with open(path, encoding="utf-8") as f:
        r = csv.DictReader(f)
        for row in r:
            c = to_float(row.get("close"))
            if c is None:
                continue
            closes.append(c)
            amounts.append(to_float(row.get("amount")))
            turns.append(to_float(row.get("turn")))
    return closes, amounts, turns


def read_latest_fund(sym):
    """读 fundamentals 最后一行 → (roe, np_yoy);无文件→(None,None)。"""
    p = os.path.join(FUND, f"{sym}.csv")
    if not os.path.exists(p):
        return None, None
    roe = npy = None
    try:
        with open(p, encoding="utf-8") as f:
            for row in csv.DictReader(f):  # 取最后一条有效
                roe = to_float(row.get("roe"))
                npy = to_float(row.get("np_yoy"))
    except Exception:
        return None, None
    return roe, npy


def main():
    ap = argparse.ArgumentParser(description="日线层 → 候选池 daily_pool.txt")
    ap.add_argument("--out", default=os.path.join(REPO, "data", "gm", "daily_pool.txt"))
    ap.add_argument("--window", type=int, default=20, help="指标回看交易日数")
    ap.add_argument("--rank", choices=RANK_KEYS, default="liquidity", help="粗排键(默认中性=均成交额)")
    ap.add_argument("--top", type=int, default=800, help="取前 N(应 ≥ 漏斗 top;0=全部过门槛者)")
    ap.add_argument("--min-price", type=float, default=2.0)
    ap.add_argument("--max-price", type=float, default=0.0, help="0=不限")
    ap.add_argument("--min-amount", type=float, default=5e7, help="近 window 日均成交额下限(元),默认5000万")
    ap.add_argument("--max-susp", type=int, default=3, help="近 window 日停牌天数上限")
    ap.add_argument("--min-roe", type=float, default=None, help="财务门槛:最新 ROE 下限(默认关)")
    ap.add_argument("--min-np-yoy", type=float, default=None, help="财务门槛:净利同比下限(默认关)")
    a = ap.parse_args()

    files = sorted(glob.glob(os.path.join(KDAY, "*.csv")))
    if not files:
        print(f"[!] 无 kday 数据: {KDAY}（先跑 baostock 日线抓取）")
        sys.exit(2)
    use_fund = a.min_roe is not None or a.min_np_yoy is not None
    print(f"日线池：{len(files)} 只 kday → 门槛(价≥{a.min_price} 均额≥{a.min_amount:.0f} 停牌≤{a.max_susp}"
          f"{' +财务' if use_fund else ''}) + 粗排[{a.rank}] 取前 {a.top}")

    kept = []
    n_short = n_gate = n_fund = 0
    for i, path in enumerate(files, 1):
        sym = os.path.basename(path)[:-4]
        try:
            closes, amounts, turns = read_kday_series(path)
        except Exception:
            continue
        m = daily_metrics(closes, amounts, turns, a.window)
        if m is None:
            n_short += 1
            continue
        if not passes_gates(m, a.min_price, a.max_price, a.min_amount, a.max_susp):
            n_gate += 1
            continue
        if use_fund:
            roe, npy = read_latest_fund(sym)
            if not fund_ok(roe, npy, a.min_roe, a.min_np_yoy):
                n_fund += 1
                continue
        kept.append((sym, m))
        if i % 1000 == 0:
            print(f"  {i}/{len(files)} … 入选 {len(kept)}", flush=True)

    picked = select_top(kept, a.rank, a.top)
    os.makedirs(os.path.dirname(a.out), exist_ok=True)
    with open(a.out, "w", encoding="utf-8") as f:
        f.write("".join(s + "\n" for s in picked))

    print(f"门槛后 {len(kept)}（历史不足 {n_short} / 门槛淘汰 {n_gate}"
          + (f" / 财务淘汰 {n_fund}" if use_fund else "") + f"）→ 粗排取前 {a.top} = {len(picked)}")
    if picked:
        prev = ", ".join(f"{s}({score(dict(kept)[s], a.rank):.3g})" for s in picked[:5])
        print(f"  前5: {prev}")
    print(f"DONE 写 {os.path.relpath(a.out, REPO)}（{len(picked)} 只）。"
          f"喂: tail.config.json 的 pool 设为 {os.path.relpath(a.out, REPO)}")


if __name__ == "__main__":
    main()
