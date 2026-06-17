# 日内(15m)特征驱动的尾盘日频选股 — 设计 spec

> 用户方向（2026-06-17/18）："考虑缩小时间周期比如看 15min 的图表来进行每天尾盘调仓的策略。" 决策：先设计因子集 + 管线（design-first，**不抓数、不写实现码**）；执行时点用 **14:45 预收盘快照**；因子库 **6 个**（正反两向都测）。

本 spec 是该方向的设计交付物。沿用项目证伪文化与 §5.3 反过拟合：本设计**明确定位为假设探针**，受 6 个月数据上限约束，**不可能达到此前 27 轮的 OOS 部署级标准**——目标是"用日线看不见的日内微结构信号快速探针/证伪次日选股 edge"，而非产出可部署策略。

---

## 1. 目标与定位

- **问题**：每日 14:45 用当日 15m 日内微结构特征做横截面选股 → 尾盘(15:00)等权买入 → 持有 1 日 → 次日尾盘再调。
- **新意**：日内特征（尾盘强弱/日内反转/收盘相对 VWAP/日内波幅/量能时序/隔夜跳空）是**日线 bar 看不见**的信号源——这是真正未测过的新数据轴，**不是已证伪因子的参数变体**（故 §5.3 不禁）。
- **诚实定位**：受数据上限（见 §3），只能做**样本内假设探针**。任一因子"过探针"仅意味着"若将来有多年 survivorship-free 日内数据值得正经 OOS 检验"，**非可部署 alpha**。

## 2. 边界与非目标

- **非目标**：日内择时进出（intraday entry/exit）、分钟级回测、做空、可部署策略认证、OOS 部署级结论。
- **范围**：仅"日内特征 → 次日横截面选股"，尾盘日频调仓，long-only，等权。
- **复用**：现有 daily 选股框架（screen 引擎 `--rebalance 1` + `scripts/daily_eval.py` + `fund.*` 通道）。**零引擎改动**（见 §4 验证）。

## 3. 数据现实（探针实测，2026-06-17）

| 源 | 15m 可得性 | 结论 |
|---|---|---|
| eastmoney `stock_zh_a_hist_min_em` | `ConnectionError`（限频） | 规模化不可用 |
| sina `stock_zh_a_minute(period=15)` | **可用**，但仅 ~6 个月（实测 2025-12-10 → 2026-06-17，~1970 bar），~5s/股，**无退市股** | 唯一可行源，但浅 + 慢 + 幸存者偏差 |

**硬约束（决定本设计只能是探针）**：
- 历史 ~6 个月 ≈ 123 交易日 ≈ 123 次日频调仓，**单一近期 regime** → 无法 OOS 切分。
- survivorship-biased（sina 不供退市）。
- 日内信号比日线**更受成本墙制约**（信号衰减快、隔夜噪声大）。
- **单位**：sina 分钟 volume 单位待确认；本设计选用的因子（VWAP 偏离、量能比、收益类）均为**比值/收益**，量纲自动抵消 → 对单位不敏感（稳健性优点）。

## 4. 架构（零引擎改动，已验证）

**关键验证**（`src/data/fundamentals.rs` + `src/dsl/eval.rs`）：fund 加载器**通用读列**——只要求首列 `time`，其后任意命名列全部读入（`col_names = headers.skip(1)`）；`as_of(t)` 返回 ≤t 最近一行的全部列；DSL `fund.<col>`（eval.rs:200）`ctx.fundamentals.get(col)`，缺列→NaN→弃权（测试 `fund.nope`→NaN 已锁）。**故日内因子可直接走 `fund.<feature>` 通道，无需改引擎。**

数据流：
```
每股 15m CSV ──(离线 Python)──> 每股「每日日内因子」CSV (time,<6因子>, date戳15:00)
                                          │
试点 universe CSV: symbol,primary(日线),context(空),fundamentals(→日内因子CSV)
                                          │
日频选股树 fund.<因子> ──> screen --backtest --rebalance 1 ──> daily_eval.py(gross/net)
```
- **primary = 既有日线 CSV**（驱动时间线 + `close[T]→close[T+1]` 持有记账）。
- 15m **只在离线算因子用**，绝不进引擎时间线（避免 reb=16 对齐脆弱性）。

