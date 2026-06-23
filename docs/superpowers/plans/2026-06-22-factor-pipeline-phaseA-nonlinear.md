# 因子管线 Phase A：非线性 + 成本感知 + 宽池 + 多折WFO 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development. Steps use `- [ ]`.

**Goal:** 在已合并的线性因子管线上，测试「非线性特征 + 成本感知选股 + 更宽票池 + 多折 WFO」能否让学习权重在周频 top-3 上超过等权基线——纯 Python、零新依赖。

**Architecture:** 复用 `scripts/{factor_lib,build_factor_matrix,train_factor_weights,eval_linear_score}.py`（master 已有，作为样板）。新增：`factor_lib.expand_features`（非单调+交互）、`build_factor_matrix` 的 `--no-membership` 宽池、`train_nonlinear.py`（多折WFO+特征扩展+α/δ选择）、`eval_nonlinear.py`（迟滞回测+逐折§5.3+对照+findings）。

**Tech Stack:** Python 3.13 + numpy + pandas（**零新依赖**；禁 sklearn/scipy/lightgbm——lightgbm 留给 Phase B）。

## Global Constraints

- 零新 Python 依赖（numpy/pandas/stdlib + 本地模块）。
- 不改 Rust 引擎/桌面；不动已合并的 4 个线性脚本（只新增/扩展 factor_lib 与 build_factor_matrix，新建 2 个脚本）。
- **无前视（关键）**：特征扩展是当期截面纯变换；交互对的选择、α、δ 一律只用**该折的 train**（含内层验证），**OOS 折绝不参与任何选择**。标签=未来5日收益。
- 口径：周频 reb5、top-3、vs 沪深300、成本 20bps。
- **宽池 caveat**：`--no-membership` 用全 roster（survivor 并集，重引入幸存者偏差）——仅用于"更多截面样本"的过拟合测试；学习 vs 等权**同池对照**故相对裁决仍有效，绝对值偏乐观，findings 必须注明。
- §5.3 闸沿用 `iterate.py`（gross>0 ∧ net-OOS>0 ∧ net-sharpe>0 ∧ be≥40bps）；多折下逐折判 + 聚合（正折占比/均值OOS）。
- 编码 utf-8；git add 显式文件，不 `-A`；commit 英文 + `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`；不 push。

---

### Task A1: factor_lib.expand_features（非线性特征）

**Files:** Modify `scripts/factor_lib.py`; Test `scripts/test_factor_lib.py`（追加）

**Interfaces — Produces:**
- `expand_features(Xrank: np.ndarray, interaction_pairs: list[tuple[int,int]]) -> np.ndarray`：输入 N×p 的排名归一矩阵(∈[0,1])，输出 N×(p + p + k)：原始 p 列 + 每列 `(x-0.5)**2`（非单调，p 列）+ 每个 `(i,j)` 对的 `Xrank[:,i]*Xrank[:,j]`（k 列）。顺序固定：[base..., sq..., inter...]。

- [ ] **Step 1: 失败测试**
```python
def test_expand_features_shape_and_values():
    import numpy as np, factor_lib as fl
    X = np.array([[0.0, 1.0], [0.5, 0.5], [1.0, 0.0]])
    out = fl.expand_features(X, [(0, 1)])
    assert out.shape == (3, 2 + 2 + 1)              # base2 + sq2 + inter1
    assert np.allclose(out[:, :2], X)               # 前 p 列=原始
    assert np.allclose(out[:, 2], (X[:, 0]-0.5)**2) # 非单调列
    assert np.allclose(out[:, 4], X[:, 0]*X[:, 1])  # 交互列
```
- [ ] **Step 2: 跑挂** `cd scripts && python -m pytest test_factor_lib.py::test_expand_features_shape_and_values -v` → FAIL
- [ ] **Step 3: 实现**（追加到 factor_lib.py）
```python
def expand_features(Xrank, interaction_pairs):
    """N×p 排名矩阵 → [原始 | (x-0.5)² | 成对乘积]。interaction_pairs=[(i,j),...]（列索引）。"""
    X = np.asarray(Xrank, float)
    sq = (X - 0.5) ** 2
    inter = (np.column_stack([X[:, i] * X[:, j] for (i, j) in interaction_pairs])
             if interaction_pairs else np.empty((X.shape[0], 0)))
    return np.column_stack([X, sq, inter])
```
- [ ] **Step 4: 跑过** 同命令 → PASS
- [ ] **Step 5: 提交** `git add scripts/factor_lib.py scripts/test_factor_lib.py && git commit`（msg: `feat(factor-pipeline): expand_features (non-monotone + interactions)`）

