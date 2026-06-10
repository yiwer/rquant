# rquant 净仓位口径（position_net）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 软打分新增净仓位口径：`exposure = Σ p·dir`，`position_net = E·r − rate·|E|`，作为 `expected_net`（逐腿）的并列指标，进 SoftMetrics/摘要/HTML headline。

**Architecture:** 在 master(HEAD `e0037f7`)上扩展。`SoftScore`/`SoftMetrics` 加字段（耦合涟漪一次切：score_soft/soft_metrics 构造 + 两处测试字面量）；traces 不动；硬模式零改动。`rate = cost_bps/1e4`（与 `CostModel::apply` 的扣减一致）。

**Tech Stack:** Rust 2024 + 既有。

> 设计依据：`docs/superpowers/specs/2026-06-10-rquant-net-position-design.md`。提交信息用英文。

---

## 文件结构
```
改动: src/backtest/soft.rs   # SoftScore + exposure/position_net；score_soft；SoftMetrics + position；soft_metrics；测试涟漪+新测试
改动: src/report/mod.rs      # print_soft_summary 加 position 行
改动: src/report/viz.rs      # render_soft_html headline 加 position 两行 + 测试字面量涟漪
改动: tests/e2e.rs、README.md
```

---

## Task 1: 核心耦合切换（SoftScore/score_soft/SoftMetrics/soft_metrics + 展示 + 涟漪 + 单测）

**Files:**
- Modify: `src/backtest/soft.rs`、`src/report/mod.rs`、`src/report/viz.rs`
- Test: `src/backtest/soft.rs`（新测试）+ 既有测试字面量更新

- [ ] **Step 1: 在 `src/backtest/soft.rs` 的 `mod tests` 加失败测试**

（测试模块已有 `bar()` 助手与 `TREE` 常量（long/flat 量化树）、`load_tree_str`。）
```rust
    #[test]
    fn position_equals_expected_for_long_flat() {
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
        // long/flat 下净仓位 ≡ 逐腿期望（成本线性）
        assert!((s.position_net - s.expected_net).abs() < 1e-12);
        assert!((s.exposure - 0.5).abs() < 1e-9);
    }

    #[test]
    fn position_nets_out_hedged_legs() {
        // 树要含 short：行内构造三叶树
        const TREE_LS: &str = r#"
meta: { name: t, forward_window: 2, stances: [long, flat, short] }
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "close > 0", goto: leaf_l, label: up } ]
    default: { goto: leaf_f, label: flat }
leaves:
  leaf_l: { stance: long }
  leaf_s: { stance: short }
  leaf_f: { stance: flat }
"#;
        let tree = load_tree_str(TREE_LS).unwrap();
        let primary = vec![
            bar("2024-01-02 14:45:00", 9.0, 9.5),
            bar("2024-01-02 15:00:00", 10.0, 10.2),
            bar("2024-01-03 09:45:00", 10.2, 11.0),
        ];
        let costs = CostModel { round_trip_bps: 10.0 };
        let mut lp = BTreeMap::new();
        lp.insert("leaf_l".to_string(), 0.6);
        lp.insert("leaf_s".to_string(), 0.4);
        let soft = SoftTrace { t: primary[0].time, leaf_probs: lp };
        let s = score_soft(&soft, &tree, &primary, 0, 2, &costs).unwrap();
        // r = 11/10 - 1 = 0.1, rate = 0.001
        // E = 0.6 - 0.4 = 0.2；position_net = 0.2*0.1 - 0.001*0.2 = 0.0198
        assert!((s.exposure - 0.2).abs() < 1e-9);
        assert!((s.position_net - 0.0198).abs() < 1e-9);
        // 逐腿：0.6*(0.1-0.001) + 0.4*(-0.1-0.001) = 0.0594 - 0.0404 = 0.019
        assert!((s.expected_net - 0.019).abs() < 1e-9);
    }

    #[test]
    fn all_flat_has_zero_exposure_and_position() {
        let tree = load_tree_str(TREE).unwrap();
        let primary = vec![
            bar("2024-01-02 14:45:00", 9.0, 9.5),
            bar("2024-01-02 15:00:00", 10.0, 10.2),
            bar("2024-01-03 09:45:00", 10.2, 11.0),
        ];
        let costs = CostModel { round_trip_bps: 10.0 };
        let mut lp = BTreeMap::new();
        lp.insert("leaf_f".to_string(), 1.0);
        let soft = SoftTrace { t: primary[0].time, leaf_probs: lp };
        let s = score_soft(&soft, &tree, &primary, 0, 2, &costs).unwrap();
        assert_eq!(s.exposure, 0.0);
        assert_eq!(s.position_net, 0.0);
    }
```
并把既有 `soft_metrics_aggregates_engaged` 测试改为同时验证 position 统计（3 个 `SoftScore` 字面量补字段 + 新断言）：
```rust
    #[test]
    fn soft_metrics_aggregates_engaged() {
        let items = vec![
            Some(SoftScore { expected_net: 0.04, engaged: 0.5, exposure: 0.5, position_net: 0.04, t1_executable: true }),
            Some(SoftScore { expected_net: -0.02, engaged: 0.3, exposure: -0.3, position_net: -0.02, t1_executable: false }),
            Some(SoftScore { expected_net: 0.0, engaged: 0.0, exposure: 0.0, position_net: 0.0, t1_executable: false }),
            None,
        ];
        let m = soft_metrics(&items, &[]);
        assert_eq!(m.total_decisions, 4);
        assert_eq!(m.scored, 3);
        assert_eq!(m.engaged.count, 2);
        assert_eq!(m.position.count, 2); // 仅 |exposure|>0 两点
    }
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test --lib backtest::soft`
Expected: 编译失败（`SoftScore` 无 exposure/position_net；`SoftMetrics` 无 position）。

