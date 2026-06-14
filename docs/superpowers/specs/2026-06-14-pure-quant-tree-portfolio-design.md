# 纯量化决策树组合 设计规格（4 原型 × 严格 WFO 认证）

日期：2026-06-14 ｜ 状态：已与用户逐节确认 ｜ 阶段约束：**本阶段不含 LLM 节点**（纯量化 DSL）

## 1. 背景与定位

rquant 现有两棵成型树占据两个位置：**regime_adaptive v4**（趋势跟踪触发树，执行层，Brooks 回调×Turtle 突破）与 **strength_portfolio v1**（横截面动量选择树，选择层）。本轮构建 4 棵**正交风格**的纯量化树填补空位，并以**严格 WFO 认证**判定哪些真正"成熟可行"——在本项目的诚实判读文化里，"成熟可行" = **WFO 期外有 edge**，不是堆 lint 干净的 YAML。

**已确认范围（2026-06-14）**：
- 验收门槛：**严格 WFO 认证**（构造 → 全样本回测 → WFO/时间切片 → 诚实判读，只认证 OS 有 edge 者，失败如实记录）
- 4 原型全选：均值回归 / Donchian 突破 / 横截面强度 v2 / 均线多头排列
- 各原型按**天然层次**验证（触发型走单标的，选择型走组合 top-N）

## 2. 资产盘点（已核对）

- **DSL 函数面（30 个）**：sma/ema/wma/rsi/atr/slope/ref/highest/lowest/crossover/crossunder/macd_line/macd_signal/macd_hist/std/sigmoid/auto/abs/max/min/count/barssince/valuewhen/log/exp/sqrt/floor/sign/pow/percentrank/corr。**4 棵树全部可用现成函数表达，无需引擎改动。**
- **数据**：10 只日线（`paper/pd_*.csv`，2.4 年：600030/600036/600276/600519/600900/601088/601318/000333/000858/300750）+ 2 只 60m（`paper/p_sh600030/600036.csv`，1 年）。
- **验证因子（F-1 实证）**：mom20 RankIC 0.109/t 2.59/单调 0.90；rsi 与其相关 0.80（冗余）。
- **工具**：`backtest --sim/--soft`、`optimize --folds K`（WFO，仅覆盖 backtest/sim）、`factor`（RankIC/分层/相关矩阵）、`portfolio --top N --rebalance K`。
- **状态量/极值**：max_price_since_entry、bars_since_exit、last_trip_return 等（schema-hardening + P3 已落地）。

## 3. 验证方法学（"严格 WFO 认证"的可执行定义）

### 3.1 触发树折叠 WFO（树 1/2/4）

- 命令：`optimize --tree <t> --grid "<p>=..." (重复) --folds 4 --sim --max-combos 500`，**逐个跑全部 10 只日线**（v4 教训：扩样本才能证伪短样本幸存者，858 即栽于此）。
- 目标：sim Sharpe（退化为 total_return，同 v3/v4 口径）。
- **认证门槛（全部满足才算"成熟可行"）**：
  1. 多数标的（≥6/10）OS 折正收益；
  2. 退化率 OS/IS > 0.5；
  3. 共识参数跨折低漂移（n_unique ≈ 1–2，非 ≈ 折数）；
  4. 最优点是**内点**（向外扩网格复核，非网格边界幸存）；
  5. 非单标的偶然（剔除无 edge 噪声票）。
- **三种诚实结局**：认证通过 / 无 edge（如实记录，证伪也是数据）/ regime 依赖（仅某类标的有效，注明适用域）。

### 3.2 强度树 v2 诚实降级（树 3）

- **必须摆上台面的限制**：`optimize` 仅覆盖 backtest/sim，**portfolio 模式无折叠 WFO**（平台已知缺口）。v2 的认证强度**弱于**折叠 WFO，报告须明写。
- 替代验证三件套：
  1. **因子前置检验**：`factor` 工作台跑第二因子候选，按 F-1 判据纳入（|RankIC|>0.03 且 |ICIR|>0.3 且与 mom20 相关<0.7）；
  2. **时间切片期外代理**：前半段独立跑（同 strength-v1），是目前唯一期外证据；
  3. **敏感性矩阵**：top×reb 9 格全正、无尖峰，默认格在平台中部（非樱桃格）。

### 3.3 全树共享纪律（已沉淀）

