# rquant：净仓位口径（position_net）— 设计文档

- **日期**：2026-06-10
- **状态**：设计已确认（三特性批次之①；②堆叠面积图、③自动模糊 DSL 各有独立 spec）
- **关联**：软遍历/软量化/LLM 分布已合并 master（HEAD `4871796`）。软打分目前只有逐腿期望 `expected_net = Σ p·net(stance)`；启用 short 后多空在分布中共存时，逐腿口径把两腿独立计费，与"现实只交易净额"不符。

---

## 1. 目标与非目标

### 目标
1. `score_soft` 新增**净仓位口径**：`exposure = Σ p·dir(stance)`（long +1 / flat 0 / short −1），`position_net = exposure·r − (cost_bps/1e4)·|exposure|`（`r = exit/entry − 1` 裸收益；exposure=0 → 0）。
2. `SoftScore` 加 `exposure`/`position_net`；`SoftMetrics` 加 `position: SignalStat`（对 |exposure|>0 的已计分点的 position_net）。
3. `print_soft_summary` 与 `render_soft_html` headline 各加 position 行。
4. **并列指标**：现有 `expected_net`（逐腿）保留对照；soft traces（`SoftStepRecord`）不动。

### 非目标（YAGNI）
- 替换 expected_net；改 soft traces 格式；硬模式任何改动；按腿计费的"gross 双边"第三口径。

## 2. 锁定决策

| # | 维度 | 选定 |
|---|---|---|
| 1 | 口径 | 净敞口 + 只交易净额：`position_net = E·r − rate·|E|`，`rate = cost_bps/1e4` |
| 2 | 落点 | `SoftScore`/`SoftMetrics` 新增并列字段；traces 不动 |
| 3 | 统计 | `position = signal_stat(position_net where |exposure|>0)` |

## 3. 实现要点

### score_soft（`backtest/soft.rs`）
- 循环内累加 `exposure += p * dir(stance)`。
- 循环后 `let r = forward_return(primary, i, fw, Stance::Long, costs)?.gross;`（裸收益；逐腿循环已用相同 i/fw 通过边界检查，故此处必 Some）。
- `position_net = if exposure == 0.0 { 0.0 } else { exposure * r - (costs.round_trip_bps / 10_000.0) * exposure.abs() }`。
- `SoftScore { expected_net, engaged, exposure, position_net, t1_executable }`。

### soft_metrics / 展示
- 收集 `position_nets: Vec<f64>`（`|s.exposure| > 0.0` 的已计分点）→ `position: signal_stat(&position_nets)`。
- `print_soft_summary` 加一行 `position: n/mean/hit/t`；`render_soft_html` headline 加 `position n` 与 `position mean_net` 两行。

### 字段涟漪（编译耦合，同任务切）
`SoftScore` 字面量：`score_soft` 构造 + `soft_metrics_aggregates_engaged` 测试（3 处）。`SoftMetrics` 字面量：`soft_metrics` 构造 + viz 测试 `render_soft_html_is_self_contained`。

## 4. 关键不变量（测试即自检）
- **long/flat 等价**：分布只含 long/flat 时 `position_net ≡ expected_net`（数学：`E=p_long`，`E·r−rate·E = p_long·(r−rate) = p_long·net_long`；成本线性）。
- **对冲净额**：构造 `{long:0.6, short:0.4}` 分布 + 已知价格 → `E=0.2`，`position_net = 0.2·r − rate·0.2` 精确值；而 `expected_net` 双腿各自计费（两者差 = 0.6·rate+0.4·rate−0.2·rate = 0.8·rate 的成本差）。
- **全 flat**：`exposure=0`、`position_net=0`、不入 position 统计。

## 5. 测试
- 单测（`backtest/soft.rs`）：long/flat 等价；对冲已知值；全 flat 零；`soft_metrics` 的 position 统计只含 |E|>0 点。
- viz：headline 含 position 行（更新既有自包含测试的 SoftMetrics 字面量）。
- e2e：既有软 e2e 加断言 `report.soft.position.count > 0`（上升趋势 long 质量 → 有敞口）。

## 6. 里程碑
- **T1**（耦合，一次切）`SoftScore`/`score_soft`/`SoftMetrics`/`soft_metrics` + print/render 展示 + 全部字面量涟漪 + 单测。
- **T2** e2e 断言 + README 一段。
