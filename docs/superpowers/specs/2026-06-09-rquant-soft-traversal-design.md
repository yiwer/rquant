# rquant：软/概率遍历（置信度加权）— 设计文档

- **日期**：2026-06-09
- **状态**：设计已确认（待 spec 评审 → 进实现计划）
- **关联**：M1–M6 + 全部 follow-up 已合并 master（HEAD `680255e`）。本设计落地 M1–M4 spec §16 的"软/概率遍历"。

---

## 1. 背景

硬遍历每节点选一支、走单条路径到单叶 → 单立场。spec §16 指出风险："硬分支在顶层判错→下面全错；真实判断是概率的"，并预留 `Decision.confidence`。本次新增**可选**软遍历：按置信度把概率质量沿 DAG 传播，得叶子分布，按期望打分。硬遍历保持默认、不变。

## 2. 目标与非目标

### 目标
1. 可选 `--soft` 模式：节点按 `(选中支: confidence, 残余 1-c → default)` 传播质量 → 叶子概率分布。
2. 按期望打分：`expected_net = Σ_leaf p(leaf)·net(leaf.stance)`。
3. 独立 `SoftReport`，硬模式 `Report` 与全部既有行为/测试不变。
4. 复用 `Decision.confidence`、`eval_quant`/`eval_llm`、`forward_return`、`SignalStat`；不动 DSL、不改 LLM 输出格式、不改 `BacktestConfig` 字段。

### 非目标（YAGNI / 后续）
- 软量化谓词（量化 `when` → 概率）：量化节点 c=1 仍硬；软量化留作后续。
- LLM 返回完整 label 概率分布（现仍用 `{label, confidence}`，按 c/(1-c) 二分）。
- 净仓位口径（多空对冲抵消）：本默认 long/flat 下与"每叶期望"等价，故用每叶期望；short 启用后的净仓位口径留后续。
- 软遍历的可视化/概率校准。

## 3. 锁定决策

| # | 维度 | 选定 |
|---|---|---|
| 1 | 概率来源 | 置信度加权（复用 `Decision.confidence`；不动 DSL/LLM 输出）|
| 2 | 残余质量 (1-c) | → 节点 `default` 分支 |
| 3 | 打分 | `expected_net = Σ_leaf p(leaf)·forward_return(...,leaf.stance).net` |
| 4 | 模式 | `--soft` 可选；硬模式默认且不变 |
| 5 | 输出 | 独立 `SoftReport`（不动硬模式 `Report`）|
| 6 | 评估面 | 软模式评估**所有可达节点**（含 LLM default 子树里的 LLM 节点 → 更多 LLM 调用；有缓存兜底）|

## 4. 架构

### 组件
- `src/engine/soft.rs`（新）：`SoftTrace` + `traverse_soft`（两阶段传播）。
- `src/backtest/soft.rs`（新）：软打分（`score_soft`）+ `SoftMetrics` + `run_soft` 编排 +（可选）soft traces 写出。
- `src/report/mod.rs`（改）：加 `SoftReport` 结构 + `print_soft_summary`（不动现有 `Report`/`print_summary`）。
- `src/cli/mod.rs`（改）：backtest 加 `--soft` 布尔旗；据此调 `run`（硬）或 `run_soft`（软）。`BacktestConfig` **不加字段**（cli 分流）。
- `src/backtest/metrics.rs`（改）：把 `signal_stat` 由私有改 `pub(crate)`，供 soft 复用（`SignalStat` 已 pub）。

### 4.1 传播算法（`engine/soft.rs`）
两阶段，避免 async 递归：

**阶段一（async）—— 收集边**：
```
edges: HashMap<String, (chosen_goto: String, c: f64, default_goto: String)>
stack = [root]
while let Some(id) = stack.pop():
    if id 是叶子 或 edges.contains_key(id): continue
    node = tree.nodes[id]
    decision = match node { Quant{..} => eval_quant(..), Llm{..} => llm.eval_llm(..).await }  // 每节点评一次
    default_goto = node 的 default goto（Quant: default.goto；Llm: default）
    edges[id] = (decision.goto, decision.confidence, default_goto)
    push decision.goto, default_goto（若为节点）
```
记忆化（`edges.contains_key`）保证每可达节点只评一次、LLM 只调一次。

**阶段二（sync）—— 记忆化求叶子分布**：
```
fn leaf_dist(id, edges, leaves, memo) -> BTreeMap<String,f64>:
    if leaves.contains_key(id): return {id: 1.0}
    if memo[id]: return it
    (chosen, c, def) = edges[id]
    out = merge( scale(leaf_dist(chosen), c), scale(leaf_dist(def), 1-c) )  // 同叶概率相加
    memo[id] = out; return out
```
`SoftTrace { t: ctx.t, leaf_probs: BTreeMap<String,f64> }`（概率和 = 1）。

