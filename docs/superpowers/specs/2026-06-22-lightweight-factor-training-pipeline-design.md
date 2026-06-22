# 轻量因子训练管线 设计（Lightweight Factor Training Pipeline）

> 状态：设计已确认（2026-06-22）。下一步 writing-plans。
> 目标读者：实现者（假设不了解本仓上下文）。

## 目标（一句话）

验证「**学习得到的因子权重 w（线性点积打分）能否在周频 top-3 口径上超过等权基线**」——纯 Python 研究态，**不改 Rust 引擎**。

## 背景与动机

- 已部署冠军 = `value_pb`(便宜度) + `np_yoy`(净利增速) **等权均值**，月频 top-3 + ST 过滤。等权 = 权重向量 w=(1,1,…) 的特例（未训练）。
- 用户提出：因子归一后组成高维向量 x，维护同维权重向量 w，标的评分 = w·x，w 由训练得到。本设计是该思路的**轻量首版**。
- 关键约束发现（本会话）：
  - 横截面排名归一**无法走现有 Rust 引擎**（引擎逐标的打分，看不到当日截面）。⇒ 训练与评估都在 Python 自洽。
  - Python 环境：numpy 2.3 / pandas 3.0 **有**；sklearn / scipy **缺**。⇒ ridge 用闭式解、Elastic-Net 用 numpy 坐标下降，**零新依赖**。
  - 周频(reb5)会显著削弱慢速价值 edge（等权 reb20 OOS +2.68 → reb5 +0.10，见账本 r130 vs r133）。本管线评估口径=**周频**，要打败的基线=周频等权(+0.10)。

## 全局约束（Global Constraints）

- **零新 Python 依赖**：仅用 numpy / pandas（标准库可）。不引入 sklearn / scipy。
- **不改 Rust 引擎 / 桌面 crate**：纯 `scripts/` 下 Python。
- **时点纪律（PIT，防前视）**：因子只用 ≤t 数据；财务按披露日生效（复用现有时点财务 CSV）；标签=严格未来收益；票池用 membership 点时成分去幸存者。
- **口径锁定**：周频 `reb5`（≈5 交易日）、`top-3`（主）+ `top-10`（参照）、基准=沪深300、训练/样本外切片 train 2018-01-02..2023-12-29 / OOS 2024-01-02..2026-06-12。
- **§5.3 诚实闸**：裁决沿用 `iterate.py` 口径（gross>0 ∧ net-OOS>0 ∧ net-Sharpe>0 ∧ break-even≥40bps ∧ tier2 无符号翻转）。falsification 是合法产出，禁止调参凑数。
- **编码纪律**：脚本 `sys.stdout.reconfigure(encoding="utf-8")`；CSV/JSON 显式 utf-8。

## 架构（3 段全 Python）

```
data/baostock/kday/*.csv  ─┐
data/fundamentals/*.csv    ├─▶ ① build_factor_matrix.py ─▶ data/factor_panel/factors.parquet
data/baostock/pa_sector_merged 或 sector_factors ─┘        (行=(date,symbol); 列=13因子 + fwd_ret_5d)
data/membership_top2000.csv (点时成分) ──────────────────────────┘
                                                          │
                              ② train_factor_weights.py ◀─┘
                              (截面排名归一 → Elastic-Net Rank-IC → 锚定 train 拟合 w；α 由 train 内部切验证选)
                                                          │  w（汇总 + 各因子 IC + 相关矩阵）
                                                          ▼
                              ③ eval_linear_score.py
                              (Python 回测器：线性分 → 过硬闸 → top-N → 周频持有 → §5.3 指标；
                               学习-w vs 等权-w 对照；附 Rust 对账)
                                                          │
                                                          ▼
                              docs/superpowers/2026-06-22-linear-factor-pipeline-findings.md
                              + data/factor_panel/weights.json（学到的 w 留档）
```

## 组件设计

### ① `scripts/build_factor_matrix.py` —— 因子矩阵导出

**职责**：对 membership roster 内每只票、每个周频调仓日，算 13 因子 + 未来 5 日收益，输出面板。

**13 精选因子（每族代表；全部现成可算；公式镜像已部署树/DSL；以原生方向喂入，符号由 w 学习）**

