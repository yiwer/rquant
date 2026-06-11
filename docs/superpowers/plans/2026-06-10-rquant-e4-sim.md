# rquant E4 持仓状态模拟（--sim）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `backtest --sim`（可配 `--soft`）：树产出目标仓位（硬=叶 stance×weight、软=E），顺序模拟器按差额交易、三段记账净值，risk 块（SL/TP/max_hold）覆盖，T+1 顺延，产出 SimReport+traces。

**Architecture:** 在 master(HEAD `f612a19`)上加第三种运行模式。`Context.sim: SimState`（默认注入，打分模式零影响）+ DSL 4 持仓标识符；`tree.risk` 块；`backtest/sim.rs` 纯记账 `sim_step` + 顺序 `run_sim`；CLI 分流。**记账语义以 spec §3 为权威**（`docs/superpowers/specs/2026-06-10-rquant-e4-sim-design.md`）——实现者先通读。

**Tech Stack:** Rust 2024 + 既有。

> 提交信息英文。黄金路径测试用**表达式链**断言（如 `0.999*1.02*(10.6/10.2)`），不硬编码长小数。

---

## 文件结构
```
改动: src/features/context.rs   # SimState + Context.sim（字面量涟漪 ~7 处补 sim: SimState::default()）
改动: src/dsl/eval.rs           # pos/entry_price/bars_held/unreal_pnl 标识符
改动: src/tree/{schema,loader}.rs  # risk 块 + RESERVED_IDENTS 扩 4 名
新增: src/backtest/sim.rs       # SimAccount/sim_step/finalize/RoundTrip/SimReport/SimStepRecord/run_sim/print_sim_summary
改动: src/backtest/mod.rs       # + pub mod sim;
改动: src/cli/mod.rs            # --sim 分流（--folds 忽略提示）
改动: tests/e2e.rs、examples/sim_tree.yaml、docs 五处、README.md
```

---

## Task 1: SimState + DSL 持仓标识符

**Files:**
- Modify: `src/features/context.rs`、`src/dsl/eval.rs`、`src/tree/loader.rs`（RESERVED_IDENTS）+ Context 字面量涟漪（grep `Context {`）

- [ ] **Step 1: context.rs**

```rust
/// --sim 模式注入的持仓状态；打分模式恒为默认（pos=0/entry=NaN/…）。
#[derive(Debug, Clone)]
pub struct SimState {
    pub pos: f64,
    pub entry_price: f64,
    pub bars_held: usize,
    pub unreal_pnl: f64,
}
impl Default for SimState {
    fn default() -> Self {
        Self { pos: 0.0, entry_price: f64::NAN, bars_held: 0, unreal_pnl: 0.0 }
    }
}
```
`Context` 加 `pub sim: SimState,`；`build_context` 构造处 `sim: SimState::default(),`。涟漪：grep 全部 `Context {` 字面量（约 7 处测试助手）补 `sim: SimState::default(),`。

- [ ] **Step 2: eval.rs 标识符（RED→GREEN）**

测试：
```rust
    #[test]
    fn sim_state_identifiers() {
        let mut ctx = ctx_from_closes(&[1.0]);
        let f = |src: &str, c: &Context| eval(&parse_str(src).unwrap(), c).unwrap();
        // 默认（非 sim）：pos=0、bars_held=0、unreal=0；entry_price=NaN → 比较弃权
        assert_eq!(f("pos == 0", &ctx), Value::Bool(true));
        assert_eq!(f("bars_held == 0", &ctx), Value::Bool(true));
        assert_eq!(f("unreal_pnl == 0", &ctx), Value::Bool(true));
        assert_eq!(f("entry_price > 0", &ctx), Value::Bool(false)); // NaN 弃权
        // 注入后可见
        ctx.sim = crate::features::context::SimState { pos: 0.5, entry_price: 10.0, bars_held: 3, unreal_pnl: -0.02 };
        assert_eq!(f("pos > 0 and bars_held >= 3", &ctx), Value::Bool(true));
        assert_eq!(f("unreal_pnl < -0.01 and entry_price == 10", &ctx), Value::Bool(true));
    }
```
实现：`Expr::Ident` match 加四臂（hour/dow 旁）：
```rust
            "pos" => Ok(Value::Scalar(ctx.sim.pos)),
            "entry_price" => Ok(Value::Scalar(ctx.sim.entry_price)),
            "bars_held" => Ok(Value::Scalar(ctx.sim.bars_held as f64)),
            "unreal_pnl" => Ok(Value::Scalar(ctx.sim.unreal_pnl)),
```

