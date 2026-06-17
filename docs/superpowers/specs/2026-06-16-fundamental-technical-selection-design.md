# 基本面×技术选股方法学：价值闸 + 动量排序（子项③）· 设计文档

- 日期：2026-06-16
- 状态：设计已与用户逐节确认（架构 A + 5 节全过），待写 spec → 自审 → 用户审 → writing-plans
- 范围：大方向「基本面 + 全市场 2000」的**子项③（最后一块）**——把②证出的**价值轴（PB）**与技术（横截面动量）组合成选股方法学，在全市场 **survivorship-free top-2000** 上做 **time-slice OOS + 跨regime + 敏感面** 验证。**不含** 新数据抓取（②已就位）、不含完整折叠 WFO（平台 backlog）。

## 1. 背景与定位

四子系统弧线收官：① 基本面进引擎（DONE `cf19391`）→ ② 全市场+幸存者+宽截面 IC（DONE `2ae484a`：**PB 价值因子 borderline——RankIC −0.046/t=−2.49/单调，质量成长证伪**）→ **③ 基本面×技术选股方法学（本 spec）**。

选股弧线（screener Phase-1/迭代）已证：**纯 OHLCV 技术信号在大盘股难稳健跑赢买入持有**；② 证：**价值（低 PB）是唯一有横截面预测力的基本面信号**。③ 把二者按用户定的**价值闸 + 技术排序**结构组合：低 PB 筛便宜池 → 池内横截面动量取 top-N。**经典「价值 + 动量」互补组合**。

## 2. 已确认决策（brainstorming）

| # | 决策 | 选择 |
|---|---|---|
| 组合口径 | 基本面×技术主从结构 | **价值闸 + 技术排序**：低 PB 筛便宜池 → 池内技术排序 top-N |
| 技术信号 | 排序用什么 | **横截面动量 mom**（价值+动量经典组合；动量是技术里唯一隐约弱正者）|
| 验证严度 | 怎么防过拟合 | **time-slice OOS（前段定参/后段验证）+ 跨regime + 敏感面（须普遍正、无尖峰）** |
| 架构 | 在哪实现 | **扩 `screen` 引擎**（横截面选股+regime切片+归因的现成验证台；接基本面+membership+横截面价值闸阶段）|
| 价值定义 | PB 还是 PB+PE | **PB-only**（②最强）；PE 作稳健性旁注 |
| 价值闸语义 | 绝对阈 vs 横截面分位 | **横截面分位**（最便宜 `value_frac`，忠于②的相对排名 IC；复用 `select_top`）|
| 构建序 | — | **先 PB-alone 基线**（验②的 PB IC 扣费后可交易？）**再叠动量** |

## 3. 引擎接线（扩 `screen`，Rust）

### 3.1 基本面 + membership 进 screen（同 FE-6 / SUB2-2 模式）

- **fundamentals**：`run_screen` / `backtest` 现按 universe 读 primaries/contexts，新增逐股 `funds: Vec<Option<FundamentalSeries>>`（`entry.fundamentals.as_ref().map(load_fundamentals_csv).transpose()?`）；`score_and_leaf`（`mod.rs:84`）的 `build_context(..., None, ...)` 改穿 `funds[i].as_ref()` → 树内 `fund.bps`/`fund.eps` 可用。
- **membership**：`ScreenRunConfig` / 回测配置加 `membership_path: Option<PathBuf>`；加载 `crate::data::membership::Membership`；每再平衡 t 的 eligible 在 fresh 基础上 ∩ `membership.effective_at(t)`（None=不过滤=冻结）。
- **行为冻结**：`fundamentals`/`membership` 均缺省 None 时，screen 与改造前**逐字一致**（现有 screener 调用、测试不破——冻结回归锁）。

### 3.2 横截面价值闸 + 动量排序（新选择路径，复用 `select_top`×2）

`ScreenConfig` 加 `value_frac: Option<f64>`（横截面价值闸保留最便宜比例）。`backtest`/`run_screen` 选择阶段：

- **`value_frac = Some(f)`（③ 路径）**：每再平衡 t——
  1. `elig` = fresh ∩ membership-at-t ∩ `quality_score` 有限（有财务、首报后）；
  2. `cheap = select_top(elig 按 quality_score 降序, ceil(f × |elig|))` —— quality 树输出 **cheapness 分**（PB 越低越高）→ 取最便宜 `f` 分位；
  3. `picks = select_top(cheap 按 tilt 强度降序, top)` —— 池内按动量强度取 top-N，等权。
- **`value_frac = None`（冻结路径）**：现有 combine-based 选择（eligible=q≥q_floor、select_top by combined）逐字不变。
- **PB-alone 基线** = `value_frac` 小 + 动量树关（或 tilt 强度恒 0）→ step 3 退化为便宜池内任意序取 top（或直接 `select_top(elig 按 quality, top)`，即最便宜 N 只）。spec 用**独立基线配置**（无动量 setup）跑此里程碑。

### 3.3 两棵树（`examples/trees/screen/`）

