# 轻量因子训练管线 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 纯 Python 研究态验证「学习得到的线性因子权重 w（评分=w·x）能否在周频 top-3 口径上超过等权基线」，不改 Rust 引擎。

**Architecture:** 三段 + 一个共享纯计算库。`factor_lib.py`(截面排名归一/Elastic-Net/Rank-IC/线性分，零 IO，重测) → `build_factor_matrix.py`(13 因子+未来5日收益→CSV 面板) → `train_factor_weights.py`(锚定 train 拟合 w→weights.json) → `eval_linear_score.py`(Python 周频回测器→§5.3 裁决，学习-w vs 等权，含 Rust 对账)。

**Tech Stack:** Python 3.13 + numpy 2.3 + pandas 3.0（**仅此**）。复用 `scripts/iterate.py` 的指标函数。

## Global Constraints

- 零新 Python 依赖：仅 numpy/pandas/标准库；**禁用 sklearn/scipy**。
- 不改 Rust 引擎 / 桌面 crate；仅新增 `scripts/` 下 Python。
- 面板与产物落 `data/factor_panel/`（在 gitignore 的 `data/` 下，不入库）。
- 时点纪律（PIT）：因子只用 ≤t 行；标签=严格未来 5 日收益；票池按 membership 点时成分（≤t 最近月末快照）掩码。
- 口径锁定：周频持有期=5 交易日；top-3 主 / top-10 参照；基准=沪深300；train 2018-01-02..2023-12-29，OOS 2024-01-02..2026-06-12。
- §5.3 裁决沿用 `iterate.py`：PASS 需 gross_ex>0 ∧ net_OOS_ex>0 ∧ net_sharpe>0 ∧ break_even≥40bps ∧ tier2 无符号翻转。成本 net=20bps（单边换手×成本）。
- 编码：每脚本首部 `import sys; sys.stdout.reconfigure(encoding="utf-8")`；读写 CSV/JSON 显式 `encoding="utf-8"`。
- 测试用 `pytest`（已有 `scripts/test_*.py` 先例）；纯计算优先 TDD；频繁提交（每任务收尾一次）。
- git add 显式文件，绝不 `-A`；commit message 英文，结尾 `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`；**不 push**（除非用户明示）。

## 数据底座（实现者须知，已核对）

- `data/baostock/kday/<sym>.csv`：`time,open,high,low,close,volume,amount,turn,pctChg`（time 为 `YYYY-MM-DD`，前复权，升序）。
- `data/fundamentals/<sym>.csv`：`time,roe,np_yoy,rev_yoy,gross_margin,eps,bps`（time=披露生效日，时点值，稀疏季频）。
- `data/baostock/pa_sector_merged/<sym>.csv`：`time,pa_*,sec_mom20,sec_trend,sec_breadth,sec_heat,roe,np_yoy,rev_yoy,gross_margin,eps,bps`（仅取 `time,sec_mom20`；2963 只覆盖）。
- `data/baostock/universe_baostock_day.csv`：`symbol,primary,context,fundamentals`（roster=1074 只；本管线票池取此 symbol 列）。
- `data/membership_top2000.csv`：`date,symbol`（月末快照，2018-01-31 起；点时成分）。
- `data/baostock/index/csi300.csv`：`time,close`（time=`YYYY-MM-DD HH:MM:SS`，用 `[:10]` 取日期）。
- `data/baostock/st_symbols.csv`：`symbol,name`（首列 symbol；硬闸剔 ST 用）。

---

### Task 1: factor_lib.py（共享纯计算库）

**Files:**
- Create: `scripts/factor_lib.py`
- Test: `scripts/test_factor_lib.py`

**Interfaces:**
- Produces（后续任务消费）：
  - `cross_sectional_rank(values: np.ndarray) -> np.ndarray`：1D，百分位排名∈[0,1]，并列取平均，NaN→0.5。
  - `rank_columns(X: np.ndarray) -> np.ndarray`：对 2D 矩阵逐列做 `cross_sectional_rank`。
  - `elastic_net_fit(X, y, alpha, l1_ratio=0.5, max_iter=1000, tol=1e-7) -> np.ndarray`：坐标下降，返回 `w`(shape=(p,))；内部中心化、无截距。
  - `rank_ic(scores: np.ndarray, fwd: np.ndarray) -> float`：Spearman=两者排名的 Pearson 相关；少于 2 有效点→`nan`。
  - `linear_score(Xrank: np.ndarray, w: np.ndarray) -> np.ndarray`：`Xrank @ w`。

