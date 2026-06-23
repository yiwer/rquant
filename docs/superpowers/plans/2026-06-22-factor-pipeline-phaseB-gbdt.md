# 因子管线 Phase B：LightGBM GBDT 集成 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development. Steps `- [ ]`.

**Goal:** 用 LightGBM 梯度提升树（非线性、自动交互）+ 多随机种子集成，测试能否在周频 top-3 上超过等权基线 / Phase A 非线性——同 WFO/迟滞/§5.3/双池纪律。

**Architecture:** 复用 Phase A 的因子面板（`data/factor_panel/factors.csv` 含 membership PIT、`factors_full.csv` 宽池）+ `factor_lib.rank_columns` + `train_nonlinear.WFO_FOLDS` + `eval_nonlinear._eligible` + `iterate` 指标。GBDT 用 13 个 rank 归一基础因子作特征（树自动找非单调+交互，**无需 expand_features**），目标=未来5日收益的**截面排名**。

**Tech Stack:** Python 3.13 + numpy + pandas + **lightgbm 4.6.0（本阶段新增依赖，已装）**。

## Global Constraints
- 依赖：本阶段允许 lightgbm（已 `pip install lightgbm`==4.6.0）；不再加别的。不改 Rust/桌面/已合并脚本，只新建 `train_gbdt.py`/`eval_gbdt.py`。
- **无前视（关键）**：GBDT 训练、早停内层验证、迟滞 δ 一律只用该折 train；OOS 仅评估。
- **可复现**：LightGBM 用 `deterministic=True, force_row_wise=True, num_threads=1, seed=s`；集成种子 `[0,1,2,3,4]`，预测取均值。测试靠此可复现。
- **强正则（弱信号防过拟合）**：浅树（num_leaves≤31, max_depth≤5）、大 min_child_samples、feature/bagging_fraction<1、lambda_l1/l2>0、learning_rate 小、n_estimators 适中 + 内层早停。**不网格搜超参**（避免元过拟合），用固定正则化默认。
- 口径：周频 reb5、top-3、vs 沪深300、成本 20bps、WFO 4 折（同 Phase A）。
- §5.3 沿用 iterate.judge；逐折判 + 聚合（均值 OOS / 正折占比）。
- 编码 utf-8；git add 显式；commit 英文 + `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`；不 push。

---

### Task B1: train_gbdt.py（GBDT 多种子集成 WFO 训练）

**Files:** Create `scripts/train_gbdt.py`; Test `scripts/test_train_gbdt.py`
**参考（committed，读不改）：** `scripts/train_nonlinear.py`（WFO_FOLDS、fold 边界、内层切、rank）、`scripts/factor_lib.py`（rank_columns）、`scripts/build_factor_matrix.py`（FACTOR_COLS, OUT）。

**Interfaces — Produces:**
- `GBDT_PARAMS`: dict 正则化 LightGBM 参数（`objective="regression"`, `num_leaves=31`, `max_depth=5`, `learning_rate=0.03`, `min_child_samples=200`, `feature_fraction=0.7`, `bagging_fraction=0.7`, `bagging_freq=1`, `lambda_l1=1.0`, `lambda_l2=1.0`, `n_estimators=300`, `deterministic=True`, `force_row_wise=True`, `num_threads=1`, `verbose=-1`）。
- `ENSEMBLE_SEEDS = [0, 1, 2, 3, 4]`。
- `build_xy_gbdt(panel, date_lo, date_hi) -> (X, y)`：窗内逐日 `rank_columns(FACTOR_COLS)` 作 X、`cross_sectional_rank(fwd_ret_5d)` 作 y，纵向堆叠（dropna fwd）。
- `train_fold_gbdt(panel, fold) -> list`：fold-train 上，对每个 seed 训一个 LightGBM（用内层末年早停），返回 K 个 booster。**OOS 不参与。**
- `ensemble_predict(models, Xrank) -> np.ndarray`：K 个 predict 取均值。
- `main()`：每折训练，存 `data/factor_panel/gbdt_models/fold{i}_seed{s}.txt`（booster.save_model）+ 一个 `gbdt_meta.json`（折边界）。

