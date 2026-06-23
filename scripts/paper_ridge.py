#!/usr/bin/env python3
"""「去相关岭组合」前向纸面册 (forward paper-trade book).

The ridge-on-gauss composite passed every back-testable gauntlet (cross-regime
6/6, cost-stress to 50bp). The only honest validation left is FORWARD: lock real
weekly top-3 picks now and realise their P&L as time passes. This is paper only —
no real money, no touch to the frozen live deployment (价值净利双核 monthly top-3).

HONEST-FORWARD CONTRACT
-----------------------
* Weights are FROZEN at inception: fit_ridge over ALL labelled history
  (panel start .. last date with a known fwd_ret_5d). Hysteresis delta is chosen
  on that same train slice (no OOS peek — there is no OOS yet). Stored in
  paper_ridge_weights.json; never silently refit. `--retrain` refreezes.
* The paper book contains ONLY rebalance dates strictly after the frozen
  train_hi. Every such date is genuine out-of-sample.
* Picks are LOCKED when made (status=open) using the eligible cross-section at
  that date — the live week has no fwd_ret_5d yet, that is the point. P&L is
  realised later from the LOCKED symbols once their fwd_ret_5d is available
  (status=closed). The journal is the source of truth for picks.

Run weekly (after the data pipeline + build_factor_matrix.py advance the panel):
    python scripts/paper_ridge.py            # advance the book, print status
    python scripts/paper_ridge.py --status   # read-only, no write
    python scripts/paper_ridge.py --retrain  # refreeze weights at today's cutoff

Reuses the VETTED primitives from eval_ridge / iterate verbatim:
  fit_ridge, _eligible, select_delta_ridge, norm_gauss, TOP_N, RIDGE_A, COST,
  to_index_relative (csi300 excess). Pick scoring is identical to backtest_ridge
  (gauss-normalise → @w → +delta*incumbent → sort by score then symbol).
"""
import sys
import os
sys.stdout.reconfigure(encoding="utf-8")
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import argparse
import csv
import datetime as _dt
import json

import numpy as np
import pandas as pd

import eval_ridge as er
import iterate as it
from build_factor_matrix import FACTOR_COLS
from test_norm_hysteresis import norm_gauss

WEIGHTS_PATH = os.path.join(er.OUT_DIR, "paper_ridge_weights.json")
JOURNAL_PATH = os.path.join(er.OUT_DIR, "paper_ridge_journal.csv")
SEP = ";"
JOURNAL_COLS = ["date", "status", "picks", "prev_picks", "turnover", "gross_ret", "net_ret"]


# ---------------------------------------------------------------------------
# Pure logic (unit-tested in test_paper_ridge.py; no I/O, no network)
# ---------------------------------------------------------------------------

def select_picks(elig, w, prev_picks, delta, top_n=er.TOP_N):
    """Pick top_n symbols from an eligible cross-section.

    Scoring is identical to eval_ridge.backtest_ridge:
        score = norm_gauss(factor_matrix) @ w   (+ delta on incumbents)
        sort by (score desc, symbol asc), take top_n.

    `elig` is a DataFrame carrying FACTOR_COLS + 'symbol'. It does NOT need
    fwd_ret_5d — the live week has none yet.
    """
    if len(elig) == 0:
        return []
    G = norm_gauss(elig[FACTOR_COLS].to_numpy(float))
    score = G @ np.asarray(w, float)
    if delta > 0.0 and prev_picks:
        inc = elig["symbol"].isin(set(prev_picks)).to_numpy()
        score = score + delta * inc.astype(float)
    gi = elig.assign(_score=score).sort_values(["_score", "symbol"], ascending=[False, True])
    return list(gi.head(top_n)["symbol"])


def _turnover(cur, prev):
    """Symmetric-difference turnover, identical to backtest_ridge."""
    cur, prev = set(cur), set(prev)
    return len(cur ^ prev) / max(len(cur) + len(prev), 1)


def realize_position(picks, prev_picks, fwd_map, cost_bps=it.COST):
    """Realise a locked position once every pick has a known fwd_ret_5d.

    Returns {gross_ret, turnover, net_ret} or None if any pick's forward return
    is still missing/NaN (position not yet matured → keep open).
    Mirrors backtest_ridge: gross = mean(fwd), net = gross - cost*turnover.
    """
    if not picks:
        return None
    rets = []
    for s in picks:
        v = fwd_map.get(s)
        if v is None or (isinstance(v, float) and np.isnan(v)):
            return None
        rets.append(float(v))
    gross = float(np.mean(rets))
    turn = _turnover(picks, prev_picks or [])
    net = gross - cost_bps / 1e4 * turn
    return {"gross_ret": gross, "turnover": turn, "net_ret": net}