- [ ] **Step 3: loader RESERVED_IDENTS 扩为 12（加 4 名）+ 测试：params 定义 `pos: 1.0` → 加载错。**

- [ ] **Step 4: `cargo test` 全绿 + clippy 干净 + Commit**

```bash
git add -A src
git commit -m "feat(features,dsl): SimState in Context with pos/entry_price/bars_held/unreal_pnl identifiers" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: risk 块

**Files:**
- Modify: `src/tree/schema.rs`、`src/tree/loader.rs`

- [ ] **Step 1: RED 测试（loader）**

```rust
    #[test]
    fn risk_block_parsed_and_validated() {
        let yaml = |risk: &str| format!(r#"
meta: {{ name: t, forward_window: 3, stances: [long, flat] }}
{risk}
root: a
nodes:
  a:
    type: quant
    branches: [ {{ when: "close > 1", goto: leaf_l, label: up }} ]
    default: {{ goto: leaf_f, label: flat }}
leaves:
  leaf_l: {{ stance: long }}
  leaf_f: {{ stance: flat }}
"#);
        let t = load_tree_str(&yaml("risk: { stop_loss: 0.05, max_hold_bars: 60 }")).unwrap();
        let r = t.risk.as_ref().unwrap();
        assert!((r.stop_loss.unwrap() - 0.05).abs() < 1e-12);
        assert_eq!(r.max_hold_bars, Some(60));
        assert!(r.take_profit.is_none());
        assert!(load_tree_str(&yaml("")).unwrap().risk.is_none());
        assert!(load_tree_str(&yaml("risk: { stop_loss: -0.1 }")).is_err());
        assert!(load_tree_str(&yaml("risk: { max_hold_bars: 0 }")).is_err());
    }
```

- [ ] **Step 2: 实现**

schema：`#[derive(Debug, Deserialize)] pub(crate) struct RiskSpec { pub(crate) stop_loss: Option<f64>, pub(crate) take_profit: Option<f64>, pub(crate) max_hold_bars: Option<usize> }`（字段 `#[serde(default)]`）；`TreeSpec` 加 `#[serde(default)] risk: Option<RiskSpec>`。
loader：`#[derive(Debug, Clone)] pub struct Risk { pub stop_loss: Option<f64>, pub take_profit: Option<f64>, pub max_hold_bars: Option<usize> }`；`Tree.risk: Option<Risk>`；校验 `stop_loss/take_profit > 0`、`max_hold_bars ≥ 1`。

- [ ] **Step 3: 全绿 + Commit**

```bash
git add src/tree/schema.rs src/tree/loader.rs
git commit -m "feat(tree): optional risk block (stop_loss/take_profit/max_hold_bars)" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: sim.rs 纯记账核心（黄金路径）

**Files:**
- Create: `src/backtest/sim.rs`；Modify: `src/backtest/mod.rs`（+ `pub mod sim;`）

- [ ] **Step 1: 类型 + sim_step + finalize（实现全文；先写测试再实现按 TDD 走）**

```rust
use chrono::{NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};

/// 平仓回合记录。reason: tree/stop/tp/max_hold/end。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundTrip {
    pub entry_t: NaiveDateTime,
    pub exit_t: NaiveDateTime,
    pub entry_px: f64,
    pub exit_px: f64,
    pub max_abs_pos: f64,
    pub trip_return: f64,
    pub bars_held: usize,
    pub reason: String,
}

