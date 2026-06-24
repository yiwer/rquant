"""Strategy diversification: blend ridge-on-gauss with the value-净利双核 baseline.

The gauntlet showed the two scorers are regime-complementary (ridge wins
down-years, equal-weight value wins some bull-years) → likely low/negative
return correlation → a blend may lift risk-adjusted return without fishing.

Both run the SAME weekly top-3 / membership / cost / hysteresis harness; ONLY
the scoring differs:
  * ridge   = norm_gauss(72 factors) @ w_ridge   (+ selected delta)
  * value   = rank(f_bm)+rank(f_npyoy), top-3     (delta 0) — the 价值净利双核 logic
Per OOS fold, derive each sleeve's weekly net-return series from its backtest
NAV, measure their correlation, and compare a fixed 50/50 return-blend against
each sleeve alone on annualized Sharpe, max-drawdown, and csi300 excess. No
parameter search — a single pre-specified 50/50 blend (optimal weight printed
as info only).
"""
import sys
import os
import argparse
import json
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
PER_YEAR = 48  # weekly periods/yr (annualization)


def nav_to_rets(holdings):
    """holdings [{t,nav}] → {date: net period return}."""
    rets, prev = {}, 1.0
    for h in holdings:
        rets[h["t"]] = h["nav"] / prev - 1.0
        prev = h["nav"]
    return rets


def _sharpe(r):
    r = np.asarray(r, float)
    return float(r.mean() / r.std() * np.sqrt(PER_YEAR)) if len(r) > 1 and r.std() > 0 else np.nan


def _maxdd(r):
    nav = np.cumprod(1.0 + np.asarray(r, float))
    peak = np.maximum.accumulate(nav)
    return float(np.max(1.0 - nav / peak)) if len(nav) else np.nan


def _cagr(r):
    nav = float(np.prod(1.0 + np.asarray(r, float)))
    yrs = len(r) / PER_YEAR
    return float(nav ** (1.0 / yrs) - 1.0) if yrs > 0 and nav > 0 else np.nan


def _excess(dates, rets, idx):
    """csi300-relative excess over the period, via to_index_relative."""
    idx_m, idx_dates = idx
    nav, hold = 1.0, []
    for d in dates:
        nav *= (1.0 + rets[d])
        hold.append({"t": d, "nav": nav})
    rep = {"holdings": hold, "regime_slices": [], "max_drawdown": 0.0,
           "turnover": 0.0, "n_rebalances": len(hold)}
    rel = it.to_index_relative(rep, idx_m, idx_dates)
    return rel["excess_return"] if rel else np.nan