def advance_journal(panel, w, delta, train_hi, st_set, journal,
                    cost_bps=it.COST, top_n=er.TOP_N):
    """Open new paper dates (> train_hi) and realise matured ones.

    `journal` is a list of row dicts (JOURNAL_COLS). Returns the updated,
    date-sorted list. Closed rows are immutable; open rows are realised when
    their forward returns become available. Pure given (panel, params, journal).
    """
    by_date = {r["date"]: dict(r) for r in journal}
    paper_dates = [d for d in sorted(panel["date"].unique()) if d > train_hi]

    for d in paper_dates:
        g = panel[panel["date"] == d]
        prior = [x for x in sorted(by_date) if x < d]
        prev_picks = _split(by_date[prior[-1]]["picks"]) if prior else []

        if d not in by_date:
            elig = er._eligible(g, st_set)          # live pick: NO dropna on fwd
            picks = select_picks(elig, w, prev_picks, delta, top_n)
            by_date[d] = {
                "date": d, "status": "open", "picks": SEP.join(picks),
                "prev_picks": SEP.join(prev_picks),
                "turnover": _turnover(picks, prev_picks), "gross_ret": "", "net_ret": "",
            }

        row = by_date[d]
        if row["status"] == "open":
            picks = _split(row["picks"])
            fwd_map = dict(zip(g["symbol"], g["fwd_ret_5d"]))
            rz = realize_position(picks, _split(row["prev_picks"]), fwd_map, cost_bps)
            if rz is not None:
                row["status"] = "closed"
                row["turnover"] = rz["turnover"]
                row["gross_ret"] = rz["gross_ret"]
                row["net_ret"] = rz["net_ret"]

    return [by_date[d] for d in sorted(by_date)]


def running_nav(rows):
    """Cumulative strategy NAV over closed rows (compound net returns)."""
    nav = 1.0
    out = []
    for r in rows:
        if r["status"] == "closed":
            nav *= (1.0 + float(r["net_ret"]))
        out.append({"date": r["date"], "status": r["status"], "nav": nav})
    return out


def _split(s):
    return [p for p in str(s).split(SEP) if p] if s else []


# ---------------------------------------------------------------------------
# Weights (freeze / load)
# ---------------------------------------------------------------------------

def build_weights(panel, st_set):
    """Freeze ridge-on-gauss weights over ALL labelled history + train-slice delta."""
    labelled = panel.dropna(subset=["fwd_ret_5d"])
    train_lo = str(panel["date"].min())
    train_hi = str(labelled["date"].max())
    w, ntr = er.fit_ridge(panel, train_lo, train_hi)
    delta = er.select_delta_ridge(panel, (train_lo, train_hi, train_lo, train_hi), w, st_set)
    return {
        "created": _dt.datetime.now().isoformat(timespec="seconds"),
        "strategy": "ridge-on-gauss / 去相关岭组合",
        "train_lo": train_lo,
        "train_hi": train_hi,
        "n_train_dates": int(ntr),
        "factor_cols": list(FACTOR_COLS),
        "weights": [float(x) for x in w],
        "delta": float(delta),
        "top_n": int(er.TOP_N),
        "ridge_a": float(er.RIDGE_A),
        "cost_bps": float(it.COST),
    }


def load_weights():
    with open(WEIGHTS_PATH, encoding="utf-8") as f:
        meta = json.load(f)
    if meta.get("factor_cols") != list(FACTOR_COLS):
        raise SystemExit(
            "[paper_ridge] frozen weights factor_cols differ from current FACTOR_COLS "
            "— the factor set changed. Re-freeze with --retrain (starts a new paper book)."
        )
    return meta


# ---------------------------------------------------------------------------
# Journal I/O
# ---------------------------------------------------------------------------

def read_journal():
    if not os.path.exists(JOURNAL_PATH):
        return []
    with open(JOURNAL_PATH, encoding="utf-8", newline="") as f:
        return [dict(r) for r in csv.DictReader(f)]


def write_journal(rows):
    with open(JOURNAL_PATH, "w", encoding="utf-8", newline="") as f:
        wtr = csv.DictWriter(f, fieldnames=JOURNAL_COLS)
        wtr.writeheader()
        for r in rows:
            wtr.writerow({k: r.get(k, "") for k in JOURNAL_COLS})


# ---------------------------------------------------------------------------
# Reporting
# ---------------------------------------------------------------------------

def _fmt(x):
    try:
        return f"{float(x):+.4f}"
    except (TypeError, ValueError):
        return "    ··"


