# rquant 软/概率遍历 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新增可选 `--soft` 软遍历：按置信度把概率质量沿 DAG 传播得叶子分布，按 `expected_net = Σ p·net` 打分，产出独立 `SoftReport`；硬模式默认且不变。

**Architecture:** 在 M1–M6+（HEAD `680255e`）上扩展。`engine/soft.rs` 两阶段传播（async 收边 + sync 求分布）；`backtest/soft.rs` 软打分/度量/编排；`report` 加独立 `SoftReport`；cli `--soft` 分流到 `run_soft`。复用 `Decision.confidence`/`eval_quant`/`eval_llm`/`forward_return`/`SignalStat`。零新依赖、不动 DSL/LLM 输出/BacktestConfig 字段。

**Tech Stack:** Rust 2024 + 既有（tokio/futures/serde/chrono）。

> 设计依据：`docs/superpowers/specs/2026-06-09-rquant-soft-traversal-design.md`。
> 关键不变量：**两阶段都按 `weight>0` 守卫**——0 质量分支不评估（省 LLM 调用）、不递归（避免引用未评估节点）。提交信息用英文。异步测试 `#[tokio::test]`。

---

## 文件结构
```
新增: src/engine/soft.rs       # SoftTrace + traverse_soft（两阶段）
改动: src/engine/mod.rs        # + pub mod soft;
新增: src/backtest/soft.rs     # SoftScore/score_soft + SoftMetrics/soft_metrics + run_soft
改动: src/backtest/mod.rs      # + pub mod soft;
改动: src/backtest/metrics.rs  # signal_stat 改 pub(crate)
改动: src/report/mod.rs        # + SoftReport / write_soft_report / print_soft_summary
改动: src/cli/mod.rs           # backtest 加 --soft，分流 run / run_soft
改动: tests/e2e.rs             # 新增软模式 e2e
改动: README.md                # 软遍历一节
```

---

## Task 1: engine/soft.rs — traverse_soft（两阶段传播）

**Files:**
- Create: `src/engine/soft.rs`
- Modify: `src/engine/mod.rs`（+ `pub mod soft;`）
- Test: 同文件

- [ ] **Step 1: `src/engine/mod.rs` 增加 `pub mod soft;`**

- [ ] **Step 2: 写失败测试（`src/engine/soft.rs`）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::bar::{Bar, Window};
    use crate::eval::llm::{LlmEvaluator, StubLlm};
    use crate::features::context::Context;
    use crate::tree::loader::load_tree_str;
    use chrono::NaiveDate;
    use std::collections::HashMap;

    fn ctx(closes: &[f64]) -> Context {
        let base = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        let bars: Vec<Bar> = closes.iter().enumerate().map(|(i, &c)| Bar {
            time: base + chrono::Duration::minutes(i as i64 * 15), open: c, high: c, low: c, close: c, volume: 1.0,
        }).collect();
        let t = bars.last().unwrap().time;
        Context { t, primary: Window { bars: bars.clone() }, context: Window { bars }, news: None }
    }

    const QUANT_TREE: &str = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "close > sma(close,3)", goto: leaf_l, label: up } ]
    default: { goto: leaf_f, label: flat }
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;

    const LLM_TREE: &str = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
root: a
nodes:
  a:
    type: llm
    prompt: "x"
    labels: { yes: leaf_l }
    default: leaf_f
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;

    const LLM_MERGE_TREE: &str = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
root: a
nodes:
  a:
    type: llm
    prompt: "x"
    labels: { yes: leaf_x }
    default: leaf_x
leaves:
  leaf_x: { stance: long }