---

### Task A2: build_factor_matrix --no-membership 宽池

**Files:** Modify `scripts/build_factor_matrix.py`; Test `scripts/test_build_factor_matrix.py`（追加）

**Interfaces:** `main` 接受 `apply_membership: bool=True` 与 `out_path`；False 时跳过 membership 掩码、写 `data/factor_panel/factors_full.csv`。抽出纯函数 `mask_by_membership(panel_df, members_at) -> df` 便于测试。CLI：`python build_factor_matrix.py --no-membership` → factors_full.csv。

- [ ] **Step 1: 失败测试**（测纯掩码函数）
```python
def test_mask_by_membership_filters_non_members():
    import pandas as pd, build_factor_matrix as bm
    panel = pd.DataFrame({"date": ["2020-01-10","2020-01-10"], "symbol": ["A","B"]})
    members_at = lambda d: {"A"}              # 仅 A 是成员
    out = bm.mask_by_membership(panel, members_at)
    assert list(out["symbol"]) == ["A"]
```
- [ ] **Step 2: 跑挂** → FAIL
- [ ] **Step 3: 实现** 抽 `mask_by_membership(panel, members_at)`（`panel[[s in members_at(d) for d,s in zip(panel.date,panel.symbol)]]`）；`main(apply_membership=True, out_path=OUT)` 在 True 时调它、False 跳过；`argparse --no-membership` 设 `apply_membership=False, out_path=OUT.replace("factors.csv","factors_full.csv")`。
- [ ] **Step 4: 跑过** `cd scripts && python -m pytest test_build_factor_matrix.py -v` → PASS（含原有3+新1）
- [ ] **Step 5: 提交** `feat(factor-pipeline): --no-membership wide-universe panel`

---

### Task A3: train_nonlinear.py（多折 WFO + 特征扩展 + α/δ）

**Files:** Create `scripts/train_nonlinear.py`; Test `scripts/test_train_nonlinear.py`
**参考样板（master 已有，读它们抄边界处理）：** `scripts/train_factor_weights.py`（build_xy/rank/elastic-net/α内层切）。

**Interfaces — Produces:**
- `WFO_FOLDS`: 锚定扩展折列表 `[(train_lo,train_hi,oos_lo,oos_hi),...]`：train 2018-01-02..{2021-12-31,2022-12-31,2023-12-31,2024-12-31}，对应 OOS {2022,2023,2024,2025-含2026H1} 全年。共 4 折。
- `select_interactions(panel, cols, train_lo, train_hi, k=5) -> list[tuple]`：按 train 窗单因子 |Rank-IC| 取最强 k 个因子的两两组合（C(k,2) 对）。
- `train_fold(panel, fold) -> dict`：该折 train 上 expand_features→排名→Elastic-Net（α 内层切，同 train_factor_weights 套路），返回 `{weights, alpha, interaction_pairs, feat_names}`。
- `main()` → `data/factor_panel/weights_nonlinear.json`：`{"folds":[{fold边界, weights, alpha, interaction_pairs}...]}`。

- [ ] **Step 1: 失败测试**（合成面板，验证无前视 + 学到非线性）
```python
def test_select_interactions_uses_only_train():
    import numpy as np, pandas as pd, train_nonlinear as tn
    from build_factor_matrix import FACTOR_COLS
    # 合成：train 段 f_bm 与未来强相关；OOS 段全反向。若选择只用 train，应选出含 f_bm 的对。
    rng=np.random.default_rng(0); rows=[]
    for d in pd.bdate_range("2018-01-02","2021-12-31",freq="5B").strftime("%Y-%m-%d"):
        for s in range(40):
            x=rng.normal(size=len(FACTOR_COLS)); fwd=1.5*x[0]+rng.normal(scale=.3)
            rows.append([d,f"s{s}",*x,fwd])
    p=pd.DataFrame(rows,columns=["date","symbol",*FACTOR_COLS,"fwd_ret_5d"])
    pairs=tn.select_interactions(p,FACTOR_COLS,"2018-01-02","2021-12-31",k=5)
    assert any(0 in pair for pair in pairs)        # f_bm(索引0) 应入选最强 5
```
- [ ] **Step 2: 跑挂** → FAIL
- [ ] **Step 3: 实现** 读 train_factor_weights.py 的 build_xy/rank/elastic-net 模式；新增 expand_features 接入（train 列用 `select_interactions` 选对，传 `fl.expand_features`）；WFO_FOLDS 锚定扩展；α 内层切（每折 train 内再切末年验证）；写 weights_nonlinear.json。**OOS 边界只用于划分，不进任何拟合/选择。**
- [ ] **Step 4: 跑过** `cd scripts && python -m pytest test_train_nonlinear.py -v` → PASS
- [ ] **Step 5: 提交** `feat(factor-pipeline): nonlinear WFO training (expand + interactions + per-fold alpha)`