#[derive(Debug)]
struct OpenTrip {
    entry_t: NaiveDateTime,
    entry_px: f64,
    open_nav: f64,
    max_abs_pos: f64,
}

/// 模拟账户（spec §3 为记账权威）。
#[derive(Debug)]
pub struct SimAccount {
    pub pos: f64,
    pub entry_price: f64,
    pub bars_held: usize,
    pub nav: f64,
    pub peak_nav: f64,
    pub max_drawdown: f64,
    pub turnover: f64,
    pub last_increase_date: Option<NaiveDate>,
    trip: Option<OpenTrip>,
}

impl Default for SimAccount {
    fn default() -> Self {
        Self {
            pos: 0.0, entry_price: f64::NAN, bars_held: 0,
            nav: 1.0, peak_nav: 1.0, max_drawdown: 0.0, turnover: 0.0,
            last_increase_date: None, trip: None,
        }
    }
}

const EPS: f64 = 1e-12;

impl SimAccount {
    fn close_trip(&mut self, exit_t: NaiveDateTime, exit_px: f64, reason: &str) -> Option<RoundTrip> {
        let trip = self.trip.take()?;
        Some(RoundTrip {
            entry_t: trip.entry_t,
            exit_t,
            entry_px: trip.entry_px,
            exit_px,
            max_abs_pos: trip.max_abs_pos,
            trip_return: self.nav / trip.open_nav - 1.0,
            bars_held: self.bars_held,
            reason: reason.to_string(),
        })
    }
}

/// 一步执行+记账：决策于上根 bar 收盘的 target，在本 bar（prev_close→open→close）执行。
/// 返回本步平掉的回合（翻向时为旧回合）。T+1：同自然日加过仓 → 减仓/翻向顺延（本步不交易）。
pub fn sim_step(
    acc: &mut SimAccount,
    prev_close: f64,
    open: f64,
    close: f64,
    exec_t: NaiveDateTime,
    target: f64,
    rate: f64,
    reason: &str,
) -> Option<RoundTrip> {
    let mut target = target.clamp(-1.0, 1.0);
    let reduces = acc.pos.abs() > EPS
        && (target.abs() < acc.pos.abs() - EPS || target * acc.pos < -EPS);
    if reduces && acc.last_increase_date == Some(exec_t.date()) {
        target = acc.pos; // T+1 顺延
    }
    // 段1：旧仓 prev_close→open
    acc.nav *= 1.0 + acc.pos * (open / prev_close - 1.0);
    let delta = target - acc.pos;
    let mut closed = None;
    if delta.abs() > EPS {
        acc.nav *= 1.0 - rate * delta.abs();
        acc.turnover += delta.abs();
        let old = acc.pos;
        let flat_or_flip = old.abs() > EPS && (target.abs() <= EPS || target * old < -EPS);
        if flat_or_flip {
            closed = acc.close_trip(exec_t, open, reason);
            acc.entry_price = f64::NAN;
            acc.bars_held = 0;
        }
        if target.abs() > EPS {
            if old.abs() <= EPS || target * old < -EPS {
                // 自 flat 开仓 / 翻向开新
                acc.trip = Some(OpenTrip { entry_t: exec_t, entry_px: open, open_nav: acc.nav, max_abs_pos: target.abs() });
                acc.entry_price = open;
                acc.bars_held = 0;
                acc.last_increase_date = Some(exec_t.date());
            } else if target.abs() > old.abs() + EPS {
                // 加仓：加权均价
                acc.entry_price = (acc.entry_price * old.abs() + open * (target.abs() - old.abs())) / target.abs();
                acc.last_increase_date = Some(exec_t.date());
            }
            // 部分减仓：entry 不变
        }
        acc.pos = target;
    }
    // 段2：新仓 open→close
    acc.nav *= 1.0 + acc.pos * (close / open - 1.0);
    if acc.pos.abs() > EPS {
        acc.bars_held += 1; // 开仓执行 bar 收盘即为 1（spec §3.5）
        if let Some(trip) = acc.trip.as_mut() {
            trip.max_abs_pos = trip.max_abs_pos.max(acc.pos.abs());
        }
    }
    acc.peak_nav = acc.peak_nav.max(acc.nav);
    acc.max_drawdown = acc.max_drawdown.max(1.0 - acc.nav / acc.peak_nav);
    closed
}

