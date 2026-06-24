# Ridge 消融研究 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在已验证的 ridge-on-gauss 基线上,用一个消融 harness 刻画 4 个轴(逐因子归一化 / dropout 数量 / 权重区间 / 聚类分模型)的效应,§5.3 诚实判读。

**Architecture:** 单文件 `scripts/train_ablation.py` 复用 `eval_ridge` 闭式原语,核心是两个**参数化、默认即复现基线**的函数(`fit_variant`、`backtest_score`)+ 一个 6 折 eval runner;4 个轴只是给这俩注入不同参数/打分函数。numpy/pandas only。

**Tech Stack:** Python, numpy, pandas;复用 `eval_ridge`(fit_ridge/backtest_ridge/select_delta_ridge/_eligible/TOP_N/RIDGE_A/ST_PATH/PANEL_MEMBERSHIP)、`test_norm_hysteresis`(norm_gauss/norm_rank/norm_winz)、`factor_lib`(cross_sectional_rank/rank_ic)、`iterate`(to_index_relative/load_index/COST)、`build_factor_matrix.FACTOR_COLS`。

## Global Constraints
- 消融/理解性研究,非 deploy 候选;任何"胜出"须过 §5.3(6 折正 + 胜基线 + 无单折依赖)否则记证伪。
- numpy/pandas only,**无 sklearn**;聚类手搓 KMeans(Lloyd + k-means++,固定种子)。
- **不改 `eval_ridge` 源**:clip 分位/归一化/dropout 用本地 `fit_variant` 封装;不改引擎/部署/72 因子集/冻结权重。
- 基线对标:ridge-on-gauss 6 折均值超额 **+0.186 / 6-6 / OOS rank-IC≈0.066**。
- 折定义:`FOLDS`(见 T1)= 2 早折(2020/2021)+ `eval_ridge.tn.WFO_FOLDS`(2022/2023/2024/2025-26),共 6;membership 池。
- 随机性固定种子(SEED=0)。

---

### Task 1: 核心引擎(参数化 fit + 通用 backtest + 6 折 runner + 基线复现)

**Files:**
- Create: `scripts/train_ablation.py`
- Test: `scripts/test_train_ablation.py`

**Interfaces:**
- Produces:
  - `fit_variant(panel, lo, hi, norm_fn=norm_gauss, clip_pct=90, drop_p=0.0, n_bags=1, seed=0) -> (w: np.ndarray[p], n_train_dates: int)`
  - `backtest_score(panel, score_fn, top_n, cost_bps, st_set, delta) -> report dict`(score_fn: `(g_df)->np.ndarray[len(g)]`;report 同 backtest_ridge:holdings/total_return/turnover/n_rebalances)
  - `select_delta_v(train_panel, score_fn, st_set) -> float`
  - `oos_rank_ic(oos_panel, score_fn, st_set) -> float`(逐 OOS 日 score vs fwd 的 rank_ic 均值)
  - `eval_variant(panel, make_score_fn, st_set, idx, label) -> dict{label, fold_excess:list, mean, pos, ic}`,其中 `make_score_fn(train_lo,train_hi)->(score_fn, delta_selector_ok)`;runner 内逐折 fit→选delta→OOS回测→to_index_relative→excess + IC。
  - `FOLDS`, `SEED=0`, `FC`(=FACTOR_COLS)

- [ ] **Step 1: 写失败测试**(基线复现:fit_variant 默认 == er.fit_ridge;backtest_score 基线 == er.backtest_ridge)