- [ ] **Step 3: 实现（`src/backtest/soft.rs`）**

(a) `SoftScore` 加字段：
```rust
#[derive(Debug, Clone, Copy)]
pub struct SoftScore {
    pub expected_net: f64,
    pub engaged: f64,
    pub exposure: f64,
    pub position_net: f64,
    pub t1_executable: bool,
}
```
(b) `score_soft`：循环内累加 exposure，循环后算 position_net：
```rust
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
    let mut exposure = 0.0;
    let mut t1 = false;
    for (leaf_id, &p) in &soft.leaf_probs {
        let stance = tree.leaves.get(leaf_id)?.stance;
        let fr = forward_return(primary, i, fw, stance, costs)?;
        expected_net += p * fr.net;
        exposure += p * match stance {
            Stance::Long => 1.0,
            Stance::Short => -1.0,
            Stance::Flat => 0.0,
        };
        if !matches!(stance, Stance::Flat) {
            engaged += p;
        }
        t1 |= fr.t1_executable;
    }
    // 净仓位口径：只交易净额 E，成本计在 |E| 上（r=裸收益；逐腿循环已过边界检查，此处必 Some）
    let r = forward_return(primary, i, fw, Stance::Long, costs)?.gross;
    let position_net = if exposure == 0.0 {
        0.0
    } else {
        exposure * r - (costs.round_trip_bps / 10_000.0) * exposure.abs()
    };
    Some(SoftScore { expected_net, engaged, exposure, position_net, t1_executable: t1 })
}
```
(c) `SoftMetrics` 加字段 `pub position: SignalStat,`（`engaged` 之后）。
(d) `soft_metrics` 收集并聚合：
```rust
    let mut engaged_nets: Vec<f64> = Vec::new();
    let mut position_nets: Vec<f64> = Vec::new();
    for s in items.iter().flatten() {
        scored += 1;
        if s.engaged > 0.0 {
            engaged_nets.push(s.expected_net);
        }
        if s.exposure.abs() > 0.0 {
            position_nets.push(s.position_net);
        }
    }
```
构造处加 `position: signal_stat(&position_nets),`。

- [ ] **Step 4: 展示层**

(a) `src/report/mod.rs` `print_soft_summary` 在 engaged 行之后加：
```rust
    println!(
        "position: n={} mean_net={:.4} hit={:.1}% t={:.2}",
        m.position.count, m.position.mean_net, m.position.hit_rate * 100.0, m.position.t_stat
    );
```
(b) `src/report/viz.rs` `render_soft_html` headline 表在 engaged 行之后加：
```rust
    let _ = write!(s, "<tr><th>position n</th><td>{}</td></tr>", m.position.count);
    let _ = write!(s, "<tr><th>position mean_net</th><td>{:.4}</td></tr>", m.position.mean_net);
```
(c) `src/report/viz.rs` 测试 `render_soft_html_is_self_contained` 的 `SoftMetrics` 字面量补 `position: signal_stat(&[0.1, 0.2]),`。

- [ ] **Step 5: 运行验证通过**

Run: `cargo test --lib backtest::soft`
Expected: 既有 + 3 新测试 PASS（含等价/对冲/全平）。
Run: `cargo test`
Expected: 全量全绿。
Run: `cargo clippy --all-targets`
Expected: 无告警（平铺执行，勿用 `2>&1`）。

- [ ] **Step 6: Commit**

```bash
git add src/backtest/soft.rs src/report/mod.rs src/report/viz.rs
git commit -m "feat(backtest): net-position scoring (exposure, position_net) alongside expected_net" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: e2e 断言 + README

**Files:**
- Modify: `tests/e2e.rs`（既有软 e2e 加断言）、`README.md`

- [ ] **Step 1: `tests/e2e.rs` 的 `soft_mode_yields_positive_engaged_edge` 断言区追加**

```rust
    assert!(report.soft.position.count > 0, "uptrend long mass => nonzero exposure points");
    assert!(report.soft.position.mean_net > 0.0, "net-position metric should also be positive on uptrend");
```

- [ ] **Step 2: 运行验证**

Run: `cargo test --test e2e soft_mode_yields_positive_engaged_edge`
Expected: PASS。
Run: `cargo test`
Expected: 全量全绿。

- [ ] **Step 3: README**（`--soft` 一节补一段）

````markdown
软报告含两套口径：`engaged`（逐腿期望 `Σ p·net`，每腿各自计成本）与 `position`（净仓位：`E = Σ p·dir`，
`position_net = E·裸收益 − rate·|E|`，多空抵消后只交易净额）。long/flat 树下二者数学等价；
启用 short 且多空共存时 `position` 是更贴近实际执行的口径。
````

- [ ] **Step 4: Commit**

```bash
git add tests/e2e.rs README.md
git commit -m "test(e2e): assert net-position metric; docs: position vs per-leg" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## 附录 A：Spec 覆盖对照（自检）
| Spec § | 实现于 |
|---|---|
| §3 score_soft（exposure/r/position_net）| Task 1 |
| §3 SoftMetrics.position + 展示两处 | Task 1 |
| §3 字面量涟漪（SoftScore×3 + SoftMetrics viz 测试）| Task 1 |
| §4 不变量（等价/对冲/全平）| Task 1 测试 |
| §5 e2e + README | Task 2 |

## 附录 B：明确不在范围（YAGNI）
- 替换 expected_net；soft traces 加 position 字段；硬模式改动；双边 gross 第三口径。