"#;

    #[tokio::test]
    async fn quant_hard_path_is_single_leaf() {
        let tree = load_tree_str(QUANT_TREE).unwrap();
        let st = traverse_soft(&tree, &ctx(&[1.0, 2.0, 3.0, 4.0, 5.0]), &LlmEvaluator::Disabled).await.unwrap();
        assert_eq!(st.leaf_probs.len(), 1);
        assert!((st.leaf_probs["leaf_l"] - 1.0).abs() < 1e-9);
    }

    #[tokio::test]
    async fn llm_node_splits_by_confidence() {
        let tree = load_tree_str(LLM_TREE).unwrap();
        let ev = LlmEvaluator::Stub(StubLlm { answers: HashMap::from([("a".to_string(), "yes".to_string())]) });
        let st = traverse_soft(&tree, &ctx(&[1.0, 2.0, 3.0]), &ev).await.unwrap();
        assert!((st.leaf_probs["leaf_l"] - 0.9).abs() < 1e-9); // stub confidence 0.9
        assert!((st.leaf_probs["leaf_f"] - 0.1).abs() < 1e-9);
        let sum: f64 = st.leaf_probs.values().sum();
        assert!((sum - 1.0).abs() < 1e-9);
    }

    #[tokio::test]
    async fn merged_branches_sum_probability() {
        let tree = load_tree_str(LLM_MERGE_TREE).unwrap();
        let ev = LlmEvaluator::Stub(StubLlm { answers: HashMap::from([("a".to_string(), "yes".to_string())]) });
        let st = traverse_soft(&tree, &ctx(&[1.0]), &ev).await.unwrap();
        assert_eq!(st.leaf_probs.len(), 1);
        assert!((st.leaf_probs["leaf_x"] - 1.0).abs() < 1e-9);
    }
}
```

- [ ] **Step 3: 运行验证失败**

Run: `cargo test --lib engine::soft`
Expected: 编译失败（`traverse_soft` / `SoftTrace` 未定义）。

- [ ] **Step 4: 写实现（测试上方）**

```rust
use crate::eval::llm::{LlmEvaluator, LlmNode};
use crate::eval::quant::eval_quant;
use crate::features::context::Context;
use crate::tree::loader::{Node, Tree};
use crate::{Error, Result};
use chrono::NaiveDateTime;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, Serialize)]
pub struct SoftTrace {
    pub t: NaiveDateTime,
    pub leaf_probs: BTreeMap<String, f64>,
}

/// 置信度加权软遍历：质量按 (选中支: c, 残余 1-c → default) 沿 DAG 传播 → 叶子分布。
/// 两阶段：①async 收边（每可达节点评一次，weight>0 才探索）②sync 记忆化求叶子分布。
pub async fn traverse_soft(tree: &Tree, ctx: &Context, llm: &LlmEvaluator) -> Result<SoftTrace> {
    // 阶段一：收集 node -> (chosen_goto, c, default_goto)
    let mut edges: HashMap<String, (String, f64, String)> = HashMap::new();
    let mut stack: Vec<String> = vec![tree.root.clone()];
    while let Some(id) = stack.pop() {
        if tree.leaves.contains_key(&id) || edges.contains_key(&id) {
            continue;
        }
        let node = tree
            .nodes
            .get(&id)
            .ok_or_else(|| Error::Engine(format!("dangling node '{id}'")))?;
        let (decision, default_goto) = match node {
            Node::Quant { branches, default } => (eval_quant(branches, default, ctx)?, default.goto.clone()),
            Node::Llm { inputs, prompt, labels, default } => {
                let ln = LlmNode { inputs, prompt, labels, default };
                (llm.eval_llm(&id, &ln, ctx).await?, default.clone())
            }
        };
        let chosen = decision.goto.clone();
        let c = decision.confidence;
        // 仅探索 weight>0 的分支（避免评估 0 质量子树 / 多余 LLM 调用）
        if c > 0.0 && tree.nodes.contains_key(&chosen) {
            stack.push(chosen.clone());
        }
        if 1.0 - c > 0.0 && tree.nodes.contains_key(&default_goto) {
            stack.push(default_goto.clone());
        }
        edges.insert(id, (chosen, c, default_goto));
    }
    // 阶段二：记忆化求叶子分布
    let mut memo: HashMap<String, BTreeMap<String, f64>> = HashMap::new();
    let mut leaf_probs = leaf_dist(&tree.root, &edges, tree, &mut memo);
    leaf_probs.retain(|_, p| *p > 0.0);
    Ok(SoftTrace { t: ctx.t, leaf_probs })
}