- [ ] **Step 1: Write the failing test**

```python
# scripts/test_factor_lib.py
import numpy as np
import factor_lib as fl

def test_cross_sectional_rank_basic():
    r = fl.cross_sectional_rank(np.array([10.0, 20.0, 30.0]))
    assert np.allclose(r, [0.0, 0.5, 1.0])

def test_cross_sectional_rank_ties_and_nan():
    r = fl.cross_sectional_rank(np.array([5.0, 5.0, 9.0, np.nan]))
    assert np.isclose(r[0], r[1])          # 并列同分
    assert np.isclose(r[3], 0.5)           # NaN→中位
    assert r[2] == max(r[:3])              # 最大值排名最高

def test_rank_ic_monotonic():
    x = np.array([1.0, 2, 3, 4, 5]); y = np.array([2.0, 4, 6, 8, 10])
    assert np.isclose(fl.rank_ic(x, y), 1.0)
    assert np.isclose(fl.rank_ic(x, -y), -1.0)

def test_elastic_net_recovers_sparse_weights():
    rng = np.random.default_rng(0)
    X = rng.normal(size=(2000, 5))
    w_true = np.array([2.0, 0.0, -1.5, 0.0, 0.0])
    y = X @ w_true + rng.normal(scale=0.05, size=2000)
    w = fl.elastic_net_fit(X, y, alpha=0.01, l1_ratio=0.5)
    assert abs(w[0] - 2.0) < 0.3 and abs(w[2] + 1.5) < 0.3
    assert abs(w[1]) < 0.2 and abs(w[3]) < 0.2 and abs(w[4]) < 0.2  # L1 压无关项

def test_elastic_net_l1ratio0_matches_ridge():
    rng = np.random.default_rng(1)
    X = rng.normal(size=(500, 4)); y = rng.normal(size=500)
    w = fl.elastic_net_fit(X, y, alpha=0.1, l1_ratio=0.0, max_iter=5000)
    Xc = X - X.mean(0); yc = y - y.mean()
    n = len(y); ridge = np.linalg.solve(Xc.T @ Xc / n + 0.1*np.eye(4), Xc.T @ yc / n)
    assert np.allclose(w, ridge, atol=1e-3)
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd scripts && python -m pytest test_factor_lib.py -v`
Expected: FAIL（`ModuleNotFoundError: No module named 'factor_lib'`）

- [ ] **Step 3: Write minimal implementation**