- 量化节点：命中支 c=1 → 全给该支；取 default → chosen=default、c=0.5 → 0.5+0.5 合并=1 给 default。⇒ 量化仍硬。
- LLM 节点：c<1 → 真软（c→labels[label]，1-c→default）。
- `chosen_goto == default_goto` 时两边合并（概率相加）。

### 4.2 打分（`backtest/soft.rs`）
```rust
pub struct SoftScore { pub expected_net: f64, pub engaged: f64, pub t1_executable: bool }

/// 对一个决策点：按叶子分布求期望净收益。任一叶子越界(None) → 整点 None。
pub fn score_soft(soft: &SoftTrace, tree: &Tree, primary: &[Bar], i: usize, fw: usize, costs: &CostModel) -> Option<SoftScore>
```
逐叶：`stance = tree.leaves[leaf].stance`；`fr = forward_return(primary,i,fw,stance,costs)?`（任一 None→返回 None）；累加 `expected_net += p*fr.net`；`engaged += p`（若 stance≠Flat）；`t1 = fr.t1_executable`（与方向无关，取任一）。

### 4.3 度量 / 报告
```rust
pub struct SoftMetrics {
    pub total_decisions: usize,
    pub scored: usize,
    pub engaged: SignalStat,   // 复用 SignalStat，over engaged(>0) 决策点的 expected_net
    pub buy_and_hold: f64,     // 同口径：primary[warmup..] 首开盘→末收盘
    pub overlap_warning: String,
}
pub struct SoftReport { pub tree_name: String, pub forward_window: usize, pub cost_bps: f64, pub soft: SoftMetrics }  // derive Serialize
```
`engaged` = `signal_stat(&[expected_net for scored points where engaged>0])`。`print_soft_summary` 打印 engaged 的 n/mean/hit/t + buy&hold + 重叠警告。

### 4.4 编排（`run_soft`）
与 `run` 同构（加载 tree/primary/context/news、构 calendar 缺口检测照旧、`buffered(N)` 并发），但每点用 `traverse_soft` + `score_soft`，聚合成 `SoftReport`，写 JSON + 可选 soft traces（每点 `{t, leaf_probs, expected_net}`）。cli：`if soft { run_soft } else { run }`。

## 5. 错误处理
- `traverse_soft`：LLM 失败已在 `eval_llm` 内回退 default（c=0、走 default）；无新错误路径。空叶分布不可能（概率和恒 1，root 必达叶）。
- `score_soft`：任一叶越界 → None（该点不计分），与硬模式一致。
- 概率守恒：阶段二每步 `c+(1-c)=1`，叶子分布和恒 1（可加调试断言）。

## 6. 测试
- `engine/soft.rs`（Stub LLM，无网络）：
  - LLM 节点 confidence=0.7 的树 → `leaf_probs = {leaf_buy:0.7, leaf_flat:0.3}`（和=1）。
  - 纯量化树 → 退化单叶 prob=1（与硬模式同叶）。
  - `chosen==default` / 菱形汇合 → 概率正确相加（和=1）。
- `backtest/soft.rs`：构造 `SoftTrace` + 合成价格 → `score_soft` 的 `expected_net = Σ p·net` 已知值；越界 → None；engaged 统计正确。
- e2e（Stub，无网络）：软全链路跑通，`SoftReport` 写出；同一上升趋势数据，软（Stub judge "go"，c=0.9）的 `engaged.mean_net` 在 0 与"纯多净收益"之间。
- 既有全部测试不变（硬模式零改动）。

## 7. 风险与诚实说明
1. **更多 LLM 调用**：软模式评估所有可达节点（含 default 子树的 LLM）；缓存缓解，但首轮成本高于硬模式。
2. **软效果集中在 LLM 节点**：量化节点 c=1 仍硬；量化密集的树软化有限，直到加软量化谓词（后续）。
3. **期望 vs 净仓位**：long/flat 下 `Σ p·net` = 按期望方向下注；short 启用后多空对冲口径有别（后续）。
4. **置信度未校准**：LLM 的 confidence 未必是真实概率；叶子分布是"伪概率"，解读需谨慎。

## 8. 里程碑
- **T1** `engine/soft.rs`：`SoftTrace` + `traverse_soft`（两阶段）+ 单测。
- **T2** `backtest/soft.rs`：`SoftScore`/`score_soft` + `SoftMetrics`（复用 `SignalStat`，需把 `signal_stat` 改 `pub(crate)`）+ `report::SoftReport`/`print_soft_summary` + 单测。
- **T3** `run_soft` 编排 + cli `--soft` 分流。
- **T4** e2e（Stub 软全链路）+ README 一节。