fn leaf_dist(
    id: &str,
    edges: &HashMap<String, (String, f64, String)>,
    tree: &Tree,
    memo: &mut HashMap<String, BTreeMap<String, f64>>,
) -> BTreeMap<String, f64> {
    if tree.leaves.contains_key(id) {
        return BTreeMap::from([(id.to_string(), 1.0)]);
    }
    if let Some(m) = memo.get(id) {
        return m.clone();
    }
    let (chosen, c, default_goto) = edges[id].clone();
    let mut out: BTreeMap<String, f64> = BTreeMap::new();
    if c > 0.0 {
        for (leaf, p) in leaf_dist(&chosen, edges, tree, memo) {
            *out.entry(leaf).or_insert(0.0) += p * c;
        }
    }
    if 1.0 - c > 0.0 {
        for (leaf, p) in leaf_dist(&default_goto, edges, tree, memo) {
            *out.entry(leaf).or_insert(0.0) += p * (1.0 - c);
        }
    }
    memo.insert(id.to_string(), out.clone());
    out
}
```

- [ ] **Step 5: 运行验证通过**

Run: `cargo test --lib engine::soft`
Expected: 三个测试 PASS。

- [ ] **Step 6: Commit**

```bash
git add src/engine/soft.rs src/engine/mod.rs
git commit -m "feat(engine): soft traversal with confidence-weighted leaf distribution" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: backtest/soft.rs 打分/度量 + report SoftReport

**Files:**
- Modify: `src/backtest/metrics.rs`（`signal_stat` 改 `pub(crate)`）
- Create: `src/backtest/soft.rs`（`SoftScore`/`score_soft` + `SoftMetrics`/`soft_metrics`）
- Modify: `src/backtest/mod.rs`（+ `pub mod soft;`）
- Modify: `src/report/mod.rs`（+ `SoftReport` / `write_soft_report` / `print_soft_summary`）
- Test: `backtest/soft.rs` 同文件

- [ ] **Step 1: `src/backtest/metrics.rs` 把 `signal_stat` 改 `pub(crate)`**

找到 `fn signal_stat(nets: &[f64]) -> SignalStat {` 改为 `pub(crate) fn signal_stat(nets: &[f64]) -> SignalStat {`。

- [ ] **Step 2: `src/backtest/mod.rs` 增加 `pub mod soft;`**

- [ ] **Step 3: 写失败测试（`src/backtest/soft.rs`）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::soft::SoftTrace;
    use crate::tree::loader::load_tree_str;
    use chrono::NaiveDateTime;
    use std::collections::BTreeMap;

    fn bar(t: &str, open: f64, close: f64) -> Bar {
        Bar {
            time: NaiveDateTime::parse_from_str(t, "%Y-%m-%d %H:%M:%S").unwrap(),
            open, high: open.max(close), low: open.min(close), close, volume: 1.0,
        }
    }
    const TREE: &str = r#"