```python
# scripts/test_train_ablation.py
import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np, pandas as pd
import eval_ridge as er
import train_ablation as ta
from test_norm_hysteresis import norm_gauss
from build_factor_matrix import FACTOR_COLS

PANEL = er.PANEL_MEMBERSHIP
def _panel():
    return pd.read_csv(PANEL, dtype={"symbol": str})

def test_fit_variant_defaults_reproduce_fit_ridge():
    p = _panel(); lo, hi = "2018-01-02", "2021-12-31"
    w_ref, n_ref = er.fit_ridge(p, lo, hi)
    w, n = ta.fit_variant(p, lo, hi)          # defaults = norm_gauss, clip90, no dropout
    assert n == n_ref
    assert np.allclose(w, w_ref, atol=1e-9), np.abs(w - w_ref).max()

def test_backtest_score_baseline_reproduces_backtest_ridge():
    p = _panel(); st = set()
    oos = p[(p["date"] >= "2022-01-02") & (p["date"] <= "2022-12-31")]
    w, _ = er.fit_ridge(p, "2018-01-02", "2021-12-31")
    ref = er.backtest_ridge(oos, w, top_n=er.TOP_N, cost_bps=20.0, st_set=st, delta=0.05)
    sf = lambda g: norm_gauss(g[FACTOR_COLS].to_numpy(float)) @ w
    got = ta.backtest_score(oos, sf, top_n=er.TOP_N, cost_bps=20.0, st_set=st, delta=0.05)
    assert abs(got["total_return"] - ref["total_return"]) < 1e-9
    assert [h["picks"] for h in got["holdings"]] == [h["picks"] for h in ref["holdings"]]
```

- [ ] **Step 2: 跑测试看失败** `python -m pytest scripts/test_train_ablation.py -q` → FAIL(模块/函数未定义)

- [ ] **Step 3: 实现核心** `scripts/train_ablation.py`

```python
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
    nav = 1.0; prev = set(); navs = []; period = []; total_turn = 0.0
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
        net = ret - cost_bps / 1e4 * turn; period.append(net)
        nav *= (1.0 + net); navs.append({"t": d, "nav": nav, "picks": list(cur)}); prev = cur
    total = navs[-1]["nav"] - 1.0 if navs else 0.0
    return {"holdings": navs, "regime_slices": [], "total_return": total,
            "max_drawdown": 0.0, "turnover": total_turn, "n_rebalances": len(navs),
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
    arr = np.array([x for x in fold_ex if x is not None and not np.isnan(x)])
    return {"label": label, "fold_excess": fold_ex, "mean": float(arr.mean()) if len(arr) else np.nan,
            "pos": int((arr > 0).sum()), "n": len(arr), "ic": float(np.nanmean(ics))}


def baseline_score_fn(panel, st_set):
    """ridge 基线 make_score_fn:默认 fit_variant + norm_gauss 打分。"""
    def make(tl, th):
        w, _ = fit_variant(panel, tl, th)
        return lambda g: norm_gauss(g[FC].to_numpy(float)) @ w
    return make
```

- [ ] **Step 4: 跑测试看通过** `python -m pytest scripts/test_train_ablation.py -q` → 2 passed
- [ ] **Step 5: 提交** `git add scripts/train_ablation.py scripts/test_train_ablation.py && git commit -m "feat(ablation): core engine — parameterized fit + generic backtest, reproduces ridge baseline"`

---

### Task 2: 轴1 逐因子归一化 + 轴3 权重区间

**Files:** Modify `scripts/train_ablation.py`, `scripts/test_train_ablation.py`

**Interfaces:**
- Consumes: `fit_variant`, `eval_variant`, `norm_gauss/rank/winz`, `FC`.
- Produces:
  - `per_factor_norms(panel, lo, hi) -> list[callable]`(每因子在 TRAIN 上比 3 种 norm 的 |rank_ic|,取最高;返回长度 p 的 norm 选择,1D)
  - `apply_per_factor_norm(M, norm_choice) -> np.ndarray`(逐列用各自 norm)
  - `weight_hhi(w) -> (hhi: float, max_share: float)`
  - axis runners:`axis1_norms(panel, st_set, idx)`、`axis3_clip(panel, st_set, idx)` 返回结果行列表

- [ ] **Step 1: 写失败测试**