- [ ] **Step 1: 失败测试**
```python
# scripts/test_train_gbdt.py
import numpy as np, pandas as pd
import train_gbdt as tg
from build_factor_matrix import FACTOR_COLS

def _toy(seed=0, extra_oos=False):
    rng=np.random.default_rng(seed); rows=[]
    for d in pd.bdate_range("2018-01-02","2021-12-31",freq="5B").strftime("%Y-%m-%d"):
        for s in range(60):
            x=rng.normal(size=len(FACTOR_COLS)); fwd=1.2*x[0]-0.8*x[3]+rng.normal(scale=.5)
            rows.append([d,f"s{s}",*x,fwd])
    if extra_oos:  # 2022 OOS：反向极端信号，若被读到会改变模型
        for d in pd.bdate_range("2022-01-03","2022-12-31",freq="5B").strftime("%Y-%m-%d"):
            for s in range(60):
                x=rng.normal(size=len(FACTOR_COLS)); fwd=-9*x[0]+rng.normal(scale=.1)
                rows.append([d,f"s{s}",*x,fwd])
    return pd.DataFrame(rows,columns=["date","symbol",*FACTOR_COLS,"fwd_ret_5d"])

FOLD=("2018-01-02","2021-12-31","2022-01-03","2022-12-31")

def test_ensemble_size_and_predict_shape():
    models=tg.train_fold_gbdt(_toy(), FOLD)
    assert len(models)==len(tg.ENSEMBLE_SEEDS)
    Xt=np.random.default_rng(1).random((20,len(FACTOR_COLS)))
    assert tg.ensemble_predict(models, Xt).shape==(20,)

def test_training_does_not_read_oos():
    Xt=np.random.default_rng(2).random((30,len(FACTOR_COLS)))
    p1=tg.ensemble_predict(tg.train_fold_gbdt(_toy(extra_oos=False),FOLD),Xt)
    p2=tg.ensemble_predict(tg.train_fold_gbdt(_toy(extra_oos=True ),FOLD),Xt)
    assert np.allclose(p1,p2)   # OOS 行不改变 fold 模型（train 切片 + 确定性种子）
```
- [ ] **Step 2: 跑挂** `cd scripts && python -m pytest test_train_gbdt.py -v` → FAIL
- [ ] **Step 3: 实现** 按 Interfaces；`train_fold_gbdt` 用 `lgb.train`（或 `LGBMRegressor`）在 fold-train 拟合，内层末年早停；确定性参数。OOS 边界只用于 fold 定义。
- [ ] **Step 4: 跑过** 同命令 → PASS（注意：确定性需 `num_threads=1`）
- [ ] **Step 5: 提交** `feat(factor-pipeline): GBDT multi-seed ensemble WFO training`

---

### Task B2: eval_gbdt.py（GBDT 迟滞回测 + 逐折§5.3 + 对照 + 跑 + findings）

**Files:** Create `scripts/eval_gbdt.py`; Test `scripts/test_eval_gbdt.py`; findings `docs/superpowers/2026-06-22-gbdt-phaseB-findings.md`
**参考（committed，读不改）：** `scripts/eval_nonlinear.py`（`_eligible` 硬闸、迟滞、select_delta、逐折聚合、双池、iterate.to_index_relative/judge 复用）。

