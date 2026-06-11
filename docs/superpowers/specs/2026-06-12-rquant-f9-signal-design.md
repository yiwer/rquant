# rquant：F-9 — signal 实盘通路（paper trading）— 设计文档

- **日期**：2026-06-12
- **状态**：设计已确认（写 spec + 计划）
- **关联**：master `b73b24c`。成熟度差距分析 F-9：从"回测报告"到"明早的单子"——最小可行实盘通路 = signal 子命令 + 系统调度。

---

## 1. 目标与非目标

### 目标
1. `rquant signal` 双口径（`--primary` xor `--universe` 互斥）：
   - **单标的 paper-sim**：state 文件存 SimAccount 快照，每次运行重放新 bar（与 `--sim` 同决策同记账），输出今日目标仓位 + Δ + reason + 纸面账户。
   - **组合清单**：最新公共时间点横截面 top-N vs 持仓 state → buy/sell/hold 清单。
2. `--commit` 落盘 state（默认 dry-run）；`--out` 结构化 JSON；单口径可选 `--fetch SYMBOL` 一条命令日常化。
3. **黄金不变量**：增量分多次 `--commit` ≡ 一次性全量重放（账户逐字段相等）。

### 非目标（YAGNI）
- 券商 API/vnpy 对接；推送/webhook（JSON 出口已留）；内置调度（文档给系统调度示例）；组合口径 --fetch（用户脚本循环）；多 state 并管。

## 2. 单标的重放语义（权威）
- 决策于 bar i 收盘、执行于 bar i+1 开盘（与 sim 同）。**可记账决策** = 拥有执行 bar 的决策（i ≤ len−2）；**悬挂决策** = 最新 bar（i = len−1）的决策——评估输出为今日信号，不记账。
- state.last_time = 已记账的最后**决策 bar** 时间。运行：重放 `time > last_time` 且 i ≤ len−2 的决策（fresh state 从 warmup 起）→ 悬挂决策评估（SimState 注入当前账户 + 风控覆盖照常，stop/tp/max_hold 可触发 → reason）→ 输出。
- **state 永远落后一根 bar**（悬挂决策明日获得执行价后自然被记账）→ 增量 ≡ 全量天然成立（黄金测试：同数据从中间任意切分两次 commit == 一次 commit，账户逐字段相等）。
- 不调用 `finalize`（持仓滚动，无期末清算）。
- 纸面口径：state 假设历史信号全部按 sim 口径成交（次开盘、cost_bps、T+1）；文档诚实声明。

## 3. state 文件
```rust
// backtest/sim.rs 新增（trip 为私有字段，转换必须住在 sim.rs）
#[derive(Serialize, Deserialize)] pub struct TripSnapshot { entry_t, entry_px, open_nav, max_abs_pos }
#[derive(Serialize, Deserialize)] pub struct AccountSnapshot {
    pos, entry_price: Option<f64> /*NaN↔None，serde_json 不允许 NaN*/, bars_held, nav, peak_nav,
    max_drawdown, turnover, last_increase_date: Option<NaiveDate>, trip: Option<TripSnapshot>,
}
impl SimAccount { pub fn snapshot(&self) -> AccountSnapshot; pub fn restore(s: &AccountSnapshot) -> SimAccount; }
```
```rust
// signal 模块
PaperState { version: u32 (=1), tree_name: String, last_time: Option<NaiveDateTime>, account: AccountSnapshot }
HoldingsState { version: u32 (=1), tree_name: String, last_time: Option<NaiveDateTime>, holdings: BTreeMap<String, f64> }
```
读取校验：版本 ≠ 1 / tree_name 与 `meta.name` 不符 / JSON 损坏 → **明确报错拒绝静默重置**（提示删除文件重建）。文件不存在 → fresh。

## 4. 组合语义
最新公共时间点 t_last；新鲜标的打分（`score_symbol` 复用，soft 旗标同义）；`select_top` → 等权目标；trades = 与 state.holdings 的 diff：`buy(0→w) / sell(w→0) / adjust(w→w') / hold`；fresh 数 < universe 数 → 打印提示（停牌出局照常）。`--commit` 更新 holdings + last_time。

## 5. 输出
- 单：`SignalReport::Single { t /*悬挂 bar 时间*/, target, current_pos, delta, reason, paper: PaperStats { nav, total_return, max_drawdown, bars_replayed, n_trips_total } }`（时间一律用 bar 时间，不用挂钟——确定性）。
- 组合：`SignalReport::Portfolio { t, n_fresh, targets: Vec<(String, f64)>, trades: Vec<TradeInstr { symbol, action, from_w, to_w }> }`。
- print 人话单子 + `--out` JSON；dry-run 显著提示"未落盘，--commit 提交"。

## 6. CLI
`Cmd::Signal`：`--tree --state`（必）；单：`--primary --context [--fetch SYMBOL --scale --datalen --adjust]`；组合：`--universe --top`；共享：`--soft --commit --out --warmup --window --cost-bps --aux LLM三件套`。互斥校验：primary xor universe；--fetch 仅单口径。`--fetch` 复用既有 fetch 臂逻辑（抽 `pub(crate) async fn run_fetch_to_csv(...)` 供 Fetch/Signal 两臂共用，行为零变）。

## 7. 测试
- 快照往返（entry NaN↔None、trip Some/None、版本/树名/损坏报错）。
- **黄金不变量**：合成多日数据任意切分点，两次 commit == 一次 commit（账户逐字段 + last_time）。
- 悬挂决策：风控触发（浮亏超 stop）→ reason="stop" 且未记账（state 不变）。
- 组合 diff：buy/sell/adjust/hold 四象限一例；fresh 过滤。
- e2e："两天"模拟（前缀 CSV commit → 全量 CSV commit → state == 全量一次 commit）；CLI 互斥报错。
- 真数据 smoke：sh600519 qfq 60m + regime 树 signal 两连跑（第二跑 bars_replayed=0 幂等）；组合 6 真股 top-2 清单。
- 文档：cli-reference（signal 全表 + 状态文件语义 + 纸面边界）+ README（日常一条命令 + Windows 任务计划程序示例行）。

## 8. 里程碑
- **T1** sim.rs 快照/恢复 + serde + 测试。
- **T2** signal 单标的重放引擎 + PaperState IO + 黄金不变量。
- **T3** 组合 diff + HoldingsState。
- **T4** CLI（互斥/--fetch 抽取复用/--commit/print/JSON）。
- **T5** e2e 两天模拟 + 文档 + 真数据 smoke。