def main():
    ap = argparse.ArgumentParser(); ap.add_argument("--json", default=None); args = ap.parse_args()
    st_set = set(pd.read_csv(er.ST_PATH)["symbol"]) if os.path.exists(er.ST_PATH) else set()
    panel = pd.read_csv(er.PANEL_MEMBERSHIP, dtype={"symbol": str})
    idx = it.load_index("csi300")
    p = len(er.FACTOR_COLS) if hasattr(er, "FACTOR_COLS") else None
    from build_factor_matrix import FACTOR_COLS
    w_eq = np.zeros(len(FACTOR_COLS)); w_eq[0] = 1.0; w_eq[1] = 1.0   # f_bm + f_npyoy

    print("blend ridge-on-gauss × value(净利双核) — weekly, 6 OOS folds (membership)")
    print(f"{'OOS':>6}{'corr':>8}{'shR':>7}{'shV':>7}{'shB':>7}{'ddR':>7}{'ddV':>7}{'ddB':>7}{'exR':>8}{'exV':>8}{'exB':>8}")
    agg = {k: [] for k in ["corr", "shR", "shV", "shB", "ddR", "ddV", "ddB", "exR", "exV", "exB", "wopt"]}
    fold_rows = []
    for tl, th, ol, oh in FOLDS:
        oos = panel[(panel["date"] >= ol) & (panel["date"] <= oh)]
        w, _ = er.fit_ridge(panel, tl, th)
        d = er.select_delta_ridge(panel, (tl, th, ol, oh), w, st_set)
        rep_r = er.backtest_ridge(oos, w, top_n=er.TOP_N, cost_bps=it.COST, st_set=st_set, delta=d)
        rep_v = er.backtest_rank_linear(oos, w_eq, top_n=er.TOP_N, cost_bps=it.COST, st_set=st_set, delta=0.0)
        rr, rv = nav_to_rets(rep_r["holdings"]), nav_to_rets(rep_v["holdings"])
        common = sorted(set(rr) & set(rv))
        if len(common) < 10:
            continue
        ar = np.array([rr[t] for t in common])
        av = np.array([rv[t] for t in common])
        ab = 0.5 * ar + 0.5 * av
        corr = float(np.corrcoef(ar, av)[0, 1]) if ar.std() > 0 and av.std() > 0 else np.nan
        # info-only optimal (min-variance) blend weight on ridge
        vr, vv, cv = ar.var(), av.var(), np.cov(ar, av)[0, 1]
        wopt = float(np.clip((vv - cv) / (vr + vv - 2 * cv), 0, 1)) if (vr + vv - 2 * cv) > 0 else 0.5
        rd = {t: ar[i] for i, t in enumerate(common)}
        vd = {t: av[i] for i, t in enumerate(common)}
        bd = {t: ab[i] for i, t in enumerate(common)}
        exR, exV, exB = _excess(common, rd, idx), _excess(common, vd, idx), _excess(common, bd, idx)
        row = dict(corr=corr, shR=_sharpe(ar), shV=_sharpe(av), shB=_sharpe(ab),
                   ddR=_maxdd(ar), ddV=_maxdd(av), ddB=_maxdd(ab), exR=exR, exV=exV, exB=exB, wopt=wopt)
        for k in agg:
            agg[k].append(row[k])
        print(f"{ol[:4]:>6}{corr:>+8.2f}{row['shR']:>7.2f}{row['shV']:>7.2f}{row['shB']:>7.2f}"
              f"{row['ddR']:>7.2f}{row['ddV']:>7.2f}{row['ddB']:>7.2f}{exR:>+8.3f}{exV:>+8.3f}{exB:>+8.3f}")
        fold_rows.append({"oos": ol[:4], "corr": corr, "sh_ridge": row["shR"], "sh_val": row["shV"],
                          "sh_blend": row["shB"], "dd_ridge": row["ddR"], "dd_val": row["ddV"],
                          "dd_blend": row["ddB"], "ex_ridge": row["exR"], "ex_val": row["exV"], "ex_blend": row["exB"]})

    print("\n=== 6-fold means ===")
    m = {k: float(np.nanmean(v)) for k, v in agg.items()}
    print(f"  return corr(ridge,value): {m['corr']:+.2f}   min-var optimal ridge weight ≈ {m['wopt']:.2f}")
    print(f"  Sharpe   ridge {m['shR']:.2f} | value {m['shV']:.2f} | blend {m['shB']:.2f}")
    print(f"  maxDD    ridge {m['ddR']:.2f} | value {m['ddV']:.2f} | blend {m['ddB']:.2f}")
    print(f"  excess   ridge {m['exR']:+.3f} | value {m['exV']:+.3f} | blend {m['exB']:+.3f}")
    best_single_sh = max(m['shR'], m['shV'])
    best_single_dd = min(m['ddR'], m['ddV'])
    print(f"\n  blend Sharpe beats best single: {m['shB'] > best_single_sh}  "
          f"({m['shB']:.2f} vs {best_single_sh:.2f})")
    print(f"  blend maxDD below best single:  {m['ddB'] < best_single_dd}  "
          f"({m['ddB']:.2f} vs {best_single_dd:.2f})")
    print(f"  → diversification {'HELPS' if (m['shB'] > best_single_sh or m['ddB'] < best_single_dd) else 'does NOT help'} "
          f"(corr {m['corr']:+.2f})")

    if args.json:
        mean = {"corr": m["corr"], "sh_ridge": m["shR"], "sh_val": m["shV"], "sh_blend": m["shB"],
                "dd_ridge": m["ddR"], "dd_val": m["ddV"], "dd_blend": m["ddB"],
                "ex_ridge": m["exR"], "ex_val": m["exV"], "ex_blend": m["exB"]}
        json.dump({"folds": fold_rows, "mean": mean}, open(args.json, "w", encoding="utf-8"),
                  ensure_ascii=False, indent=2)
        print(f"[eval_blend] json → {args.json}")


if __name__ == "__main__":
    main()
