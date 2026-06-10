# 架构说明

本文档基于 `src/` 目录实际模块结构整理，描述当前实现状态。

---

## 九层架构

```
CSV 文件
  └─ data/         数据层
       └─ features/    特征/Context 层
            └─ dsl/        DSL 层
                 └─ tree/       树层
                      └─ eval/       评估器层
                           └─ engine/     引擎层
                                └─ backtest/   回测度量层
                                     └─ report/    报告层
                                          └─ cli/       命令行层
```

### 1. 数据层（`src/data/`）

负责所有外部数据的读写与交易日历。`bar.rs` 定义核心数据结构 `Bar`（一根 K 线的 OHLCV）与 `Window`（有序 bar 切片）；`reader.rs` 读取 CSV，验证时间严格递增与 high≥low；`sina.rs` 实现新浪财经 K 线抓取（`fetch_sina_klines`，带重试，`scale=240` 为日线别名）；`calendar.rs` 实现 `AShareCalendar`（工作日 + 节假日集合）与 `read_holidays`（YYYY-MM-DD 一行，`#` 注释）；`news.rs` 定义新闻记录与 `NewsView`（最近 5 条切片）；`gaps.rs` 在 primary 序列上检测缺失交易日与不完整交易日。抓取与回测完全解耦：回测只读本地 CSV 快照，不发起网络请求。

### 2. 特征 / Context 层（`src/features/`）

`indicators.rs` 实现所有技术指标纯函数（sma/ema/wma/rsi/atr/slope/highest/lowest/crossover/crossunder/macd_line/macd_signal/macd_hist/std）；`context.rs` 的 `build_context` 在任意时刻 t 构建 `Context`：以 `partition_point(time ≤ t)` 为闸门，截取 primary 与 context 的最近 `window` 根可见 bar（**防未来函数**），以及 time≤t 的最近 5 条新闻。Context 是整个评估管线中唯一能被节点看到的信息结构。

### 3. DSL 层（`src/dsl/`）

实现量化谓词的词法分析（`lexer.rs`）、语法分析（`parser.rs`，Pratt 算符优先解析）、AST（`ast.rs`，`Expr` 枚举）与求值（`eval.rs`）。支持三种求值模式：`eval` 返回 `Value`（Scalar/Series/Bool）；`eval_bool` 用于分支 when；`eval_scalar` 用于 strength；`eval_fuzzy` 用于 `strength: "auto"`（Gödel 模糊逻辑）。NaN 弃权语义：所有比较（包括 `==` / `!=`）在任一操作数为 NaN 时返回 false，预热期自动弃权走 default。

### 4. 树层（`src/tree/`）

`schema.rs` 定义 YAML 的 serde 类型（`TreeSpec`/`NodeSpec`/`BranchSpec`/`LeafSpec`/`Meta`/`Stance`）；`loader.rs` 将 YAML 编译为运行时树（`Tree`），同时解析所有 DSL 表达式、解析 `strength`（`Strength::Expr` / `Strength::Auto(scale)`），并执行完整的五项校验：root 必须是节点、无悬空引用、所有节点可达、DAG 无环、叶子 stance 在 `meta.stances` 内。报错精确到节点 id 与表达式内容。

### 5. 评估器层（`src/eval/`）

`quant.rs` 实现硬模式（`eval_quant`，首真分支）与软模式（`quant_branch_dist`，首真泄漏）；`llm/` 子模块实现 LLM 节点评估，包含 prompt 渲染（`prompt.rs`）、OpenAI 标准客户端（`client.rs`，带重试）、文件缓存（`cache.rs`，content-addressed sha256 键）和分发枚举（`mod.rs`：`LlmEvaluator::OpenAi/Disabled/Stub`）。`Decision` 结构携带 goto/label/confidence/rationale，供引擎与 Trace 消费。

### 6. 引擎层（`src/engine/`）

`traversal.rs` 的 `traverse` 实现硬遍历：从 root 出发，每步调用量化或 LLM 评估器，产出 `Trace`（完整路径 + 叶子 stance）；`soft.rs` 的 `traverse_soft` 实现软遍历：两阶段（async 收边 + 记忆化求叶子分布），产出 `SoftTrace`（叶子概率分布，Σ=1）；`trace.rs` 定义 `Trace`/`StepRecord`/`SoftTrace` 数据结构。引擎层不含任何金融逻辑。

### 7. 回测度量层（`src/backtest/`）

`forward_return.rs` 计算前瞻收益（`t+1` 开盘起 `forward_window` 根 bar，扣往返成本 bps）；`costs.rs` 定义 `CostModel`；`metrics.rs` 聚合硬模式度量（按叶子/按 stance/整体，含 buy&hold 基准、t 值、重叠警告）；`soft.rs` 的 `score_soft` 计算期望净收益与净仓位 `position_net`，`soft_metrics` 聚合软模式度量；`walkforward.rs` 按时间等分 K 折，每折独立出度量；`gaps.rs` 检测数据缺口；`runner.rs` 将以上组件串联为完整的硬模式回测流程（异步有序并发，`buffered(concurrency)`）；`soft.rs` 同构地实现软模式流程。

`sim.rs` 实现第三种运行模式——**顺序权益模拟**（`--sim`）：`SimAccount` 持有持仓/净值/峰值等状态；`sim_step` 按三段记账（旧仓段、成本段、新仓段）步进，内含 T+1 同日禁减仓约束；`finalize` 期末强制清算；`run_sim` 顺序（无并发）逐 bar 调度树遍历 + 风控覆盖 + `sim_step`，输出 `SimReport`（含 `Vec<RoundTrip>` 回合列表）与可选逐步 traces。成本口径：单边 `(cost_bps/2)/1e4 × |Δ|`，一进一出合计往返成本；T+1 口径：同自然日加仓当日禁减仓（整体顺延）。完整记账语义见 `docs/superpowers/specs/2026-06-10-rquant-e4-sim-design.md` §3。

