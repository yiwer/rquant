"""§5: 用【部署对齐目标】重训因子权重 —— 直接最大化 top-3 周频净超额（非样本内 Rank-IC）。

目标非凸（含 top-3 选择 + 成本 + 基准），Elastic-Net 失效 → 用免导全局优化器
Differential Evolution（种群化、非凸，ACO/CMA 同族；无 scipy 故手搓）。

诚实纪律（leak-free）：每折 w 只在该折 train 窗优化，OOS 仅评估。对比等权（零训练）。
预期（见对话 §4-④）：换对目标能从 EN 的 −0.195 止损回 ~0，但大概率仍不产正 alpha——
若 DE-w 稳超等权则是首个真裂缝（需查泄露）；若 ≈/< 等权则确证墙是信号天花板、非目标/训练。
"""
import sys; sys.stdout.reconfigure(encoding="utf-8")
import os, numpy as np, pandas as pd
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import factor_lib as fl
from build_factor_matrix import FACTOR_COLS, OUT as PANEL

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ST_PATH = os.path.join(REPO, "data", "baostock", "st_symbols.csv")
CSI = os.path.join(REPO, "data", "baostock", "index", "csi300.csv")
COST_BPS = 20.0
TOPN = 3
LOGAMT_FLOOR = float(np.log(5e7))
FOLDS = [("2018-01-02", "2021-12-31", "2022-01-01", "2022-12-31"),
         ("2018-01-02", "2022-12-31", "2023-01-01", "2023-12-31"),
         ("2018-01-02", "2023-12-31", "2024-01-01", "2024-12-31"),
         ("2018-01-02", "2024-12-31", "2025-01-01", "2026-12-31")]


def load():
    panel = pd.read_csv(PANEL, dtype={"symbol": str}).dropna(subset=["fwd_ret_5d"])
    FC = [c for c in FACTOR_COLS if c in panel.columns]            # 已有因子（面板里实际存在的）
    st = set(pd.read_csv(ST_PATH)["symbol"]) if os.path.exists(ST_PATH) else set()
    csi = pd.read_csv(CSI); csi["d"] = csi["time"].str[:10]
    csi = csi.drop_duplicates("d").sort_values("d")
    csi["bfwd"] = csi["close"].shift(-5) / csi["close"] - 1        # 基准同口径 5 交易日前向
    bench = dict(zip(csi["d"], csi["bfwd"]))
    return panel, FC, st, bench


def prep(panel, FC, st, bench, lo, hi):
    """窗内逐周：合规截面 → 因子截面排名堆叠成 bigX + 每周边界/fwd/symbols/基准。"""
    sub = panel[(panel["date"] >= lo) & (panel["date"] <= hi)]
    Xs, bounds, fwd, syms, bl, n = [], [], [], [], [], 0
    for d, g in sub.groupby("date"):
        elig = ((~g["symbol"].isin(st)) & (g["f_roe"] > 0) & (g["f_bm"] > 0)
                & (g["f_logamt"] >= LOGAMT_FLOOR))
        g = g[elig]
        if len(g) < TOPN + 2:
            continue
        Xs.append(fl.rank_columns(g[FC].to_numpy(float)))
        bounds.append((n, n + len(g))); n += len(g)
        fwd.append(g["fwd_ret_5d"].to_numpy(float)); syms.append(g["symbol"].to_numpy())
        b = bench.get(d, np.nan); bl.append(0.0 if pd.isna(b) else float(b))
    return {"X": np.vstack(Xs), "bounds": bounds, "fwd": fwd, "syms": syms, "b": bl}


def net_excess(W, w):
    """该权重在窗内的【平均每周 top-3 净超额】（扣 20bp 单边换手成本，减基准）。"""
    s = W["X"] @ w
    out, prev = [], set()
    for k, (a, b) in enumerate(W["bounds"]):
        ss = s[a:b]
        top = np.argpartition(-ss, TOPN)[:TOPN]
        port = float(W["fwd"][k][top].mean())
        names = set(W["syms"][k][top])
        turn = len(names - prev) / TOPN if prev else 1.0
        out.append(port - W["b"][k] - COST_BPS / 1e4 * turn)
        prev = names
    return float(np.mean(out)) if out else -9.9


def de_opt(obj, dim, pop=48, gens=60, F=0.6, CR=0.9, seed=0):
    """Differential Evolution，最大化 obj。返回 (best_w, best_fit)。"""
    rng = np.random.default_rng(seed)
    P = rng.normal(0, 1, (pop, dim))
    fit = np.array([obj(P[i]) for i in range(pop)])
    for _ in range(gens):
        for i in range(pop):
            idx = rng.choice(pop, 3, replace=False)
            while i in idx:
                idx = rng.choice(pop, 3, replace=False)
            a, b, c = P[idx]
            mut = a + F * (b - c)
            cr = rng.random(dim) < CR
            cr[rng.integers(dim)] = True
            trial = np.where(cr, mut, P[i])
            ft = obj(trial)
            if ft > fit[i]:
                P[i], fit[i] = trial, ft
    j = int(fit.argmax())
    return P[j], float(fit[j])


def main():
    panel, FC, st, bench = load()
    dim = len(FC); eqw = np.ones(dim)
    print(f"factors(existing)={dim}  ST={len(st)}  COST={COST_BPS}bp  topN={TOPN}")
    de_oos, eq_oos = [], []
    for tlo, thi, olo, ohi in FOLDS:
        tw = prep(panel, FC, st, bench, tlo, thi)
        ow = prep(panel, FC, st, bench, olo, ohi)
        w, tf = de_opt(lambda v: net_excess(tw, v), dim, seed=0)
        de, eq = net_excess(ow, w), net_excess(ow, eqw)
        eqtr = net_excess(tw, eqw)
        de_oos.append(de); eq_oos.append(eq)
        print(f"OOS {olo[:4]}: train DE={tf:+.5f} (eq {eqtr:+.5f}) | "
              f"OOS DE={de:+.5f}  eq={eq:+.5f}  Δ={de - eq:+.5f}")
    de_oos, eq_oos = np.array(de_oos), np.array(eq_oos)
    print("\n=== aggregate (mean weekly net excess, bp) ===")
    print(f"DE-trained (deploy obj): mean={de_oos.mean()*1e4:+.1f}bp  pos={int((de_oos>0).sum())}/4  min={de_oos.min()*1e4:+.1f}bp")
    print(f"Equal-weight (no train): mean={eq_oos.mean()*1e4:+.1f}bp  pos={int((eq_oos>0).sum())}/4  min={eq_oos.min()*1e4:+.1f}bp")
    print(f"DE beats equal-weight: {int((de_oos>eq_oos).sum())}/4 folds  (mean Δ {((de_oos-eq_oos).mean())*1e4:+.1f}bp)")


if __name__ == "__main__":
    main()