/// 期末清算：仍持仓 → 按末收盘计成本平仓（reason="end"）。
pub fn finalize(acc: &mut SimAccount, last_t: NaiveDateTime, last_close: f64, rate: f64) -> Option<RoundTrip> {
    if acc.pos.abs() <= EPS {
        return None;
    }
    acc.nav *= 1.0 - rate * acc.pos.abs();
    acc.turnover += acc.pos.abs();
    let closed = acc.close_trip(last_t, last_close, "end");
    acc.pos = 0.0;
    acc.entry_price = f64::NAN;
    acc.bars_held = 0;
    acc.peak_nav = acc.peak_nav.max(acc.nav);
    acc.max_drawdown = acc.max_drawdown.max(1.0 - acc.nav / acc.peak_nav);
    closed
}
```

- [ ] **Step 2: 黄金路径测试（表达式链断言；先写→RED→实现→GREEN）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
    }

    #[test]
    fn golden_walk_enter_hold_exit() {
        // bars: b0 c=10 | b1 o=10 c=10.2 | b2 o=10.4 c=10.6 | b3 o=10.8 c=10.6
        // rate=0.001。i0: target 1 → exec b1；i1: hold → b2 无交易；i2: target 0 → exec b3 平仓。
        // 注意：执行时间须跨自然日（入场日 T+1 禁止当日平仓——纯记账路径用三天展开）
        let mut acc = SimAccount::default();
        let rt1 = sim_step(&mut acc, 10.0, 10.0, 10.2, t("2024-01-02 10:00:00"), 1.0, 0.001, "tree");
        assert!(rt1.is_none());
        assert_relative_eq!(acc.nav, 0.999 * (10.2 / 10.0), epsilon = 1e-12);
        assert_relative_eq!(acc.entry_price, 10.0);
        assert_eq!(acc.bars_held, 1);
        let rt2 = sim_step(&mut acc, 10.2, 10.4, 10.6, t("2024-01-03 10:00:00"), 1.0, 0.001, "tree");
        assert!(rt2.is_none());
        assert_relative_eq!(acc.nav, 0.999 * (10.6 / 10.0), epsilon = 1e-12); // 连续持仓 = 链式收益
        assert_eq!(acc.bars_held, 2);
        let rt3 = sim_step(&mut acc, 10.6, 10.8, 10.6, t("2024-01-04 10:00:00"), 0.0, 0.001, "tree").unwrap();
        // 平仓后 nav = 0.999*(10.8/10.0)*0.999；段2 pos=0 不变
        assert_relative_eq!(acc.nav, 0.999 * (10.8 / 10.0) * 0.999, epsilon = 1e-12);
        assert_eq!(acc.pos, 0.0);
        assert!(acc.entry_price.is_nan());
        assert_eq!(rt3.exit_px, 10.8);
        assert_eq!(rt3.bars_held, 2);
        assert_eq!(rt3.reason, "tree");
        // trip_return 以回合 open_nav（入场成本后、入场 bar 段2 前）为基：
        // open_nav = 0.999；close 时 nav = 0.999×(10.8/10)×0.999 → trip_return = (10.8/10)×0.999 − 1
        assert_relative_eq!(rt3.trip_return, (10.8 / 10.0) * 0.999 - 1.0, epsilon = 1e-12);
        assert_relative_eq!(acc.turnover, 2.0);
    }

    #[test]
    fn t1_defers_same_day_reduction() {
        let mut acc = SimAccount::default();
        // 同一自然日：开仓后立刻请求平仓 → 顺延；次日可平
        sim_step(&mut acc, 10.0, 10.0, 10.0, t("2024-01-02 10:00:00"), 1.0, 0.0, "tree");
        let r = sim_step(&mut acc, 10.0, 10.0, 10.0, t("2024-01-02 10:15:00"), 0.0, 0.0, "tree");
        assert!(r.is_none());
        assert_eq!(acc.pos, 1.0); // 被顺延
        let r2 = sim_step(&mut acc, 10.0, 10.0, 10.0, t("2024-01-03 09:45:00"), 0.0, 0.0, "tree");
        assert!(r2.is_some());
        assert_eq!(acc.pos, 0.0);
    }

    #[test]
    fn flip_closes_old_and_opens_new() {
        let mut acc = SimAccount::default();
        sim_step(&mut acc, 10.0, 10.0, 10.0, t("2024-01-02 10:00:00"), 1.0, 0.0, "tree");
        let closed = sim_step(&mut acc, 10.0, 10.0, 10.0, t("2024-01-03 10:00:00"), -0.5, 0.001, "tree").unwrap();
        assert_eq!(closed.exit_px, 10.0);
        assert_eq!(acc.pos, -0.5);
        assert_relative_eq!(acc.entry_price, 10.0);
        assert_eq!(acc.bars_held, 1); // 新回合从 1 起
        assert_relative_eq!(acc.turnover, 1.0 + 1.5); // |Δ|=1.5 一次计
    }

    #[test]
    fn add_position_weighted_entry_and_partial_reduce_keeps_entry() {
        let mut acc = SimAccount::default();
        sim_step(&mut acc, 10.0, 10.0, 10.0, t("2024-01-02 10:00:00"), 0.5, 0.0, "tree");
        sim_step(&mut acc, 10.0, 12.0, 12.0, t("2024-01-03 10:00:00"), 1.0, 0.0, "tree");
        assert_relative_eq!(acc.entry_price, (10.0 * 0.5 + 12.0 * 0.5) / 1.0); // 11.0
        sim_step(&mut acc, 12.0, 12.0, 12.0, t("2024-01-04 10:00:00"), 0.4, 0.0, "tree");
        assert_relative_eq!(acc.entry_price, 11.0); // 部分减仓 entry 不变
        assert_eq!(acc.pos, 0.4);
    }

    #[test]
    fn finalize_liquidates_with_cost() {
        let mut acc = SimAccount::default();
        sim_step(&mut acc, 10.0, 10.0, 11.0, t("2024-01-02 10:00:00"), 1.0, 0.001, "tree");
        let nav_before = acc.nav;
        let rt = finalize(&mut acc, t("2024-01-02 10:15:00"), 11.0, 0.001).unwrap();
        assert_relative_eq!(acc.nav, nav_before * 0.999, epsilon = 1e-12);
        assert_eq!(rt.reason, "end");
        assert_eq!(acc.pos, 0.0);
    }
}
```