```python
def test_per_factor_norm_picks_max_train_ic():
    # 构造:某因子用 rank 时 IC 明显更高 → 选 rank。合成 2 列 + fwd。
    import train_ablation as ta, numpy as np, pandas as pd
    # 列0 与 fwd 单调(任何 norm IC 同);列1 注入异常值使 winz/rank 与 gauss 排序一致 → 仍取首个最高
    # 简化断言:返回长度==len(FC) 的可调用列表,且对常量列回退 norm_gauss 不报错
    p = pd.read_csv(ta.er.PANEL_MEMBERSHIP, dtype={"symbol": str})
    ch = ta.per_factor_norms(p, "2018-01-02", "2019-12-31")
    assert len(ch) == len(ta.FC)

def test_weight_hhi_dispersion():
    import train_ablation as ta, numpy as np
    h1, m1 = ta.weight_hhi(np.array([1.0, 0, 0, 0]))   # 集中
    h2, m2 = ta.weight_hhi(np.array([1.0, 1, 1, 1]))   # 均匀
    assert h1 == 1.0 and m1 == 1.0
    assert abs(h2 - 0.25) < 1e-9 and abs(m2 - 0.25) < 1e-9

def test_clip_pct_changes_dispersion():
    # 更紧的 clip → 更均匀(HHI 更小或相等)
    import train_ablation as ta, numpy as np
    p = pd.read_csv(ta.er.PANEL_MEMBERSHIP, dtype={"symbol": str})
    w99, _ = ta.fit_variant(p, "2018-01-02", "2021-12-31", clip_pct=99)
    w50, _ = ta.fit_variant(p, "2018-01-02", "2021-12-31", clip_pct=50)
    assert ta.weight_hhi(w50)[0] <= ta.weight_hhi(w99)[0] + 1e-9
```

- [ ] **Step 2: 跑测试看失败** → FAIL(未定义)

- [ ] **Step 3: 实现**

```python
NORMS = {"gauss": norm_gauss, "rank": norm_rank, "winz": norm_winz}

def apply_per_factor_norm(M, norm_choice):
    """M:(n,p);norm_choice:长度 p 的 norm 名列表。逐列用各自 norm(各 norm 对单列调用)。"""
    M = np.asarray(M, float)
    cols = [NORMS[norm_choice[j]](M[:, [j]])[:, 0] for j in range(M.shape[1])]
    return np.column_stack(cols)

def per_factor_norms(panel, lo, hi):
    """每因子选 TRAIN |rank_ic| 最高的 norm(gauss/rank/winz)。返回长度 p 的 norm 名列表。"""
    sub = panel[(panel["date"] >= lo) & (panel["date"] <= hi)].dropna(subset=["fwd_ret_5d"])
    p = len(FC)
    acc = {nm: np.zeros(p) for nm in NORMS}; cnt = 0
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
        cnt += 1
    choice = []
    for j in range(p):
        best_nm = max(NORMS, key=lambda nm: acc[nm][j])
        choice.append(best_nm)
    return choice

def weight_hhi(w):
    a = np.abs(np.asarray(w, float)); s = a.sum()
    if s == 0:
        return 0.0, 0.0
    shares = a / s
    return float((shares ** 2).sum()), float(shares.max())

def axis1_norms(panel, st_set, idx):
    rows = []
    for nm, fn in [("gauss(基线)", norm_gauss), ("rank", norm_rank), ("winz", norm_winz)]:
        def mk(tl, th, fn=fn):
            w, _ = fit_variant(panel, tl, th, norm_fn=fn)
            return lambda g: fn(g[FC].to_numpy(float)) @ w
        rows.append(eval_variant(panel, mk, st_set, idx, f"norm={nm}"))
    # 逐因子选 norm
    def mk_pf(tl, th):
        ch = per_factor_norms(panel, tl, th)
        sub = panel[(panel["date"] >= tl) & (panel["date"] <= th)].dropna(subset=["fwd_ret_5d"])
        # 用逐因子归一化拟合(本地 Gram 累加)
        p = len(FC); Gram = np.zeros((p, p)); b = np.zeros(p)
        for d, g in sub.groupby("date"):
            g = g.dropna(subset=["fwd_ret_5d"])
            if len(g) < 5: continue
            G = apply_per_factor_norm(g[FC].to_numpy(float), ch)
            y = fl.cross_sectional_rank(g["fwd_ret_5d"].to_numpy(float)) - 0.5
            Gram += G.T @ G; b += G.T @ y
        lam = er.RIDGE_A * np.mean(np.diag(Gram))
        w = np.linalg.solve(Gram + lam * np.eye(p), b)
        q = np.percentile(np.abs(w), 90) + 1e-12; w = np.clip(w, -q, q)
        return lambda g: apply_per_factor_norm(g[FC].to_numpy(float), ch) @ w
    rows.append(eval_variant(panel, mk_pf, st_set, idx, "norm=per-factor(TRAIN-IC)"))
    return rows

def axis3_clip(panel, st_set, idx):
    rows = []
    for cp in [99, 90, 75, 50]:
        def mk(tl, th, cp=cp):
            w, _ = fit_variant(panel, tl, th, clip_pct=cp)
            return lambda g: norm_gauss(g[FC].to_numpy(float)) @ w
        r = eval_variant(panel, mk, st_set, idx, f"clip=p{cp}{'(基线)' if cp == 90 else ''}")
        # 附全样本权重弥散
        w_full, _ = fit_variant(panel, "2018-01-02", "2026-06-04", clip_pct=cp)
        hhi, mx = weight_hhi(w_full); r["hhi"] = hhi; r["max_share"] = mx
        rows.append(r)
    return rows
```

