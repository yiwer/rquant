# 三个 top-3 集中持仓策略 对比 设计（top3-concentrated-strategies）

> 状态：brainstorm 定稿（用户 ok），待 writing-plans。日期：2026-06-21。
> 前序：价值三核（已部署/已验证）是唯一稳健边；15m/反向/FF3/PA+板块 overlay 全证伪。本轮把价值核心落成 3 个**集中(top-3)可投资变体**并诚实对比。

## 0. 背景 / 诚实前提

用户要 3 个**平均持仓前 3** 的策略对比：
- **策略1** = 纯价值三核 → top-3。
- **策略2** = 价值核心初筛（最便宜约 30）→ 对这 30 只用 **1h K线×100根 PA** 强化筛选 → top-3。
- **策略3** = 行业因子初筛（强势板块）+ 价值核心深度权重 → top-3。

**两条诚实前提（已与用户确认，必须贯穿）**：
1. **top-3 统计脆弱**：3 只票回测被极少数个股主宰、净值噪声大、§5.3 裁决不可靠。⇒ 每策略**同时报 top-3（目标）与 top-10（稳定性参照）**，避免把运气当 alpha。
2. **PA/板块此前无 edge**（已证伪）；本轮结构不同（硬两段 + 集中），诚实重测，先验仍是策略2/3 难超策略1，数据说话。

## 1. 已定决策（brainstorm）

| 决策 | 结论 |
|---|---|
| 持仓 | 主口径 **top-3**；每策略附 **top-10** 稳定性参照 |
| 策略2 两段式 | 用引擎现成 **`value_frac`**（价值留最便宜一档 → 内部 PA 排序），**非**严格"恰好30"wrapper（用户接受近似） |
| 1h 数据 | k15m **重采样**成 1h（4×15m=1h，A股 4 根/日），取近 100 根算 PA；不抓新数据 |
| 引擎 | **零改动**，复用 `run_screen` 的 quality/setup/value_frac/top + iterate |
| 基准 | **vs csi300（主）+ vs EW（参考）**，§5.3 闸 |

## 2. 三策略架构（引擎零改动）

### 策略1 — 纯价值 top-3（零新基建）
`examples/screen/iter/s1_value_top3.yaml`：quality=三核(value_pb+rev_yoy+gm)，`merge.top=3`，universe=`universe_baostock_day.csv`，月频 reb20。

### 策略2 — 价值初筛 → 1h-PA 强化 → top-3（唯一需新基建）
- **新 builder** `scripts/build_pa1h_value_universe.py`：每股 k15m 重采样 1h（open=首/high=max/low=min/close=末/vol=和）→ 取序列算 PA（复用 `build_pa_features.pa_features()` 逻辑，pa1h_* 前缀）→ **滞后1日** → merge_asof 财务 → `data/baostock/pa1h_value_merged/<sym>.csv`(time + pa1h_* + 6 财务) → `data/baostock/universe_pa1h_value.csv`(primary=kday)。iterate 新增 `pa1hv` 轴。
- `examples/screen/iter/s2_value_pa1h_top3.yaml`：quality=三核(驱动 value_frac 选最便宜档)，setup=`pa1h_overlay`(PA-1h 排序器)，`value_frac≈0.03`(1073 留最便宜 ~30)，`lambda` 高(PA 主导档内排序)，`merge.top=3`，周频 reb5。即"最便宜 30 只里按 价值×PA 选前 3"。
- **新树** `examples/trees/screen/pa1h_overlay.yaml`：仅上升趋势内给正分（gate `fund.pa1h_dir>0 or fund.pa1h_struct>=1`），权重 = 回调/H1H2/通道/顺势信号（复用 pa_overlay 思路，列名 pa1h_*）。

### 策略3 — 行业强度 × 价值 深度权重 → top-3（零新基建，复用 universe_pa_sector）
机制 = 引擎现成 quality/setup tilt（同 overlay）：`quality_trees`=三核(价值合成)，`setup_trees`=新树 `sector_strength.yaml`（weight=`sigmoid(fund.sec_mom20 / sec_scale)`，板块越强分越高），`merge.lambda` 高（板块强度**深度加权**价值），`merge.top=3`，universe=`universe_pa_sector.csv`，月频 reb20。
- 配置 `examples/screen/iter/s3_sector_value_top3.yaml`。
- 最终排序 ≈ 价值 × 板块强度 → top-3 即"强势板块里的便宜优质票"。**实现注**：引擎无 `sector_frac` 硬闸，故"行业初筛"以**强力倾斜**实现（高板块强度×高价值的票自然浮到前 3，等效初筛）；这是与引擎 combine 最契合的写法，无需新引擎能力。

## 3. 验证方法学（iterate，§5.3）

- 各策略跑 `iterate`：vs csi300（主）+ vs EW（参考），train 2018-23/OOS 2024-26，net@20bps。
- **每策略 top-3 与 top-10 各跑一遍**（top 改 merge.top 或 --top）。
- **横向对比**：3 策略 net/OOS/Sharpe/最大回撤/换手 + 与"价值三核 top-50 基线"（已知 vs csi300 PASS）对照，看集中 + PA/板块是否增益。
- **赢的判据**：某策略（尤其 top-10 口径，稳）net-OOS > 价值 top-50 基线 且过 §5.3 闸；top-3 口径作目标但标注其噪声。
- 诚实判读：证伪也写清；不参数钓鱼（lambda/sec_floor 给合理缺省 + 有限敏感性）。

## 4. 测试

- `build_pa1h_value_universe.py` 纯计算单测（pytest）：1h 重采样正确（4×15m OHLCV 聚合）、PA 复用、滞后1日无前视、财务 as-of。
- 3 树 + 3 配置 yaml 加载/lint 测试。
- 收尾：相关 pytest 全绿 + 真数据跑出 6 轮（3 策略 × {top3,top10}）账本 + findings。

## 5. 范围边界（YAGNI）

- 策略2 用 `value_frac` 近似两段（非严格恰好30 wrapper）。
- 1h 由 15m 重采样（不抓新数据）。
- 不改引擎/部署/价值核心树本身；纯新增 1 builder + 3 树 + 3 配置 + 1 iterate 轴。
- top-3 的统计噪声、单一 OOS regime、long-only beta、T+1、幸存者偏差沿用并标注。
- 不做持仓内逐 bar 真择时（signal/sim），仍是调仓时选股（用户此前选定的口径）。

## 6. 参数缺省（实现定，可后调）

top ∈ {3, 10}；reb：策略1/3 月频20、策略2 周频5；value_frac=0.03（≈30/1073）；lambda(策略2 PA)=1.5；lambda(策略3 板块)=1.5、sec_scale=0.1（sec_mom20 尺度）；1h 窗口 100 根；PA 同 build_pa_features 缺省。