- 吊灯/固定止损兜底（每棵都有出场保险）；
- **冷却写成阻断分支**（`bars_since_exit < cool_k → flat`；打分模式该量恒 NaN→落空→打分零影响，WFO 口径不退化成纯 flat——P3 教科书纪律）；
- T+1 同日加仓禁减仓（引擎 sim_step 已处理）；
- 成本 10bps 往返含；
- 前视安全：突破比较一律 `ref(highest/lowest(...), 1)`（比上一窗口，不自指）。

## 4. 四棵树设计

### 4.1 树 1 · 均值回归 / 超跌反弹（单标的触发，日线）

- **理论**：A股短期反转异象，与 v4 趋势跟踪正交（四棵中正交性最强，分散价值最高）。
- **进出场**：
  - regime 闸：`close > highest(close, n_dd) * dd_keep`（深度破位不接飞刀，反转在震荡/温和趋势有效、崩盘失效）；
  - 入场：`close < ema(close, n_ma) - k_dev * std(close, n_std)`（布林下轨深跌）**且** `rsi(n_rsi) < rsi_lo`（超卖）；
  - 出场：`close > ema(close, n_ma)`（回归均线即走，短持有）。
- **风控**：`risk: {stop_loss: stop_n*atr, max_hold_bars: hold_k}`（反弹不来则时间止损）；冷却阻断分支 `bars_since_exit < cool_k → flat`。
- **参数（默认 / 网格）**：n_ma 20、n_std 20、n_rsi 14、n_dd 60、dd_keep 0.80、hold_k 10、cool_k 3 固定；**扫** `k_dev∈{1.5,2.0,2.5}` × `rsi_lo∈{25,30,35}` × `stop_n∈{1.5,2.0,2.5}`（27 组合，扩格复核内点）。
- **文件**：`examples/mean_reversion_1.yaml`。

### 4.2 树 2 · 动量突破 Donchian（单标的触发，日线）

- **理论**：动量为核（mom20 F-1 实证）；Donchian N 日新高是动量经典触发，与 v4 Brooks 回调入场形态不同。
- **进出场**：
  - 入场：`close > ref(highest(high, n_break), 1)`（N 日新高，ref 防自指/前视）**且** `volume > sma(volume, n_vol) * vol_mult`（量能确认）**且** `ema(close, n_fast) > ema(close, n_slow)`（趋势过滤，只在上升趋势突破）；
  - 出场：`close < max_price_since_entry - chand_n * atr(n_atr)`（ATR 吊灯，用持仓极值状态量，承 v3）。
- **风控**：吊灯即止损；冷却阻断分支；参数门控 `s1_on > 0 and last_trip_return > 0 → flat`（亏损后跳过立即回场，v4 S1 纪律）。
- **参数（默认 / 网格）**：n_vol 20、n_fast 20、n_slow 60、n_atr 14、cool_k 6、**s1_on 1（固定开，不入网格，保持网格 ≤3 参）**；**扫** `n_break∈{20,40,55}`（Turtle 经典）× `vol_mult∈{1.2,1.5,2.0}` × `chand_n∈{2.5,3.0,3.5}`（27 组合）。
- **文件**：`examples/donchian_breakout_1.yaml`。
- **可选 60m 复跑**：日线认证后，用 2 只 60m 标的复核周期稳健性（n_break 按周期重设，参考 v4 双周期做法）；非认证必需。

### 4.3 树 3 · 横截面多因子强度 v2（组合选择，日线 top-N）

- **理论**：在 strength-v1（纯动量分位）上叠加**第二个验证因子**做正交增强。
- **第二因子前置筛选**（4.3 子调查，先于建树）：`factor` 工作台跑候选，按 §3.2 判据择优：
  - **低波动**：`-percentrank(atr_v/close, n_rank)`（低波动异象，真正正交于动量——首选押注）；
  - 短期反转：`-mom5`（5 日反转，需验与 mom20 相关<0.7）；
  - 趋势持续度：`count(close > ema_t, n_q)/n_q`（偏动量近邻，相关可能超阈）。
  - **因子检验形态澄清**：`factor` 命令对每 bar 横截面排序算 RankIC，故检验用**原始 per-symbol 序列**表达式（如 `vol=atr(14)/close` 期望负 RankIC、`rev5=-1*(close/ref(close,5)-1)`），**不**用 percentrank（percentrank 是自归一，留给树内做跨标的可比，不进因子检验）。
  - **决策规则**：选 |RankIC| 最高且与 mom20 相关<0.7 者；若全部相关≥0.7 或无一过 F-1 线，则 v2 退回纯动量 + 文档说明（诚实结局）。