meta: { name: t, forward_window: 2, stances: [long, flat] }
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "close > 0", goto: leaf_l, label: up } ]
    default: { goto: leaf_f, label: flat }
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;

    #[test]
    fn score_soft_expected_net() {
        let tree = load_tree_str(TREE).unwrap();
        let primary = vec![
            bar("2024-01-02 14:45:00", 9.0, 9.5),
            bar("2024-01-02 15:00:00", 10.0, 10.2),
            bar("2024-01-03 09:45:00", 10.2, 11.0),
        ];
        let costs = CostModel { round_trip_bps: 10.0 };
        let mut lp = BTreeMap::new();
        lp.insert("leaf_l".to_string(), 0.5);
        lp.insert("leaf_f".to_string(), 0.5);
        let soft = SoftTrace { t: primary[0].time, leaf_probs: lp };
        let s = score_soft(&soft, &tree, &primary, 0, 2, &costs).unwrap();
        // long net = 11/10-1-0.001 = 0.099; flat = 0; expected = 0.5*0.099
        assert!((s.expected_net - 0.0495).abs() < 1e-9);
        assert!((s.engaged - 0.5).abs() < 1e-9);
        assert!(s.t1_executable);
    }

    #[test]
    fn score_soft_out_of_range_is_none() {
        let tree = load_tree_str(TREE).unwrap();
        let primary = vec![bar("2024-01-02 14:45:00", 9.0, 9.5), bar("2024-01-02 15:00:00", 10.0, 10.2)];
        let costs = CostModel { round_trip_bps: 10.0 };
        let mut lp = BTreeMap::new();
        lp.insert("leaf_l".to_string(), 1.0);
        let soft = SoftTrace { t: primary[0].time, leaf_probs: lp };
        assert!(score_soft(&soft, &tree, &primary, 1, 2, &costs).is_none());
    }

    #[test]
    fn soft_metrics_aggregates_engaged() {
        let items = vec![
            Some(SoftScore { expected_net: 0.04, engaged: 0.5, t1_executable: true }),
            Some(SoftScore { expected_net: -0.02, engaged: 0.3, t1_executable: false }),
            Some(SoftScore { expected_net: 0.0, engaged: 0.0, t1_executable: false }),
            None,
        ];
        let m = soft_metrics(&items, &[]);
        assert_eq!(m.total_decisions, 4);
        assert_eq!(m.scored, 3);
        assert_eq!(m.engaged.count, 2);
    }
}
```

- [ ] **Step 4: 运行验证失败**

Run: `cargo test --lib backtest::soft`
Expected: 编译失败（`SoftScore`/`score_soft`/`soft_metrics` 未定义）。

- [ ] **Step 5: 写 `src/backtest/soft.rs` 实现（测试上方）**

```rust
use crate::backtest::costs::CostModel;
use crate::backtest::forward_return::forward_return;
use crate::backtest::metrics::{signal_stat, SignalStat};
use crate::data::bar::Bar;
use crate::engine::soft::SoftTrace;
use crate::tree::loader::Tree;
use crate::tree::schema::Stance;
use serde::Serialize;

#[derive(Debug, Clone, Copy)]
pub struct SoftScore {
    pub expected_net: f64,
    pub engaged: f64,
    pub t1_executable: bool,
}

/// 按叶子分布求期望净收益；任一叶子前瞻越界(None) → 整点 None。
pub fn score_soft(
    soft: &SoftTrace,
    tree: &Tree,
    primary: &[Bar],
    i: usize,
    fw: usize,
    costs: &CostModel,
) -> Option<SoftScore> {
    let mut expected_net = 0.0;
    let mut engaged = 0.0;
    let mut t1 = false;
    for (leaf_id, &p) in &soft.leaf_probs {
        let stance = tree.leaves.get(leaf_id)?.stance;
        let fr = forward_return(primary, i, fw, stance, costs)?;
        expected_net += p * fr.net;
        if !matches!(stance, Stance::Flat) {
            engaged += p;
        }
        t1 = fr.t1_executable;
    }
    Some(SoftScore { expected_net, engaged, t1_executable: t1 })
}

#[derive(Debug, Serialize)]
pub struct SoftMetrics {
    pub total_decisions: usize,
    pub scored: usize,
    pub engaged: SignalStat,
    pub buy_and_hold: f64,
    pub overlap_warning: String,
}

