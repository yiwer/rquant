# rquant：E4 — 持仓状态模拟（--sim）— 设计文档

- **日期**：2026-06-10
- **状态**：设计已确认（写 spec + 计划）
- **关联**：master `f1a897c`。差距分析 G2/G6（最高级缺口）：前瞻打分模式逐点独立、无持仓状态，无法表达入出场不对称/止损/持有期；本设计加第三种运行模式——**顺序权益模拟**。前瞻打分仍是默认（信号质量研究），`--sim` 是到"策略回测"的桥。

---

## 1. 目标与非目标

### 目标
1. `backtest --sim`（可与 `--soft` 组合）：逐 bar 顺序模拟，树产出**目标仓位**（硬：叶 `stance×weight`；软：`E=Σp·w·dir`，无需 forward_return），模拟器交易差额、分段记账净值。
2. DSL 持仓标识符 `pos/entry_price/bars_held/unreal_pnl`（非 sim 模式取默认 → 同树双模式可跑；空仓 `entry_price=NaN` → 引用它的比较自动弃权）。
3. 树顶层可选 `risk:` 块（`stop_loss`/`take_profit`/`max_hold_bars`，bar 收盘检测、次 bar 开盘强制出场、优先于树目标）。
4. T+1 强制：同自然日加仓 → 当日禁减仓（整体顺延）。
5. `SimReport`（总收益/最大回撤/回合数/胜率/平均持仓/换手/buy&hold + 回合列表）+ `--traces` 逐 bar `{t,target,pos,nav}` + 摘要打印。

### 非目标（YAGNI / 后续）
- 涨跌停不可交易过滤（需逐标的限制元数据）；盘中价位成交（bar 粒度只用 open/close）；杠杆/保证金；`--sim` 下 walk-forward 分折（`--folds` 被忽略并提示）；HTML sim 渲染（`report --sim` 留 follow-up）；多标的。

## 2. 锁定决策

| # | 维度 | 选定 |
|---|---|---|
| 1 | 叶子语义 | 目标仓位（复用 stance×weight；现有树零改动可跑 --sim）|
| 2 | 风控 v1 | stop_loss/take_profit/max_hold_bars（树 `risk:` 块，可选、>0 校验）|
| 3 | 成本 | 单边 `(cost_bps/2)/1e4 × |Δ|`（一进一出合计 round_trip，与打分模式衔接）|
| 4 | T+1 | 同自然日加仓→当日禁减仓（减仓请求整体顺延；含翻向）|
| 5 | 胜率 | 按平仓回合，`trip_return = nav_close/nav_open − 1`（含成本，净值口径）|
| 6 | 期末 | 仍持仓 → 末 bar 收盘强制清算（计成本，reason="end"），指标完整 |
| 7 | HTML | 本期不做 |

## 3. 执行与记账语义（权威定义）

设决策于 bar i 收盘、执行于 bar i+1 开盘；`rate = (cost_bps/2)/1e4`。

1. **决策**（bar i 收盘）：构建 Context（time≤t 闸门 + 注入 SimState）。`unreal_pnl = if pos≠0 { (close_i/entry_price − 1)·sign(pos) } else 0`。
2. **风控覆盖**（pos≠0 时按序检查，命中即 target=0 并记 reason）：`unreal ≤ −stop_loss` → stop；`unreal ≥ take_profit` → tp；`bars_held ≥ max_hold_bars` → max_hold。未命中 → 树目标（reason=tree）。
3. **T+1**：`Δ = target − pos`。若 Δ 使 |pos| 减小或翻向，且 `last_increase_date == bar_{i+1} 自然日` → 本 bar 不交易（Δ=0，顺延）。
4. **记账**（bar i+1）：`nav *= 1 + pos_old·(open/prev_close − 1)`；交易 → `nav *= 1 − rate·|Δ|`；`nav *= 1 + pos_new·(close/open − 1)`。
5. **状态**：加仓 → `entry_price = 加权均价(entry_price·|pos_old| + open·|Δ增|)/|pos_new|`（自 flat 开仓 → entry=open，记 trip 开始 nav 与时间）；减至 flat → 回合关闭（exit=open、trip_return=nav/trip_open_nav−1、reason）、entry=NaN、bars_held=0；部分减仓 → entry 不变。翻向 = 关旧回合 + 开新回合（一次 |Δ| 计成本）。`bars_held` 计数起点：**开仓执行的那根 bar 收盘即为 1**，此后持仓中每根 bar +1（决策于 bar i 收盘看到的是含 bar i 的计数）。`|pos| 增加` 时记 `last_increase_date = 执行日`。
6. **期末**：循环到 `len−2`（执行需要 i+1）；结束后若 pos≠0 → 按末 bar close 清算（`nav *= 1 − rate·|pos|`，回合 reason="end"）。
7. **指标**：`total_return = nav−1`；`max_drawdown = max(1 − nav/运行峰值)`；`turnover = Σ|Δ|`（含清算）；`buy_and_hold` = 首个执行 bar 开盘 → 末 bar 收盘（同执行口径）。

