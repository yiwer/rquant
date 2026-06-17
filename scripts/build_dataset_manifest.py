#!/usr/bin/env python3
"""装配 baostock 5yr 回测数据集：扫描覆盖 → rquant universe CSV + manifest.json（+轻量校验）。

数据集布局（data/baostock/，gitignored，可复现）：
  kday/<sym>.csv          日线 qfq OHLCV(+turn,pctChg) 2018+      ← rquant primary（日频回测）
  k15m/<sym>.csv          15m  qfq OHLCV(+amount)        2021+      ← rquant primary（日内→日频）
  features_day/<sym>.csv  日线扩展TA指标（独立分析存档）
  features_15m/<sym>.csv  15m 扩展TA指标（独立分析存档）
  sector/<industry>.csv   各行业等权日线序列(ret/index/n/breadth)
  sector_membership.csv   股→行业；sector_daily_panel.csv 横截面板块面板
产出：
  universe_baostock_day.csv / universe_baostock_15m.csv  （symbol,primary,context,fundamentals[财务]）
  dataset_manifest.json    覆盖/日期范围/总条数/质量旗标/来源
TA 指标为独立存档（引擎 DSL 亦可现算），不进 universe fundamentals 列（该列指向真财务 data/fundamentals）。
"""
import argparse, glob, json, os
import pandas as pd

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def scan_dir(d):
    """→ {sym: (n_rows, first_time, last_time, monotonic_ok, dup)}"""
    info = {}
    for p in glob.glob(os.path.join(d, "*.csv")):
        sym = os.path.splitext(os.path.basename(p))[0]
        try:
            t = pd.read_csv(p, usecols=["time"])["time"]
        except Exception:
            info[sym] = (0, None, None, False, 0); continue
        n = len(t)
        if n == 0:
            info[sym] = (0, None, None, True, 0); continue
        td = pd.to_datetime(t)
        info[sym] = (n, t.iloc[0], t.iloc[-1], bool(td.is_monotonic_increasing), int(td.duplicated().sum()))
    return info


def abs_fwd(*parts):
    return os.path.abspath(os.path.join(REPO, *parts)).replace("\\", "/")


def write_universe(out, syms, bars_subdir, fund_dir):
    with open(out, "w", encoding="utf-8", newline="") as f:
        f.write("symbol,primary,context,fundamentals\n")
        for s in sorted(syms):
            fp = abs_fwd("data", "fundamentals", f"{s}.csv")
            fund = fp if os.path.exists(os.path.join(REPO, "data", "fundamentals", f"{s}.csv")) else ""
            f.write(f"{s},{abs_fwd('data','baostock',bars_subdir,s+'.csv')},,{fund}\n")
    return sum(1 for _ in syms)


def summ(info):
    rows = [v[0] for v in info.values() if v[0] > 0]
    firsts = [v[1] for v in info.values() if v[1]]
    lasts = [v[2] for v in info.values() if v[2]]
    bad = [s for s, v in info.items() if v[0] > 0 and (not v[3] or v[4] > 0)]
    return {"n_symbols": len([v for v in info.values() if v[0] > 0]),
            "total_rows": int(sum(rows)),
            "date_min": min(firsts) if firsts else None,
            "date_max": max(lasts) if lasts else None,
            "n_quality_flags": len(bad), "quality_flag_symbols": sorted(bad)[:20]}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", default=os.path.join(REPO, "data", "baostock"))
    a = ap.parse_args()
    kday = scan_dir(os.path.join(a.root, "kday"))
    k15m = scan_dir(os.path.join(a.root, "k15m"))
    fday = scan_dir(os.path.join(a.root, "features_day"))
    f15m = scan_dir(os.path.join(a.root, "features_15m"))

    day_syms = [s for s, v in kday.items() if v[0] > 0]
    m15_syms = [s for s, v in k15m.items() if v[0] > 0]
    write_universe(os.path.join(a.root, "universe_baostock_day.csv"), day_syms, "kday", "fundamentals")
    write_universe(os.path.join(a.root, "universe_baostock_15m.csv"), m15_syms, "k15m", "fundamentals")

    secdir = os.path.join(a.root, "sector")
    n_sectors = len(glob.glob(os.path.join(secdir, "*.csv"))) if os.path.isdir(secdir) else 0
    manifest = {
        "dataset": "rquant baostock 5yr backtest dataset",
        "source": "baostock qfq (adjustflag=2); survivorship-free union of monthly top-2000 since 2021",
        "kday": summ(kday), "k15m": summ(k15m),
        "features_day": {"n_symbols": len([v for v in fday.values() if v[0] > 0])},
        "features_15m": {"n_symbols": len([v for v in f15m.values() if v[0] > 0])},
        "sectors": {"n_industries": n_sectors,
                    "membership_rows": (len(pd.read_csv(os.path.join(a.root, "sector_membership.csv")))
                                        if os.path.exists(os.path.join(a.root, "sector_membership.csv")) else 0)},
        "target_universe_size": 5115,
    }
    with open(os.path.join(a.root, "dataset_manifest.json"), "w", encoding="utf-8") as f:
        json.dump(manifest, f, ensure_ascii=False, indent=2)
    print(json.dumps(manifest, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
