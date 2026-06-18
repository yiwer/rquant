#!/usr/bin/env python3
"""行业中性归因（引擎零改动，后验）：把价值组合 vs 指数的超额拆成
   ① 行业配置效应（持有便宜板块——板块指数加权 vs CSI300）
   ② 板块内选择效应（个股 vs 其所属板块指数）
回答"价值边是行业押注还是个股选择"。用 data/baostock/sector/<行业>.csv 的 index 列(板块EW净值)。

用法: python scripts/analyze_sector.py <run_net.json> [--benchmark csi300]
"""
import argparse, bisect, csv, glob, json, os, sys

try:
    sys.stdout.reconfigure(encoding="utf-8")
except Exception:
    pass
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
KDAY = os.path.join(REPO, "data", "baostock", "kday")
SECTOR = os.path.join(REPO, "data", "baostock", "sector")
INDEX_DIR = os.path.join(REPO, "data", "baostock", "index")

_close, _sec = {}, {}


def close_at(sym, d, lag=0):
    if sym not in _close:
        dates, closes = [], []
        p = os.path.join(KDAY, f"{sym}.csv")
        if os.path.exists(p):
            with open(p, encoding="utf-8") as f:
                for row in csv.DictReader(f):
                    dates.append(row["time"][:10]); closes.append(float(row["close"]))
        _close[sym] = (dates, closes)
    dates, closes = _close[sym]
    if not dates:
        return None
    i = bisect.bisect_right(dates, d) - 1 + lag
    return closes[i] if 0 <= i < len(closes) else None


def sector_level(industry, d):
    """板块 EW 指数水平(index 列) at ≤d。"""
    if industry not in _sec:
        dates, lv = [], []
        p = os.path.join(SECTOR, f"{industry}.csv")
        if os.path.exists(p):
            with open(p, encoding="utf-8") as f:
                for row in csv.DictReader(f):
                    dates.append(row["time"][:10]); lv.append(float(row["index"]))
        _sec[industry] = (dates, lv)
    dates, lv = _sec[industry]
    if not dates:
        return None
    i = bisect.bisect_right(dates, d) - 1
    return lv[i] if 0 <= i < len(lv) else None


def load_index(name):
    m = {}
    with open(os.path.join(INDEX_DIR, f"{name}.csv"), encoding="utf-8") as f:
        for row in csv.DictReader(f):
            m[row["time"][:10]] = float(row["close"])
    return m, sorted(m)


def idx_ret(m, dates, d0, d1):
    def at(d):
        i = bisect.bisect_right(dates, d) - 1
        return m[dates[i]] if i >= 0 else None
    a, b = at(d0), at(d1)
    return (b / a - 1) if (a and b) else None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("run_json")
    ap.add_argument("--benchmark", default="csi300")
    a = ap.parse_args()
    rep = json.load(open(a.run_json, encoding="utf-8"))
    hold = [h for h in rep["holdings"]]
    ind = {}
    with open(os.path.join(REPO, "data", "baostock", "sector_membership.csv"), encoding="utf-8") as f:
        for row in csv.DictReader(f):
            ind[row["symbol"]] = row["industry"]
    m, dates = load_index(a.benchmark)

    # 累计三条 NAV：组合实际 / 行业配置(板块指数加权) / 基准
    nav_p = nav_a = nav_b = 1.0
    miss_sec = set()
    for i in range(len(hold) - 1):
        sel = hold[i]["selected"]
        if not sel:
            continue
        t0, t1 = hold[i]["t"][:10], hold[i + 1]["t"][:10]
        w = 1.0 / len(sel)
        rp = ra = 0.0
        sec_w = {}
        for s, _ in sel:
            p0, p1 = close_at(s, t0), close_at(s, t1)
            if p0 and p1 and p0 > 0:
                rp += w * (p1 / p0 - 1)
            sec_w[ind.get(s, "?")] = sec_w.get(ind.get(s, "?"), 0) + w
        for sec, ws in sec_w.items():
            l0, l1 = sector_level(sec, t0), sector_level(sec, t1)
            if l0 and l1 and l0 > 0:
                ra += ws * (l1 / l0 - 1)
            else:
                miss_sec.add(sec)
                p0 = p1 = None  # 板块缺 → 用组合该段贡献近似(配置=选择, 该板块贡献抵消)
                ra += ws * 0.0
        rb = idx_ret(m, dates, t0, t1) or 0.0
        nav_p *= 1 + rp; nav_a *= 1 + ra; nav_b *= 1 + rb
    tp, ta, tb = nav_p - 1, nav_a - 1, nav_b - 1
    print(f"=== 行业中性归因 · {os.path.basename(a.run_json)} (bench {a.benchmark}) ===")
    print(f"调仓 {len(hold)-1} 次；板块缺数据的行业数 {len(miss_sec)}（其配置回报记 0，略低估配置效应）")
    print(f"\n累计回报（毛，行业归因为价格口径，不含成本）：")
    print(f"  组合实际      r_p = {tp:>+8.2%}")
    print(f"  行业配置(板块指数加权) r_a = {ta:>+8.2%}")
    print(f"  基准 {a.benchmark}          r_b = {tb:>+8.2%}")
    print(f"\n超额拆解 vs {a.benchmark}（总超额 = 配置 + 选择）：")
    print(f"  行业配置效应 (r_a − r_b)  = {ta-tb:>+8.2%}   ← 持有便宜板块(中特估/红利)的贡献")
    print(f"  板块内选择效应 (r_p − r_a) = {tp-ta:>+8.2%}   ← 板块内挑到便宜个股的贡献")
    print(f"  总超额 (r_p − r_b)        = {tp-tb:>+8.2%}")
    if tp - tb != 0:
        print(f"\n  配置占比 {(ta-tb)/(tp-tb):.0%}  / 选择占比 {(tp-ta)/(tp-tb):.0%}")


if __name__ == "__main__":
    main()