/// 聚合软度量：engaged = 在 engaged>0 的已评分点上对 expected_net 做 SignalStat。
/// `primary` 应传评估窗口段（warmup 之后），buy_and_hold 同口径。
pub fn soft_metrics(items: &[Option<SoftScore>], primary: &[Bar]) -> SoftMetrics {
    let total = items.len();
    let mut scored = 0;
    let mut engaged_nets: Vec<f64> = Vec::new();
    for s in items.iter().flatten() {
        scored += 1;
        if s.engaged > 0.0 {
            engaged_nets.push(s.expected_net);
        }
    }
    let buy_and_hold = if primary.len() >= 2 {
        primary.last().unwrap().close / primary[0].open - 1.0
    } else {
        0.0
    };
    SoftMetrics {
        total_decisions: total,
        scored,
        engaged: signal_stat(&engaged_nets),
        buy_and_hold,
        overlap_warning: "前瞻窗口重叠 → 样本自相关，t 值偏乐观，勿据此鼓吹显著性".into(),
    }
}
```

- [ ] **Step 6: `src/report/mod.rs` 加 `SoftReport` + 函数**

在 `use` 区加 `use crate::backtest::soft::SoftMetrics;`，并在文件中追加：
```rust
#[derive(Debug, Serialize)]
pub struct SoftReport {
    pub tree_name: String,
    pub forward_window: usize,
    pub cost_bps: f64,
    pub soft: SoftMetrics,
}

pub fn write_soft_report(report: &SoftReport, path: &Path) -> Result<()> {
    let json = serde_json::to_string_pretty(report)?;
    std::fs::write(path, json)?;
    Ok(())
}

pub fn print_soft_summary(report: &SoftReport) {
    let m = &report.soft;
    println!("=== rquant SOFT backtest: {} ===", report.tree_name);
    println!("forward_window={} cost_bps={}", report.forward_window, report.cost_bps);
    println!("decisions={} scored={}", m.total_decisions, m.scored);
    println!(
        "engaged : n={} mean_expected_net={:.4} hit={:.1}% t={:.2}",
        m.engaged.count, m.engaged.mean_net, m.engaged.hit_rate * 100.0, m.engaged.t_stat
    );
    println!("buy&hold={:.4}", m.buy_and_hold);
    println!("[warn] {}", m.overlap_warning);
}
```

- [ ] **Step 7: 运行验证通过**

Run: `cargo test --lib backtest::soft`
Expected: 三个测试 PASS。
Run: `cargo build`
Expected: 通过。

- [ ] **Step 8: Commit**

```bash
git add src/backtest/metrics.rs src/backtest/soft.rs src/backtest/mod.rs src/report/mod.rs
git commit -m "feat(backtest): soft scoring (expected_net), SoftMetrics, SoftReport" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: run_soft 编排 + cli --soft

**Files:**
- Modify: `src/backtest/soft.rs`（追加 `eval_point_soft` + `run_soft`）
- Modify: `src/cli/mod.rs`（backtest 加 `--soft`，分流 `run`/`run_soft`）

> **参照**：`run_soft`/`eval_point_soft` 与 `src/backtest/runner.rs` 的 `run`/`eval_point` **同构**。先打开 `runner.rs` 看 `eval_point` 的确切签名（`build_context` 参数类型、`read_bars_csv`/`read_news_csv`/`load_tree_file` 用法、`buffered` 循环），软版照搬，只把"遍历+打分"换成 `traverse_soft`+`score_soft`、结果类型换成 `Option<SoftScore>`。本任务无单测（由 Task 4 e2e 覆盖），但须 `cargo build` + `cargo test`（既有全绿）+ `cargo clippy` 通过。

- [ ] **Step 1: 在 `src/backtest/soft.rs` 追加 `eval_point_soft` + `run_soft`**