def print_status(rows, meta):
    closed = [r for r in rows if r["status"] == "closed"]
    open_rows = [r for r in rows if r["status"] == "open"]
    print(f"\n=== 「去相关岭组合」纸面册 (forward paper-trade) ===")
    print(f"  权重冻结: train {meta['train_lo']}..{meta['train_hi']} "
          f"({meta['n_train_dates']} 周) · delta={meta['delta']:.2f} · "
          f"top{meta['top_n']} · cost={meta['cost_bps']:.0f}bp · 冻结于 {meta['created']}")
    print(f"  纸面起点: > {meta['train_hi']}  |  已结算 {len(closed)} 周 · 持仓中 {len(open_rows)} 周")

    if closed:
        print(f"\n  --- 已结算 (realised) ---")
        print(f"  {'date':>12}{'turn':>7}{'gross':>9}{'net':>9}{'nav':>9}")
        navs = running_nav(rows)
        nav_by_date = {n["date"]: n["nav"] for n in navs}
        for r in closed:
            print(f"  {r['date']:>12}{float(r['turnover']):>7.2f}"
                  f"{_fmt(r['gross_ret']):>9}{_fmt(r['net_ret']):>9}"
                  f"{nav_by_date[r['date']]:>9.4f}")
        cum = nav_by_date[closed[-1]['date']] - 1.0
        print(f"  累计净收益 (raw, top-3 等权篮): {cum:+.4f}  ({len(closed)} 周)")
        # csi300-relative cross-check via the vetted to_index_relative
        ex = cumulative_excess(rows)
        if ex is not None:
            print(f"  累计超额 vs csi300 (to_index_relative, 丢首周惯例): {ex:+.4f}")

    if open_rows:
        print(f"\n  --- 持仓中 (open, 待结算) ---")
        for r in open_rows:
            print(f"  {r['date']:>12}  picks: {r['picks'].replace(SEP, ', ')}  "
                  f"(turn={float(r['turnover']):.2f} vs 上周)")
    print()


def cumulative_excess(rows):
    """csi300-relative cumulative excess over closed rows, via iterate.to_index_relative.

    Returns None if fewer than 2 closed rows or the index series is unavailable.
    Note: to_index_relative uses nav[-1]/nav[0], i.e. it drops the first period
    (same convention as the validated gauntlets) — reported as a cross-check only.
    """
    closed = [r for r in rows if r["status"] == "closed"]
    if len(closed) < 2:
        return None
    try:
        idx_m, idx_dates = it.load_index("csi300")
    except (FileNotFoundError, OSError):
        return None
    nav = 1.0
    holdings = []
    for r in closed:
        nav *= (1.0 + float(r["net_ret"]))
        holdings.append({"t": r["date"], "nav": nav})
    report = {"holdings": holdings, "regime_slices": [], "max_drawdown": 0.0,
              "turnover": 0.0, "n_rebalances": len(holdings)}
    rel = it.to_index_relative(report, idx_m, idx_dates)
    return rel["excess_return"] if rel else None


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main():
    ap = argparse.ArgumentParser(description="「去相关岭组合」前向纸面册")
    ap.add_argument("--status", action="store_true", help="只读打印当前册，不推进/不写盘")
    ap.add_argument("--retrain", action="store_true", help="按今天最新已标注数据重冻权重（开新册）")
    args = ap.parse_args()

    panel = pd.read_csv(er.PANEL_MEMBERSHIP, dtype={"symbol": str})
    st_set = set(pd.read_csv(er.ST_PATH)["symbol"]) if os.path.exists(er.ST_PATH) else set()

    if args.status:
        if not os.path.exists(WEIGHTS_PATH):
            raise SystemExit("[paper_ridge] no frozen weights yet — run without --status first.")
        print_status(read_journal(), load_weights())
        return

    if args.retrain or not os.path.exists(WEIGHTS_PATH):
        action = "RETRAIN" if os.path.exists(WEIGHTS_PATH) else "INIT"
        meta = build_weights(panel, st_set)
        with open(WEIGHTS_PATH, "w", encoding="utf-8") as f:
            json.dump(meta, f, ensure_ascii=False, indent=2)
        nz = int(np.sum(np.abs(meta["weights"]) > 1e-9))
        print(f"[paper_ridge] {action} weights → {WEIGHTS_PATH}")
        print(f"  train {meta['train_lo']}..{meta['train_hi']} ({meta['n_train_dates']} 周), "
              f"{len(meta['weights'])} factors ({nz} non-zero), delta={meta['delta']:.2f}")
    else:
        meta = load_weights()

    w = np.array(meta["weights"], float)
    rows = advance_journal(panel, w, meta["delta"], meta["train_hi"], st_set, read_journal())
    write_journal(rows)
    print(f"[paper_ridge] journal → {JOURNAL_PATH}  ({len(rows)} 周记录)")
    print_status(rows, meta)


if __name__ == "__main__":
    main()
