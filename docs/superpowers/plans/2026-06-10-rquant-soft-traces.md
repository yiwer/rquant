# rquant soft traces 文件 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 软模式 `--traces` 给出时写逐点 JSONL（`{t, leaf_probs, expected_net}`），并删掉旧的"软模式 --traces 静默无效"告警。

**Architecture:** 在 master(HEAD `dd0ab7e`)上扩展。`backtest/soft.rs` 加 `SoftStepRecord`；`report/mod.rs` 加 `write_soft_traces_jsonl`；`run_soft` 的 `eval_point_soft` 改为保留 `SoftTrace`（返回 `(SoftTrace, Option<SoftScore>)`），按需写 traces。纯增量，硬模式/现有软度量不变。

**Tech Stack:** Rust 2024 + 既有（serde/serde_json/chrono）。

> 设计依据：`docs/superpowers/specs/2026-06-10-rquant-soft-traces-design.md`。提交信息用英文。

---

## 文件结构
```
改动: src/backtest/soft.rs   # + SoftStepRecord；eval_point_soft 返回 (SoftTrace, Option<SoftScore>)；run_soft 写 traces + 删告警
改动: src/report/mod.rs      # + write_soft_traces_jsonl
改动: tests/e2e.rs           # soft + --traces e2e
改动: README.md              # --soft 一节补 traces 说明
```

---

## Task 1: SoftStepRecord + write_soft_traces_jsonl

**Files:**
- Modify: `src/backtest/soft.rs`（`SoftStepRecord` + 往返测试）
- Modify: `src/report/mod.rs`（`write_soft_traces_jsonl`）
- Test: 两文件

- [ ] **Step 1: 在 `src/backtest/soft.rs` 的 `mod tests` 加往返失败测试**

```rust
    #[test]
    fn soft_step_record_round_trips() {
        use std::collections::BTreeMap;
        let mut lp = BTreeMap::new();
        lp.insert("leaf_l".to_string(), 0.7);
        lp.insert("leaf_f".to_string(), 0.3);
        let t = chrono::NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        let rec = SoftStepRecord { t, leaf_probs: lp, expected_net: Some(0.05) };
        let json = serde_json::to_string(&rec).unwrap();
        let back: SoftStepRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(back.t, t);
        assert_eq!(back.leaf_probs.len(), 2);
        assert_eq!(back.expected_net, Some(0.05));
        // None 也往返
        let rec2 = SoftStepRecord { t, leaf_probs: BTreeMap::new(), expected_net: None };
        let back2: SoftStepRecord = serde_json::from_str(&serde_json::to_string(&rec2).unwrap()).unwrap();
        assert_eq!(back2.expected_net, None);
    }
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test --lib backtest::soft::tests::soft_step_record_round_trips`
Expected: 编译失败（`SoftStepRecord` 未定义）。

- [ ] **Step 3: 加 `SoftStepRecord`（`src/backtest/soft.rs`）**

确保文件顶部 `use` 含（已有的保留，缺的补）：把现有 `use serde::Serialize;` 改为 `use serde::{Deserialize, Serialize};`（若没有 serde 行则新增该行）；并加 `use chrono::NaiveDateTime;` 与 `use std::collections::BTreeMap;`（若未导入）。然后在 `SoftScore` 定义附近加：
```rust
/// 软模式逐点 trace 记录：决策点时间、叶子分布、期望净收益（未计分点为 None）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoftStepRecord {
    pub t: NaiveDateTime,
    pub leaf_probs: BTreeMap<String, f64>,
    pub expected_net: Option<f64>,
}
```

- [ ] **Step 4: 在 `src/report/mod.rs` 加 `write_soft_traces_jsonl`**

顶部 `use` 加 `use crate::backtest::soft::SoftStepRecord;`（与现有 `use crate::backtest::soft::SoftMetrics;` 可合并为 `use crate::backtest::soft::{SoftMetrics, SoftStepRecord};`）。在 `write_soft_report` 附近加（`std::io::Write` 已在文件顶部导入）：
```rust
pub fn write_soft_traces_jsonl(records: &[SoftStepRecord], path: &Path) -> Result<()> {
    let mut f = std::fs::File::create(path)?;
    for r in records {
        let line = serde_json::to_string(r)?;
        writeln!(f, "{line}")?;
    }
    Ok(())
}
```

- [ ] **Step 5: 在 `src/report/mod.rs` 的 `mod tests` 加写入测试**

```rust
    #[test]
    fn soft_traces_jsonl_one_line_per_record() {
        use crate::backtest::soft::SoftStepRecord;
        use chrono::NaiveDate;
        use std::collections::BTreeMap;
        let t = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        let mut lp = BTreeMap::new();
        lp.insert("x".to_string(), 1.0);
        let recs = vec![
            SoftStepRecord { t, leaf_probs: lp.clone(), expected_net: Some(0.1) },
            SoftStepRecord { t, leaf_probs: lp, expected_net: None },
        ];
        let f = tempfile::NamedTempFile::new().unwrap();
        write_soft_traces_jsonl(&recs, f.path()).unwrap();
        let content = std::fs::read_to_string(f.path()).unwrap();
        assert_eq!(content.lines().count(), 2);
        let first: SoftStepRecord = serde_json::from_str(content.lines().next().unwrap()).unwrap();
        assert_eq!(first.expected_net, Some(0.1));
    }
```

- [ ] **Step 6: 运行验证通过**

Run: `cargo test --lib backtest::soft::tests::soft_step_record_round_trips`
Expected: PASS。
Run: `cargo test --lib report::tests::soft_traces_jsonl_one_line_per_record`
Expected: PASS。
Run: `cargo build`
Expected: 通过。

- [ ] **Step 7: Commit**

