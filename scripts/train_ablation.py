"""Ridge 消融研究 harness。复用 eval_ridge 闭式原语;参数化 fit + 通用 backtest,
默认即复现 ridge-on-gauss 基线。4 轴:归一化/dropout/权重区间/聚类分模型。numpy only。"""
import sys, os
sys.stdout.reconfigure(encoding="utf-8")
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np, pandas as pd
import eval_ridge as er
import factor_lib as fl
import iterate as it
from build_factor_matrix import FACTOR_COLS as FC
from test_norm_hysteresis import norm_gauss, norm_rank, norm_winz

SEED = 0
DELTA_GRID = [0.0, 0.02, 0.05, 0.1]
FOLDS = [
    ("2018-01-02", "2019-12-31", "2020-01-02", "2020-12-31"),
    ("2018-01-02", "2020-12-31", "2021-01-02", "2021-12-31"),
] + list(er.tn.WFO_FOLDS)


def fit_variant(panel, lo, hi, norm_fn=norm_gauss, clip_pct=90, drop_p=0.0, n_bags=1, seed=SEED):
    """参数化 ridge 拟合。默认(norm_gauss/clip90/无dropout/单袋)逐字复现 er.fit_ridge。
    drop_p>0:每袋随机遮蔽 drop_p 比例的因子列(置零)→ bagging 平均。"""
    sub = panel[(panel["date"] >= lo) & (panel["date"] <= hi)].dropna(subset=["fwd_ret_5d"])
    p = len(FC)
    rng = np.random.default_rng(seed)
    bags = []
    n_dates = 0
    for _b in range(max(1, n_bags)):
        keep = np.ones(p, bool)
        if drop_p > 0.0:
            keep = rng.random(p) >= drop_p
            if not keep.any():
                keep[rng.integers(p)] = True
        Gram = np.zeros((p, p)); b = np.zeros(p); n = 0
        for d, g in sub.groupby("date"):
            g = g.dropna(subset=["fwd_ret_5d"])
            if len(g) < 5:
                continue
            G = norm_fn(g[FC].to_numpy(float))
            if drop_p > 0.0:
                G = G * keep                       # 遮蔽列置零
            y = fl.cross_sectional_rank(g["fwd_ret_5d"].to_numpy(float)) - 0.5
            Gram += G.T @ G; b += G.T @ y; n += 1
        n_dates = n
        if n == 0:
            bags.append(np.zeros(p)); continue
        lam = er.RIDGE_A * np.mean(np.diag(Gram))
        w = np.linalg.solve(Gram + lam * np.eye(p), b)
        q = np.percentile(np.abs(w), clip_pct) + 1e-12
        bags.append(np.clip(w, -q, q))
    return np.mean(bags, axis=0), n_dates


def backtest_score(panel, score_fn, top_n, cost_bps, st_set, delta):
    """通用周频回测——逐字镜像 er.backtest_ridge,仅把 score 换成 score_fn(g)。"""
    panel = panel.sort_values(["date", "symbol"])
    nav = 1.0; prev = set(); navs = []; total_turn = 0.0
    for d in sorted(panel["date"].unique()):
        g = er._eligible(panel[panel["date"] == d].dropna(subset=["fwd_ret_5d"]), st_set)
        if len(g) < top_n:
            continue
        score = np.asarray(score_fn(g), float)
        if delta > 0.0 and prev:
            score = score + delta * g["symbol"].isin(prev).to_numpy().astype(float)
        gi = g.assign(_score=score).sort_values(["_score", "symbol"], ascending=[False, True])
        pick = gi.head(top_n)
        ret = float(pick["fwd_ret_5d"].mean()); cur = set(pick["symbol"])
        turn = len(cur ^ prev) / max(len(cur) + len(prev), 1); total_turn += turn
        net = ret - cost_bps / 1e4 * turn
        nav *= (1.0 + net); navs.append({"t": d, "nav": nav, "picks": list(cur)}); prev = cur
    peak = -1e9
    mdd = 0.0
    for h in navs:
        peak = max(peak, h["nav"])
        mdd = max(mdd, 1.0 - h["nav"] / peak)
    total = navs[-1]["nav"] - 1.0 if navs else 0.0
    return {"holdings": navs, "regime_slices": [], "total_return": total,
            "max_drawdown": mdd, "turnover": total_turn, "n_rebalances": len(navs),
            "excess_return": 0.0}