```python
# scripts/factor_lib.py
"""轻量因子管线共享纯计算：截面排名归一 / Elastic-Net / Rank-IC / 线性分。零 IO，零外部依赖（仅 numpy）。"""
import numpy as np


def cross_sectional_rank(values):
    """1D 百分位排名 ∈[0,1]，并列取平均名次，NaN→0.5（截面中位）。单元素→0.5。"""
    v = np.asarray(values, dtype=float)
    out = np.full(v.shape, 0.5)
    mask = ~np.isnan(v)
    m = int(mask.sum())
    if m <= 1:
        return out
    x = v[mask]
    order = np.argsort(x, kind="mergesort")
    ranks = np.empty(m)
    sx = x[order]
    i = 0
    while i < m:                       # 并列取平均名次（0-based）
        j = i
        while j + 1 < m and sx[j + 1] == sx[i]:
            j += 1
        ranks[order[i:j + 1]] = (i + j) / 2.0
        i = j + 1
    out[mask] = ranks / (m - 1)        # 归一到 [0,1]
    return out


def rank_columns(X):
    """对 2D 矩阵每列做 cross_sectional_rank。"""
    X = np.asarray(X, dtype=float)
    return np.column_stack([cross_sectional_rank(X[:, j]) for j in range(X.shape[1])])


def rank_ic(scores, fwd):
    """Spearman = 两者截面排名的 Pearson 相关；<2 有效点→nan。"""
    s = np.asarray(scores, float); f = np.asarray(fwd, float)
    mask = ~(np.isnan(s) | np.isnan(f))
    if mask.sum() < 2:
        return float("nan")
    rs = cross_sectional_rank(s[mask]); rf = cross_sectional_rank(f[mask])
    if rs.std() == 0 or rf.std() == 0:
        return float("nan")
    return float(np.corrcoef(rs, rf)[0, 1])


def elastic_net_fit(X, y, alpha, l1_ratio=0.5, max_iter=1000, tol=1e-7):
    """坐标下降解 Elastic-Net（中心化，无截距）。
    min (1/2n)‖yc−Xc w‖² + alpha(l1_ratio‖w‖₁ + (1−l1_ratio)/2‖w‖₂²)。"""
    X = np.asarray(X, float); y = np.asarray(y, float)
    n, p = X.shape
    Xc = X - X.mean(0); yc = y - y.mean()
    col_ss = (Xc ** 2).sum(0)          # 每列平方和
    w = np.zeros(p)
    r = yc.copy()                      # 残差 = yc − Xc w（w=0 起）
    l1 = alpha * l1_ratio
    l2 = alpha * (1.0 - l1_ratio)
    for _ in range(max_iter):
        w_max = 0.0
        for j in range(p):
            if col_ss[j] == 0:
                continue
            rho = Xc[:, j] @ r / n + (col_ss[j] / n) * w[j]
            denom = col_ss[j] / n + l2
            new = np.sign(rho) * max(abs(rho) - l1, 0.0) / denom
            if new != w[j]:
                r += Xc[:, j] * (w[j] - new)   # 增量更新残差
                w_max = max(w_max, abs(new - w[j]))
                w[j] = new
        if w_max < tol:
            break
    return w


def linear_score(Xrank, w):
    """线性打分 = 排名矩阵 · 权重。"""
    return np.asarray(Xrank, float) @ np.asarray(w, float)
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd scripts && python -m pytest test_factor_lib.py -v`
Expected: PASS（5 passed）

- [ ] **Step 5: Commit**

```bash
git add scripts/factor_lib.py scripts/test_factor_lib.py
git commit -m "feat(factor-pipeline): shared pure compute lib (rank-norm/elastic-net/rank-ic)"
```

---

### Task 2: build_factor_matrix.py（因子矩阵导出）

**Files:**
- Create: `scripts/build_factor_matrix.py`
- Test: `scripts/test_build_factor_matrix.py`

**Interfaces:**
- Consumes: 无（读磁盘 CSV）。
- Produces: `data/factor_panel/factors.csv`，列固定顺序：
  `date,symbol,f_bm,f_npyoy,f_revyoy,f_roe,f_gm,f_mom20,f_mom120,f_rev5,f_trend60,f_atr,f_rvol,f_logamt,f_secmom,fwd_ret_5d`
  并暴露纯函数供测试：
  - `atr14(high, low, close) -> np.ndarray`（Wilder ATR，前 13 位 NaN 容忍）
  - `compute_symbol_factors(kday: pd.DataFrame, fund: pd.DataFrame, sec: pd.DataFrame|None) -> pd.DataFrame`（返回逐日全因子+fwd_ret_5d，index=date）
  - `FACTOR_COLS: list[str]`（13 因子名，固定顺序，供 train/eval 复用）

- [ ] **Step 1: Write the failing test**

```python
# scripts/test_build_factor_matrix.py
import numpy as np, pandas as pd
import build_factor_matrix as bm

def test_atr14_first_values():
    high = np.array([10.0, 11, 12]); low = np.array([9.0, 9.5, 11]); close = np.array([9.5, 10.5, 11.5])
    a = bm.atr14(high, low, close)
    assert np.isnan(a[0])                      # 首根无前收→NaN
    assert a[-1] > 0 and not np.isnan(a[-1])

def test_factor_cols_count_and_order():
    assert bm.FACTOR_COLS[0] == "f_bm" and bm.FACTOR_COLS[-1] == "f_secmom"
    assert len(bm.FACTOR_COLS) == 13

def test_compute_symbol_factors_pit_and_label():
    n = 140
    dates = pd.bdate_range("2020-01-01", periods=n).strftime("%Y-%m-%d")
    close = pd.Series(np.linspace(10, 24, n))   # 单调上行
    kday = pd.DataFrame({"time": dates, "open": close, "high": close*1.01,
                         "low": close*0.99, "close": close, "volume": 1e6, "amount": close*1e6})
    fund = pd.DataFrame({"time": [dates[0]], "roe": [12.0], "np_yoy": [30.0], "rev_yoy": [10.0],
                         "gross_margin": [40.0], "eps": [1.0], "bps": [5.0]})
    out = bm.compute_symbol_factors(kday, fund, None)
    row = out.loc[dates[130]]                   # 中段某日
    assert np.isclose(row["f_bm"], 5.0 / close.iloc[130])     # bps/close
    assert np.isclose(row["f_npyoy"], 30.0)                   # 时点财务前向填充
    assert row["f_mom20"] > 0                                 # 上行→正动量
    # 标签=未来5日收益，单调上行应为正
    assert row["fwd_ret_5d"] > 0
    assert np.isnan(out.iloc[-1]["fwd_ret_5d"])              # 末尾无未来→NaN
    assert np.isnan(row["f_secmom"])                          # sec=None→NaN
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd scripts && python -m pytest test_build_factor_matrix.py -v`
Expected: FAIL（`ModuleNotFoundError`）