文件顶部 `use` 区补：
```rust
use crate::backtest::runner::BacktestConfig;
use crate::data::news::{read_news_csv, NewsRecord};
use crate::data::reader::read_bars_csv;
use crate::engine::soft::traverse_soft;
use crate::eval::llm::LlmEvaluator;
use crate::features::context::build_context;
use crate::report::{write_soft_report, SoftReport};
use crate::tree::loader::load_tree_file;
use crate::Result;
use futures::stream::{self, StreamExt};
```
在测试模块之前追加：
```rust
#[allow(clippy::too_many_arguments)]
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
) -> Result<Option<SoftScore>> {
    let t = primary[i].time;
    let ctx = build_context(primary, context, news, t, window);
    let soft = traverse_soft(tree, &ctx, llm).await?;
    Ok(score_soft(&soft, tree, primary, i, fw, costs))
}

/// 软遍历回测：与 `run` 同构，每点用 traverse_soft + score_soft，聚合成 SoftReport。
pub async fn run_soft(cfg: &BacktestConfig, llm: &LlmEvaluator) -> Result<SoftReport> {
    let tree = load_tree_file(&cfg.tree_path)?;
    let primary = read_bars_csv(&cfg.primary_path)?;
    let context = read_bars_csv(&cfg.context_path)?;
    let news = match &cfg.news_path {
        Some(p) => read_news_csv(p)?,
        None => vec![],
    };
    let costs = CostModel { round_trip_bps: cfg.cost_bps };
    let fw = tree.meta.forward_window;
    let start = cfg.warmup.min(primary.len());
    let results: Vec<Option<SoftScore>> = stream::iter(start..primary.len())
        .map(|i| eval_point_soft(i, &primary, &context, &news, &tree, &costs, fw, cfg.window, llm))
        .buffered(cfg.concurrency.max(1))
        .collect::<Vec<Result<Option<SoftScore>>>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>>>()?;
    let metrics = soft_metrics(&results, &primary[start..]);
    let report = SoftReport {
        tree_name: tree.meta.name.clone(),
        forward_window: fw,
        cost_bps: cfg.cost_bps,
        soft: metrics,
    };
    write_soft_report(&report, &cfg.out_path)?;
    Ok(report)
}
```
> 若 `build_context`/`read_*`/`load_tree_file` 的确切签名与上面略有出入（如 primary 传 `&Window` 而非 `&[Bar]`），以 `runner.rs` 的 `eval_point`/`run` 为准对齐。

- [ ] **Step 2: `src/cli/mod.rs` 加 `--soft` 并分流**

`Cmd::Backtest { ... }` 变体加（`holidays` 旁）：
```rust
        /// Soft/probabilistic traversal: propagate confidence-weighted leaf distribution
        #[arg(long, default_value_t = false)]
        soft: bool,
```
`main` 的 `Cmd::Backtest { ... }` 解构加 `soft`，并把"构造 `BacktestConfig` 后 `run` + `print_summary`"那段改为分流：
```rust
            if soft {
                let report = crate::backtest::soft::run_soft(&cfg, &llm).await?;
                crate::report::print_soft_summary(&report);
            } else {
                let report = run(&cfg, &llm).await?;
                crate::report::print_summary(&report);
            }
```

- [ ] **Step 3: 验证**

Run: `cargo build` → 通过。
Run: `cargo test` → 既有全绿（本任务未加测试）。
Run: `cargo clippy --all-targets` → 无告警（平铺执行，勿用 `2>&1`）。
Run: `cargo run -- backtest --help` → 用法含 `--soft`。

- [ ] **Step 4: Commit**