- [ ] **Step 3: `cargo test --lib backtest::sim` 全 PASS + clippy + Commit**

```bash
git add src/backtest/sim.rs src/backtest/mod.rs
git commit -m "feat(backtest): sim accounting core (sim_step/finalize) with golden-walk tests" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: run_sim 编排 + SimReport/traces

**Files:**
- Modify: `src/backtest/sim.rs`（追加 SimReport/SimStepRecord/run_sim/print_sim_summary）

- [ ] **Step 1: 类型与编排（追加到 sim.rs；加载/构 ctx 模式 mirror `run_soft`，逐 bar 顺序 await）**

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct SimReport {
    pub tree_name: String,
    pub cost_bps: f64,
    pub total_return: f64,
    pub max_drawdown: f64,
    pub n_round_trips: usize,
    pub win_rate: f64,
    pub avg_hold_bars: f64,
    pub turnover: f64,
    pub buy_and_hold: f64,
    pub trades: Vec<RoundTrip>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SimStepRecord {
    pub t: NaiveDateTime,
    pub target: f64,
    pub pos: f64,
    pub nav: f64,
}

pub async fn run_sim(cfg: &BacktestConfig, llm: &LlmEvaluator, soft: bool) -> Result<SimReport>
```
`run_sim` 逻辑（顺序，无 buffered）：
1. 加载 tree/primary/context/news/aux（mirror `run_soft` 的加载段；缺口告警同样跑）。
2. `let rate = cfg.cost_bps / 2.0 / 10_000.0; let start = cfg.warmup.min(primary.len());`
3. `for i in start..primary.len().saturating_sub(1)`：
   - `let mut ctx = build_context(...t = primary[i].time...)`；
   - `ctx.sim = SimState { pos: acc.pos, entry_price: acc.entry_price, bars_held: acc.bars_held, unreal_pnl: 按 spec §3.1 用 close_i 计算 };`
   - **风控覆盖**（`tree.risk` 且 pos≠0，按 stop→tp→max_hold 顺序）→ `(target, reason)`；未触发 → 树目标：硬 `traverse(...).await?` → `tree.leaves[&trace.leaf]` 的 `stance dir × weight`；软 `traverse_soft(...).await?` → `E = Σ p·w·dir`（按 leaf_probs 与 leaves 计算）；reason="tree"。
   - `if let Some(rt) = sim_step(&mut acc, close_i, open_{i+1}, close_{i+1}, t_{i+1}, target, rate, reason) { trips.push(rt); }`
   - traces 收集 `SimStepRecord { t: primary[i].time, target, pos: acc.pos, nav: acc.nav }`。