- **`value_pb.yaml`（价值/quality 树）**：单 Long 叶，`weight: "1 / (1 + close/fund.bps)"`——PB=close/fund.bps，此式把 PB∈(0,∞) **严格单调递减映到 (0,1)**（PB→0 得 1、PB→∞ 得 0），cheaper→higher，**全程不饱和**（distinct PB→distinct 分，排名不丢）。fund.bps 首报前 NaN → 整式 NaN → weight 弃权 → quality_score 非有限 → 不入便宜池。供 quality_score。
- **`momentum_xs.yaml`（动量 setup 树）**：单 Long 叶，`weight: "sigmoid((close/ref(close, mom_n) - 1) * mom_scale)"`（params: mom_n/mom_scale）——近 mom_n 日收益经 `sigmoid` **严格单调映到 (0,1)、不饱和**，distinct 动量→distinct 强度。供 tilt 强度。
- **关键：weight 引擎自带 clamp[0,1]**——故 weight 表达式必须本身落 (0,1) **单调不饱和**（否则截断成并列 1 会毁排名）；上两式即为此设计（不用 clamp01/min/max，它们会饱和）。`sigmoid`/`ref`/算术均 DSL 既有（Phase-2/3）。两树经 lint（恒假/空转）+ 加载测试。

## 4. 验证协议（time-slice OOS + 跨regime + 敏感面）

### 4.1 两递进里程碑（各自诚实判定）

1. **PB-alone 基线**（`value_momentum_v1_baseline.yaml`：value 树 + 无动量）：最便宜 N 只 vs 基准，全期 + OOS。判②的 PB 横截面 IC **扣费后是否转化为可交易超额**。
2. **价值闸 + 动量**（`value_momentum_v1.yaml`）：vs PB-alone 基线 + vs 基准。判**池内动量是否在价值之上再添 alpha**。

### 4.2 OOS / regime / 敏感面

- **时间切片**：前段 **2018-01…2022-12** 选参（value_frac/mom_n/top/rebalance），后段 **2023-01…2026-06** 留出 **OOS 验证（决不在 OOS 调参）**。screen `--backtest` 加 `--from/--to` 日期窗（若无则加）切分样本。
- **跨 regime**：复用 screen 现有 regime 切片输出（牛/熊/震荡）；重点 2018 熊 + 2022 回调是否扛跌。
- **敏感面**：`value_frac{0.2,0.3,0.5} × mom_n{20,60} × top{20,30,50} × rebalance{10,20}` 网格——**须普遍正超额、无尖峰**（单点尖峰=过拟合，按 §5.3 弃、不追）。
- **基准**：top-2000 等权、同节奏、无成本（隔离选股 α vs universe β）；策略腿换手成本单边 rt/2。

### 4.3 诚实判定

works / inconclusive / falsified 均如实写入 findings `docs/superpowers/2026-06-16-fundamental-technical-selection-findings.md`。**不调参凑超额（§5.3）**。价值因子②本为 borderline → ③ 完全可能证伪（PB 扣费后不可交易，或动量不增益）——那是有效产出（省下错方法学的后续投入）。

## 5. 文件

| 文件 | 改动 |
|---|---|
| `src/screen/mod.rs` | `run_screen`/`score_and_leaf` 穿 funds + membership；`value_frac=Some` 时两段 `select_top` 价值闸路径；冻结 None 路径 |
| `src/screen/backtest.rs` | 回测循环穿 funds + membership-at-t mask + 两段选择；time-slice `from/to` 窗 |
| `src/screen/config.rs` | `ScreenConfig` 加 `value_frac: Option<f64>`（serde default None 冻结）|
| `src/cli`（screen 子命令）| `--membership`/`--from`/`--to` 可选参透传 |
| `examples/trees/screen/value_pb.yaml` | 新：价值/cheapness 树（fund.bps）|
| `examples/trees/screen/momentum_xs.yaml` | 新：横截面动量 setup 树 |
| `examples/screen/value_momentum_v1.yaml` | 新：价值闸+动量配置 |
| `examples/screen/value_momentum_v1_baseline.yaml` | 新：PB-alone 基线配置（无动量）|
| `docs/{dsl-reference,cli-reference}.md` | screen `--membership`/`--from/--to`/`value_frac` + fund. 在 screen 可用 |
| `docs/superpowers/2026-06-16-fundamental-technical-selection-findings.md` | 新：两里程碑 OOS/regime/敏感面 诚实判定 |
| 闸 | `cargo test --workspace` + `clippy --workspace --all-targets`（screen 是引擎公共路径）+ 价值闸单测（横截面最便宜 frac 正确）+ **无-membership/无-fund 冻结回归** |

## 6. 诚实边界（非目标）

- 子项③ = 价值闸+动量选股方法学 + OOS/regime/敏感面验证；**不**新抓数据（②就位）、**不**做完整折叠 WFO（time-slice OOS 是本子项严度上限，平台 WFO-fold 留 backlog）、**不**做实盘部署（findings 通过才谈）。
- **survivorship**：top-2000-at-t 含退市股活跃期；覆盖 94.4%（缺退市尾，②已声明）。
- **价值闸**：横截面分位（`select_top`×2），忠于②的相对 PB 排名 IC（非绝对 PB 阈）。
- **point-in-time 三闸**：membership 排名≤d + `fund.as_of(t)`≤t（首报前弃权）+ 决策 t 用收盘、t+1 执行。
- **行为冻结**：funds/membership/value_frac 缺省时 screen 逐字同改造前。
- **§5.3 反过拟合**：敏感面须普遍正、OOS 不调参；falsification 是有效产出。
- screen 是引擎公共路径 → 闸必 `--workspace`。