| # | 因子 | 计算（pandas，逐票时序后取调仓日） | 数据源 |
|---|---|---|---|
| 1 | 价值·账面市值比 | `bps/close`（高=便宜） | fund.bps + kday.close |
| 2 | 成长·净利增速 | `np_yoy` | fund |
| 3 | 成长·营收增速 | `rev_yoy` | fund |
| 4 | 质量·ROE | `roe` | fund |
| 5 | 质量·毛利率 | `gross_margin` | fund |
| 6 | 动量·20日 | `close/close.shift(20) - 1` | kday |
| 7 | 动量·120日 | `close/close.shift(120) - 1` | kday |
| 8 | 反转·5日 | `close/close.shift(5) - 1`（符号由 w 学，预期负） | kday |
| 9 | 趋势·均线偏离 | `close/close.rolling(60).mean() - 1` | kday |
| 10 | 波动·ATR/价 | `atr14/close`（Wilder ATR；符号由 w 学，预期负） | kday(OHLC) |
| 11 | 量能·相对成交量 | `volume/volume.rolling(20).mean()` | kday |
| 12 | 流动性·对数成交额 | `log((close*volume).rolling(20).mean())` | kday |
| 13 | 行业·板块动量 | `sec_mom20` | pa_sector_merged |

**ATR14（pandas 实现，Wilder）**：`tr = max(high-low, |high-prev_close|, |low-prev_close|)`；`atr = tr.ewm(alpha=1/14, adjust=False).mean()`。

**标签**：`fwd_ret_5d = close.shift(-5)/close - 1`（严格未来，按调仓日取）。

**调仓日**：从公共交易日历（各票日期并集，升序）每 5 个交易日取一个，覆盖 2018-01..2026-06。

**时点纪律**：因子在日期 t 只用 ≤t 行；财务列来自时点财务 CSV（已是披露生效值）；fwd_ret 用 t 之后的 close；membership 在 t 仅保留当期成分（`effective_at`：≤t 最近再平衡日的成分集）。

**输出**：`data/factor_panel/factors.parquet`（列：`date,symbol,f01..f13,fwd_ret_5d`）。缺失因子值留 NaN（训练/评估时按截面中位填充或排名时跳过）。

**确定性**：无随机；纯 pandas 向量化。

### ② `scripts/train_factor_weights.py` —— 训练 w

**职责**：读面板 → 截面排名归一 → 在 train 窗用 Elastic-Net 最大化 Rank-IC 拟合 w → 输出。

**硬闸（在归一/训练前按截面过滤候选）**：剔 ST（`data/baostock/st_symbols.csv`）；`sma(close*volume,20) ≥ 5e7`（流动性，可由 f12 反推或重算）；`roe > 0`。硬闸**不进** x。

**截面排名归一（每个调仓日，池内）**：每因子 → 百分位排名 ∈[0,1]（`rank(pct=True)`，并列取平均，NaN→0.5 即中位）。标签同样取 `fwd_ret_5d` 的截面百分位排名 `y_rank ∈[0,1]`（Rank-IC 目标，抗异常值）。

**Elastic-Net（numpy 坐标下降，纯实现，~40 行）**：
- 目标 `min_w (1/2N)‖y_rank − Xw‖² + α(λ‖w‖₁ + (1−λ)/2‖w‖₂²)`，X 已标准化（排名已在 [0,1]，再中心化）。
- 坐标下降软阈值更新；迭代至收敛（容差/最大轮）。`λ`(l1_ratio) 默认 0.5。
- 闭式 ridge 作为 `λ=0` 特例与单测对照（`w=(XᵀX+αI)⁻¹Xᵀy`）。

**α 选择（不看 OOS，消歧确定）**：train 窗内按**时间**再切——内层拟合 2018-01-02..2022-12-30、内层验证 2023 全年；在网格 α∈{0.001, 0.003, 0.01, 0.03, 0.1} 上各拟合，选**内层验证 Rank-IC 最高**的 α；再用**整个 train 窗**以该 α 重拟合得最终 w。l1_ratio λ 固定 0.5（不调，避免双层搜索过拟合）。

**切分（v1 锚定单切）**：train 2018-01-02..2023-12-29 → 冻结 w → OOS 2024-01-02..2026-06-12。多折 WFO = v1.1 加固（不在本 spec 范围）。

**输出**：`data/factor_panel/weights.json`（`{factor: weight}` + 选中的 α + 各因子 train/OOS 的单因子 Rank-IC + 因子相关矩阵）。打印权重表（哪些被 L1 归零）。

### ③ `scripts/eval_linear_score.py` —— Python 回测器 + 验收

**职责**：用学到的 w 与等权 w 跑同一回测器，出 §5.3 指标与对照；含 Rust 对账。

**回测循环（周频）**：对每个调仓日 t：过硬闸 → 截面排名归一（同②）→ 线性分 `score = X·w` → 取 score 最高 `top-N`(N=3 主, 10 参照) 等权持有 → 持有期收益 = 成分 `fwd_ret_5d` 均值 → 扣单边换手成本（成本 bps；换手=与上期持仓的差异）。串成净值曲线。

