# 价值 + PA短线择时 + 板块轮动 overlay — findings

> 执行 plan `plans/2026-06-21-value-pa-sector-overlay.md`（子代理驱动 T1-T5）。用户接受的方案：价值定"买什么"（慢）+ PA入场&板块轮动 overlay（setup-tilt，lambda=短线比率，周频 reb5）。
> 诚实文化 §5.3：本轮把用户亲自设计的、最复杂的一版短线 overlay（多条件 PA：趋势/结构/回调/H1H2/通道/信号K强度 + 板块动量/广度/成交额热度）完整建好并验证。

## 结论（一句话）

**PA+板块短线 overlay 不增益、反而单调稀释价值核心。** 纯价值（lambda=0）vs csi300 **PASS（net-OOS +0.28）**；加 overlay 后 net/OOS/vs-EW/vs-csi300 全面下降、换手 7%→51% 暴涨。价值三核在周频 reb5 上仍成立，但**不能被 PA 倾斜**。

## 方法学（无前视 + 真 train/OOS）

- 子代理驱动建 3 个 builder（全程 ≤t + 滞后1交易日）：`build_pa_features.py`（日线 PA：pa_dir/pa_struct(HH/HL 滚动)/pa_regime(ER)/pa_pullback/pa_h1/pa_h2/pa_chan(ATR)/pa_sig_with/pa_sig_cnt/pa_ext，2964 股）；`build_sector_factors.py`（板块 sec_mom20/sec_trend/sec_breadth/sec_heat→逐股）；`build_pa_sector_universe.py`（财务 as-of + PA + 板块 merge → `universe_pa_sector.csv` 2963 股）。iterate.py 新增 `paov` 轴。
- `quality_trees`=价值三核(value_pb+rev_yoy+gm)，`setup_trees`=`pa_overlay`（仅上升趋势内给正倾斜：板块强/热 + 回调买点 + 顺势信号，减过度延展/逆势）。`merge.lambda`=短线比率。
- 评估 = `iterate.py --axis paov --rebalance 5`，train 2018-01..2023-12 / OOS 2024-01..2026-06，net@20bps，**双基准 EW + csi300**，§5.3 闸。

## 结果（r51-56）

| 轮 | 配置 | net vs-EW | OOS(EW) | net vs-csi300 | OOS(csi300) | 轮换手 | 判定 |
|---|---|---|---|---|---|---|---|
| 51 | **λ=0 纯价值基线** | −0.37 | −0.23 | — | — | 7.2% | (vs-EW 微负) |
| 52 | + overlay λ0.3 | −1.31 | −0.59 | — | — | 41% | 证伪 |
| 53 | + overlay λ0.5 | −1.88 | −0.71 | — | — | 47% | 证伪 |
| 54 | + overlay λ0.7 | −2.10 | −0.84 | — | — | 51% | 证伪 |
| 55 | **λ=0 纯价值 vs csi300** | (−0.37) | — | **+1.59** | **+0.28** | 7.2% | **PASS**（be212、无翻转） |
| 56 | overlay λ0.3 vs csi300 | (−0.56g) | — | +0.69 | −0.07 | 41% | 证伪（in-sample-only） |

## 判读

1. **单调稀释**：lambda 越大越差，net-OOS(EW) −0.23→−0.59→−0.71→−0.84、gross −0.20→−1.54、换手 7%→51%。加 PA+板块倾斜把价值核心越推越坏。
2. **vs 两个基准都坏**：vs-csi300 也是 baseline +1.59 → overlay(λ0.3) +0.69、OOS +0.28 → −0.07。不是基准伪影。
3. **基线本身 vs csi300 PASS**（net-OOS +0.28、Sharpe 0.60、换手仅 7%、break-even 212bps、tier2 无翻转）——**价值三核即便在周频 reb5、2963 股池上仍稳健跑赢可交易指数**。它输等权(−0.37)只是 2963 股 EW 的小盘 beta。
4. **无 lambda 优于基线 → 跳过消融**（plan 规定）；即便最复杂、用户亲自设计的 PA+板块多条件 overlay 也无法增益。

**机制**：与全程一致——短线/技术面倾斜给价值核心**加换手与噪声、不加 alpha**；正交性原理再次兑现（PA/板块因子个体无 edge → 混入只稀释）。"价值-防御、慢调仓"是唯一稳健边；任何把它往短线推的尝试都降它。

## 诚实边界

vs-EW 为负是 2963 股等权的小盘 beta（不可投资基准）；vs csi300（可交易）基线 PASS 才是有意义口径。单一 2018-26 OOS、long-only beta、T+1、幸存者偏差沿用。**可投资结论：用价值三核（慢、reb 月/周皆可、vs 指数稳健），勿加 PA/板块短线倾斜。** 若仍要短线，唯一未穷尽的是「持仓内真择时」(signal/sim 引擎，逐 bar 进出而非调仓倾斜)，本轮范围外。

## 工程交付（可复用）

`build_pa_features.py`(+11 PA 特征单测) / `build_sector_factors.py`(+板块因子单测) / `build_pa_sector_universe.py`(+merge as-of 单测) / `pa_overlay.yaml` 树 + 4 配置 + iterate `paov` 轴。即便策略证伪，这套 PA/板块特征管线 + 验证框架可复用于将来"持仓内择时"或新数据。
