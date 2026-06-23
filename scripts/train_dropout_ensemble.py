"""因子 Dropout 集成训练（用户提案 = 特征装袋 / random-subspace bagging）。

每轮随机丢弃部分因子(权重=0)、在存活子集上 ridge 拟合(存活者吸收被丢者)、多轮平均
消除随机性 → 降方差、防因子共适应。+ gauss 归一化(中心化,使加权合适) + 权重截断(防突出)。
4 折 WFO(每折 train 拟合,OOS 评)直接验——不用单 split(已被骗 5 次)。
对照:单 ridge(无 dropout) + 等权(天花板)。口径:top-3 & top-2 周频净(扣20bp)。

诚实先验:dropout 平均把权重收缩/推向均匀(≈等权天花板);大概率打平等权、难超(墙=信号非平稳
非权重方差);真有价值的可能=它是首个 4 折稳定不崩的训练模型。
"""
import sys; sys.stdout.reconfigure(encoding="utf-8")
import os, numpy as np, pandas as pd
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import factor_lib as fl
from build_factor_matrix import FACTOR_COLS, OUT as PANEL
from test_norm_hysteresis import norm_gauss

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ST_PATH = os.path.join(REPO, "data", "baostock", "st_symbols.csv")
CSI = os.path.join(REPO, "data", "baostock", "index", "csi300.csv")
COST_BPS = 20.0
DROP_P = 0.30          # 每轮丢弃比例
ROUNDS = 60            # 装袋轮次(消除随机)
RIDGE_A = 0.10         # ridge λ = RIDGE_A × mean(diag Gram)
FOLDS = [("2021-12-31", "2022"), ("2022-12-31", "2023"),
         ("2023-12-31", "2024"), ("2024-12-31", "2025")]


def load_weeks():
    df = pd.read_csv(PANEL, dtype={"symbol": str}).dropna(subset=["fwd_ret_5d"])
    FC = [c for c in FACTOR_COLS if c in df.columns]
    st = set(pd.read_csv(ST_PATH)["symbol"]) if os.path.exists(ST_PATH) else set()
    csi = pd.read_csv(CSI); csi["d"] = csi["time"].str[:10]
    csi = csi.drop_duplicates("d").sort_values("d"); csi["bf"] = csi["close"].shift(-5)/csi["close"]-1
    bench = dict(zip(csi["d"], csi["bf"]))
    WK = []
    for d, g in df.groupby("date"):
        e = g[(~g["symbol"].isin(st)) & (g["f_roe"] > 0) & (g["f_bm"] > 0) & (g["f_logamt"] >= np.log(5e7))]
        if len(e) < 12:
            continue
        G = norm_gauss(e[FC].to_numpy(float))                       # 中心化归一(评分用)
        y = fl.cross_sectional_rank(e["fwd_ret_5d"].to_numpy(float)) - 0.5  # 居中目标
        b = bench.get(d, np.nan)
        WK.append((d, G, y, e["symbol"].to_numpy(), e["fwd_ret_5d"].to_numpy(float),
                   0.0 if pd.isna(b) else float(b)))
    return WK, FC


def train_gram(weeks):
    """累积 train 期 Gram=ΣGᵀG 与 b=ΣGᵀy（一次扫描，之后子集拟合极快）。"""
    p = weeks[0][1].shape[1]
    Gram = np.zeros((p, p)); Xty = np.zeros(p)
    for _, G, y, *_ in weeks:
        Gram += G.T @ G; Xty += G.T @ y
    return Gram, Xty


def ridge_subset(Gram, Xty, cols, lam):
    k = len(cols)
    A = Gram[np.ix_(cols, cols)] + lam * np.eye(k)
    wk = np.linalg.solve(A, Xty[cols])
    w = np.zeros(Gram.shape[0]); w[cols] = wk
    return w


def fit_dropout_bag(Gram, Xty, p_drop, rounds, lam, seed=0):
    rng = np.random.default_rng(seed)
    P = Gram.shape[0]; acc = np.zeros(P)
    for _ in range(rounds):
        keep = np.where(rng.random(P) >= p_drop)[0]
        if len(keep) < 3:
            continue
        acc += ridge_subset(Gram, Xty, keep, lam)
    w = acc / rounds
    q = np.percentile(np.abs(w), 90) + 1e-12          # 截断:防单因子突出
    return np.clip(w, -q, q)


def backtest(weeks, w, N):
    net, held = [], set()
    for d, G, y, sy, fwd, b in weeks:
        sc = G @ w
        top = np.argpartition(-sc, N)[:N]
        names = set(sy[top]); turn = len(names ^ held) / N
        net.append(float(fwd[top].mean()) - b - COST_BPS/1e4*turn); held = names
    return np.array(net)


def cum(n): return float(np.prod(1+n)-1)


def main():
    WK, FC = load_weeks()
    print(f"因子={len(FC)} 周数={len(WK)} dropout_p={DROP_P} rounds={ROUNDS} ridgeA={RIDGE_A}")
    for N in (3, 2):
        print(f"\n===== top-{N} 4折WFO (净扣{int(COST_BPS)}bp) =====")
        print(f"{'OOS':>6}{'dropout集成':>12}{'单ridge':>10}{'等权':>9}")
        agg = {"drop": [], "single": [], "eq": []}
        for thi, oy in FOLDS:
            tr = [w for w in WK if w[0] <= thi]
            oo = [w for w in WK if (w[0][:4] == oy if oy != "2025" else w[0] >= "2025-01-01")]
            Gram, Xty = train_gram(tr)
            lam = RIDGE_A * np.mean(np.diag(Gram))
            w_drop = fit_dropout_bag(Gram, Xty, DROP_P, ROUNDS, lam)
            w_single = np.clip(ridge_subset(Gram, Xty, list(range(len(FC))), lam),
                               *(lambda q: (-q, q))(np.percentile(np.abs(ridge_subset(Gram, Xty, list(range(len(FC))), lam)), 90)))
            w_eq = np.sign(Xty); w_eq[w_eq == 0] = 1.0
            cd, cs, ce = cum(backtest(oo, w_drop, N)), cum(backtest(oo, w_single, N)), cum(backtest(oo, w_eq, N))
            agg["drop"].append(cd); agg["single"].append(cs); agg["eq"].append(ce)
            print(f"{oy:>6}{cd:>+11.1%}{cs:>+10.1%}{ce:>+9.1%}")
        for k, lab in [("drop", "dropout集成"), ("single", "单ridge"), ("eq", "等权")]:
            a = np.array(agg[k])
            print(f"  {lab:10} 均值={a.mean():+.1%}  正折={int((a>0).sum())}/4  最差={a.min():+.1%}")


if __name__ == "__main__":
    main()