## 5. 无前视与执行时点（14:45 预收盘快照）

- 因子用日内 bar **截至 14:45**（含）计算（排除最后的 15:00 bar），戳在 T 日 **15:00:00**。
- 决策在 T 日 15:00（screen 在日线 bar 评估），`fund.as_of(T 15:00)` 取 T 行；该行输入数据全部 ≤14:45 **< 执行价 15:00** → **决策严格早于成交价，无自我前视**（比现有日频框架"同一收盘价决策+成交"更诚实）。
- 执行在 15:00 收盘（市价 MOC 口径），持有 `close[T]→close[T+1]`（日线）→ 全在未来，无泄漏。
- **半日市/停牌**：14:45 前 bar 数 < 2 → 该日该股弃权（因子 NaN）。停牌日无 15m → 无因子行 + 日线 is_fresh 过滤。

## 6. 日内因子库 v1（6 因子，正反两向都测）

每股每日从 ≤14:45 的 15m bar 计算。`prev_close` = 前一交易日**日线**收盘。VWAP 用典型价 `(H+L+C)/3` 按 volume 加权。

| # | 因子 | 公式（截至 14:45） | 假设（正向） |
|---|---|---|---|
| 1 | `last_leg` 尾盘动量 | `close@14:45 / close@13:45 − 1` | 尾盘走强→次日续涨（收盘买盘含信息）|
| 2 | `intraday_rev` 日内反转 | `−(close@14:45 / open@09:45 − 1)` | 日内过度延伸→隔夜回补 |
| 3 | `close_vs_vwap` 收盘强度 | `close@14:45 / VWAP(09:45..14:45) − 1` | 收在 VWAP 上=吸筹→续强 |
| 4 | `intraday_range` 日内波幅 | `(max(high)−min(low))[..14:45] / prev_close` | 高波/高关注→（多半反向防御）|
| 5 | `vol_tilt` 量能后移 | `Σvol(13:15..14:45) / Σvol(09:45..14:45)` | 尾盘放量=知情流 |
| 6 | `overnight` 隔夜跳空 | `open@09:45 / prev_close − 1` | 跳空续动 or 反转 |

- **正反两向**：每因子两棵树——`_hi`（选因子高者）与 `_lo`（选因子低者）。共 **12 个纯日内选股器**。让数据决定方向，不预设。
- **归一**：树叶 `weight = sigmoid(k · z)`，`z` = 因子值（`_lo` 取 `−`），`k` = scale 参数；`sigmoid ∈ (0,1)` 满足 `select_top>0` + 正单调。闸 `when close > 0`（恒真占位），因子 NaN → weight NaN → 弃权。
- v1 **不含价值×日内组合**（用户选纯 6 因子；若 v1 有因子过探针，再议 value_frac 两段叠加）。

## 7. 因子 CSV schema

`data/intraday_factors/<sym>.csv`（gitignored，可由管线复现）：
```
time,last_leg,intraday_rev,close_vs_vwap,intraday_range,vol_tilt,overnight
2025-12-10 15:00:00,0.0031,-0.0122,0.0008,0.0254,0.452,0.0040
...
```
- 首列 `time` 戳 15:00:00（匹配日线 bar）；6 因子列；缺值留空→引擎读 NaN→弃权。

## 8. 管线脚本接口（本阶段只定义，不实现/不抓数）

1. **`scripts/build_intraday_universe.py`** → `data/universe_intraday.csv`
   - 入：`data/membership_top2000.csv` 最新月成员 + 各股近 ~20 日线 `close×volume`。
   - 出：取流动性最高 **~150-200 只**（且当前在市、sina 可拉）的 universe CSV（`symbol,primary,context,fundamentals`：primary→既有日线，fundamentals→`data/intraday_factors/<sym>.csv`）。
   - 幸存者偏差声明（仅当前在市）。