```bash
git add src/backtest/soft.rs src/report/mod.rs
git commit -m "feat(report): SoftStepRecord + write_soft_traces_jsonl" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: run_soft 写 traces + 删告警 + e2e + README

**Files:**
- Modify: `src/backtest/soft.rs`（`eval_point_soft` 返回类型 + `run_soft`）
- Modify: `tests/e2e.rs`、`README.md`

- [ ] **Step 1: 改 `eval_point_soft` 返回 `(SoftTrace, Option<SoftScore>)`**

把 `src/backtest/soft.rs` 的 `eval_point_soft`（当前返回 `Result<Option<SoftScore>>`）改为：
```rust
async fn eval_point_soft(
    i: usize,
    primary: &[Bar],
    context: &[Bar],
    news: &[NewsRecord],
    tree: &Tree,
    costs: &CostModel,
    fw: usize,
    window: usize,
    llm: &LlmEvaluator,
) -> Result<(SoftTrace, Option<SoftScore>)> {
    let t = primary[i].time;
    let ctx = build_context(primary, context, news, t, window);
    let soft = traverse_soft(tree, &ctx, llm).await?;
    let score = score_soft(&soft, tree, primary, i, fw, costs);
    Ok((soft, score))
}
```

- [ ] **Step 2: 改 `run_soft`（写 traces + 删告警）**

(a) 删掉这段（当前在 costs 之前）：
```rust
    if cfg.traces_path.is_some() {
        eprintln!("[rquant] note: --traces is not written in --soft mode yet (SoftReport carries expected_net only)");
    }
```
(b) 把 results 收集 + metrics 聚合那段改为：
```rust
    let results: Vec<(SoftTrace, Option<SoftScore>)> = stream::iter(start..primary.len())
        .map(|i| eval_point_soft(i, &primary, &context, &news, &tree, &costs, fw, cfg.window, llm))
        .buffered(cfg.concurrency.max(1))
        .collect::<Vec<Result<(SoftTrace, Option<SoftScore>)>>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>>>()?;
    let scores: Vec<Option<SoftScore>> = results.iter().map(|(_, s)| *s).collect();
    let metrics = soft_metrics(&scores, &primary[start..]);
    if let Some(tp) = &cfg.traces_path {
        let records: Vec<SoftStepRecord> = results
            .iter()
            .map(|(tr, s)| SoftStepRecord {
                t: tr.t,
                leaf_probs: tr.leaf_probs.clone(),
                expected_net: s.map(|x| x.expected_net),
            })
            .collect();
        crate::report::write_soft_traces_jsonl(&records, tp)?;
    }
```
（`SoftReport` 构造与 `write_soft_report` 不变。）

- [ ] **Step 3: 验证**

Run: `cargo test`
Expected: 既有全绿（含既有软测试；metrics 仍从 scores 聚合）。
Run: `cargo clippy --all-targets`
Expected: 无告警（平铺执行，勿用 `2>&1`）。

- [ ] **Step 4: e2e（`tests/e2e.rs`）**

> 复用既有 `soft_mode_yields_positive_engaged_edge` 的 fixture（含 LLM 节点的树 + Stub + 上升趋势 + `BacktestConfig`），但把 `traces_path` 设为一个 tempfile `.jsonl`，跑 `run_soft` 后断言 traces 写出。

```rust
#[tokio::test]
async fn soft_traces_written_when_path_given() {
    // 复用 soft_mode_yields_positive_engaged_edge 的 fixture（tree/primary/context/Stub ev），
    // 但 BacktestConfig.traces_path = Some(traces_f.path().to_path_buf())。
    let report = rquant::backtest::soft::run_soft(&cfg, &ev).await.unwrap();
    let content = std::fs::read_to_string(traces_f.path()).unwrap();
    let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), report.soft.total_decisions, "one line per decision point");
    let first: rquant::backtest::soft::SoftStepRecord = serde_json::from_str(lines[0]).unwrap();
    assert!(!first.leaf_probs.is_empty(), "each record carries a leaf distribution");
}
```
> 把注释展开为真实代码：照搬 `soft_mode_yields_positive_engaged_edge` 的 fixture，加 `traces_f`（`tempfile::Builder::new().suffix(".jsonl").tempfile().unwrap()`）并在 `BacktestConfig` 里 `traces_path: Some(traces_f.path().to_path_buf())`。`report.soft.total_decisions` 是写出的行数（每决策点一条）。

- [ ] **Step 5: 运行验证**

Run: `cargo test --test e2e soft_traces_written_when_path_given`
Expected: PASS。
Run: `cargo test`
Expected: 全量全绿。

- [ ] **Step 6: README 补一句**（`--soft` 一节内）

````markdown
软模式也支持 `--traces <file>`：写出逐点 JSONL（每决策点 `{t, leaf_probs, expected_net}`，未计分点 `expected_net` 为 null），可用于离线分析软遍历的叶子分布（report 软曲线消费为后续）。
````

- [ ] **Step 7: Commit**

```bash
git add src/backtest/soft.rs tests/e2e.rs README.md
git commit -m "feat(backtest): write soft traces jsonl when --traces given; drop stale warning" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## 附录 A：Spec 覆盖对照（自检）
| Spec § | 实现于 |
|---|---|
| §4.1 SoftStepRecord (Serialize+Deserialize) | Task 1 |
| §4.2 write_soft_traces_jsonl | Task 1 |
| §4.3 run_soft（eval_point_soft 返回 trace+score；写 traces；删告警）| Task 2 |
| §6 测试（往返/写入/e2e）| Task 1/2 |
| §5 错误处理（写失败冒泡；不给 --traces 不写无告警）| Task 2 |

## 附录 B：明确不在范围（YAGNI）
- report 消费软 traces 画软曲线（后续）；记录加 engaged/t1 字段；SoftReport 结构改动。
