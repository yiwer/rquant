#!/usr/bin/env python3
"""两腿组合分析（引擎零改动，后验）：防御价值腿 + 进攻成长腿按 w 配比混合，扫 w 求 Sharpe 与 OOS 兼顾。
混合在 nav 段收益层：br = w·value_ret + (1-w)·growth_ret（每调仓再平衡回 w 配比），cumulate → 混合 nav。
两腿须同一调仓时间线（同 universe/from/to/reb）。Sharpe 按月频段年化(×sqrt(12))，全腿/混合同法可比。

用法: python scripts/analyze_twoleg.py <value_net.json> <growth_net.json> [--benchmark csi300]
"""
import argparse, bisect, csv, json, math, os, sys

try:
    sys.stdout.reconfigure(encoding="utf-8")
except Exception:
    pass
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
INDEX_DIR = os.path.join(REPO, "data", "baostock", "index")
PPY = 12  # 月频段/年


def load_index(name):
    m = {}
    with open(os.path.join(INDEX_DIR, f"{name}.csv"), encoding="utf-8") as f:
        for row in csv.DictReader(f):
            m[row["time"][:10]] = float(row["close"])
    return m, sorted(m)


def idx_at(m, dates, d):
    i = bisect.bisect_right(dates, d) - 1
    return m[dates[i]] if i >= 0 else None


def navs(report):
    return [(h["t"][:10], h["nav"]) for h in report["holdings"] if h.get("nav", 0) > 0]


def metrics(nav, m, dates, regimes):
    """nav=[(date,nav)] → 年化Sharpe/最大回撤/总收益 + 各窗超额 vs 指数。"""
    rets = [nav[i + 1][1] / nav[i][1] - 1 for i in range(len(nav) - 1)]
    mean = sum(rets) / len(rets)
    var = sum((r - mean) ** 2 for r in rets) / (len(rets) - 1)
    sd = math.sqrt(var)
    sharpe = (mean / sd * math.sqrt(PPY)) if sd > 0 else float("nan")
    peak = dd = 0.0
    for _, v in nav:
        peak = max(peak, v); dd = max(dd, 1 - v / peak)
    total = nav[-1][1] / nav[0][1] - 1

    def win(d0, d1):
        sub = [(d, v) for d, v in nav if d0 <= d <= d1]
        if len(sub) < 2:
            return None
        sr = sub[-1][1] / sub[0][1] - 1
        x0, x1 = idx_at(m, dates, sub[0][0]), idx_at(m, dates, sub[-1][0])
        return sr - (x1 / x0 - 1) if (x0 and x1) else None

    full = win(nav[0][0], nav[-1][0])
    reg = {s["label"]: win(s["from"], s["to"]) for s in regimes}
    return sharpe, dd, total, full, reg


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("value_json"); ap.add_argument("growth_json")
    ap.add_argument("--benchmark", default="csi300")
    a = ap.parse_args()
    v = json.load(open(a.value_json, encoding="utf-8"))
    g = json.load(open(a.growth_json, encoding="utf-8"))
    m, dates = load_index(a.benchmark)
    regimes = v.get("regime_slices", [])
    oos_lbl = next((s["label"] for s in regimes if "OOS" in s["label"]), None)

    vn, gn = navs(v), navs(g)
    # 按日期对齐（取交集，保序）
    gmap = dict(gn)
    aligned = [(d, vv, gmap[d]) for d, vv in vn if d in gmap]
    if len(aligned) < 12:
        raise SystemExit(f"两腿对齐点太少({len(aligned)})——调仓时间线不一致？")
    vseg = [aligned[i + 1][1] / aligned[i][1] - 1 for i in range(len(aligned) - 1)]
    gseg = [aligned[i + 1][2] / aligned[i][2] - 1 for i in range(len(aligned) - 1)]
    advdates = [d for d, _, _ in aligned]

    print(f"=== 两腿组合：价值 {os.path.basename(a.value_json)} × 成长 {os.path.basename(a.growth_json)} (bench {a.benchmark}) ===")
    print(f"对齐调仓点 {len(aligned)}；w=价值腿权重，(1-w)=成长腿\n")
    print(f"{'w(价值)':>8}{'净总':>9}{'超额':>9}{'OOS超额':>10}{'年化Sharpe':>12}{'最大回撤':>10}")
    rows = []
    for w in (1.0, 0.8, 0.7, 0.6, 0.5, 0.4, 0.3, 0.0):
        nav = [(advdates[0], 1.0)]
        cur = 1.0
        for i in range(len(vseg)):
            cur *= 1 + (w * vseg[i] + (1 - w) * gseg[i])
            nav.append((advdates[i + 1], cur))
        sh, dd, tot, full, reg = metrics(nav, m, dates, regimes)
        oos = reg.get(oos_lbl)
        rows.append((w, tot, full, oos, sh, dd))
        print(f"{w:>8.1f}{tot:>+9.2f}{full:>+9.2f}{(oos if oos is not None else float('nan')):>+10.2f}{sh:>12.2f}{dd:>10.3f}")
    # 推荐：Sharpe 与 OOS 的平衡（各自 minmax 归一后等权打分）
    shs = [r[4] for r in rows]; ooss = [r[3] for r in rows]
    def nz(x, lo, hi): return (x - lo) / (hi - lo) if hi > lo else 0.5
    slo, shi = min(shs), max(shs); olo, ohi = min(ooss), max(ooss)
    best = max(rows, key=lambda r: nz(r[4], slo, shi) + nz(r[3], olo, ohi))
    print(f"\nSharpe+OOS 均衡最优配比：w(价值)={best[0]:.1f} / 成长={1-best[0]:.1f} "
          f"→ Sharpe {best[4]:.2f}、OOS 超额 {best[3]:+.2f}、净总 {best[1]:+.2f}、回撤 {best[5]:.3f}")


if __name__ == "__main__":
    main()