- [ ] **Step 3: Write minimal implementation**

```python
# scripts/build_factor_matrix.py
"""导出周频因子面板：13 精选因子 + 未来5日收益。PIT + membership 点时掩码。
产出 data/factor_panel/factors.csv（行=(date,symbol)）。复用见 docs/.../specs/2026-06-22-...-design.md。"""
import sys; sys.stdout.reconfigure(encoding="utf-8")
import os, numpy as np, pandas as pd

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
KDAY = os.path.join(REPO, "data", "baostock", "kday")
FUND = os.path.join(REPO, "data", "fundamentals")
SEC = os.path.join(REPO, "data", "baostock", "pa_sector_merged")
ROSTER = os.path.join(REPO, "data", "baostock", "universe_baostock_day.csv")
MEMBERSHIP = os.path.join(REPO, "data", "membership_top2000.csv")
OUT_DIR = os.path.join(REPO, "data", "factor_panel")
OUT = os.path.join(OUT_DIR, "factors.csv")
HOLD = 5            # 周频持有期（交易日）
FROM, TO = "2018-01-01", "2026-06-30"

FACTOR_COLS = ["f_bm", "f_npyoy", "f_revyoy", "f_roe", "f_gm", "f_mom20",
               "f_mom120", "f_rev5", "f_trend60", "f_atr", "f_rvol", "f_logamt", "f_secmom"]


def atr14(high, low, close, n=14):
    high = np.asarray(high, float); low = np.asarray(low, float); close = np.asarray(close, float)
    prev = np.concatenate([[np.nan], close[:-1]])
    tr = np.maximum(high - low, np.maximum(np.abs(high - prev), np.abs(low - prev)))
    atr = np.full(len(tr), np.nan)
    if len(tr) > n:
        atr[n] = np.nanmean(tr[1:n + 1])                     # 首个 ATR=前 n 根 TR 均值
        for i in range(n + 1, len(tr)):
            atr[i] = (atr[i - 1] * (n - 1) + tr[i]) / n      # Wilder 平滑
    return atr


def compute_symbol_factors(kday, fund, sec):
    """逐日因子 + 未来5日收益；index=date(str)。kday 升序。fund 时点前向填充。sec 可 None。"""
    df = kday.copy().reset_index(drop=True)
    c = df["close"].astype(float); v = df["volume"].astype(float)
    out = pd.DataFrame({"date": df["time"].values})
    # 财务时点前向填充：把 fund 对齐到交易日（≤t 最近披露值）
    f = fund.sort_values("time").copy()
    fmap = pd.DataFrame({"time": df["time"].values})
    fmap = pd.merge_asof(fmap, f, on="time")                 # 需 time 升序、同为 str→改用日期键
    out["f_bm"] = fmap["bps"].values / c.values
    out["f_npyoy"] = fmap["np_yoy"].values
    out["f_revyoy"] = fmap["rev_yoy"].values
    out["f_roe"] = fmap["roe"].values
    out["f_gm"] = fmap["gross_margin"].values
    out["bps_raw"] = fmap["bps"].values
    out["f_mom20"] = (c / c.shift(20) - 1).values
    out["f_mom120"] = (c / c.shift(120) - 1).values
    out["f_rev5"] = (c / c.shift(5) - 1).values
    out["f_trend60"] = (c / c.rolling(60).mean() - 1).values
    out["f_atr"] = atr14(df["high"], df["low"], df["close"]) / c.values
    out["f_rvol"] = (v / v.rolling(20).mean()).values
    amt = (c * v).rolling(20).mean()
    out["f_logamt"] = np.log(amt.where(amt > 0)).values
    if sec is not None and len(sec):
        s = sec.rename(columns={"sec_mom20": "f_secmom"})[["time", "f_secmom"]]
        out = out.merge(s, on="date", how="left", left_on="date", right_on="time").drop(columns=["time"], errors="ignore")
    else:
        out["f_secmom"] = np.nan
    out["fwd_ret_5d"] = (c.shift(-HOLD) / c - 1).values
    return out.set_index("date")
```

