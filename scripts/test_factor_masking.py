"""因子遮蔽 × 归一化 —— 验用户假设：噪声/偏向因子拖累合成分，遮蔽+合适归一化能否救场。

关键：rank-IC 对单调变换不变 → 归一化只在【合成层】起作用。故在 composite(top-N) 层测：
  遮蔽：硬(丢|train-IC|<θ) / 软(IC加权,弱因子自动≈0权重="确保权重下表征合适")
  归一化：masked 集上 rank vs gauss vs winz
训练期(2018-2021)定 符号/|IC|/遮蔽集/权重，OOS(2022-2026)评(无前视)。
口径：top-2 & top-3 周频净(扣20bp)，对比 净累计/Sharpe/命中率/逐年。
"""
import sys; sys.stdout.reconfigure(encoding="utf-8")
import os, numpy as np, pandas as pd
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import factor_lib as fl
from build_factor_matrix import FACTOR_COLS, OUT as PANEL
from test_norm_hysteresis import norm_rank, norm_gauss, norm_winz

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ST_PATH = os.path.join(REPO, "data", "baostock", "st_symbols.csv")
CSI = os.path.join(REPO, "data", "baostock", "index", "csi300.csv")
COST_BPS = 20.0
SPLIT = "2022-01-01"            # train < SPLIT ; OOS >= SPLIT
MASK_THETA = 0.02              # |train rank-IC| 阈值（硬遮蔽）
NORMS = {"rank": norm_rank, "gauss": norm_gauss, "winz": norm_winz}


def load():
    df = pd.read_csv(PANEL, dtype={"symbol": str}).dropna(subset=["fwd_ret_5d"])
    FC = [c for c in FACTOR_COLS if c in df.columns]
    st = set(pd.read_csv(ST_PATH)["symbol"]) if os.path.exists(ST_PATH) else set()
    csi = pd.read_csv(CSI); csi["d"] = csi["time"].str[:10]
    csi = csi.drop_duplicates("d").sort_values("d"); csi["bf"] = csi["close"].shift(-5)/csi["close"]-1
    bench = dict(zip(csi["d"], csi["bf"]))
    return df, FC, st, bench


def train_factor_ic(df, FC):
    """训练期(<SPLIT)逐因子平均 rank-IC（norm 不变）→ 符号 + |IC| 用于遮蔽/加权。"""
    tr = df[df["date"] < SPLIT]
    ic = {}
    for c in FC:
        v = [fl.rank_ic(g[c].to_numpy(float), g["fwd_ret_5d"].to_numpy(float))
             for _, g in tr.groupby("date") if g[c].notna().sum() >= 20]
        ic[c] = float(np.nanmean(v)) if v else 0.0
    return ic


def weeks_oos(df, FC, st, bench):
    """OOS(>=SPLIT) 逐周：预计算 3 种归一化矩阵 + fwd/syms/bench/date。"""
    sub = df[df["date"] >= SPLIT]
    out = []
    for d, g in sub.groupby("date"):
        e = g[(~g["symbol"].isin(st)) & (g["f_roe"] > 0) & (g["f_bm"] > 0) & (g["f_logamt"] >= np.log(5e7))]
        if len(e) < 12:
            continue
        M = e[FC].to_numpy(float)
        normed = {k: fn(M) for k, fn in NORMS.items()}
        b = bench.get(d, np.nan)
        out.append((d, normed, e["symbol"].to_numpy(), e["fwd_ret_5d"].to_numpy(float), 0.0 if pd.isna(b) else float(b)))
    return out


def backtest(weeks, w, norm_key, N):
    """给权重向量 w（已含符号/遮蔽）+ 归一化 + top-N，算净序列。"""
    net, held = [], set()
    for d, normed, sy, fwd, b in weeks:
        sc = normed[norm_key] @ w
        top = np.argpartition(-sc, N)[:N]
        names = set(sy[top]); turn = len(names ^ held) / N
        net.append(float(fwd[top].mean()) - b - COST_BPS/1e4*turn); held = names
    return np.array(net)


def stats(net):
    cum = float(np.prod(1+net)-1); sh = float(net.mean()/(net.std()+1e-12)*np.sqrt(50))
    hit = float((net > 0).mean())
    eq = np.cumprod(1+net); dd = float(((eq-np.maximum.accumulate(eq))/np.maximum.accumulate(eq)).min())
    return cum, sh, hit, dd


def main():
    df, FC, st, bench = load()
    ic = train_factor_ic(df, FC)
    icv = np.array([ic[c] for c in FC])
    sgn = np.sign(icv); sgn[sgn == 0] = 1.0
    keep = np.abs(icv) >= MASK_THETA
    print(f"因子总数={len(FC)}  遮蔽后保留(|train-IC|>={MASK_THETA})={int(keep.sum())}  丢弃={int((~keep).sum())}")
    print(f"  丢弃的: {[FC[i] for i in range(len(FC)) if not keep[i]]}")

    weeks = weeks_oos(df, FC, st, bench)
    print(f"OOS 周数={len(weeks)} ({weeks[0][0]}..{weeks[-1][0]})\n")

    # 权重方案（均已 sign-aligned）
    w_all_eq   = sgn.copy()                                  # 全因子等权
    w_mask_eq  = sgn * keep                                  # 硬遮蔽等权
    w_all_icw  = icv.copy()                                  # 全因子 IC 加权（软遮蔽）
    w_mask_icw = icv * keep                                  # 遮蔽 + IC 加权

    specs = [
        ("全72·等权·rank (基线)", w_all_eq, "rank"),
        ("硬遮蔽·等权·rank",      w_mask_eq, "rank"),
        ("全72·IC加权·rank(软遮蔽)", w_all_icw, "rank"),
        ("遮蔽·IC加权·rank",      w_mask_icw, "rank"),
        ("遮蔽·IC加权·gauss",     w_mask_icw, "gauss"),
        ("遮蔽·IC加权·winz",      w_mask_icw, "winz"),
        ("遮蔽·等权·gauss",       w_mask_eq, "gauss"),
    ]
    for N in (3, 2):
        print(f"===== top-{N} (OOS 2022-2026, 净扣{int(COST_BPS)}bp) =====")
        print(f"{'composite':30}{'净累计':>9}{'Sharpe':>8}{'命中率':>8}{'maxDD':>8}")
        for name, w, nk in specs:
            c, s, h, dd = stats(backtest(weeks, w, nk, N))
            print(f"{name:30}{c:>+8.0%}{s:>+8.2f}{h:>+8.0%}{dd:>+8.0%}")
        print()


if __name__ == "__main__":
    main()