```bash
git add src/backtest/soft.rs src/cli/mod.rs
git commit -m "feat(backtest,cli): run_soft orchestration and --soft flag" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: e2e（Stub 软全链路）+ README

**Files:**
- Modify: `tests/e2e.rs`（新增软模式 e2e）
- Modify: `README.md`（软遍历一节）

> **参照**：`tests/e2e.rs` 已有 `llm_node_changes_path_vs_disabled`，它构造了"含 LLM 节点的树 + Stub LLM + 上升趋势 CSV + `BacktestConfig`"。新测试**复用它的 fixture 构造方式**（同样的 tree YAML、`gen_primary_csv`/`gen_context_csv`、Stub answers、`BacktestConfig`），只把调用换成 `run_soft`，断言 `SoftReport`。

- [ ] **Step 1: 在 `tests/e2e.rs` 追加软模式测试**

```rust
#[tokio::test]
async fn soft_mode_yields_positive_engaged_edge() {
    // 复用 llm_node_changes_path_vs_disabled 的 fixture：含 LLM 节点的树、上升趋势数据、Stub 判 "go"。
    // 构造同款 tree YAML（llm 节点：标签→leaf_long、default→leaf_flat）、gen_primary_csv()/gen_context_csv()、
    // StubLlm{ answers: {<llm_node_id>: "go"} } 包成 LlmEvaluator::Stub、BacktestConfig（out_f 等）。
    // 然后：
    let report = rquant::backtest::soft::run_soft(&cfg, &ev).await.unwrap();
    let m = &report.soft;
    assert!(m.scored > 0, "should score points");
    assert!(m.engaged.count > 0, "soft mode should engage (some long mass)");
    assert!(m.engaged.mean_net > 0.0, "uptrend + judge go(c=0.9) → positive expected net");
    let content = std::fs::read_to_string(out_f.path()).unwrap();
    assert!(content.contains("engaged"));
}
```
> 把注释里的 fixture 用 `llm_node_changes_path_vs_disabled` 的真实代码替换（tree YAML、`cfg`、`ev`、`out_f`）。`ev` 是 `LlmEvaluator::Stub(...)`，判 "go"（c=0.9）。`run_soft` 第二参收 `&ev`。

- [ ] **Step 2: 运行验证**

Run: `cargo test --test e2e soft_mode_yields_positive_engaged_edge` → PASS。
Run: `cargo test` → 全量全绿。
Run: `cargo clippy --all-targets` → 无告警。

- [ ] **Step 3: README 加一节**（LLM 一节之后）

````markdown
## 软/概率遍历（`--soft`，可选）

默认是**硬遍历**：每节点选一支、走单路径到单叶。加 `--soft` 切换为**置信度加权软遍历**：
每节点按 `(选中支: confidence, 残余 1-c → default)` 把概率质量沿决策 DAG 传播，得**叶子概率分布**，
再按期望打分 `expected_net = Σ p(leaf)·net(leaf.stance)`，输出 `SoftReport`（`soft.engaged` 为参与点的期望净收益统计）。

```bash
cargo run --release -- backtest --tree examples/trend_tree.yaml \
  --primary 15m.csv --context 1h.csv --soft --out soft_report.json
```

说明：软效果目前体现在 **LLM 节点**（量化节点 confidence=1.0 仍硬）；软模式会评估所有可达节点
（含 LLM `default` 子树里的 LLM 节点），LLM 调用比硬模式多（有缓存兜底）。LLM 的 confidence
是"伪概率"、未做校准，叶子分布请谨慎解读。
````

- [ ] **Step 4: Commit**

```bash
git add tests/e2e.rs README.md
git commit -m "test(e2e): soft-mode full path; docs: --soft section" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## 附录 A：Spec 覆盖对照（自检）
| Spec 章节 | 实现于 |
|---|---|
| §4.1 traverse_soft 两阶段（weight>0 守卫）| Task 1 |
| §4.2 score_soft（Σ p·net、越界→None）| Task 2 |
| §4.3 SoftMetrics（复用 SignalStat）/ SoftReport / print_soft_summary | Task 2 |
| §4 signal_stat 改 pub(crate) | Task 2 |
| §4.4 run_soft 编排 + cli `--soft`（BacktestConfig 不变）| Task 3 |
| §6 测试（soft 单测 / 打分单测 / e2e Stub 全链路）| Task 1/2/4 |
| §5 错误处理（eval_llm 内回退；越界 None）| Task 1/2 |
| §7 风险（README 诚实说明）| Task 4 |

## 附录 B：明确不在范围（YAGNI）
- 软量化谓词、LLM label 完整分布、净仓位口径、概率校准、逐点 leaf_probs 文件输出（本期 `SoftReport` 仅含 `SoftMetrics`）。
- 软模式不做缺口检测（`SoftReport` 无 `gaps` 字段；缺口检测是硬模式 `run` 的功能）。
