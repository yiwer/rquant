# 价值选股 + PA短线择时 + 板块轮动 overlay 设计（value-pa-sector-overlay）

> 状态：brainstorm 定稿，待 writing-plans。日期：2026-06-21。
> 前序：30 轮日频 + 16 轮 15m/反向/FF3 因子迭代已证伪「短线/技术面横截面**选股**」；唯一稳健边=慢速价值三核（已部署）。
> 本模块换思路：**价值定"买什么"（慢），PA+板块定"倾斜谁/何时"（短线 overlay，占一定比率）**——绕开"选股无 edge"的墙。

## 0. 背景与动机

用户实操三件事：① 看基本面买什么；② 看板块近期强度/资金/热度；③ 看个股 PA 时机（回调/趋势/结构）。
已验证：纯价值三核每季选票几乎不变（换手 8–18%/月）→ 用户批评"结果固化、不是短线"。
✅ 接受的方案：**价值三核（quality）+ PA短线择时 & 板块轮动（setup tilt，lambda=短线比率）**，周频调仓让短线有 teeth、避免固化。

## 1. 已定决策（brainstorm）

| 决策 | 结论 |
|---|---|
| 结合方式 | **setup-tilt**：quality=价值，setup=PA+板块，`merge.lambda`=短线比率（扫 0.3/0.5/0.7） |
| 节奏/周期 | **周频 reb5 + 日线 PA**（与周持有协调；日线数据 2018 起、验证更稳） |
| 板块 | 折进短线 overlay；热度=**本地代理**（板块动量+breadth+聚合成交额），真资金流(北向/龙虎榜)不在数据集、v1 不抓 |
| PA thesis | **回调入场 与 趋势确认 都做成特征**，回测消融让数据选，不预设 |
| 引擎 | **零改动**，复用 `run_screen` 的 quality/setup/lambda combine + iterate.py 验证 |

## 2. 架构（零引擎改动）

```
价值三核 (quality_trees)  ──┐
  value_pb + rev_yoy + gm    ├─ combine(lambda) ─→ 排序 top-50 ─→ 周频(reb5) 等权持有
PA+板块 overlay (setup_trees)┘
  pa_overlay 树（PA入场分 × 板块轮动分）
```
- universe `data/baostock/universe_pa_sector.csv`：primary=kday（日线，前向收益）；fundamentals=**合并**（财务 roe/bps/rev_yoy/gross_margin + PA 特征 + 板块特征），季度财务按公告日 as-of、PA/板块**滞后1交易日**（无前视）。
- `quality_trees` = 复用 value_pb / growth_revyoy / quality_gm（已验证）。
- `setup_trees` = 新树 `pa_overlay`（读 PA + 板块特征打"短线入场分"）。
- `merge.lambda` = 短线比率；lambda=0 即纯价值（基线对照）。

## 3. 数据构造（3 个新 builder，Python，全程 ≤t、滞后1日无前视）

### 3.1 `scripts/build_pa_features.py` → `data/baostock/pa_features/<sym>.csv`（日线 PA，滞后1日）
| 列 | 含义 | 算法（日线 OHLC） |
|---|---|---|
| `pa_ema20` | EMA20 偏离 | close/ema20 − 1（>0=上方） |
| `pa_dir` | 趋势方向 | sign(EMA20 5日斜率)（+1/0/−1） |
| `pa_struct` | 摆动结构 | ±k(=3) fractal pivot → 末两高(HH/LH)+末两低(HL/LL) → +2…−2 |
| `pa_regime` | 震荡/趋势 | 效率比 ER(20)=\|close−close[20]\|/Σ\|Δclose\|（高=趋势） |
| `pa_pullback` | 回调深度 | 上升趋势中：(highest(high,20)−close)/highest(high,20)，且 close>ema20 才计 |
| `pa_h1` `pa_h2` | Brooks 入场 | 上升回调后首/次根 high>前一根 high（barssince 序列态） |
| `pa_chan` | 通道宽窄 | ATR(14)/close（小=窄=低风险入场） |
| `pa_sig_with` | 顺势信号K强 | 上升趋势中上涨K：实体占比×(close−low)/range |
| `pa_sig_cnt` | 逆势信号K强 | 下跌K同式（用于减分） |
| `pa_ext` | 过度延展 | max(0, close/ema20 − 1) 的超阈部分 |

