"""Pre-live gauntlet ③: capacity / liquidity for ridge-on-gauss.

How much AUM can the weekly top-3 ridge-on-gauss hold before its own trading
moves prices? For each of the 6 OOS folds, re-run the VETTED selection
(fit_ridge → select_delta → backtest_ridge) and read the per-rebalance picks
from the report's holdings. Each pick's ADV proxy = exp(f_logamt) (that day's ¥
turnover; the eligibility floor is ¥50M). Equal-weight top-3 ⇒ each name gets
AUM/TOP_N, which must stay under a participation cap p of its ADV:

    AUM/TOP_N ≤ p · ADV_i  ∀ held i   ⇒   max_AUM = TOP_N · p · min_i(ADV_i).

Reports the distribution of per-rebalance min-ADV and max_AUM at p∈{10%,25%}.
Conservative: assumes the FULL position is rebuilt each rebalance; hysteresis
means only ~1/3 of the book actually rotates weekly, so live capacity is higher.
"""
import sys
import os
sys.stdout.reconfigure(encoding="utf-8")
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import numpy as np
import pandas as pd

import eval_ridge as er
import iterate as it

FOLDS = [
    ("2018-01-02", "2019-12-31", "2020-01-02", "2020-12-31"),
    ("2018-01-02", "2020-12-31", "2021-01-02", "2021-12-31"),
] + list(er.tn.WFO_FOLDS)
PARTICIP = [0.10, 0.25]


def main():
    st_set = set(pd.read_csv(er.ST_PATH)["symbol"]) if os.path.exists(er.ST_PATH) else set()
    panel = pd.read_csv(er.PANEL_MEMBERSHIP, dtype={"symbol": str})

    # ADV proxy: exp(f_logamt) = that day's ¥ turnover, keyed by (date, symbol).
    adv = {(d, s): float(np.exp(la))
           for d, s, la in zip(panel["date"], panel["symbol"], panel["f_logamt"])
           if pd.notna(la)}

    per_reb_minadv = []   # min ADV across the 3 picks, one per rebalance
    print("ridge-on-gauss capacity (6 folds, membership; ADV=exp(f_logamt)=日¥成交额)")
    print(f"{'OOS':>8}{'rebs':>6}{'medMinADV/¥':>16}{'p10MinADV/¥':>16}")
    for tl, th, ol, oh in FOLDS:
        oos = panel[(panel["date"] >= ol) & (panel["date"] <= oh)]
        w, _ = er.fit_ridge(panel, tl, th)
        d = er.select_delta_ridge(panel, (tl, th, ol, oh), w, st_set)
        rep = er.backtest_ridge(oos, w, top_n=er.TOP_N, cost_bps=it.COST, st_set=st_set, delta=d)
        fold_min = []
        for h in rep["holdings"]:
            advs = [adv.get((h["t"], s)) for s in h["picks"]]
            advs = [a for a in advs if a is not None]
            if advs:
                m = min(advs)
                fold_min.append(m)
                per_reb_minadv.append(m)
        if fold_min:
            fm = np.array(fold_min)
            print(f"{ol[:4]:>8}{len(fold_min):>6}{np.median(fm):>16,.0f}{np.percentile(fm, 10):>16,.0f}")

    arr = np.array(per_reb_minadv)
    print(f"\n=== aggregate over {len(arr)} rebalances (min-ADV of the 3 picks) ===")
    for q, lbl in [(50, "median"), (25, "p25"), (10, "p10"), (0, "min")]:
        print(f"  {lbl:>6} min-ADV = ¥{np.percentile(arr, q):,.0f}")

    print(f"\n=== max AUM = TOP_N({er.TOP_N}) × p × min-ADV  (conservative full-build) ===")
    print(f"{'particip':>10}{'median ¥':>18}{'p10 ¥ (worst wk)':>20}{'min ¥':>18}")
    for p in PARTICIP:
        med = er.TOP_N * p * np.percentile(arr, 50)
        p10 = er.TOP_N * p * np.percentile(arr, 10)
        mn = er.TOP_N * p * arr.min()
        print(f"{('%.0f%%' % (p * 100)):>10}{med:>18,.0f}{p10:>20,.0f}{mn:>18,.0f}")

    print(f"\n  注:迟滞 δ 周换手~0.33 → 实际每周仅换~1 只,滚动期容量高于此全建仓保守值。")
    print(f"  注:ADV=单日¥成交额代理(eligibility floor ¥50M);分散到多日建仓可进一步放大容量。")


if __name__ == "__main__":
    main()