> 注：`merge_asof` 需日期可比较。实现时把 `time` 列统一转 `pd.to_datetime` 后 asof，再格式化回 `%Y-%m-%d` 字符串作 index（见 Step 3b）。Step 1 测试用 bdate 字符串升序，确保 asof 工作。

- [ ] **Step 3b: 修正 merge_asof 日期键 + 主流程（追加到同文件）**

```python
# —— compute_symbol_factors 内：把 asof 改为日期键（替换上面 fmap 两行）——
#   fmap = pd.DataFrame({"time": pd.to_datetime(df["time"])})
#   f2 = f.assign(time=pd.to_datetime(f["time"]))
#   fmap = pd.merge_asof(fmap, f2, on="time")
# 其余引用 fmap["bps"] 等不变；out["date"] 仍用原 df["time"]（字符串）。
# sec 合并改：out 暂存字符串 date 列做 key。

def _weekly_dates(all_dates):
    """全交易日并集升序，每 HOLD 个取一个调仓日。"""
    ds = sorted(set(all_dates))
    return ds[::HOLD]

def main():
    os.makedirs(OUT_DIR, exist_ok=True)
    roster = pd.read_csv(ROSTER)["symbol"].tolist()
    mem = pd.read_csv(MEMBERSHIP)                      # date,symbol（月末快照）
    mem_dates = sorted(mem["date"].unique())
    def members_at(d):                                # ≤d 最近快照的成分集
        i = np.searchsorted(mem_dates, d, side="right") - 1
        if i < 0: return set()
        return set(mem[mem["date"] == mem_dates[i]]["symbol"])
    frames, all_dates = {}, set()
    for sym in roster:
        kp = os.path.join(KDAY, f"{sym}.csv"); fp = os.path.join(FUND, f"{sym}.csv")
        if not (os.path.exists(kp) and os.path.exists(fp)): continue
        kday = pd.read_csv(kp); fund = pd.read_csv(fp)
        kday = kday[(kday["time"] >= FROM) & (kday["time"] <= TO)]
        if len(kday) < 130: continue
        sp = os.path.join(SEC, f"{sym}.csv")
        sec = pd.read_csv(sp)[["time", "sec_mom20"]].rename(columns={"time": "date"}) if os.path.exists(sp) else None
        fac = compute_symbol_factors(kday, fund, sec)
        frames[sym] = fac; all_dates.update(fac.index)
    rebs = _weekly_dates(all_dates)
    rows = []
    for sym, fac in frames.items():
        idx = fac.index.intersection(rebs)
        sub = fac.loc[idx, FACTOR_COLS + ["fwd_ret_5d"]].copy()
        sub.insert(0, "symbol", sym); sub.insert(0, "date", sub.index)
        # membership 点时掩码：仅保留该日属当期成分
        sub = sub[[s in members_at(d) for d, s in zip(sub["date"], sub["symbol"])]]
        rows.append(sub)
    panel = pd.concat(rows, ignore_index=True).sort_values(["date", "symbol"])
    panel.to_csv(OUT, index=False, encoding="utf-8")
    print(f"wrote {len(panel)} rows x {len(FACTOR_COLS)} factors -> {OUT}  (dates {panel['date'].min()}..{panel['date'].max()})")

if __name__ == "__main__":
    main()
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd scripts && python -m pytest test_build_factor_matrix.py -v`
Expected: PASS（3 passed）。修 Step 3 的 asof 后再跑确认。

- [ ] **Step 5: 生成真实面板（联网无关，纯本地计算）**

