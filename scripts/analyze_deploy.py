#!/usr/bin/env python3
"""部署加固分析器：真实 T+1 执行回放 + 容量评估（引擎零改动，后验）。

输入 = 一个 screen 回测 run 的 JSON（含 holdings: 每次调仓 selected[sym,w] + nav）。
① T+1 执行：从 holdings 重放 NAV，对比"决策即成交 close[T]"(lag0,引擎口径) vs
   "决策 close[T]、成交滞后 1 根 bar close[T+1]"(lag1,真实)，量化执行拖累；对指数算超额。
   注：引擎选中后等权(1/n)，故重放用等权(非 JSON 里的 score 权重)。
② 容量：从 selected 名读 kday `amount`(真成交额) → 持仓名日成交额分布 → 按 %ADV 估最大可部署 AUM。

用法: python scripts/analyze_deploy.py <run_net.json> [--benchmark csi300] [--build-days 1]
"""
import argparse, bisect, csv, json, os, sys

try:
    sys.stdout.reconfigure(encoding="utf-8")
except Exception:
    pass
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
KDAY = os.path.join(REPO, "data", "baostock", "kday")
INDEX_DIR = os.path.join(REPO, "data", "baostock", "index")
COST = 20.0
RATE = COST / 2 / 10000.0   # 单边费率
TRADING_DAYS = 242

_close_cache, _amt_cache = {}, {}


def _load(sym):
    if sym in _close_cache:
        return _close_cache[sym], _amt_cache[sym]
    dates, closes, amts = [], [], []
    p = os.path.join(KDAY, f"{sym}.csv")
    if os.path.exists(p):
        with open(p, encoding="utf-8") as f:
            for row in csv.DictReader(f):
                dates.append(row["time"][:10]); closes.append(float(row["close"]))
                try:
                    amts.append(float(row["amount"]))
                except (KeyError, ValueError):
                    amts.append(float("nan"))
    _close_cache[sym] = (dates, closes); _amt_cache[sym] = (dates, amts)
    return _close_cache[sym], _amt_cache[sym]


def close_at(sym, d, lag):
    """lag=0: ≤d 最近收盘(引擎 close[T])；lag=1: d 之后第 1 根 bar 收盘(T+1 成交)。"""
    (dates, closes), _ = _load(sym)
    if not dates:
        return None
    i = bisect.bisect_right(dates, d) - 1
    j = i + lag
    return closes[j] if 0 <= j < len(closes) else None


def amt_avg(sym, d, n=20):
    """≤d 最近 n 日平均成交额(RMB)。"""
    _, (dates, amts) = _load(sym)
    if not dates:
        return None
    i = bisect.bisect_right(dates, d)
    w = [a for a in amts[max(0, i - n):i] if a == a]  # 去 NaN
    return sum(w) / len(w) if w else None


def replay(holdings, lag):
    """等权重放 NAV 曲线 [(date, nav)]，含单边成本。lag=0 引擎口径 / lag=1 真实 T+1。"""
    nav, w_old, out = 1.0, {}, [(holdings[0]["t"][:10], 1.0)]
    for i in range(len(holdings) - 1):
        t_i, t_n = holdings[i]["t"][:10], holdings[i + 1]["t"][:10]
        sel = holdings[i]["selected"]
        w_new = {s: 1.0 / len(sel) for s, _ in sel} if sel else {}
        tov = sum(abs(w_new.get(s, 0) - w_old.get(s, 0)) for s in set(w_old) | set(w_new))
        nav *= 1 - RATE * tov
        r = 0.0
        for s, w in w_new.items():
            p0, p1 = close_at(s, t_i, lag), close_at(s, t_n, lag)
            if p0 and p1 and p0 > 0:
                r += w * (p1 / p0 - 1)
        nav *= 1 + r
        out.append((t_n, nav)); w_old = w_new
    return out


def load_index(name):
    m = {}
    with open(os.path.join(INDEX_DIR, f"{name}.csv"), encoding="utf-8") as f:
        for row in csv.DictReader(f):
            m[row["time"][:10]] = float(row["close"])
    return m, sorted(m)