4. `if let Some(rt) = finalize(&mut acc, 末bar时间, 末close, rate) { trips.push(rt); }`
5. 指标：`win_rate = trips 中 trip_return>0 占比（空→0）`；`avg_hold_bars = trips bars_held 均值（空→0）`；`buy_and_hold = 末close / primary[start+1].open − 1`（首个执行 bar 开盘起，与执行口径一致；`start+1 ≥ len` 时 0）。
6. 写 `cfg.out_path`（serde_json pretty）；`cfg.traces_path` 给出时逐行写 `SimStepRecord`；返回 SimReport。
`print_sim_summary`：total_return/max_dd/n_trips/win_rate/avg_hold/turnover/buy&hold 各一行（中文标签，参照 print_soft_summary 风格）。

- [ ] **Step 2: 集成测试（sim.rs `mod tests` 内，tokio）**

小树（`pos == 0 and close > 0` 进 / `bars_held >= 2` 出 / `pos > 0` 持有，三分支 + default flat）+ 行内 6 根上升 bar（写 tempfile CSV 经 BacktestConfig 跑 `run_sim`）→ 断言 `n_round_trips ≥ 1`、`total_return` 为有限数、traces 行数 = 决策数。risk 树（stop_loss 0.01 + 下跌数据）→ trips[0].reason == "stop"。

- [ ] **Step 3: 全绿 + clippy + Commit**

```bash
git add src/backtest/sim.rs
git commit -m "feat(backtest): run_sim orchestration with risk overlay, SimReport and step traces" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 5: CLI --sim + e2e

**Files:**
- Modify: `src/cli/mod.rs`、`tests/e2e.rs`

- [ ] **Step 1: cli**

`Backtest` 变体加 `#[arg(long, default_value_t = false)] sim: bool,`（文档注释：Position-state simulation mode (sequential equity)；与 --soft 可组合）。分流（在现有 if soft 之前）：
```rust
            if sim {
                if folds >= 2 {
                    eprintln!("[rquant] note: --folds is ignored in --sim mode");
                }
                let report = crate::backtest::sim::run_sim(&cfg, &llm, soft).await?;
                crate::backtest::sim::print_sim_summary(&report);
            } else if soft { ... 现状 ... } else { ... 现状 ... }
```