- **结构**：沿用 strength-v1 三道闸（波动分位 / 创伤回撤 / 趋势态）+ 分级叶；强度分 = 动量分位与第二因子分位加权 `w_mom * mom_pct + (1-w_mom) * factor2_pct`；全分支 auto 软强度（组合层推荐 --soft）。
- **验证**：§3.2 三件套（因子前置 + 时间切片 + 9 格敏感性），**无折叠 WFO**。
- **参数（默认 / 扫）**：阈值带多数继承 v1；**扫** w_mom∈{0.5,0.6,0.7}（敏感性而非 WFO）；top×reb 9 格矩阵。
- **文件**：`examples/strength_portfolio_2.yaml`。

### 4.4 树 4 · 趋势均线多头排列（单标的触发，日线）

- **理论**：经典三均线多头排列（快>中>慢），最教科书趋势跟踪。已认可与 v4 风格最近、正交性最弱；价值在于干净的基准趋势树（v4 是 regime 切换 Brooks，本棵是纯排列）。
- **进出场**：
  - 入场：`ema(close, n_f) > ema(close, n_m) and ema(close, n_m) > ema(close, n_s)`（多头排列）**且** `close > ema(close, n_f)`（回踩不破、不追高）；
  - 出场：`crossunder(ema(close, n_f), ema(close, n_m))`（快线下穿中线，排列瓦解）或吊灯 `close < max_price_since_entry - chand_n * atr(n_atr)`。
- **风控**：吊灯；冷却阻断分支。
- **参数（默认 / 网格）**：n_f 10、n_m 20、n_atr 14、cool_k 6 固定；**扫** `n_s∈{40,55,60,90}`（慢线=趋势长度旋钮，最具影响力的单一参，n_f/n_m 固定保证排列语义不破）× `chand_n∈{2.5,3.0,3.5}`（12 组合）。
  - 注：optimize 网格是独立笛卡尔积，三均线元组**不能**当三参独立扫（会出 n_f>n_m 的废组合）——故只扫慢线 n_s + chand_n 两个正交旋钮，快/中线固定为经典 10/20。
- **文件**：`examples/ma_stack_1.yaml`。

## 5. 交付物

- 4 棵 `examples/*.yaml`（每棵：头注理论/参数表、lint 零警告、真数据跑通）；
- 1 份对比实验报告 `docs/superpowers/2026-06-14-pure-quant-tree-portfolio.md`：每棵 per-symbol WFO 表 + 认证结论（通过/无 edge/regime 依赖）+ 4 棵横向对比 + 诚实边界；
- 强度 v2 的因子前置检验小结（纳入哪个因子、为何）。
- **deploy/ 冻结副本与纸面盘接入：留用户定**（WFO 认证 ≠ 上线决策；认证通过的树由用户决定是否接 signal 第 4/5 账本）。

## 6. 诚实边界（先声明）

1. 触发树 WFO 用 10 只人工挑选的跨行业流动名单——**OS 跨标的稳健性**是主证据，绝对收益数字次之。
2. 强度 v2 无折叠 WFO（组合工具缺口），时间切片是唯一期外代理——认证强度弱于触发树，报告明写。
3. 某些原型可能整体无 edge（尤其均线多头与 v4 重叠、均值回归在趋势市可能失效）——**证伪是有效产出**，不强行调参凑数。
4. 全样本期（2024→2026）多数标的 bh 为负（跌势样本），触发树的防守性结构（低回撤超额）可能优于绝对收益——判读以风险调整口径为准。
5. 本阶段无 LLM 节点；消息面/事件驱动留待后续阶段。

## 7. 测试与纪律

- 每棵树构造后 `tree`（隐式 load+lint）零警告闸 + `backtest --sim` 真数据跑通（非空交易或合理空仓）；
- 示例树全集纳入 `all_example_trees_lint_clean` 总闸（lint.rs 既有测试）；
- WFO 扫参用既有 `optimize`，无引擎改动；
- git add 点名文件、提交信息英文、提交前 status 检查（既定纪律）。
