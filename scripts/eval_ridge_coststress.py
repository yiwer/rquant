"""Pre-live gauntlet ②: cost-stress the ridge-on-gauss candidate.

Weekly top-3 turnover is the known weakness. Re-run the 6-fold ridge-on-gauss
(2020-2026, membership) at escalating cost (20/30/40/50 bp). Hysteresis delta is
selected ONCE at 20bp (conservative: not re-tuned per cost — if it survives
higher cost without re-tuning, it is robust). Reports 6-fold mean net excess +
positive-fold count per cost level, and average per-rebalance turnover.
"""
import sys; sys.stdout.reconfigure(encoding="utf-8")
import os, numpy as np, pandas as pd
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import iterate as it
import eval_ridge as er

FOLDS = [
    ("2018-01-02", "2019-12-31", "2020-01-02", "2020-12-31"),
    ("2018-01-02", "2020-12-31", "2021-01-02", "2021-12-31"),
] + list(er.tn.WFO_FOLDS)
COSTS = [20.0, 30.0, 40.0, 50.0]


def main():
    st_set = set(pd.read_csv(er.ST_PATH)["symbol"]) if os.path.exists(er.ST_PATH) else set()
    idx_m, idx_dates = it.load_index("csi300")
    panel = pd.read_csv(er.PANEL_MEMBERSHIP, dtype={"symbol": str})

    # Per fold: fit once, select delta once (at 20bp), then backtest at each cost.
    per_cost = {c: [] for c in COSTS}
    turnovers = []
    print(f"ridge-on-gauss cost-stress, 6 folds (membership)")
    print(f"{'OOS':>8}" + "".join(f"{'%dbp' % int(c):>9}" for c in COSTS) + f"{'turn/reb':>10}")
    for tl, th, ol, oh in FOLDS:
        oos = panel[(panel["date"] >= ol) & (panel["date"] <= oh)]
        w, _ = er.fit_ridge(panel, tl, th)
        d = er.select_delta_ridge(panel, (tl, th, ol, oh), w, st_set)
        row = []
        turn = None
        for c in COSTS:
            rep = er.backtest_ridge(oos, w, top_n=er.TOP_N, cost_bps=c, st_set=st_set, delta=d)
            rel = it.to_index_relative(rep, idx_m, idx_dates)
            ex = rel["excess_return"] if rel else None
            per_cost[c].append(ex)
            row.append(ex)
            if turn is None:
                turn = rep.get("turnover", None)
                nreb = rep.get("n_rebalances", None)
                turn = (turn / nreb) if (turn is not None and nreb) else turn
        turnovers.append(turn if turn is not None else np.nan)
        print(f"{ol[:4]:>8}" + "".join(f"{x:>+9.3f}" for x in row) + f"{(turn if turn is not None else float('nan')):>10.2f}")

    print(f"\n=== 6-fold mean net excess by cost ===")
    for c in COSTS:
        v = np.array([x for x in per_cost[c] if x is not None])
        print(f"  {int(c)}bp: mean={v.mean():+.4f}  pos={int((v>0).sum())}/{len(v)}  min={v.min():+.4f}")
    print(f"\n  avg per-rebalance turnover ≈ {np.nanmean(turnovers):.2f}  (top-3: 1.0=full rotation)")
    print(f"  break-even cost ≈ where mean→0; +0.186@20bp with ~{np.nanmean(turnovers):.0%} turnover gives the slope.")


if __name__ == "__main__":
    main()