- [ ] **Step 4: 跑测试看通过** → 3 passed(加之前 2 = 5)
- [ ] **Step 5: 提交** `git commit -m "feat(ablation): axis1 per-factor norm + axis3 weight-clip/dispersion"`

---

### Task 3: 轴2 dropout-bagging

**Files:** Modify `scripts/train_ablation.py`, `scripts/test_train_ablation.py`

**Interfaces:**
- Consumes: `fit_variant`(已含 drop_p/n_bags), `eval_variant`.
- Produces: `axis2_dropout(panel, st_set, idx) -> rows`

- [ ] **Step 1: 写失败测试**

```python
def test_dropout_p0_reproduces_baseline_weights():
    import train_ablation as ta, numpy as np, pandas as pd
    p = pd.read_csv(ta.er.PANEL_MEMBERSHIP, dtype={"symbol": str})
    w0, _ = ta.fit_variant(p, "2018-01-02", "2021-12-31", drop_p=0.0, n_bags=1)
    wb, _ = ta.er.fit_ridge(p, "2018-01-02", "2021-12-31")
    assert np.allclose(w0, wb, atol=1e-9)

def test_dropout_masks_columns():
    # drop_p=1.0 但保至少一列 → 权重几乎全 0(只一列非零方向)
    import train_ablation as ta, numpy as np, pandas as pd
    p = pd.read_csv(ta.er.PANEL_MEMBERSHIP, dtype={"symbol": str})
    w, _ = ta.fit_variant(p, "2018-01-02", "2019-12-31", drop_p=1.0, n_bags=1, seed=1)
    assert int((np.abs(w) > 1e-9).sum()) <= 1
```

- [ ] **Step 2: 跑测试看失败** → FAIL(axis2_dropout 未定义;前两个 fit_variant 测试应已过)

- [ ] **Step 3: 实现**

```python
def axis2_dropout(panel, st_set, idx, n_bags=20):
    rows = []
    for pdrop in [0.0, 0.25, 0.5, 0.75]:
        def mk(tl, th, pdrop=pdrop):
            w, _ = fit_variant(panel, tl, th, drop_p=pdrop, n_bags=(1 if pdrop == 0 else n_bags))
            return lambda g: norm_gauss(g[FC].to_numpy(float)) @ w
        rows.append(eval_variant(panel, mk, st_set, idx,
                                 f"dropout p={pdrop}{'(基线)' if pdrop == 0 else f' ×{n_bags}袋'}"))
    return rows
```