### 8. 报告层（`src/report/`）

`mod.rs` 定义 `Report`/`SoftReport` 的序列化结构，实现 JSON 写出（`serde_json::to_string_pretty`，确定性）与 JSONL trace 写出；`curve.rs` 从 traces 推导累计前瞻收益曲线、直方图、叶子概率堆叠；`viz.rs` 将所有数据渲染为自包含 HTML（内联 SVG）。

### 9. 命令行层（`src/cli/`）

`mod.rs` 用 clap 定义三个子命令（`backtest`/`fetch`/`report`），调用各层业务函数，处理 LLM 配置（三项非空则启用 `OpenAiLlm`，否则 `Disabled`），将软/硬模式路由到对应的 runner 与报告函数。

---

## 数据流

```
CSV
 │  read_bars_csv
 ▼
Vec<Bar>
 │  build_context(primary, context, news, t, window)
 ▼                          ↑ 防未来函数闸门（time ≤ t）
Context { primary, context, news }
 │
 ├─ 硬遍历: traverse(tree, ctx, llm)
 │    └─ eval_quant / llm.eval_llm  →  Decision  →  Trace { path, leaf, stance }
 │         └─ forward_return(primary, i, fw, stance, costs)  →  ForwardResult
 │              └─ compute_metrics  →  Metrics
 │                   └─ walk_forward (若 --folds≥2)  →  WalkForward
 │                        └─ write_report + write_traces_jsonl
 │
 ├─ 软遍历: traverse_soft(tree, ctx, llm)
 │    └─ quant_branch_dist / llm.eval_llm_dist  →  SoftTrace { leaf_probs }
 │         └─ score_soft  →  SoftScore { expected_net, position_net }
 │              └─ soft_metrics  →  SoftMetrics
 │                   └─ write_soft_report + write_soft_traces_jsonl
 │
 └─ 模拟 (--sim): 顺序，无并发，每 bar 注入 SimState
      ├─ 硬: traverse(tree, ctx, llm)  →  target = stance×weight
      └─ 软: traverse_soft(tree, ctx, llm)  →  target = Σp·w·dir
           └─ 风控覆盖 (stop/tp/max_hold)  →  sim_step(&mut SimAccount, ...)
                └─ finalize  →  SimReport { total_return, max_drawdown, trades, … }
                     └─ print_sim_summary

report JSON + traces JSONL
 │  render_report_files
 ▼
自包含 HTML（内联 SVG）
```

---

## 与原始设计 spec 的偏离

原始设计见 `docs/superpowers/specs/2026-06-09-rquant-decision-tree-backtest-design.md`（§16 等）。

### 已实现（spec §16 预留接口中描述为"未来工作"的）

| 功能 | 实现位置 |
|---|---|
| 软/概率遍历（`--soft`） | `engine/soft.rs`、`backtest/soft.rs` |
| probs 协议（label 概率分布） | `eval/llm/prompt.rs`（`parse_answer`），`eval/llm/mod.rs`（`dist_to_gotos`） |
| strength / auto 强度表达式 | `tree/loader.rs`（`parse_strength`）、`eval/quant.rs`（`quant_branch_dist`） |
| `position_net`（净仓位口径） | `backtest/soft.rs`（`score_soft`） |
| Walk-forward（`--folds K`） | `backtest/walkforward.rs`、`backtest/runner.rs` |
| HTML 可视化（累计曲线、直方图、堆叠图） | `report/viz.rs`、`report/curve.rs` |
| 新浪 fetcher（`fetch` 子命令） | `data/sina.rs`、`cli/mod.rs` |
| 持仓状态模拟（`--sim`，顺序权益模拟） | `backtest/sim.rs`、`cli/mod.rs` |

### 设计中提及但尚未实现

| 功能 | spec 位置 | 状态 |
|---|---|---|
| SQLite / Parquet 缓存层 | §5、§6、§9 | 未实现；数据读写仅 CSV；`data/` 无缓存模块 |
| 涨跌停可执行性过滤 | §8 | 未实现；`t1_executable` 字段标记 T+1 可执行性，但无涨跌停判断 |
| `SymbolMeta`（标的元数据） | §6（Context 结构） | 未实现；`Context` 无 `meta` 字段 |
| 随机基准对比 | §10 | 未实现 |

---

## 复现性不变量

以下属性由代码实现保证，支撑回测的可复现性：

1. **BTreeMap 排序**：所有按键迭代的映射（`node_label_counts`、`by_leaf`、`by_stance`、`leaf_probs`、LLM `probs`）使用 `BTreeMap`，遍历顺序确定性（字典序）。
2. **确定性 prompt 渲染**：`render_user`（`eval/llm/prompt.rs`）中 label 排序后拼接（`labels.sort_unstable()`），价格定宽格式（`.4f`），inputs 按声明顺序追加。
3. **Content-addressed 缓存**：LLM 缓存键 = `sha256(model \0 base_url \0 system_prompt \0 node_id \0 rendered)`；更换端点、修改系统提示词或 prompt 时旧缓存自动失效。
4. **字节稳定的 JSON / HTML**：`write_report` 用 `serde_json::to_string_pretty`，字段顺序由 serde 派生保证；`render_html` 产出的 HTML 对相同输入字节一致（测试 `write_report_is_deterministic`）。
5. **原子缓存写入**：`FileCache::put` 先写唯一临时文件再 rename，并发写同一键不产生半截文件。