Run: `cd "E:/rust-app/rquant" && python scripts/build_factor_matrix.py`
Expected: 打印 `wrote N rows ...`，N 约 30–40 万；`data/factor_panel/factors.csv` 生成。

- [ ] **Step 6: Commit**

```bash
git add scripts/build_factor_matrix.py scripts/test_build_factor_matrix.py
git commit -m "feat(factor-pipeline): weekly factor matrix export (13 factors, PIT, membership)"
```

---

### Task 3: train_factor_weights.py（训练 w）

**Files:**
- Create: `scripts/train_factor_weights.py`
- Test: `scripts/test_train_factor_weights.py`

**Interfaces:**
- Consumes: `factor_lib`（`rank_columns`/`elastic_net_fit`/`rank_ic`）、`build_factor_matrix.FACTOR_COLS`、面板 `data/factor_panel/factors.csv`。
- Produces: `data/factor_panel/weights.json`（`{"weights": {factor: w}, "alpha": a, "l1_ratio": 0.5, "factor_ic_train": {...}, "factor_ic_oos": {...}}`）。纯函数：
  - `build_xy(panel: pd.DataFrame, date_lo, date_hi) -> (X, y, dates)`：窗内逐日截面排名归一后纵向堆叠（X=因子排名，y=fwd_ret_5d 的截面排名）。
  - `select_alpha(panel) -> float`：内层时间切（拟合 2018-01-02..2022-12-30 / 验证 2023）选验证 Rank-IC 最高的 alpha∈{0.001,0.003,0.01,0.03,0.1}。

- [ ] **Step 1: Write the failing test**

```python
# scripts/test_train_factor_weights.py
import numpy as np, pandas as pd
import train_factor_weights as tw
from build_factor_matrix import FACTOR_COLS

def _toy_panel():
    rng = np.random.default_rng(0); rows = []
    for d in pd.bdate_range("2018-01-02", "2023-06-30", freq="5B").strftime("%Y-%m-%d"):
        for s in range(50):
            x = rng.normal(size=len(FACTOR_COLS))
            fwd = 0.8 * x[0] - 0.5 * x[3] + rng.normal(scale=0.3)   # f_bm 正、f_roe 负
            rows.append([d, f"s{s}", *x, fwd])
    return pd.DataFrame(rows, columns=["date", "symbol", *FACTOR_COLS, "fwd_ret_5d"])

def test_build_xy_shapes_and_rank_range():
    X, y, dates = tw.build_xy(_toy_panel(), "2018-01-01", "2023-12-31")
    assert X.shape[1] == len(FACTOR_COLS)
    assert X.min() >= 0 and X.max() <= 1                  # 排名归一∈[0,1]
    assert len(y) == X.shape[0]

def test_train_learns_expected_signs():
    panel = _toy_panel()
    X, y, _ = tw.build_xy(panel, "2018-01-01", "2023-12-31")
    import factor_lib as fl
    w = fl.elastic_net_fit(X, y, alpha=0.001, l1_ratio=0.5)
    assert w[0] > 0           # f_bm 正贡献
    assert w[3] < 0           # f_roe 负贡献（构造如此）
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd scripts && python -m pytest test_train_factor_weights.py -v`
Expected: FAIL（`ModuleNotFoundError`）

- [ ] **Step 3: Write minimal implementation**

```python
# scripts/train_factor_weights.py
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
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd scripts && python -m pytest test_train_factor_weights.py -v`
Expected: PASS（2 passed）

- [ ] **Step 5: 训练真实 w**

Run: `cd "E:/rust-app/rquant" && python scripts/train_factor_weights.py`
Expected: 打印 alpha 扫描 + 权重表 + `weights.json` 生成（OOS 期不参与拟合）。

- [ ] **Step 6: Commit**

```bash
git add scripts/train_factor_weights.py scripts/test_train_factor_weights.py
git commit -m "feat(factor-pipeline): train w via cross-sectional rank-IC elastic-net (anchored train, inner alpha)"
```

---

### Task 4: eval_linear_score.py（Python 周频回测器 + §5.3 裁决）

**Files:**
- Create: `scripts/eval_linear_score.py`
- Test: `scripts/test_eval_linear_score.py`