**指标（镜像 iterate.py 定义）**：
- gross（cost=0）/ net（cost=20bps）总收益与对沪深300 超额；
- regime 切片 train/OOS 超额（用 csi300 重算，复用 `iterate.py::to_index_relative` 同口径）；
- net 绝对 Sharpe；break-even bps；单边换手/调仓；
- tier2 敏感性：top∈{10,30,50} 的净超额符号一致性（轻量版可只跑 top 维，reb 固定 5）。

**验收双条件**：① 学习-w 过 §5.3 闸；② `net-OOS(学习-w) > net-OOS(等权-w)`（同周频 top-3 口径）。两者皆满足 = "学习权重有用"，否则诚实记证伪。

**Rust 对账（防自欺，必做）**：用**等权** value_pb+np_yoy 跑本 Python 回测器（周频 top-3 membership vs csi300），与 Rust 引擎账本 **r133（周频等权 net-OOS +0.10）** 对齐（容差，如 |Δ net-OOS|<0.1 且方向同号）。对不上**先修回测器**再信任学习-w 结果。

**输出**：`docs/superpowers/2026-06-22-linear-factor-pipeline-findings.md`（w 表 + 各因子 IC + 学习 vs 等权对照 + Rust 对账 + 裁决 + 诚实边界）。

## 数据流与接口

- ① 产 `factors.parquet`（消费：②③）。
- ② 产 `weights.json`（消费：③）。
- ③ 产 findings.md（人读）。
- 三脚本可独立运行（②③ 读 ① 的产物），便于迭代与测试。

## 文件结构

- 新建：`scripts/build_factor_matrix.py`、`scripts/train_factor_weights.py`、`scripts/eval_linear_score.py`
- 新建测试：`scripts/test_build_factor_matrix.py`、`scripts/test_train_factor_weights.py`、`scripts/test_eval_linear_score.py`
- 产物目录：`data/factor_panel/`（gitignore 下的 data/，不入库）
- findings：`docs/superpowers/2026-06-22-linear-factor-pipeline-findings.md`
- 复用（只读/调用）：`data/baostock/kday`、`data/fundamentals`、`data/baostock/pa_sector_merged`、`data/membership_top2000.csv`、`data/baostock/index/csi300.csv`、`data/baostock/st_symbols.csv`；`iterate.py` 的指标定义（可 import 复用 `to_index_relative`/`break_even`/`regime_excess`/`load_index`）。

## 测试策略（TDD，纯计算优先）

| 单元 | 测试 |
|---|---|
| 截面排名归一 | 已知小截面 → 期望分位；全 NaN→0.5；并列取平均 |
| Elastic-Net 求解 | 合成数据 `y=Xw*+噪声` → 还原 `w*`（L1 能把无关因子压到~0）；`λ=0` 等于闭式 ridge |
| Rank-IC | 单调数据 IC=1、反单调=-1、随机≈0 |
| ATR14 | 对已知 OHLC 序列手算前几个值 |
| 回测器不变量 | 零成本 gross≥net；单票 top-1 收益=该票 fwd_ret；换手=0 时净=毛 |
| 时点/前视 | 标签确为未来；因子不引用 >t 行（构造越界数据应不影响 t 的因子值） |
| 集成 | Rust 对账（等权 vs r133） |

## 风险与对策

| 风险 | 对策 |
|---|---|
| 过拟合（可训练 w 自由度大） | L1/L2 正则；α 不看 OOS；单一诚实 OOS；必须超基线；§5.3 闸 |
| 共线性（价值/动量族互相关） | L2 稳定 + L1 选择 + 报告相关矩阵 |
| 前视/泄露 | PIT 三纪律；标签严格未来；membership 点时 |
| Python 回测器有 bug | Rust 对账（必过）才信任后续 |
| 公式与引擎漂移 | 因子公式镜像部署树；对账兜底 |
| 幸存者偏差 | membership roster（与冠军同口径，已知 caveat：并集仍部分残留） |
| 周频换手吃 edge | 已知（基线 +0.10）；正是本实验要看 w 能否抵消 |

## 范围外（YAGNI / 后续）

- 多折走步 WFO（v1 单切已够验证假设）。
- 接回 Rust 引擎做线性打分器 / 部署（属"重量级因子平台"，另立项）。
- 扩广集 ~25-30 因子（两步走第二步，本管线跑通后再扩）。
- 🔴 需新拉的因子（现金流/北向/分析师等）——新数据工程，不在此。

## 成功标准

1. 三脚本可独立跑通，单测全绿，Rust 对账通过。
2. 产出 weights.json + findings.md，给出学习-w vs 等权 的 §5.3 对照与明确裁决。
3. 诚实结论：无论"学习权重超过等权"成立与否，都如实记录（含因子权重、IC、换手、边界）。
