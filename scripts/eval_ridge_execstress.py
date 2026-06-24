"""Pre-live gauntlet: A-share EXECUTION-REALISM stress test for ridge-on-gauss.

The vetted §5.3 verdict (+0.186, 6/6 folds) is a *backtestable* upper bound that
assumes (a) you can transact every pick at its signal-day close and (b) a flat
20bp cost. Neither holds for a weekly top-3, small-cap-tilted A-share book:

  1. LIMIT-UP UNBUYABLE — a pick that closed locked at its daily upper limit
     (一字板 / 封板) cannot actually be bought at that close. The label still
     credits its forward return → systematic inflation. We exclude limit-up-locked
     names from the candidate pool before taking top-3.
  2. MARKET IMPACT — at real AUM, trading AUM/top_n through a name's ADV moves the
     price. We add a participation-dependent square-root impact on top of the flat
     cost, swept over AUM levels around the capacity finding (~¥54M @ 10% particip.).

Reuses the VETTED primitives verbatim (fit_ridge / select_delta_ridge / _eligible /
norm_gauss / to_index_relative); the ONLY change is inside the backtest loop.
Reports a degradation ladder vs the frictionless +0.186 baseline, all on the same
6 OOS folds / membership pool, with the frictionless run reproduced as a control.
"""
import sys
import os

sys.stdout.reconfigure(encoding="utf-8")
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import numpy as np
import pandas as pd

import eval_ridge as er
import exec_model as em
import iterate as it
import train_nonlinear as tn
from build_factor_matrix import FACTOR_COLS, KDAY
from test_norm_hysteresis import norm_gauss

# 6 OOS folds: 2020+2021 (earlier regimes) + the 4 WFO folds — same set as gauntlet ①.
FOLDS = [
    ("2018-01-02", "2019-12-31", "2020-01-02", "2020-12-31"),
    ("2018-01-02", "2020-12-31", "2021-01-02", "2021-12-31"),
] + list(tn.WFO_FOLDS)

LOCK_FLAGS_PATH = os.path.join(er.OUT_DIR, "lock_flags.csv")
# AUM levels (¥) swept for impact: around the capacity finding (~¥54M @10% particip.).
AUM_GRID = [1e7, 3e7, 5e7, 1e8]
IMPACT_K = 100.0  # square-root impact (bps) at 100% participation


def compute_lock_rows(kday, symbol, dates):
    """Per-(date,symbol) limit-lock flags for the rebalance dates in `dates`.

    kday: DataFrame with columns time, open, high, low, close, pctChg (baostock daily).
    Returns list of {date, symbol, lock_up, lock_down}. Pure (no I/O).
    """
    k = kday.copy()
    k["d"] = k["time"].astype(str).str[:10]
    k = k[k["d"].isin(dates)]
    rows = []
    for _, r in k.iterrows():
        d = r["d"]
        lim = em.board_limit_pct(symbol, d)
        pct = float(r["pctChg"]) if pd.notna(r["pctChg"]) else float("nan")
        up = em.is_locked_up(pct, float(r["close"]), float(r["high"]), lim)
        dn = em.is_locked_down(pct, float(r["close"]), float(r["low"]), lim)
        rows.append({"date": d, "symbol": symbol, "lock_up": up, "lock_down": dn})
    return rows


