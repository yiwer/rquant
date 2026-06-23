"""归一化方式 × 迟滞缓冲带 —— 直击"周频换手成本是净收益杀手"。

用户两点：
  ① 周频=检查时机非强制交易 → 持仓未跌出缓冲带就不动（迟滞）→ 降换手→降成本
  ② 重构归一化（避免 rank-uniform 把分数压成一团=过早收敛/统计误差）
本实验：等权 top-3（选股固定、无优化器），扫 归一化 × 缓冲带 K，扣真实成本，对比基线。
  归一化：rank-uniform(原) / rank-gauss(Φ⁻¹秩，恢复尾部离散) / winsor-z(±3σ截尾后标准化)
  迟滞：持有名次在 top-K 内则保留，跌出才卖，空位由 top-3 补。K=3=每周全调（基线）。
绝对净口径（累计/Sharpe/maxDD/周均换手），对照 csi300。
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
LOGF = float(np.log(5e7))


def _ppf(p):
    """Acklam 逆正态 CDF（向量化，无 scipy）。"""
    a = [-3.969683028665376e1, 2.209460984245205e2, -2.759285104469687e2, 1.383577518672690e2, -3.066479806614716e1, 2.506628277459239e0]
    b = [-5.447609879822406e1, 1.615858368580409e2, -1.556989798598866e2, 6.680131188771972e1, -1.328068155288572e1]
    c = [-7.784894002430293e-3, -3.223964580411365e-1, -2.400758277161838e0, -2.549732539343734e0, 4.374664141464968e0, 2.938163982698783e0]
    d = [7.784695709041462e-3, 3.224671290700398e-1, 2.445134137142996e0, 3.754408661907416e0]
    p = np.clip(np.asarray(p, float), 1e-9, 1 - 1e-9)
    x = np.zeros_like(p); lo, hi = 0.02425, 1 - 0.02425
    m = p < lo; q = np.sqrt(-2 * np.log(p[m]))
    x[m] = (((((c[0]*q+c[1])*q+c[2])*q+c[3])*q+c[4])*q+c[5]) / ((((d[0]*q+d[1])*q+d[2])*q+d[3])*q+1)
    m = p > hi; q = np.sqrt(-2 * np.log(1 - p[m]))
    x[m] = -(((((c[0]*q+c[1])*q+c[2])*q+c[3])*q+c[4])*q+c[5]) / ((((d[0]*q+d[1])*q+d[2])*q+d[3])*q+1)
    m = (p >= lo) & (p <= hi); q = p[m]-0.5; r = q*q
    x[m] = (((((a[0]*r+a[1])*r+a[2])*r+a[3])*r+a[4])*r+a[5])*q / (((((b[0]*r+b[1])*r+b[2])*r+b[3])*r+b[4])*r+1)
    return x


def norm_rank(M):
    return fl.rank_columns(M)


def norm_gauss(M):
    return np.column_stack([_ppf(fl.cross_sectional_rank(M[:, j])) for j in range(M.shape[1])])


def norm_winz(M):
    out = np.empty_like(M, float)
    for j in range(M.shape[1]):
        v = M[:, j].astype(float); mu = np.nanmean(v); sd = np.nanstd(v)
        if not np.isfinite(sd) or sd == 0:
            out[:, j] = 0.0; continue
        v = np.where(np.isnan(v), mu, v)
        v = np.clip(v, mu - 3*sd, mu + 3*sd)
        out[:, j] = (v - v.mean()) / (v.std() + 1e-12)
    return out


def load_weeks():
    df = pd.read_csv(PANEL, dtype={"symbol": str}).dropna(subset=["fwd_ret_5d"])
    FC = [c for c in FACTOR_COLS if c in df.columns]
    st = set(pd.read_csv(ST_PATH)["symbol"]) if os.path.exists(ST_PATH) else set()
    csi = pd.read_csv(CSI); csi["d"] = csi["time"].str[:10]
    csi = csi.drop_duplicates("d").sort_values("d"); csi["bf"] = csi["close"].shift(-5)/csi["close"]-1
    bench = dict(zip(csi["d"], csi["bf"]))
    weeks = []
    for d, g in df.groupby("date"):
        e = g[(~g["symbol"].isin(st)) & (g["f_roe"] > 0) & (g["f_bm"] > 0) & (g["f_logamt"] >= LOGF)]
        if len(e) < 12:
            continue
        b = bench.get(d, np.nan)
        weeks.append({"M": e[FC].to_numpy(float), "fwd": e["fwd_ret_5d"].to_numpy(float),
                      "syms": e["symbol"].to_numpy(), "b": 0.0 if pd.isna(b) else float(b)})
    return weeks, FC


def backtest(weeks, norm_fn, K):
    """迟滞缓冲带 K：持有名次<K 则留，跌出才卖，空位 top-3 补。返回 (net数组, 周均换手)。"""
    net, turns, held = [], [], set()
    for wk in weeks:
        sc = norm_fn(wk["M"]).mean(1)              # 等权合成分
        order = np.argsort(-sc)
        syms = wk["syms"]
        rank_of = {syms[idx]: r for r, idx in enumerate(order)}
        keep = {s for s in held if rank_of.get(s, 10**9) < K}     # 仍在缓冲带内 → 留
        new = list(keep)
        for idx in order:                           # 空位用最高分未持有者补
            if len(new) >= TOPN:
                break
            s = syms[idx]
            if s not in keep:
                new.append(s)
        new = set(new[:TOPN])
        turn = len(new ^ held) / TOPN
        idxs = [np.where(syms == s)[0][0] for s in new]
        port = float(wk["fwd"][idxs].mean()) if idxs else wk["b"]
        net.append(port - wk["b"] - COST_BPS/1e4*turn)
        turns.append(turn); held = new
    return np.array(net), float(np.mean(turns))


def stats(r):
    r = np.asarray(r); cum = float(np.prod(1+r)-1); sh = float(r.mean()/(r.std()+1e-12)*np.sqrt(50))
    eq = np.cumprod(1+r); dd = float(((eq-np.maximum.accumulate(eq))/np.maximum.accumulate(eq)).min())
    return cum, sh, dd


def main():
    weeks, FC = load_weeks()
    mkt = np.array([w["b"] for w in weeks])
    mc, msh, mdd = stats(mkt)
    print(f"周数={len(weeks)}  因子={len(FC)}  成本={COST_BPS}bp  (净=超额, 已减csi300)")
    print(f"{'csi300基准累计(绝对)':30} cum(abs)={np.prod(1+mkt)-1:+.1%}")
    print(f"\n{'归一化×缓冲K':22}{'净超额累计':>11}{'Sharpe':>9}{'maxDD':>9}{'周均换手':>9}")
    for name, fn in [("rank", norm_rank), ("gauss", norm_gauss), ("winz", norm_winz)]:
        for K in (3, 6, 10, 15):
            net, tn = backtest(weeks, fn, K)
            c, s, dd = stats(net)
            tag = " ←基线" if (name == "rank" and K == 3) else ""
            print(f"{name+'  K='+str(K):22}{c:>+10.1%}{s:>+9.2f}{dd:>+9.1%}{tn:>8.0%}{tag}")
        print()


if __name__ == "__main__":
    main()