- [ ] **Step 2: e2e**

`sim_full_chain`：上升趋势 fixture + 入/出/持有三分支树（horizon 无关），`run_sim(&cfg, &Disabled, false)` → `total_return.is_finite() && n_round_trips >= 1`；软：同树 `run_sim(..., true)` 跑通。旧树兼容：`trend_tree` 风格无 pos 条件树跑 `--sim` 不 panic。

- [ ] **Step 3: 全绿 + clippy + `--help` 含 --sim + Commit**

```bash
git add src/cli/mod.rs tests/e2e.rs
git commit -m "feat(cli): --sim mode wiring (composable with --soft); e2e" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 6: example + 文档 + 真数据 smoke

**Files:**
- Create: `examples/sim_tree.yaml`；Modify: docs 五处、README.md、`src/tree/loader.rs`（example 可加载测试）

- [ ] **Step 1: examples/sim_tree.yaml**

```yaml
meta:
  name: sim_demo
  forward_window: 16     # sim 模式不用，但 schema 必填
  stances: [long, flat]

params: { ma_n: 20 }

risk: { stop_loss: 0.05, max_hold_bars: 60 }

root: gate

nodes:
  gate:
    type: quant
    branches:
      - when: "pos == 0 and crossover(close, sma(close, ma_n))"
        goto: leaf_full
        label: enter
      - when: "pos > 0 and crossunder(close, sma(close, ma_n))"
        goto: leaf_flat
        label: exit
      - when: "pos > 0"
        goto: leaf_full
        label: hold
    default: { goto: leaf_flat, label: idle }

leaves:
  leaf_full: { stance: long }
  leaf_flat: { stance: flat }
```
loader 测试 `loads_sim_tree_example`（include_str!）。

- [ ] **Step 2: 文档**

- `docs/cli-reference.md`：`--sim` 行 + 模式对照小节（打分 vs 模拟）。
- `docs/tree-yaml-schema.md`：`risk:` 块 + 4 个持仓标识符为保留名 + sim 下叶子=目标仓位语义。
- `docs/dsl-reference.md`：标识符表加 pos/entry_price/bars_held/unreal_pnl（非 sim 默认值、entry NaN 弃权）。
- `docs/architecture.md`：第三种模式一段（顺序、无并发、记账口径指向 spec）。
- `README.md`：sim 一节（示例命令 + 与打分模式的定位区别 + 诚实边界：bar 粒度无盘中成交、无涨跌停过滤）。

- [ ] **Step 3: 真数据 smoke（手动，不入库）**

fetch sh600000 60m → `backtest --tree examples/sim_tree.yaml --sim`，确认 SimReport 合理输出（回合数/回撤/净值有限）后删除临时文件。

- [ ] **Step 4: 全绿 + Commit**

```bash
git add examples/sim_tree.yaml docs README.md src/tree/loader.rs
git commit -m "docs+example: sim mode docs, sim_tree example, loader test" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## 附录 A：Spec 覆盖对照（自检）
| Spec § | 实现于 |
|---|---|
| §4.1 SimState/Context/DSL/保留名 | Task 1 |
| §4.2 risk 块 | Task 2 |
| §3 记账语义（sim_step/finalize/T+1/翻向/加权/期末）| Task 3（黄金测试逐条钉）|
| §4.3 run_sim/SimReport/traces/print | Task 4 |
| §4.4 CLI（--folds 忽略提示）| Task 5 |
| §6 测试 + 文档 + smoke | Task 3-6 |

## 附录 B：明确不在范围（YAGNI）
- 涨跌停过滤；盘中价位成交；杠杆；sim 下 walk-forward；HTML sim 渲染（follow-up：`report --sim`）；多标的。
