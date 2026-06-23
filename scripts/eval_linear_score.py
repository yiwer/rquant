# scripts/eval_linear_score.py
"""Python 周频回测器：线性分→硬闸→top-N→§5.3 裁决；学习-w vs 等权对照 + Rust 对账。"""
import sys, os; sys.stdout.reconfigure(encoding="utf-8")
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import json, numpy as np, pandas as pd
import factor_lib as fl
from build_factor_matrix import FACTOR_COLS, OUT as PANEL, OUT_DIR
import iterate as it

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ST = os.path.join(REPO, "data", "baostock", "st_symbols.csv")
TRAIN = ("train", "2018-01-02", "2023-12-29")
OOS = ("2024-26_OOS", "2024-01-02", "2026-06-12")
LIQ_FLOOR_LOG = float(np.log(5e7))           # 流动性地板 = log(5000万)


def _eligible(g, st_set):
    """硬闸：非 ST ∧ roe>0 ∧ bps>0(f_bm>0) ∧ 流动性≥地板。返回过闸子集。"""
    ok = (~g["symbol"].isin(st_set)) & (g["f_roe"] > 0) & (g["f_bm"] > 0) & (g["f_logamt"] >= LIQ_FLOOR_LOG)
    return g[ok]


def backtest(panel, w, top_n, cost_bps, st_set):
    panel = panel.sort_values(["date", "symbol"])
    nav, prev, navs = 1.0, set(), []
    dates = sorted(panel["date"].unique())
    total_turn = 0.0  # accumulated turnover across all rebalances
    for d in dates:
        g = _eligible(panel[panel["date"] == d].dropna(subset=["fwd_ret_5d"]), st_set)
        if len(g) < top_n:
            continue
        score = fl.linear_score(fl.rank_columns(g[FACTOR_COLS].to_numpy(float)), w)
        gi = g.assign(_score=score).sort_values(["_score", "symbol"], ascending=[False, True])
        pick = gi.head(top_n)
        ret = float(pick["fwd_ret_5d"].mean())
        cur = set(pick["symbol"])
        turn = len(cur ^ prev) / max(len(cur) + len(prev), 1)    # 对称差比（双边）
        total_turn += turn  # accumulate per-rebalance turnover
        nav *= (1.0 + ret - cost_bps / 1e4 * turn)
        navs.append({"t": d, "nav": nav, "picks": list(cur)})
        prev = cur
    total = navs[-1]["nav"] - 1.0 if navs else 0.0
    peak = -1e9; mdd = 0.0
    for h in navs:
        peak = max(peak, h["nav"]); mdd = max(mdd, 1 - h["nav"] / peak)
    rets = np.diff([1.0] + [h["nav"] for h in navs])
    sharpe = float(np.mean(rets) / np.std(rets) * np.sqrt(48)) if len(rets) > 1 and np.std(rets) > 0 else None
    return {"holdings": navs, "regime_slices": [{"label": L, "from": a, "to": b} for L, a, b in [TRAIN, OOS]],
            "risk": {"sharpe": sharpe}, "total_return": total, "max_drawdown": mdd,
            "turnover": total_turn, "n_rebalances": len(navs), "excess_return": 0.0}


def eval_weights(panel, w, label, st_set):
    idx = it.load_index("csi300")
    g = backtest(panel, w, top_n=3, cost_bps=0.0, st_set=st_set)
    n = backtest(panel, w, top_n=3, cost_bps=it.COST, st_set=st_set)
    gi = it.to_index_relative(g, *idx); ni = it.to_index_relative(n, *idx)
    verdict, flags, m = it.judge(gi, ni, sweep=None)
    print(f"[{label}] verdict={verdict} net_ex={m['net_ex']:+.3f} OOS={m['net_oos_ex']} "
          f"sharpe={m['net_sharpe']} be={m['break_even']} flags={flags}")
    return {"label": label, "verdict": verdict, "metrics": m, "flags": flags}


def main():
    panel = pd.read_csv(PANEL, dtype={"symbol": str})
    st_set = set(pd.read_csv(ST)["symbol"]) if os.path.exists(ST) else set()
    learned = json.load(open(os.path.join(OUT_DIR, "weights.json"), encoding="utf-8"))["weights"]
    w_learned = np.array([learned[f] for f in FACTOR_COLS])
    w_equal = np.zeros(len(FACTOR_COLS)); w_equal[0] = 1.0; w_equal[1] = 1.0   # 等权基线=价值+净利
    r_eq = eval_weights(panel, w_equal, "equal(value+npyoy)", st_set)
    r_ln = eval_weights(panel, w_learned, "learned", st_set)
    print("\n=== 裁决 ===")
    print(f"等权 net-OOS={r_eq['metrics']['net_oos_ex']}  学习 net-OOS={r_ln['metrics']['net_oos_ex']}")
    win = (r_ln["verdict"] == "PASS" and r_ln["metrics"]["net_oos_ex"] is not None
           and r_eq["metrics"]["net_oos_ex"] is not None
           and r_ln["metrics"]["net_oos_ex"] > r_eq["metrics"]["net_oos_ex"])
    print("学习权重", "✅ 超过等权且过闸" if win else "❌ 未超过等权 / 未过闸")

if __name__ == "__main__":
    main()