def select_delta_v(train_panel, score_fn, st_set, top_n=None):
    top_n = top_n or er.TOP_N
    best_d, best = 0.0, -np.inf
    for dd in DELTA_GRID:
        rep = backtest_score(train_panel, score_fn, top_n, it.COST, st_set, dd)
        if rep["total_return"] > best:
            best, best_d = rep["total_return"], dd
    return best_d


def oos_rank_ic(oos_panel, score_fn, st_set):
    ics = []
    for d, g in oos_panel.groupby("date"):
        g = er._eligible(g.dropna(subset=["fwd_ret_5d"]), st_set)
        if len(g) < 20:
            continue
        ic = fl.rank_ic(np.asarray(score_fn(g), float), g["fwd_ret_5d"].to_numpy(float))
        if not np.isnan(ic):
            ics.append(ic)
    return float(np.mean(ics)) if ics else np.nan


def eval_variant(panel, make_score_fn, st_set, idx, label):
    """make_score_fn(train_lo, train_hi) -> score_fn(g)->scores。逐折 fit→选delta→OOS回测→excess+IC。"""
    idx_m, idx_dates = idx
    fold_ex, ics = [], []
    for tl, th, ol, oh in FOLDS:
        oos = panel[(panel["date"] >= ol) & (panel["date"] <= oh)]
        sf = make_score_fn(tl, th)
        train_panel = panel[(panel["date"] >= tl) & (panel["date"] <= th)]
        d = select_delta_v(train_panel, sf, st_set)
        rep = backtest_score(oos, sf, er.TOP_N, it.COST, st_set, d)
        rel = it.to_index_relative(rep, idx_m, idx_dates)
        fold_ex.append(rel["excess_return"] if rel else np.nan)
        ics.append(oos_rank_ic(oos, sf, st_set))
    arr = np.array([x for x in fold_ex if not np.isnan(x)])
    return {"label": label, "fold_excess": fold_ex, "mean": float(arr.mean()) if len(arr) else np.nan,
            "pos": int((arr > 0).sum()), "n": len(arr), "ic": float(np.nanmean(ics))}


def baseline_score_fn(panel, st_set):
    """ridge 基线 make_score_fn:默认 fit_variant + norm_gauss 打分。"""
    def make(tl, th):
        w, _ = fit_variant(panel, tl, th)
        return lambda g: norm_gauss(g[FC].to_numpy(float)) @ w
    return make


# ── Axis 1: per-factor normalization ──────────────────────────────────────────

NORMS = {"gauss": norm_gauss, "rank": norm_rank, "winz": norm_winz}


def apply_per_factor_norm(M, norm_choice):
    """M:(n,p); norm_choice: length-p list of norm names. Apply each col's own norm."""
    M = np.asarray(M, float)
    cols = [NORMS[norm_choice[j]](M[:, [j]])[:, 0] for j in range(M.shape[1])]
    return np.column_stack(cols)


def _argmax_norm_per_factor(acc: dict, p: int) -> list:
    """Pure helper: acc maps norm-name -> length-p array of summed |IC|.
    Returns per-factor argmax norm name list of length p."""
    return [max(acc, key=lambda nm: acc[nm][j]) for j in range(p)]


def per_factor_norms(panel, lo, hi):
    """For each factor pick the norm (gauss/rank/winz) with highest |rank_ic| on TRAIN.
    Returns length-p list of norm name strings."""
    sub = panel[(panel["date"] >= lo) & (panel["date"] <= hi)].dropna(subset=["fwd_ret_5d"])
    p = len(FC)
    acc = {nm: np.zeros(p) for nm in NORMS}
    for d, g in sub.groupby("date"):
        g = g.dropna(subset=["fwd_ret_5d"])
        if len(g) < 20:
            continue
        fwd = g["fwd_ret_5d"].to_numpy(float)
        X = g[FC].to_numpy(float)
        for nm, fn in NORMS.items():
            Xn = fn(X)
            for j in range(p):
                ic = fl.rank_ic(Xn[:, j], fwd)
                if not np.isnan(ic):
                    acc[nm][j] += abs(ic)
    return _argmax_norm_per_factor(acc, p)