**Interfaces:**
- Consumes: `factor_lib`、`build_factor_matrix.FACTOR_COLS`、`iterate`（`load_index/to_index_relative/break_even/regime_excess/detect_sign_flip/judge`）、面板、`weights.json`、`st_symbols.csv`、`csi300.csv`。
- Produces: 纯函数 + 报告：
  - `backtest(panel, w, top_n, cost_bps, st_set) -> dict`：周频回测，返回 report-dict（`holdings=[{t,nav}]`、`regime_slices=[{label,from,to}]`、`risk={sharpe}`、`total_return/max_drawdown/turnover/n_rebalances`）。
  - `eval_weights(panel, w, label, st_set) -> dict`：跑 gross+net → `to_index_relative` 算超额 → `judge` 出裁决。

- [ ] **Step 1: Write the failing test**

```python
# scripts/test_eval_linear_score.py
import numpy as np, pandas as pd
import eval_linear_score as ev
from build_factor_matrix import FACTOR_COLS

def _panel_two_dates():
    rows = []
    # 两个调仓日，每日 4 只票；f_bm 越大未来收益越高（单因子可分）
    for d, base in [("2024-01-02", 0.10), ("2024-01-09", 0.05)]:
        for s, bm in enumerate([0.1, 0.2, 0.3, 0.4]):
            x = [0.0]*len(FACTOR_COLS); x[0] = bm                       # 仅 f_bm 变化
            fwd = bm + base                                             # 越便宜未来越高
            rows.append([d, f"s{s}", *x, fwd])
    p = pd.DataFrame(rows, columns=["date","symbol",*FACTOR_COLS,"fwd_ret_5d"])
    p["f_roe"] = 10.0; p["f_logamt"] = 20.0                            # 过硬闸：roe>0、流动性高
    return p

def test_backtest_top1_picks_highest_score():
    p = _panel_two_dates()
    w = np.zeros(len(FACTOR_COLS)); w[0] = 1.0                          # 只用 f_bm
    rep = ev.backtest(p, w, top_n=1, cost_bps=0.0, st_set=set())
    # top-1 每期选 f_bm 最大(s3)，收益=其 fwd；两期复利
    navs = [h["nav"] for h in rep["holdings"]]
    assert navs[-1] > 1.0
    assert rep["n_rebalances"] == 2

def test_zero_cost_gross_ge_net():
    p = _panel_two_dates(); w = np.zeros(len(FACTOR_COLS)); w[0] = 1.0
    g = ev.backtest(p, w, 2, 0.0, set()); n = ev.backtest(p, w, 2, 20.0, set())
    assert g["total_return"] >= n["total_return"] - 1e-9

def test_st_excluded():
    p = _panel_two_dates(); w = np.zeros(len(FACTOR_COLS)); w[0] = 1.0
    rep = ev.backtest(p, w, 1, 0.0, st_set={"s3"})                      # 剔最高分 s3
    # s3 被剔 → top-1 应回补 s2，不应出现 s3
    assert all("s3" not in h.get("picks", []) for h in rep["holdings"])
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd scripts && python -m pytest test_eval_linear_score.py -v`
Expected: FAIL（`ModuleNotFoundError`）

- [ ] **Step 3: Write minimal implementation**

```python
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
        nav *= (1.0 + ret - cost_bps / 1e4 * turn)
        navs.append({"t": d, "nav": nav, "picks": list(cur)})
        prev = cur
    total = navs[-1]["nav"] - 1.0 if navs else 0.0
    peak = -1e9; mdd = 0.0
    for h in navs:
        peak = max(peak, h["nav"]); mdd = max(mdd, 1 - h["nav"] / peak)
    rets = np.diff([1.0] + [h["nav"] for h in navs])
    sharpe = float(np.mean(rets) / np.std(rets) * np.sqrt(48)) if len(rets) > 1 and np.std(rets) > 0 else None
    turns = []  # 单边换手/调仓（近似：用 nav 序列重算太繁，下方 eval 用 to_index_relative 不需要；保留占位）
    return {"holdings": navs, "regime_slices": [{"label": L, "from": a, "to": b} for L, a, b in [TRAIN, OOS]],
            "risk": {"sharpe": sharpe}, "total_return": total, "max_drawdown": mdd,
            "turnover": 0.0, "n_rebalances": len(navs), "excess_return": 0.0}


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
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd scripts && python -m pytest test_eval_linear_score.py -v`
Expected: PASS（3 passed）