2. **`scripts/fetch_intraday.py`**（sina 15m，~5s/股，resume + 退避 + `requests.getproxies` 补丁）
   - 入：universe 标的列表。出：`data/intraday_15m/<sym>.csv`（time,open,high,low,close,volume）。
   - resume：陈旧整重拉（防接缝）；无数据股记录跳过。
3. **`scripts/build_intraday_factors.py`**（纯计算，自带单测）
   - 入：`data/intraday_15m/<sym>.csv` + 日线 `data/<sym>.csv`（prev_close）。
   - 出：`data/intraday_factors/<sym>.csv`（§7 schema）。
   - 含 §5 边界处理（半日/首日/停牌弃权）。
4. **选股树/配置**：`examples/trees/screen/intraday_<factor>_<hi|lo>.yaml`（12 棵）+ `examples/screen/daily_intraday_<factor>_<hi|lo>.yaml`（regimes=单 6mo 窗 + 时间二分）。

## 9. 评估协议 + 诚实闸

- 跑：`python scripts/daily_eval.py examples/screen/daily_intraday_<f>_<dir>.yaml --from 2025-12-10 --to 2026-06-17 --warmup 5 --window 10`（日内因子无需价格 warmup）。net cost 20bps（+30bps 压力档）。
- 报：每因子每向 gross/net 总收益·超额·Sharpe·**每日换手·break-even 成本**·样本内 regime + **时间二分**（前半/后半，唯一弱稳健检查）。
- **诚实闸（因子"过探针"需全满足）**：① 毛超额>0（源头有信号）；② 净超额>0 且净 Sharpe>0（扛得住真实成本）；③ break-even ≥ 2× 真实成本；④ 前/后半同号（非单段伪影）。
- **过探针 ≠ 可部署**：仅标记"值得将来用多年 survivorship-free 日内数据正经 OOS 检验"。绝不为好看数字调 scale/方向凑过线（§5.3）。
- **预期诚实声明**：先验偏向证伪（日频日线选股已全证伪 + 日内成本墙更陡）；但日内是真新信号源，值得一次探针。

## 10. 试点 universe 与成本/规模预算

- universe：~150-200 当前最流动股（足够横截面 top-50 选股；太少则 top-N 无意义）。
- fetch 预算：~200 × 5s ≈ **~17 min**（一次性）；6mo × 200 股 15m ≈ 可控体积。
- 回测：daily_eval 单因子双跑（6mo、~200 股）远快于全样本（<10s/对）。

## 11. 风险与已知局限（全部诚实在案）

1. **6 个月单一近期 regime** → 无 OOS，结论只能是样本内探针。
2. **幸存者偏差**（sina 无退市）不可消除，仅声明。
3. **日内成本墙更陡**：日内信号换手通常高、衰减快 → 净额大概率被吃。
4. **sina volume 单位**未定（本因子集对单位不敏感，缓解）。
5. **14:45→15:00 滑移**：真实 MOC 成交价 ≠ 14:45 快照价；已用预收盘快照将此滑移显性化（最现实口径）。
6. **regime 特异陷阱**（参考迭代 27 中度反转 2024 单年 Sharpe 1.14 假象）：6mo 单段尤其危险，时间二分是仅有的弱护栏。

## 12. 后续（greenlight 后，非本 spec 范围）

approve 本 spec 后，单独 greenlight 触发：worktree → 实现 3 脚本（TDD）→ fetch（联网 ~17min）→ build factors → 12 因子双跑 daily_eval → 诚实判读写 findings。若任一因子过探针 → 标记 future-OOS 候选；大概率全证伪 → 同样如实记录，收束。

---

**自审**：占位符无；架构假设（fund 通用列）已代码验证；执行时点/无前视有因果论证；诚实闸与数据局限显式；范围聚焦单一 design。待用户审阅。