- [ ] **Step 4: 跑测试看通过** → 2 passed(累计 7)
- [ ] **Step 5: 提交** `git commit -m "feat(ablation): axis2 dropout-bagging sweep"`

---

### Task 4: 轴4 聚类→分模型(numpy KMeans + 过拟合护栏)

**Files:** Modify `scripts/train_ablation.py`, `scripts/test_train_ablation.py`

**Interfaces:**
- Consumes: `fit_variant`(用 norm_gauss 单簇拟合)、`eval_variant`、`norm_gauss`、`FC`。
- Produces:
  - `kmeans_fit(X, k, seed=SEED, iters=50) -> centroids np.ndarray[k,p]`(Lloyd + k-means++ 初始化)
  - `kmeans_assign(X, centroids) -> labels np.ndarray[n]`
  - `train_centroids(panel, lo, hi, k) -> centroids`(对 TRAIN 全周 gauss 因子 fit)
  - `cluster_score_fn(panel, lo, hi, k) -> (score_fn, guard dict)`:每簇 fit ridge;打分时每股按最近质心用其簇权重;guard 含 per-cluster TRAIN 样本量。
  - `axis4_cluster(panel, st_set, idx) -> rows`(K∈{2,3,5} + 池化基线;每行附 guard:min/avg 簇样本量、簇稳定性)
  - `cluster_stability(panel, ol, oh, centroids) -> float`(同股相邻周类别变动率,越低越稳)

- [ ] **Step 1: 写失败测试**

```python
def test_kmeans_separates_two_blobs():
    import train_ablation as ta, numpy as np
    rng = np.random.default_rng(0)
    A = rng.normal(-5, 0.3, (50, 3)); B = rng.normal(5, 0.3, (50, 3))
    X = np.vstack([A, B])
    cen = ta.kmeans_fit(X, 2, seed=0)
    lab = ta.kmeans_assign(X, cen)
    # 同一 blob 内标签一致
    assert len(set(lab[:50])) == 1 and len(set(lab[50:])) == 1
    assert lab[0] != lab[50]

def test_kmeans_deterministic():
    import train_ablation as ta, numpy as np
    X = np.random.default_rng(1).normal(0, 1, (80, 4))
    c1 = ta.kmeans_fit(X, 3, seed=7); c2 = ta.kmeans_fit(X, 3, seed=7)
    assert np.allclose(c1, c2)
```

- [ ] **Step 2: 跑测试看失败** → FAIL(未定义)

- [ ] **Step 3: 实现**