- [ ] **Step 5: Commit**

```bash
git add scripts/eval_linear_score.py scripts/test_eval_linear_score.py
git commit -m "feat(factor-pipeline): python weekly backtester + §5.3 verdict (learned vs equal)"
```

---

### Task 5: Rust 对账 + 跑全管线 + findings

**Files:**
- Create: `docs/superpowers/2026-06-22-linear-factor-pipeline-findings.md`
- 复用: 前 4 任务脚本 + `target/release/rquant.exe`

**Interfaces:**
- Consumes: `eval_linear_score.backtest`（等权单因子档）、Rust `rquant screen --backtest`。

- [ ] **Step 1: Rust 对账基准（单因子价值，周频 top-3）**

Run（PowerShell，data/ 可见）：
```
target\release\rquant.exe screen --backtest --config deploy\value_pb_deploy_tree_frozen.yaml `
  --universe data\baostock\universe_baostock_day.csv --membership data\membership_top2000.csv `
  --from 2018-01-01 --to 2026-06-30 --rebalance 5 --top 3 --cost-bps 20 --out tmps\xcheck_value.json
```
记录其 OOS 段净超额（从 report 的 holdings+regime 用 iterate 口径，或直接 total_return 做粗对账）。

- [ ] **Step 2: Python 对账（同口径单因子价值）**

写一次性片段（或 `eval_linear_score` 加 `--xcheck` 分支）：`w` 仅 `f_bm=1`，`backtest(panel, w, 3, 20, st_set=空)`，与 Step 1 比 OOS 净超额方向同号、量级相近（容差 |Δ|<0.3 绝对 或 30% 相对）。
Expected：方向一致、量级相近即通过（时间/权重变换约定差异允许小偏离）。**对不上→先修 backtest 再继续。**

- [ ] **Step 3: 跑全管线**

Run:
```
python scripts/build_factor_matrix.py
python scripts/train_factor_weights.py
python scripts/eval_linear_score.py
```
Expected：打印学习 vs 等权裁决 + "✅/❌"。

- [ ] **Step 4: 写 findings**

把权重表、各因子 train/OOS IC、学习 vs 等权（net-OOS/Sharpe/be/换手）、Rust 对账结果、裁决与诚实边界写入 `docs/superpowers/2026-06-22-linear-factor-pipeline-findings.md`。无论学习是否超过等权都如实记。

- [ ] **Step 5: Commit**

```bash
git add scripts/eval_linear_score.py docs/superpowers/2026-06-22-linear-factor-pipeline-findings.md
git commit -m "feat(factor-pipeline): rust cross-check + full run + findings"
```

---

## Self-Review（已过）

- **Spec 覆盖**：① build(T2) ② train(T3) ③ eval(T4) ⑤ Rust 对账(T5) ⑥ 文件/测试(各任务) — 全覆盖；§5.3 闸=复用 iterate.judge(T4)；零依赖=仅 numpy/pandas(全任务)。
- **占位扫描**：无 TBD；`backtest` 的 `turnover` 字段置 0（`to_index_relative` 不读它，§5.3 裁决不依赖换手；findings 里如需展示换手可在 T4 加单边换手累计，非裁决项）——已显式注明，非占位。
- **类型一致**：`FACTOR_COLS` 单一来源(build) 全程复用；`backtest`→report-dict 字段对齐 `to_index_relative`/`judge` 所读键（holdings/regime_slices/risk/excess_return）；`weights.json` 形态 train 产/eval 消费一致。
- **已知偏差（非缺陷，findings 说明）**：Python 回测器收益用 close-to-close 5 日、引擎用 t+1 开盘 N 根 → 与 Rust 非 bit-exact，故对账用容差而非等值；等权基线在 Python 用 f_bm+f_npyoy 排名均值，与引擎"变换权重均值"略不同，故对账主用单因子（排名序与变换序一致）。

## Global 提醒（实现期纪律）

- 每任务 5 步：写失败测试 → 跑挂 → 实现 → 跑过 → 提交。
- 跑数据脚本用真实 `data/`（Bash 工具沙箱化，须用 PowerShell 跑 build/train/eval 与 Rust 对账）。
- pytest 单测在 `scripts/` 下跑（纯计算，不碰 data/）。
