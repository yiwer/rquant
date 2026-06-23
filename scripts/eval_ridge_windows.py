"""Pre-live gauntlet ①: ridge-on-gauss across MORE OOS window families.

Adds earlier OOS folds (2020 COVID crash/recovery, 2021 rotation) to the
existing 2022-2026 WFO, to test whether the +0.222 edge is regime-robust or
2022-2026-specific. Reuses eval_ridge's VETTED primitives (fit_ridge,
select_delta_ridge, backtest_ridge, equal-weight backtest_rank_linear) +
iterate's index-relative excess. Membership pool only (honest, no survivorship).
"""
import sys; sys.stdout.reconfigure(encoding="utf-8")
import os, numpy as np, pandas as pd
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import iterate as it
import eval_ridge as er
from build_factor_matrix import FACTOR_COLS

# Extended anchored-train folds: 2 NEW earlier + the existing 4
FOLDS = [
    ("2018-01-02", "2019-12-31", "2020-01-02", "2020-12-31"),   # NEW: OOS 2020
    ("2018-01-02", "2020-12-31", "2021-01-02", "2021-12-31"),   # NEW: OOS 2021
] + list(er.tn.WFO_FOLDS)                                         # 2022/2023/2024/2025-26


def main():
    st_set = set(pd.read_csv(er.ST_PATH)["symbol"]) if os.path.exists(er.ST_PATH) else set()
    idx_m, idx_dates = it.load_index("csi300")
    panel = pd.read_csv(er.PANEL_MEMBERSHIP, dtype={"symbol": str})
    p = len(FACTOR_COLS)
    w_eq = np.zeros(p); w_eq[0] = 1.0; w_eq[1] = 1.0   # f_bm + f_npyoy (eval_nonlinear eq)

    print(f"ridge-on-gauss across {len(FOLDS)} OOS folds (membership, cost {it.COST}bp)")
    print(f"{'OOS':>10}{'ridge':>10}{'equal':>10}{'delta':>7}{'n_tr':>6}")
    rg_all, eq_all, rg_new = [], [], []
    for tl, th, ol, oh in FOLDS:
        oos = panel[(panel["date"] >= ol) & (panel["date"] <= oh)]
        w, ntr = er.fit_ridge(panel, tl, th)
        d = er.select_delta_ridge(panel, (tl, th, ol, oh), w, st_set)
        rep = er.backtest_ridge(oos, w, top_n=er.TOP_N, cost_bps=it.COST, st_set=st_set, delta=d)
        rel = it.to_index_relative(rep, idx_m, idx_dates)
        rg = rel["excess_return"] if rel else None
        repe = er.backtest_rank_linear(oos, w_eq, top_n=er.TOP_N, cost_bps=it.COST, st_set=st_set, delta=0.0)
        rele = it.to_index_relative(repe, idx_m, idx_dates)
        eq = rele["excess_return"] if rele else None
        yr = ol[:4]
        print(f"{yr:>10}{rg:>+10.4f}{eq:>+10.4f}{d:>7.2f}{ntr:>6}" if rg is not None else f"{yr:>10}  None")
        if rg is not None:
            rg_all.append(rg); eq_all.append(eq)
            if yr in ("2020", "2021"):
                rg_new.append(rg)

    rg_all, eq_all = np.array(rg_all), np.array(eq_all)
    print(f"\n=== 6-fold aggregate (membership) ===")
    print(f"  ridge:  mean={rg_all.mean():+.4f}  pos={int((rg_all>0).sum())}/{len(rg_all)}  min={rg_all.min():+.4f}")
    print(f"  equal:  mean={eq_all.mean():+.4f}  pos={int((eq_all>0).sum())}/{len(eq_all)}")
    print(f"  ridge beats equal: {int((rg_all>eq_all).sum())}/{len(rg_all)} folds")
    print(f"  NEW folds (2020,2021) ridge: {[f'{v:+.4f}' for v in rg_new]}  → {'both positive' if all(v>0 for v in rg_new) else 'NOT all positive'}")
    print(f"\n  §5.3-ish: ridge mean>0 AND pos>{len(rg_all)}/2 AND beats equal = "
          f"{rg_all.mean()>0 and (rg_all>0).sum()>len(rg_all)/2 and rg_all.mean()>eq_all.mean()}")


if __name__ == "__main__":
    main()