```python
def kmeans_fit(X, k, seed=SEED, iters=50):
    X = np.asarray(X, float); n = len(X); rng = np.random.default_rng(seed)
    # k-means++ 初始化
    cen = [X[rng.integers(n)]]
    for _ in range(1, k):
        d2 = np.min([((X - c) ** 2).sum(1) for c in cen], axis=0)
        probs = d2 / d2.sum() if d2.sum() > 0 else np.ones(n) / n
        cen.append(X[rng.choice(n, p=probs)])
    cen = np.array(cen)
    for _ in range(iters):
        lab = kmeans_assign(X, cen)
        new = np.array([X[lab == j].mean(0) if (lab == j).any() else cen[j] for j in range(k)])
        if np.allclose(new, cen):
            break
        cen = new
    return cen

def kmeans_assign(X, centroids):
    X = np.asarray(X, float)
    d = np.stack([((X - c) ** 2).sum(1) for c in centroids], axis=1)
    return d.argmin(1)

def train_centroids(panel, lo, hi, k):
    sub = panel[(panel["date"] >= lo) & (panel["date"] <= hi)].dropna(subset=["fwd_ret_5d"])
    rows = [norm_gauss(g[FC].to_numpy(float)) for _, g in sub.groupby("date") if len(g) >= 5]
    X = np.vstack(rows)
    return kmeans_fit(X, k)

def cluster_score_fn(panel, lo, hi, k):
    cen = train_centroids(panel, lo, hi, k)
    sub = panel[(panel["date"] >= lo) & (panel["date"] <= hi)].dropna(subset=["fwd_ret_5d"])
    p = len(FC)
    Gram = [np.zeros((p, p)) for _ in range(k)]; bb = [np.zeros(p) for _ in range(k)]
    cnt = np.zeros(k)
    for d, g in sub.groupby("date"):
        g = g.dropna(subset=["fwd_ret_5d"])
        if len(g) < 5: continue
        G = norm_gauss(g[FC].to_numpy(float))
        lab = kmeans_assign(G, cen)
        y = fl.cross_sectional_rank(g["fwd_ret_5d"].to_numpy(float)) - 0.5
        for j in range(k):
            mask = lab == j
            if mask.sum() == 0: continue
            Gj = G[mask]; Gram[j] += Gj.T @ Gj; bb[j] += Gj.T @ y[mask]; cnt[j] += mask.sum()
    W = np.zeros((k, p))
    for j in range(k):
        if cnt[j] == 0: continue
        lam = er.RIDGE_A * np.mean(np.diag(Gram[j])) if np.trace(Gram[j]) > 0 else 1.0
        w = np.linalg.solve(Gram[j] + lam * np.eye(p), bb[j])
        q = np.percentile(np.abs(w), 90) + 1e-12; W[j] = np.clip(w, -q, q)
    def score_fn(g):
        G = norm_gauss(g[FC].to_numpy(float)); lab = kmeans_assign(G, cen)
        return np.array([G[i] @ W[lab[i]] for i in range(len(G))])
    guard = {"cluster_samples": cnt.tolist(), "min_samples": float(cnt.min()), "avg_samples": float(cnt.mean())}
    return score_fn, cen, guard

def cluster_stability(panel, ol, oh, centroids):
    """同股相邻周类别变动率(0=完全稳定,1=每周都变)。"""
    oos = panel[(panel["date"] >= ol) & (panel["date"] <= oh)]
    last = {}; changes = 0; total = 0
    for d in sorted(oos["date"].unique()):
        g = oos[oos["date"] == d]
        G = norm_gauss(g[FC].to_numpy(float)); lab = kmeans_assign(G, centroids)
        for sym, l in zip(g["symbol"].values, lab):
            if sym in last:
                total += 1; changes += int(last[sym] != l)
            last[sym] = l
    return float(changes / total) if total else np.nan

def axis4_cluster(panel, st_set, idx):
    rows = []
    rows.append(eval_variant(panel, baseline_score_fn(panel, st_set), st_set, idx, "pooled(基线 K=1)"))
    for k in [2, 3, 5]:
        guards = {}
        def mk(tl, th, k=k, guards=guards):
            sf, cen, gd = cluster_score_fn(panel, tl, th, k)
            guards["last"] = gd; guards["cen"] = cen; guards["tl"] = tl
            return sf
        r = eval_variant(panel, mk, st_set, idx, f"cluster K={k}")
        # 末折护栏(stability 用最后一折 OOS)
        gd = guards.get("last", {})
        r["min_samples"] = gd.get("min_samples"); r["avg_samples"] = gd.get("avg_samples")
        tl0, th0, ol0, oh0 = FOLDS[-1]
        sf2, cen2, _ = cluster_score_fn(panel, tl0, th0, k)
        r["stability_chg"] = cluster_stability(panel, ol0, oh0, cen2)
        rows.append(r)
    return rows
```

- [ ] **Step 4: 跑测试看通过** → 2 passed(累计 9)
- [ ] **Step 5: 提交** `git commit -m "feat(ablation): axis4 numpy-KMeans per-cluster ridge + overfit guards"`

---

### Task 5: 主编排 + findings 文档