## 4. 架构

### 4.1 SimState / Context / DSL
```rust
// features/context.rs
#[derive(Debug, Clone)]
pub struct SimState { pub pos: f64, pub entry_price: f64, pub bars_held: usize, pub unreal_pnl: f64 }
impl Default for SimState { fn default() -> Self { Self { pos: 0.0, entry_price: f64::NAN, bars_held: 0, unreal_pnl: 0.0 } } }
// Context 加 pub sim: SimState（Default 注入；涟漪：Context 字面量补 sim: SimState::default()）
// build_context 不加参（打分模式恒默认）；sim 循环构建 ctx 后直接赋 ctx.sim = state。
```
`dsl/eval.rs` Ident 特判（hour/dow 旁）：`pos/entry_price/bars_held/unreal_pnl` → Scalar。loader `RESERVED_IDENTS` 扩入这 4 名（params/factors 不得遮蔽）。

### 4.2 risk 块
`TreeSpec` 加 `#[serde(default)] risk: Option<RiskSpec>`；`RiskSpec { stop_loss: Option<f64>, take_profit: Option<f64>, max_hold_bars: Option<usize> }`；runtime `Tree.risk: Option<Risk>`（同形，f64 > 0 / usize ≥ 1 校验）。

### 4.3 模拟器（`src/backtest/sim.rs`，新）
- 纯记账步进：`pub fn sim_step(state: &mut SimAccount, prev_close: f64, open: f64, close: f64, target: f64, rate: f64, exec_date: NaiveDate) -> StepOutcome`——T+1 钳制、三段 nav、状态更新、回合开/关事件。**黄金路径手算测试以表达式链断言**（如 `0.999*1.02*(10.6/10.2)`），不硬编码长小数。
- `pub async fn run_sim(cfg: &BacktestConfig, llm: &LlmEvaluator, soft: bool) -> Result<SimReport>`：顺序循环（无 buffered），每步 traverse / traverse_soft 取 target，风控覆盖，调 sim_step；聚合指标；写 `sim_report.json` + 可选 traces。
- `SimReport { tree_name, cost_bps, total_return, max_drawdown, n_round_trips, win_rate, avg_hold_bars, turnover, buy_and_hold, trades: Vec<RoundTrip>, overlap 无关 }`；`RoundTrip { entry_t, exit_t, entry_px, exit_px, max_abs_pos, trip_return, bars_held, reason }`（reason: tree/stop/tp/max_hold/end）。serde Serialize+Deserialize。
- traces：`SimStepRecord { t, target, pos, nav }` JSONL。

### 4.4 CLI
`backtest --sim`：分流 `run_sim(&cfg, &llm, soft)`；`--sim` 下 `--folds` 忽略 + eprintln 提示。打印 `print_sim_summary`。

## 5. 错误处理
risk 校验失败/树加载错 → 加载期；价格 ≤0 已由 reader 防（open>0 校验在 forward_return 有，sim 对 prev_close/open 0 防御 → Error::Data）；其余无新运行时错误路径。LLM 失败照旧回退 default。

## 6. 测试
- `sim_step` 黄金路径（开仓/持有/出场/再开仓/期末清算，nav 表达式链断言）；T+1 同日加→减顺延；翻向=关旧开新；成本 |Δ| 口径；部分加仓加权均价。
- 风控：stop/tp/max_hold 各一例（优先于树）；风控后 reason 记录。
- DSL：4 标识符默认值（非 sim ctx）；`entry_price` NaN 弃权；loader 遮蔽校验。
- run_sim：小树小数据集成（硬）；软 target=E 连续调仓一例；旧树（无 pos 条件）可跑。
- e2e：硬/软 sim 全链路 + 真数据 smoke（手动）。
- 文档：cli-reference（--sim）、tree-yaml-schema（risk 块 + 4 标识符保留名）、dsl-reference（持仓标识符）、architecture（第三模式）、README。

## 7. 里程碑
- **T1** SimState/Context（字面量涟漪）+ DSL 4 标识符 + RESERVED 扩展 + 测试。
- **T2** `risk:` schema/loader + 校验测试。
- **T3** `sim.rs` `SimAccount`/`sim_step` 纯记账 + 黄金路径/T+1/翻向测试。
- **T4** `run_sim` 编排（硬/软 target + 风控覆盖 + 回合聚合）+ SimReport/traces/print。
- **T5** CLI `--sim` 分流（--folds 忽略提示）+ e2e（硬/软）。
- **T6** 文档五处 + 真数据 smoke + example sim 树。
