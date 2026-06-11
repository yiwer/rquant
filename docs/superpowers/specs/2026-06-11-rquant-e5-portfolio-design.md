# rquant：E5 — 横截面组合层（portfolio 子命令）— 设计文档

- **日期**：2026-06-11
- **状态**：设计已确认（写 spec + 计划）
- **关联**：master `2ba6f66`。差距分析 G8（最后一块）：单标的 → 多标的横截面选股。软敞口 E 是天然横截面分数。

---

## 1. 目标与非目标

### 目标
1. 新子命令 `rquant portfolio --tree t.yaml --universe universe.csv --top N --rebalance K [--soft] [--aux ...] --out port.json [--traces holdings.jsonl]`。
2. universe CSV（`symbol,primary[,context]`，context 缺省=primary）；公共时间线=全标的 bar 时间有序并集；每 K 个时间点调仓（首个调仓点为第 `--warmup` 个时间点）。
3. 调仓时刻 t：**新鲜**（恰有 bar 在 t）的标的打分（硬=叶 dir×weight；软=E）；`score>0` 取前 N **等权**（并列按 symbol 字典序）；不足取全部；零个→空仓。
4. 组合记账：区间收益=成员"t 时 close → t' 时最后已知 close"均值（停牌按最后已知价计价持有）；换手=Σ|Δw|、成本=`(cost_bps/2)/1e4×换手`；期末按市值报告（不强制清算）；**基准=全 universe 等权同口径无成本**。
5. `PortfolioReport` + 逐期 holdings traces + 摘要打印。

### 非目标（YAGNI / 后续）
- 做空腿；分数加权；行业/市值中性；新闻输入（--news 不开放）；打分并发（v1 顺序，LLM 靠缓存）；HTML 渲染（与 report --sim 一道留 follow-up）；T+1 强制（调仓间隔通常 ≥1 日，相邻调仓同自然日时一次性警告）。

## 2. 锁定决策

| # | 维度 | 选定 |
|---|---|---|
| 1 | 权重 | 等权 top-N；分数只排序不加权（伪概率不当量纲）|
| 2 | 方向 | 纯多头（score≤0 → 空仓该标的）|
| 3 | 停牌 | 打分期：不新鲜→当期出局；持有期：按最后已知 close 计价 |
| 4 | 基准 | universe 等权、同调仓时间线、同计价口径、无成本 |
| 5 | 期末 | 按市值报告（不清算，文档注明）|
| 6 | 破平 | score 并列按 symbol 字典序（确定性）|

## 3. 架构

### 3.1 universe 读取器（`src/data/universe.rs`，新）
```rust
pub struct UniverseEntry { pub symbol: String, pub primary: PathBuf, pub context: PathBuf }
pub fn read_universe_csv(path: &Path) -> Result<Vec<UniverseEntry>>
```
表头 `symbol,primary[,context]`；context 列缺失或空 → 取 primary；symbol 非空且唯一；按 symbol 字典序排序返回（确定性）。

### 3.2 组合引擎（`src/backtest/portfolio.rs`，新）
- **加载**：每标的 `read_bars_csv`（primary/context）；`--aux` 共享表。
- **时间线**：`BTreeSet<NaiveDateTime>` 并集 → Vec；调仓点 `timeline[warmup], timeline[warmup+K], ...`（越界即止；少于 2 个调仓点 → Error::Data 提示数据不足）。
- **打分** `score_symbol(sym_data, tree, llm, soft, aux, t, window) -> Result<Option<f64>>`：不新鲜 → None；否则 build_context（news 空）+ traverse/traverse_soft → 硬 `dir×weight` / 软 `E`。
- **选择** `select_top(scores: &[(String, f64)], n) -> Vec<(String, f64)>`：score>0、按 (score 降序, symbol 升序)、取前 n。
- **记账纯函数** `accrue(weights: &BTreeMap<String,f64>, px_start: &BTreeMap<String,f64>, px_end: &BTreeMap<String,f64>) -> f64`（区间收益；权重和可 <1，现金部分收益 0）+ `turnover(old, new) -> f64`。px = 最后已知 close（per-symbol partition_point）。
- **循环**：逐调仓点：选新成员 → `turnover` → `nav *= 1 − rate×turnover` → 下一调仓点（或末时间点）`accrue` → `nav *= 1 + r`；基准同节奏（全员等权、无成本）。逐期记录 `HoldingsRecord { t, nav, benchmark_nav, selected: Vec<(String, f64)> }`。
- 相邻调仓点同自然日 → 首次发现时 eprintln 一次 T+1 提示。

### 3.3 报告
```rust
pub struct PortfolioReport {
    pub tree_name: String, pub cost_bps: f64, pub top_n: usize, pub rebalance: usize,
    pub n_rebalances: usize, pub avg_members: f64,
    pub total_return: f64, pub max_drawdown: f64, pub turnover: f64,
    pub benchmark_return: f64,
    pub holdings: Vec<HoldingsRecord>,
}
```
（Serialize+Deserialize；`--traces` 把 holdings 逐行 JSONL——与内嵌并存，traces 便于流式消费。）`print_portfolio_summary`：总收益/基准/超额/回撤/换手/调仓次数/平均成员数。

### 3.4 CLI（`Cmd::Portfolio`）
`--tree --universe --top(默认5) --rebalance(默认16) --warmup(默认100) --window(默认100) --cost-bps(默认10) --soft --aux(重复) --out(默认 portfolio.json) --traces` + LLM 三件套（与 backtest 同）。

## 4. 测试
- universe 读取器：双列/三列/空 context 回退/重复 symbol 报错/排序。
- 时间线/新鲜度：错位时间戳并集正确；不新鲜标的 None。
- `select_top`：score>0 过滤、降序、字典序破平、不足 N。
- 记账黄金路径（表达式链）：3 标的 2 期手算（选入/换手/成本/停牌最后价计价/基准）；首个调仓全建仓换手=1。
- 集成：合成 3 标的（一只明显跑赢）top-1 → 组合收益≈该标的、跑赢基准。
- e2e + 真数据 smoke（拉 4 只真股票 60m 选 2）。
- 文档：cli-reference（portfolio 子命令全表）、README 一节、architecture 一段。

## 5. 里程碑
- **T1** universe 读取器 + 测试。
- **T2** `portfolio.rs` 骨架：时间线/新鲜度/打分/select_top + 测试。
- **T3** 记账纯函数（accrue/turnover）+ 黄金路径 + 组合循环 + 集成测试。
- **T4** CLI `Cmd::Portfolio` + PortfolioReport/traces/print + e2e。
- **T5** 文档 + 真数据 smoke。
