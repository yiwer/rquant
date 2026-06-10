# rquant：soft traces 文件 — 设计文档

- **日期**：2026-06-10
- **状态**：设计已确认（写 spec + 计划）
- **关联**：软遍历/软量化已合并 master（HEAD `270c8a2`）。软遍历落地时把逐点 soft traces 延后（`run_soft` 见 `--traces` 仅 eprintln 告警不写）；本设计补上。

---

## 1. 背景

硬模式 `run` 在 `--traces` 给出时写 `traces.jsonl`（每点 `{t, path, leaf, stance}`）。软模式 `run_soft` 当前：`eval_point_soft` 求完 `SoftTrace` 即丢、只留 `SoftScore`，`--traces` 给出时仅打印"软模式暂不写 traces"。本设计让软模式也写逐点 traces（`{t, leaf_probs, expected_net}`），并消掉该告警。纯增量：硬模式、现有软度量、`SoftReport` 不变。本期**只产出文件**；`report` 消费软 traces 画曲线是后续独立一步。

## 2. 目标与非目标

### 目标
1. 软模式 `--traces <path>` 给出时写 JSONL，每决策点一条 `{t, leaf_probs, expected_net}`（越界/未计分点 `expected_net=null`）。
2. 删掉"`--traces` is not written in `--soft` mode yet"告警。
3. `SoftStepRecord` 带 `Serialize`+`Deserialize`（为将来 report 软曲线消费）。

### 非目标（YAGNI / 后续）
- `report` 消费软 traces 画软曲线/分布（后续）。
- 记录里加 `engaged`/`t1_executable` 字段（YAGNI）。
- 软 `SoftReport` 结构改动（不动）。

## 3. 锁定决策

| # | 维度 | 选定 |
|---|---|---|
| 1 | 记录形态 | 每决策点 `{t, leaf_probs, expected_net}`（未计分 `expected_net=null`）|
| 2 | 范围 | 所有决策点（含未计分点的叶子分布）|
| 3 | 触发 | 仅 `--traces`(cfg.traces_path) 给出时写；删旧告警 |
| 4 | 序列化 | `SoftStepRecord` 带 Serialize + Deserialize |
| 5 | 边界 | 只产出文件；report 软曲线消费留后续 |

## 4. 架构

### 4.1 `SoftStepRecord`（`backtest/soft.rs`）
```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct SoftStepRecord {
    pub t: NaiveDateTime,
    pub leaf_probs: BTreeMap<String, f64>,
    pub expected_net: Option<f64>,
}
```
（放 `backtest/soft.rs`：它已 `use` `SoftTrace`(engine/soft) 与 `SoftScore`。`leaf_probs` 取自 `SoftTrace`，`expected_net` 取自 `SoftScore`。）

### 4.2 `write_soft_traces_jsonl`（`report/mod.rs`）
```rust
pub fn write_soft_traces_jsonl(records: &[SoftStepRecord], path: &Path) -> Result<()>
```
逐行 `serde_json::to_string` + `writeln!`，与既有 `write_traces_jsonl` 同构。

### 4.3 `run_soft` 改动（`backtest/soft.rs`）
- `eval_point_soft` 返回类型由 `Result<Option<SoftScore>>` 改为 `Result<(SoftTrace, Option<SoftScore>)>`（保留 trace）：
  ```rust
  let soft = traverse_soft(tree, &ctx, llm).await?;
  let score = score_soft(&soft, tree, primary, i, fw, costs);
  Ok((soft, score))
  ```
- `run_soft` 的 `buffered` 收 `Vec<(SoftTrace, Option<SoftScore>)>`；
  - 聚合 metrics：`let scores: Vec<Option<SoftScore>> = results.iter().map(|(_, s)| *s).collect();`（`SoftScore: Copy`）→ `soft_metrics(&scores, &primary[start..])`（**逻辑不变**）。
  - 若 `cfg.traces_path` 给出 → 构造 `records`（`t`/`leaf_probs` from trace、`expected_net = s.map(|x| x.expected_net)`）→ `write_soft_traces_jsonl`。
  - **删掉** `if cfg.traces_path.is_some() { eprintln!("...not written...") }`。

## 5. 错误处理
- 写 traces 失败 → 冒泡 `Result`（同硬模式）。
- 不给 `--traces` → 不写、无告警、无行为变化。
- 空决策（warmup ≥ len）→ records 为空 → 写出空文件（0 行），不报错。

## 6. 测试
- `SoftStepRecord` 往返：serialize → deserialize 相等（含 `expected_net: None` 与 `Some`）。
- `write_soft_traces_jsonl`：N 条 → N 行、每行可 `from_str` 回 `SoftStepRecord`。
- e2e：软模式 + `--traces` → JSONL 文件存在、行数 = 决策点数、每行含 `leaf_probs`，计分点 `expected_net` 非 null；不给 `--traces` 时不写、无告警。
- 既有软测试不变（metrics 仍从 scores 聚合；硬模式零改动）。

## 7. 风险
1. **trace 体量**：每点一条 + leaf_probs map；大回测文件可能大（同硬 traces，可接受）。
2. **重构 eval_point_soft 返回类型**：内部签名变，须保证 metrics 聚合逻辑逐字等价（测试覆盖）。
3. **伪概率**：leaf_probs 仍是未校准伪概率（同软遍历，解读谨慎）。

## 8. 里程碑
- **T1** `SoftStepRecord` + `report::write_soft_traces_jsonl` + 往返/写入单测。
- **T2** `run_soft`：`eval_point_soft` 返回 `(SoftTrace, Option<SoftScore>)` + 写 traces + 删告警 + e2e + README。