**Interfaces:**
- `backtest_gbdt(panel, models, top_n, cost_bps, st_set, delta)`：同 eval_nonlinear 的迟滞回测，但打分 = `ensemble_predict(models, rank_columns(当期截面))`（不是 w·expand）。返回同形 report-dict（holdings/regime_slices/risk/total_return/max_drawdown/turnover/n_rebalances/excess_return）。
- `select_delta_gbdt(panel, fold, models, st_set)`：δ∈{0,0.02,0.05,0.1} 在 fold-train 选净 train 超额最高（不看 OOS）。
- `main()`：membership + full 两池；逐折 `models=train_gbdt.train_fold_gbdt(panel,fold)` → δ → backtest OOS → §5.3；对照**等权基线**(同 eval_nonlinear) 与（如存在）Phase A 非线性结果（读其 findings 数值或注明）；聚合；写 findings（逐折表、GBDT vs 等权 vs Phase-A、宽池 survivor caveat、诚实裁决）。

- [ ] **Step 1: 失败测试**
```python
# scripts/test_eval_gbdt.py
import numpy as np, pandas as pd
import eval_gbdt as eg
from build_factor_matrix import FACTOR_COLS
class _Stub:           # 假模型：预测=第0列（f_bm）
    def predict(self,X): return np.asarray(X)[:,0]
def _panel():
    rows=[]
    for d,b in [("2024-01-02",0.0),("2024-01-09",0.01)]:
        for s,v in enumerate([0.40,0.41,0.30]):
            x=[0.0]*len(FACTOR_COLS); x[0]=v
            rows.append([d,f"s{s}",*x,0.05])
    p=pd.DataFrame(rows,columns=["date","symbol",*FACTOR_COLS,"fwd_ret_5d"]); p["f_roe"]=10;p["f_logamt"]=20
    return p
def test_gbdt_hysteresis_reduces_turnover():
    p=_panel(); m=[_Stub()]
    t0=eg.backtest_gbdt(p,m,1,0.0,set(),delta=0.0)["turnover"]
    t1=eg.backtest_gbdt(p,m,1,0.0,set(),delta=0.5)["turnover"]
    assert t1<=t0
def test_gbdt_backtest_zero_cost_ge_net():
    p=_panel(); m=[_Stub()]
    g=eg.backtest_gbdt(p,m,2,0.0,set(),0.0)["total_return"]; n=eg.backtest_gbdt(p,m,2,20.0,set(),0.0)["total_return"]
    assert g>=n-1e-9
```
- [ ] **Step 2: 跑挂** → FAIL
- [ ] **Step 3: 实现** 读 eval_nonlinear.py，把打分换成 `ensemble_predict(models, rank)`；复用 `_eligible`/迟滞/聚合；select_delta_gbdt。
- [ ] **Step 4: 跑过** `cd scripts && python -m pytest test_eval_gbdt.py -v` → PASS
- [ ] **Step 5: 跑真数据 + findings**（PowerShell）：`python scripts/eval_gbdt.py`（内部 train+eval 双池四折，GBDT 较慢，可能 20-40min）→ 写 findings：逐折表(两池) + GBDT vs 等权 vs Phase-A非线性 + 裁决(均值/稳定性) + survivor caveat + lightgbm 依赖注明。诚实记录。
- [ ] **Step 6: 提交** `feat(factor-pipeline): GBDT eval (hysteresis + multi-fold) + findings`

---

## Self-Review
- 覆盖：GBDT 集成训练(B1, 多种子=用户"随机初始集"正解)、迟滞回测+逐折§5.3+双池对照(B2)。✓
- 无前视：B1/B2 train/δ 限 fold-train；OOS 仅评估；测试 `test_training_does_not_read_oos` 对抗 OOS 反向信号。✓
- 类型一致：train_fold_gbdt→list[booster]，ensemble_predict 消费一致；backtest_gbdt report 形对齐 iterate 消费。✓
- 占位：测试真断言；run=B2 Step5（PowerShell，data/ 沙箱）。✓
- 诚实预期写 findings：GBDT 容量更大→均值或更高、方差大概率更大；多种子集成降种子方差但非 regime 方差；稳定性墙 + 信号天花板大概率仍在。