**Files:** Modify `scripts/train_ablation.py`(加 `main()`);Create `docs/superpowers/2026-06-24-ridge-ablation-findings.md`

**Interfaces:** Consumes 全部 axis runner。

- [ ] **Step 1: 实现 main()**

```python
def _print_rows(title, rows, extra=()):
    print(f"\n=== {title} ===")
    hdr = f"{'variant':<28}{'mean':>9}{'pos':>6}{'IC':>9}" + "".join(f"{e:>10}" for e in extra)
    print(hdr)
    for r in rows:
        line = f"{r['label']:<28}{r['mean']:>+9.4f}{str(r['pos'])+'/'+str(r['n']):>6}{r['ic']:>+9.4f}"
        for e in extra:
            v = r.get(e); line += f"{(f'{v:.3f}' if isinstance(v, float) else str(v)):>10}"
        print(line)

def main():
    st_set = set(pd.read_csv(er.ST_PATH)["symbol"]) if os.path.exists(er.ST_PATH) else set()
    panel = pd.read_csv(er.PANEL_MEMBERSHIP, dtype={"symbol": str})
    idx = it.load_index("csi300")
    print("Ridge 消融研究 — 6 折 OOS(membership)。基线 ridge-on-gauss ≈ +0.186 / 6-6 / IC≈0.066")
    _print_rows("轴1 逐因子归一化", axis1_norms(panel, st_set, idx))
    _print_rows("轴3 权重区间(clip 分位)", axis3_clip(panel, st_set, idx), extra=("hhi", "max_share"))
    _print_rows("轴2 dropout-bagging", axis2_dropout(panel, st_set, idx))
    _print_rows("轴4 聚类→分模型", axis4_cluster(panel, st_set, idx), extra=("min_samples", "stability_chg"))

if __name__ == "__main__":
    main()
```

- [ ] **Step 2: 跑全量** `python scripts/train_ablation.py 2>&1 | tee /tmp/ablation.out`(~数分钟;6 折 × 多变体 × fit/backtest)。预期:基线行 ≈ +0.186/6;各轴变体多数 ≤ 基线;轴4 K↑→mean↓ + min_samples↓。
- [ ] **Step 3: 写 findings** `docs/superpowers/2026-06-24-ridge-ablation-findings.md`——四轴表 + 判读(每轴是否有效应/证伪 vs 基线;轴4 过拟合三件套结论;§5.3 裁决)。用真实跑出的数字,不留占位。
- [ ] **Step 4: 提交** `git commit -m "feat(ablation): main orchestration + findings (4-axis effect study)"`

---

## Self-Review

**Spec 覆盖**:轴1→T2 `axis1_norms`+`per_factor_norms`✓;轴2→T3 `axis2_dropout`(fit_variant drop_p)✓;轴3→T2 `axis3_clip`+`weight_hhi`✓;轴4→T4 KMeans+per-cluster+guards✓;统一 §5.3 eval→T1 `eval_variant`(对标基线+IC)✓;过拟合护栏→T4(min_samples/stability)✓;findings→T5✓;基线复现→T1 测试✓。
**占位扫描**:无 TBD;各步含完整代码/命令。
**类型一致**:`fit_variant`(panel,lo,hi,norm_fn,clip_pct,drop_p,n_bags,seed)→(w,n) 全任务一致;`backtest_score(panel,score_fn,top_n,cost_bps,st_set,delta)`、`eval_variant(panel,make_score_fn,st_set,idx,label)→dict{label,fold_excess,mean,pos,n,ic}`、`make_score_fn(tl,th)->score_fn(g)` 贯穿一致;`kmeans_fit/assign`、`weight_hhi(w)->(hhi,max_share)`、`per_factor_norms->list[str]`/`apply_per_factor_norm(M,choice)` 一致。
**约束**:全程不改 eval_ridge 源(clip 等用 fit_variant 本地封装);numpy only;不动引擎/部署/72 因子。
