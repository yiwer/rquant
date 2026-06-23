"""市场择时叠加（用户 ①大盘行情 的有效形态）——严格诚实口径。

横截面选股已证弱且贴成本上限；本实验测【另一根杆】：用大盘 regime 决定满仓 top-3 vs 空仓。
它绕开横截面墙（降 regime 暴露），非打破它。市场择时是量化里最易自欺的（独立转折最少），
故按防自欺设计：
  - 选股固定=等权 top-3（不调，把择时当唯一变量）
  - 规则【预先锁定】标准 200 日均线（零事后自由度）；另列多 lookback 敏感性带（非挑最优）
  - 扣成本：持仓期周换手 + 进出市场整体换手（择时为翻动付费）
  - 去 2022 分解：edge 是否只靠规避一次崩盘
  - 报 regime 转折次数 = 真有效样本（统计功效约束）
绝对口径（累计/Sharpe/maxDD），对照"始终满仓"与"csi300 buy&hold"。
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
START = "2019-10-01"          # 200MA 预热后


def weekly_top3():
    """等权 top-3 逐周：(date, 持仓名集, 组合绝对收益, 市场收益)。"""
    df = pd.read_csv(PANEL, dtype={"symbol": str}).dropna(subset=["fwd_ret_5d"])
    FC = [c for c in FACTOR_COLS if c in df.columns]
    st = set(pd.read_csv(ST_PATH)["symbol"]) if os.path.exists(ST_PATH) else set()
    csi = pd.read_csv(CSI); csi["d"] = csi["time"].str[:10]
    csi = csi.drop_duplicates("d").sort_values("d")
    csi["bf"] = csi["close"].shift(-5) / csi["close"] - 1
    bench = dict(zip(csi["d"], csi["bf"]))
    rows = []
    for d, g in df.groupby("date"):
        if d < START:
            continue
        e = g[(~g["symbol"].isin(st)) & (g["f_roe"] > 0) & (g["f_bm"] > 0) & (g["f_logamt"] >= LOGF)]
        if len(e) < TOPN + 2:
            continue
        sc = fl.rank_columns(e[FC].to_numpy(float)).mean(1)
        top = np.argpartition(-sc, TOPN)[:TOPN]
        names = frozenset(e["symbol"].to_numpy()[top])
        b = bench.get(d, np.nan)
        rows.append((d, names, float(e["fwd_ret_5d"].to_numpy()[top].mean()), 0.0 if pd.isna(b) else float(b)))
    return pd.DataFrame(rows, columns=["d", "names", "port", "mkt"])


def regime_signals():
    """各 ≤t 大盘 regime 布尔序列（date→risk_on）。"""
    csi = pd.read_csv(CSI); csi["d"] = csi["time"].str[:10]
    csi = csi.drop_duplicates("d").sort_values("d").reset_index(drop=True)
    sig = {}
    for n in (120, 150, 200, 250):
        ma = csi["close"].rolling(n).mean()
        sig[f"MA{n}"] = {d: bool(c > m) for d, c, m in zip(csi["d"], csi["close"], ma) if pd.notna(m)}
    for m in (40, 60):
        r = csi["close"] / csi["close"].shift(m) - 1
        sig[f"MOM{m}"] = {d: bool(v > 0) for d, v in zip(csi["d"], r) if pd.notna(v)}
    return sig


def run(P, ron):
    """给 risk_on 字典，算逐周净收益（扣换手成本：持仓换手 + 进出市场）。返回 net 数组 + 转折数。"""
    net, prev = [], frozenset()
    trans = 0
    last_on = None
    for d, names, port, mkt in P[["d", "names", "port", "mkt"]].itertuples(index=False):
        on = ron.get(d, True)
        held = names if on else frozenset()
        # 对称差换手 / TOPN：进场=1、退场=1、换名≈ Δ/3
        turn = len(held ^ prev) / TOPN
        r = (port if on else 0.0) - COST_BPS / 1e4 * turn
        net.append(r); prev = held
        if last_on is not None and on != last_on:
            trans += 1
        last_on = on
    return np.array(net), trans


def stats(r):
    r = np.asarray(r)
    cum = float(np.prod(1 + r) - 1)
    sh = float(r.mean() / (r.std() + 1e-12) * np.sqrt(50))
    eq = np.cumprod(1 + r)
    dd = float(((eq - np.maximum.accumulate(eq)) / np.maximum.accumulate(eq)).min())
    return cum, sh, dd


def main():
    P = weekly_top3()
    sig = regime_signals()
    print(f"周期数={len(P)}  ({P['d'].iloc[0]}..{P['d'].iloc[-1]})  成本={COST_BPS}bp  选股=等权top3")

    # 基准：始终满仓（含周换手成本）、csi300 买入持有
    always_net, _ = run(P, {d: True for d in P["d"]})
    ac, ash, add = stats(always_net)
    mc, msh, mdd = stats(P["mkt"].values)
    print(f"\n{'策略':22}{'累计净':>10}{'Sharpe':>9}{'maxDD':>9}{'转折':>6}")
    print(f"{'always 始终满仓':22}{ac:>+9.1%}{ash:>+9.2f}{add:>+9.1%}{'-':>6}")
    print(f"{'csi300 buy&hold':22}{mc:>+9.1%}{msh:>+9.2f}{mdd:>+9.1%}{'-':>6}")

    print("\n--- 择时（预锁 MA200；其余=敏感性带，非挑最优）---")
    results = {}
    for k in ["MA200", "MA120", "MA150", "MA250", "MOM40", "MOM60"]:
        net, tr = run(P, sig[k])
        c, s, dd = stats(net)
        results[k] = net
        star = " ★预锁" if k == "MA200" else ""
        print(f"{k:22}{c:>+9.1%}{s:>+9.2f}{dd:>+9.1%}{tr:>6}{star}")

    # 去 2022 分解（用预锁 MA200）：edge 是否只靠规避 2022 崩盘
    mask22 = P["d"].str[:4] == "2022"
    a_ex = always_net[~mask22.values]; t_ex = results["MA200"][~mask22.values]
    print(f"\n去掉2022:  always 累计={np.prod(1+a_ex)-1:+.1%} Sh={a_ex.mean()/(a_ex.std()+1e-12)*np.sqrt(50):+.2f}"
          f"  |  MA200择时 累计={np.prod(1+t_ex)-1:+.1%} Sh={t_ex.mean()/(t_ex.std()+1e-12)*np.sqrt(50):+.2f}")
    print(f"含2022:    always Sh={ash:+.2f}  |  MA200择时 Sh={stats(results['MA200'])[1]:+.2f}")

    # 逐年（always vs MA200）
    P2 = P.copy(); P2["y"] = P2["d"].str[:4]
    print("\n逐年累计净 (always / MA200择时):")
    for y in sorted(P2["y"].unique()):
        m = (P2["y"] == y).values
        print(f"  {y}: always {np.prod(1+always_net[m])-1:+7.1%}   MA200 {np.prod(1+results['MA200'][m])-1:+7.1%}")


if __name__ == "__main__":
    main()