# ── Axis 3: weight value-range / dispersion ───────────────────────────────────

def weight_hhi(w):
    """Herfindahl-Hirschman index of absolute weight shares. Returns (hhi, max_share)."""
    a = np.abs(np.asarray(w, float))
    s = a.sum()
    if s == 0:
        return 0.0, 0.0
    shares = a / s
    return float((shares ** 2).sum()), float(shares.max())


def axis1_norms(panel, st_set, idx):
    """Axis 1: compare gauss / rank / winz / per-factor-IC norm variants."""
    rows = []
    for nm, fn in [("gauss(基线)", norm_gauss), ("rank", norm_rank), ("winz", norm_winz)]:
        def mk(tl, th, fn=fn):
            w, _ = fit_variant(panel, tl, th, norm_fn=fn)
            return lambda g: fn(g[FC].to_numpy(float)) @ w
        rows.append(eval_variant(panel, mk, st_set, idx, f"norm={nm}"))

    def mk_pf(tl, th):
        ch = per_factor_norms(panel, tl, th)
        sub = panel[(panel["date"] >= tl) & (panel["date"] <= th)].dropna(subset=["fwd_ret_5d"])
        p = len(FC); Gram = np.zeros((p, p)); b = np.zeros(p)
        for d, g in sub.groupby("date"):
            g = g.dropna(subset=["fwd_ret_5d"])
            if len(g) < 5:
                continue
            G = apply_per_factor_norm(g[FC].to_numpy(float), ch)
            y = fl.cross_sectional_rank(g["fwd_ret_5d"].to_numpy(float)) - 0.5
            Gram += G.T @ G; b += G.T @ y
        lam = er.RIDGE_A * np.mean(np.diag(Gram))
        w = np.linalg.solve(Gram + lam * np.eye(p), b)
        q = np.percentile(np.abs(w), 90) + 1e-12
        w = np.clip(w, -q, q)
        return lambda g: apply_per_factor_norm(g[FC].to_numpy(float), ch) @ w

    rows.append(eval_variant(panel, mk_pf, st_set, idx, "norm=per-factor(TRAIN-IC)"))
    return rows


# ── Axis 2: dropout-count sensitivity via bagging-ridge ───────────────────────

def axis2_dropout(panel, st_set, idx, n_bags=20):
    """Axis 2: sweep drop_p ∈ {0, 0.25, 0.5, 0.75}; p=0 → baseline (n_bags=1)."""
    rows = []
    for pdrop in [0.0, 0.25, 0.5, 0.75]:
        def mk(tl, th, pdrop=pdrop):
            w, _ = fit_variant(panel, tl, th, drop_p=pdrop,
                               n_bags=(1 if pdrop == 0 else n_bags))
            return lambda g: norm_gauss(g[FC].to_numpy(float)) @ w
        rows.append(eval_variant(panel, mk, st_set, idx,
                                 f"dropout p={pdrop}{'(基线)' if pdrop == 0 else f' ×{n_bags}袋'}"))
    return rows


def axis3_clip(panel, st_set, idx):
    """Axis 3: compare clip percentile variants; attach weight-dispersion metrics."""
    rows = []
    for cp in [99, 90, 75, 50]:
        def mk(tl, th, cp=cp):
            w, _ = fit_variant(panel, tl, th, clip_pct=cp)
            return lambda g: norm_gauss(g[FC].to_numpy(float)) @ w
        r = eval_variant(panel, mk, st_set, idx, f"clip=p{cp}{'(基线)' if cp == 90 else ''}")
        w_full, _ = fit_variant(panel, "2018-01-02", "2026-06-04", clip_pct=cp)
        hhi, mx = weight_hhi(w_full)
        r["hhi"] = hhi; r["max_share"] = mx
        rows.append(r)
    return rows