### 3.2 `scripts/build_sector_factors.py` → `data/baostock/sector_factors/<sym>.csv`（每股=其所属板块，滞后1日）
读 `sector_membership.csv`（股→行业）+ `sector/<行业>.csv`（板块日线 ret/index/breadth）+ 聚合个股 kday `amount` 为板块成交额：
| 列 | 含义 | 算法 |
|---|---|---|
| `sec_mom20` | 板块强度/涨幅 | 板块 index 近20日收益 |
| `sec_trend` | 板块趋势 | 板块 index / 其 MA20 − 1 |
| `sec_breadth` | 板块广度/热度 | breadth 的 5 日均（上涨家数占比） |
| `sec_heat` | 板块成交额热度 | 板块当日 Σamount / 其 MA20（相对放量；横截面可比用 z-score） |

### 3.3 `scripts/build_pa_sector_universe.py` → `data/baostock/universe_pa_sector.csv`
逐股 merge_asof：财务(公告日) + PA特征(滞后1日) + 板块特征(滞后1日) → 一份 fundamentals/<sym>.csv；universe 列 primary=kday、fundamentals=合并文件。复用 `iterate.py` 新增 `paov` 轴（universe=此，frm 2018-01、to 2026-06、reb 默认 5）。

## 4. PA+板块 setup 树（`examples/trees/screen/pa_overlay.yaml`）

thesis：**在便宜价值股里，优先"所在板块强/热 + 个股处于上升趋势的回调买点"**。仅对上升趋势加正倾斜（不追下跌价值陷阱）。
```
setup = sigmoid(
   w_sec_mom·z(sec_mom20) + w_sec_heat·z(sec_heat或breadth)      # 板块轮动
 + w_pull·pa_pullback + w_h12·(pa_h1+pa_h2) + w_narrow·(−pa_chan) # 回调入场
 + w_sigw·pa_sig_with + w_trend·max(pa_struct,0)                  # 顺势/结构确认
 − w_ext·pa_ext − w_sigc·pa_sig_cnt )                             # 过度延展/逆势减分
```
- gate：`pa_dir>0 或 pa_struct≥1`（仅上升趋势/HL 结构内给倾斜）；否则 setup→0（不倾斜，不反向）。
- 消融开关：回调族（pull/h12/narrow）vs 趋势确认族（struct/sig_with）vs 板块族（sec_mom/heat）分别可关，回测看各自贡献。
- 参数 w_* 在配置 params 里，先给合理缺省、再敏感性扫描（不参数钓鱼）。

## 5. 验证方法学（iterate.py，§5.3 闸）

- 轴 `paov`，universe_pa_sector，**reb5**，train 2018-01..2023-12 / OOS 2024-01..2026-06，net@20bps，基准 **EW 与 csi300 双跑**。
- **基线** = 纯价值三核（lambda 0，同 universe/reb5）。**处理** = 价值+overlay（lambda 0.3/0.5/0.7）。
- **赢的判据**：处理 net-OOS **> 基线** 且过 §5.3 闸（gross>0 ∧ net-OOS>0 ∧ net-Sharpe>0 ∧ break-even≥40bps ∧ tier2 无符号翻转）。
- **消融**：若整体赢，逐族开关（板块/回调/趋势）定位真正有用的子条件；若整体输，同样消融看是否某一族单独有用、其余拖累。
- 诚实预期：纯单因子倾斜曾稀释三核(r37-39)；本设计是多条件、有连贯 thesis 的 overlay + 周频（短信号有 teeth），是真正不同的尝试。闸说了算，证伪也是产出。

## 6. 测试

- 3 个 builder 各配纯计算单测（pivot/HH-HL 分类、H1/H2 序列、ER、板块映射与 as-of、滞后1日无前视）——TDD。
- 树 lint（加载期恒假/空转闸）。
- 收尾：`cargo test --workspace` + 真数据：跑基线 vs overlay（≥1 个 lambda）出账本轮卡。
- CLI 对拍：`rquant screen --config <overlay> --universe universe_pa_sector --backtest`。

## 7. 范围边界（YAGNI）

- v1 **不抓真资金流**（北向/龙虎榜）——用本地代理；若 overlay 验证有效再考虑 v2 联网补。
- v1 **不做持仓内真择时**（signal/sim 引擎的逐bar进出）——用 setup-tilt 近似（用户已选）。
- 日线 PA，不用 15m（时标与周持有协调）。
- 不改价值三核/部署/引擎核心；纯新增 builder + 树 + 配置 + iterate 轴。
- 幸存者偏差/单一 OOS regime/long-only beta 边界沿用，结果如实标注。

## 8. 参数缺省（实现时定，可后调）

top 50；reb 5；lambda ∈ {0,0.3,0.5,0.7}；pivot k=3；ER/通道/板块窗口=20；EMA=20；w_* 给等权合理缺省后做敏感性。