def backtest_ridge_exec(panel, w, top_n, cost_bps, st_set, delta,
                        lock_col="lock_up", aum=0.0, impact_k=0.0):
    """Mirror of eval_ridge.backtest_ridge with two execution-realism knobs.

    With lock_col absent/all-False AND aum/impact_k == 0 this is bit-for-bit
    identical to the vetted backtest_ridge (see test_knobs_off_equals_vetted_baseline).

    Knob 1 (limit-up exclusion): rows where panel[lock_col] is True are dropped
      from the eligible candidate pool BEFORE ranking → cannot be bought.
    Knob 2 (impact cost): each rebalance adds a participation-dependent square-root
      impact (bps) on top of cost_bps, charged on the turned fraction. Per-name
      traded notional = aum/top_n; ADV = exp(f_logamt).
    """
    panel = panel.sort_values(["date", "symbol"])
    nav = 1.0
    prev = set()
    navs = []
    period_rets = []
    total_turn = 0.0

    TRAIN = ("train", "2018-01-02", "2023-12-29")
    OOS = ("2024-26_OOS", "2024-01-02", "2026-06-30")

    dates = sorted(panel["date"].unique())

    for d in dates:
        g = er._eligible(panel[panel["date"] == d].dropna(subset=["fwd_ret_5d"]), st_set)

        # Knob 1: drop limit-up-locked names (can't buy at that close)
        if lock_col in g.columns:
            g = g[~g[lock_col].fillna(False).astype(bool)]

        if len(g) < top_n:
            continue

        G = norm_gauss(g[FACTOR_COLS].to_numpy(float))
        score = G @ np.asarray(w, float)

        if delta > 0.0 and prev:
            is_incumbent = g["symbol"].isin(prev).to_numpy()
            score = score + delta * is_incumbent.astype(float)

        gi = g.assign(_score=score).sort_values(["_score", "symbol"], ascending=[False, True])
        pick = gi.head(top_n)
        ret = float(pick["fwd_ret_5d"].mean())
        cur = set(pick["symbol"])

        turn = len(cur ^ prev) / max(len(cur) + len(prev), 1)
        total_turn += turn

        # Knob 2: participation-dependent square-root impact (bps), on traded fraction
        impact_bps = 0.0
        if aum > 0 and impact_k > 0 and turn > 0:
            advs = np.exp(pick["f_logamt"].to_numpy(float))
            per_name_notional = aum / top_n
            imps = [em.sqrt_impact_bps(per_name_notional, float(a), impact_k) for a in advs]
            impact_bps = float(np.mean(imps))

        ret_net = ret - (cost_bps + impact_bps) / 1e4 * turn
        period_rets.append(ret_net)
        nav *= (1.0 + ret_net)
        navs.append({"t": d, "nav": nav, "picks": list(cur)})
        prev = cur

    total = navs[-1]["nav"] - 1.0 if navs else 0.0
    peak = -1e9
    mdd = 0.0
    for h in navs:
        peak = max(peak, h["nav"])
        mdd = max(mdd, 1.0 - h["nav"] / peak)

    pr = np.array(period_rets)
    sharpe = float(np.mean(pr) / np.std(pr) * np.sqrt(48)) if len(pr) > 1 and np.std(pr) > 0 else None

    return {
        "holdings": navs,
        "regime_slices": [{"label": L, "from": a, "to": b} for L, a, b in [TRAIN, OOS]],
        "risk": {"sharpe": sharpe},
        "total_return": total,
        "max_drawdown": mdd,
        "turnover": total_turn,
        "n_rebalances": len(navs),
        "excess_return": 0.0,
    }


# ---------------------------------------------------------------------------
# Lock-flag sidecar: built once from kday, cached to data/factor_panel/lock_flags.csv
# ---------------------------------------------------------------------------

def load_or_build_lock_flags(panel, force=False):
    """Return a DataFrame of (date, symbol, lock_up, lock_down) for every panel pair.

    Cached to LOCK_FLAGS_PATH; rebuilt from kday on first run or when force=True.
    """
    if os.path.exists(LOCK_FLAGS_PATH) and not force:
        print(f"  lock_flags: cache hit {LOCK_FLAGS_PATH}")
        return pd.read_csv(LOCK_FLAGS_PATH, dtype={"symbol": str})

    dates_by_sym = panel.groupby("symbol")["date"].apply(lambda s: set(s.astype(str))).to_dict()
    rows = []
    miss = 0
    for sym, dates in dates_by_sym.items():
        kp = os.path.join(KDAY, f"{sym}.csv")
        if not os.path.exists(kp):
            miss += 1
            continue
        kday = pd.read_csv(kp, usecols=["time", "open", "high", "low", "close", "pctChg"])
        rows.extend(compute_lock_rows(kday, sym, dates))
    flags = pd.DataFrame(rows)
    flags.to_csv(LOCK_FLAGS_PATH, index=False, encoding="utf-8")
    n_up = int(flags["lock_up"].sum()) if len(flags) else 0
    print(f"  lock_flags: built {len(flags)} rows ({n_up} limit-up), {miss} symbols missing kday → {LOCK_FLAGS_PATH}")
    return flags


def attach_lock_flags(panel, flags):
    """Left-join lock_up onto the panel; missing → False."""
    m = panel.merge(flags[["date", "symbol", "lock_up"]], on=["date", "symbol"], how="left")
    m["lock_up"] = m["lock_up"].fillna(False).astype(bool)
    return m


# ---------------------------------------------------------------------------
# Aggregation helpers
# ---------------------------------------------------------------------------