def idx_at(m, dates, d):
    i = bisect.bisect_right(dates, d) - 1
    return m[dates[i]] if i >= 0 else None


def window_excess(nav, d0, d1, m, dates):
    sub = [(d, v) for d, v in nav if d0 <= d <= d1]
    if len(sub) < 2:
        return None
    sr = sub[-1][1] / sub[0][1] - 1
    x0, x1 = idx_at(m, dates, sub[0][0]), idx_at(m, dates, sub[-1][0])
    return sr, x1 / x0 - 1, sr - (x1 / x0 - 1)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("run_json")
    ap.add_argument("--benchmark", default="csi300")
    ap.add_argument("--build-days", type=int, default=1, help="建仓可摊到的交易日数(容量按此放大)")
    a = ap.parse_args()
    rep = json.load(open(a.run_json, encoding="utf-8"))
    hold = rep["holdings"]
    regimes = rep.get("regime_slices", [])
    m, dates = load_index(a.benchmark)

    print(f"=== 部署加固分析 · {os.path.basename(a.run_json)} (bench {a.benchmark}) ===")
    print(f"调仓次数 {len(hold)-1}")

    # ① T+1 执行拖累
    print("\n--- ① 真实 T+1 执行（lag0=引擎 close[T] / lag1=成交滞后1bar）---")
    nav0, nav1 = replay(hold, 0), replay(hold, 1)
    t0, t1 = nav0[0][0], nav0[-1][0]
    tot0, tot1 = nav0[-1][1] - 1, nav1[-1][1] - 1
    print(f"{'':16}{'lag0(引擎)':>14}{'lag1(T+1)':>14}{'拖累':>12}")
    print(f"{'净总收益':16}{tot0:>+14.4f}{tot1:>+14.4f}{tot1-tot0:>+12.4f}")
    for label, d0, d1 in ([("全样本", t0, t1)] +
                          [(s["label"], s["from"], s["to"]) for s in regimes]):
        e0 = window_excess(nav0, d0, d1, m, dates)
        e1 = window_excess(nav1, d0, d1, m, dates)
        if e0 and e1:
            print(f"{label+' 超额':16}{e0[2]:>+14.4f}{e1[2]:>+14.4f}{e1[2]-e0[2]:>+12.4f}")

    # ② 容量
    print(f"\n--- ② 容量评估（持仓名 20日均成交额；建仓摊 {a.build_days} 日）---")
    per_reb_min, per_reb_med = [], []
    for h in hold[:-1]:
        sel = h["selected"]
        if not sel:
            continue
        advs = [v for v in (amt_avg(s, h["t"][:10]) for s, _ in sel) if v]
        if advs:
            advs.sort()
            per_reb_min.append(advs[0]); per_reb_med.append(advs[len(advs) // 2])
    if per_reb_min:
        n = max(len(h["selected"]) for h in hold[:-1] if h["selected"])
        per_reb_min.sort(); per_reb_med.sort()
        worst_min = per_reb_min[0]
        med_min = per_reb_min[len(per_reb_min) // 2]
        med_med = per_reb_med[len(per_reb_med) // 2]
        print(f"持仓数 N≈{n}；最不流动持仓名 20日均成交额：worst {worst_min/1e8:.2f}亿  中位 {med_min/1e8:.2f}亿")
        print(f"持仓名 ADV 中位数（典型流动性）：{med_med/1e8:.2f}亿/日")
        print(f"{'%ADV假设':>10}{'容量(按最不流动名,worst)':>26}{'容量(中位调仓)':>20}")
        for p in (0.05, 0.10, 0.20):
            cap_worst = n * p * worst_min * a.build_days
            cap_med = n * p * med_min * a.build_days
            print(f"{int(p*100):>9}%{cap_worst/1e8:>22.1f}亿{cap_med/1e8:>18.1f}亿")
        print("（容量 ≈ N × %ADV × 最不流动持仓名日成交额 × 建仓天数；约束=最不流动的必持名）")


if __name__ == "__main__":
    main()