---

### Task A4: eval_nonlinear.py（迟滞回测 + 逐折§5.3 + 对照 + 跑 + findings）

**Files:** Create `scripts/eval_nonlinear.py`; Test `scripts/test_eval_nonlinear.py`; findings `docs/superpowers/2026-06-22-nonlinear-phaseA-findings.md`
**参考样板：** `scripts/eval_linear_score.py`（backtest/_eligible/eval_weights/iterate 复用）。

**Interfaces:**
- `backtest_hysteresis(panel, w, expand_fn, top_n, cost_bps, st_set, delta) -> report`：同 eval_linear_score.backtest，但①打分前 `expand_features`②**迟滞**：上期持仓 symbol 的分数加 `delta` 优势再排序选 top-N（delta=0 等价无迟滞），降换手。
- `select_delta(panel, fold, w, expand_fn, st_set) -> float`：在该折 **train** 上网格 δ∈{0,0.02,0.05,0.1} 选**净** train 超额最高者（不看 OOS）。
- `main()`：对 membership 面板 + full 面板各跑；逐折用对应 weights_nonlinear.json 的 w + 选定 δ → 逐折 OOS §5.3；与**等权基线**(f_bm=f_npyoy=1,无扩展,δ=0)逐折对照；聚合(均值OOS/正折占比)；写 findings（含宽池 survivor caveat、逐折表、最终裁决：非线性是否过闸且超等权）。

- [ ] **Step 1: 失败测试**（迟滞降换手）
```python
def test_hysteresis_reduces_turnover():
    import numpy as np, pandas as pd, eval_nonlinear as en
    from build_factor_matrix import FACTOR_COLS
    # 两期，分数让 δ=0 时每期换不同票、δ大时保持
    rows=[]
    for d,bump in [("2024-01-02",0.0),("2024-01-09",0.01)]:
        for s,b in enumerate([0.40,0.41,0.30]):    # s0/s1 分数接近、轮流领先
            x=[0.0]*len(FACTOR_COLS); x[0]=b+(bump if s==1 else 0)
            rows.append([d,f"s{s}",*x,0.05])
    p=pd.DataFrame(rows,columns=["date","symbol",*FACTOR_COLS,"fwd_ret_5d"]); p["f_roe"]=10; p["f_logamt"]=20
    w=np.zeros(len(FACTOR_COLS)); w[0]=1.0; ident=lambda X,_p=[]:X
    t0=en.backtest_hysteresis(p,w,lambda X:X,1,0.0,set(),delta=0.0)["turnover"]
    t1=en.backtest_hysteresis(p,w,lambda X:X,1,0.0,set(),delta=0.5)["turnover"]
    assert t1 <= t0
```
- [ ] **Step 2: 跑挂** → FAIL
- [ ] **Step 3: 实现** 读 eval_linear_score.py；加 expand_fn 调用 + 迟滞逻辑 + select_delta + 逐折聚合 + 双面板。复用 iterate 的 to_index_relative/judge/break_even。
- [ ] **Step 4: 跑过** `cd scripts && python -m pytest test_eval_nonlinear.py -v` → PASS
- [ ] **Step 5: 跑真数据 + findings**（PowerShell，data/ 可见）：`python scripts/build_factor_matrix.py --no-membership`（产 factors_full.csv）；`python scripts/train_nonlinear.py`；`python scripts/eval_nonlinear.py`；把逐折表/对照/裁决/caveat 写 findings。
- [ ] **Step 6: 提交** `feat(factor-pipeline): nonlinear eval (hysteresis + multi-fold WFO) + findings`

---

## Self-Review
- 覆盖设计：非单调+交互(A1)、宽池(A2)、WFO+α/δ无前视(A3)、迟滞+逐折§5.3+对照+findings(A4)。✓
- 无前视：A3/A4 的 interactions/α/δ 均限 train；OOS 仅评估。✓
- 类型一致：expand_features 签名 A1 定义、A3/A4 消费一致；weights_nonlinear.json 形态 A3 产 A4 消费。✓
- 占位：测试均含真实断言；run 步骤为 Task A4 Step5（数据脚本走 PowerShell，Bash 沙箱不见 data/）。✓
- 诚实预期写入 findings：非线性大概率仍受限于信号天花板；宽池 survivor 偏差仅相对裁决有效。