def _aggregate(per_fold):
    vals = [v for _, v in per_fold if v is not None]
    mean = float(np.mean(vals)) if vals else None
    pos = sum(1 for v in vals if v > 0)
    n = len(vals)
    verdict = mean is not None and mean > 0 and pos > n / 2
    return mean, pos, n, verdict


# ---------------------------------------------------------------------------
# Main orchestration — degradation ladder vs frictionless +0.186 baseline
# ---------------------------------------------------------------------------

def main(force_locks=False):
    st_set = set(pd.read_csv(er.ST_PATH)["symbol"]) if os.path.exists(er.ST_PATH) else set()
    idx_data = it.load_index("csi300")
    panel = pd.read_csv(er.PANEL_MEMBERSHIP, dtype={"symbol": str})
    print(f"Panel: {len(panel)} rows, dates {panel['date'].min()}..{panel['date'].max()}; ST {len(st_set)}")

    flags = load_or_build_lock_flags(panel, force=force_locks)
    panel = attach_lock_flags(panel, flags)

    rungs = [("frictionless", False, 0.0), ("limit-up excl", True, 0.0)]
    rungs += [(f"+impact ¥{a/1e6:.0f}M", True, a) for a in AUM_GRID]

    results = {name: [] for name, _, _ in rungs}
    lock_total = 0
    lock_hit = 0

    print(f"\n{'='*64}\nPer-fold OOS excess (vs csi300) by execution rung\n{'='*64}")
    for fold in FOLDS:
        tl, th, ol, oh = fold
        oos = panel[(panel["date"] >= ol) & (panel["date"] <= oh)].copy()
        if len(oos) == 0:
            continue
        w, n_tr = er.fit_ridge(panel, tl, th)
        delta = er.select_delta_ridge(panel, fold, w, st_set)

        # Frictionless baseline picks → diagnostic: how many entries were limit-up-locked
        base = er.backtest_ridge(oos, w, top_n=er.TOP_N, cost_bps=it.COST, st_set=st_set, delta=delta)
        lu = {(r.date, r.symbol): bool(r.lock_up)
              for r in oos[["date", "symbol", "lock_up"]].itertuples(index=False)}
        for h in base["holdings"]:
            for s in h["picks"]:
                lock_total += 1
                if lu.get((h["t"], s), False):
                    lock_hit += 1

        line = [f"  {ol[:4]} (Δ={delta:.2f}, {n_tr}tr): "]
        for name, lock_on, aum in rungs:
            if name == "frictionless":
                rep = base
            else:
                rep = backtest_ridge_exec(
                    oos, w, top_n=er.TOP_N, cost_bps=it.COST, st_set=st_set, delta=delta,
                    lock_col="lock_up", aum=aum, impact_k=(IMPACT_K if aum > 0 else 0.0),
                )
            rel = it.to_index_relative(rep, idx_data[0], idx_data[1])
            ex_val = rel["excess_return"] if rel else None
            results[name].append((ol[:4], ex_val))
            line.append(f"{name}={ex_val:+.3f}" if ex_val is not None else f"{name}=NA")
        print("  ".join(line))

    print(f"\n{'='*64}\nDegradation ladder (6-fold mean OOS excess)\n{'='*64}")
    base_mean, _, _, _ = _aggregate(results["frictionless"])
    print(f"{'rung':>18}{'mean':>10}{'pos':>7}{'§5.3':>7}{'vs baseline':>14}")
    for name, _lo, _a in rungs:
        mean, pos, n, verdict = _aggregate(results[name])
        if mean is None:
            print(f"{name:>18}{'NA':>10}")
            continue
        drop = "" if name == "frictionless" else f"{(mean-base_mean):+.3f}"
        print(f"{name:>18}{mean:>+10.3f}{f'{pos}/{n}':>7}{str(verdict):>7}{drop:>14}")

    pct = 100.0 * lock_hit / lock_total if lock_total else 0.0
    print(f"\nDIAGNOSTIC: of {lock_total} frictionless top-3 entries, {lock_hit} ({pct:.1f}%) "
          f"closed limit-up locked on entry day → unbuyable (the inflation channel).")
    print(f"\nCONTROL: frictionless mean should reproduce gauntlet ① ~+0.186 "
          f"(actual {base_mean:+.3f}).")


if __name__ == "__main__":
    main(force_locks="--rebuild-locks" in sys.argv)
