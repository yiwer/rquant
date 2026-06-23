"""训练线性因子权重 w：截面排名归一 → Elastic-Net Rank-IC（锚定 train）→ weights.json。"""
import sys; sys.stdout.reconfigure(encoding="utf-8")
import os, json, numpy as np, pandas as pd
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import factor_lib as fl
from build_factor_matrix import FACTOR_COLS, OUT as PANEL, OUT_DIR

TRAIN_LO, TRAIN_HI = "2018-01-02", "2023-12-29"
OOS_LO, OOS_HI = "2024-01-02", "2026-06-12"
INNER_FIT_HI, INNER_VAL_LO = "2022-12-30", "2023-01-01"     # 内层切
ALPHAS = [0.001, 0.003, 0.01, 0.03, 0.1]
WEIGHTS = os.path.join(OUT_DIR, "weights.json")


def build_xy(panel, date_lo, date_hi):
    """窗内每日：因子截面排名 + fwd 截面排名，纵向堆叠。丢弃 fwd 为 NaN 的行。"""
    sub = panel[(panel["date"] >= date_lo) & (panel["date"] <= date_hi)]
    Xs, ys, ds = [], [], []
    for d, g in sub.groupby("date"):
        g = g.dropna(subset=["fwd_ret_5d"])
        if len(g) < 5:
            continue
        Xr = fl.rank_columns(g[FACTOR_COLS].to_numpy(float))
        yr = fl.cross_sectional_rank(g["fwd_ret_5d"].to_numpy(float))
        Xs.append(Xr); ys.append(yr); ds += [d] * len(g)
    return np.vstack(Xs), np.concatenate(ys), ds


def _val_rank_ic(panel, w, lo, hi):
    """验证窗内逐日 Rank-IC 均值（线性分 vs fwd）。"""
    sub = panel[(panel["date"] >= lo) & (panel["date"] <= hi)]
    ics = []
    for _, g in sub.groupby("date"):
        g = g.dropna(subset=["fwd_ret_5d"])
        if len(g) < 5:
            continue
        score = fl.linear_score(fl.rank_columns(g[FACTOR_COLS].to_numpy(float)), w)
        ics.append(fl.rank_ic(score, g["fwd_ret_5d"].to_numpy(float)))
    return float(np.nanmean(ics)) if ics else float("nan")


def select_alpha(panel):
    Xtr, ytr, _ = build_xy(panel, TRAIN_LO, INNER_FIT_HI)
    best, best_ic = ALPHAS[0], -np.inf
    for a in ALPHAS:
        w = fl.elastic_net_fit(Xtr, ytr, alpha=a, l1_ratio=0.5)
        ic = _val_rank_ic(panel, w, INNER_VAL_LO, TRAIN_HI)
        print(f"  alpha={a}: inner-val rank-IC={ic:+.4f}")
        if ic > best_ic:
            best, best_ic = a, ic
    return best


def _factor_ic(panel, lo, hi):
    """各单因子在窗内的平均 Rank-IC（诊断用）。"""
    sub = panel[(panel["date"] >= lo) & (panel["date"] <= hi)]
    acc = {f: [] for f in FACTOR_COLS}
    for _, g in sub.groupby("date"):
        g = g.dropna(subset=["fwd_ret_5d"])
        if len(g) < 5:
            continue
        for f in FACTOR_COLS:
            acc[f].append(fl.rank_ic(g[f].to_numpy(float), g["fwd_ret_5d"].to_numpy(float)))
    return {f: (float(np.nanmean(v)) if v else None) for f, v in acc.items()}


def main():
    panel = pd.read_csv(PANEL, dtype={"symbol": str})
    alpha = select_alpha(panel)
    Xtr, ytr, _ = build_xy(panel, TRAIN_LO, TRAIN_HI)
    w = fl.elastic_net_fit(Xtr, ytr, alpha=alpha, l1_ratio=0.5)
    out = {"weights": {f: float(wi) for f, wi in zip(FACTOR_COLS, w)},
           "alpha": alpha, "l1_ratio": 0.5,
           "factor_ic_train": _factor_ic(panel, TRAIN_LO, TRAIN_HI),
           "factor_ic_oos": _factor_ic(panel, OOS_LO, OOS_HI)}
    with open(WEIGHTS, "w", encoding="utf-8") as fp:
        json.dump(out, fp, ensure_ascii=False, indent=2)
    print(f"alpha={alpha}  weights:")
    for f, wi in sorted(out["weights"].items(), key=lambda kv: -abs(kv[1])):
        print(f"  {f:12} {wi:+.4f}")
    print(f"-> {WEIGHTS}")

if __name__ == "__main__":
    main()
