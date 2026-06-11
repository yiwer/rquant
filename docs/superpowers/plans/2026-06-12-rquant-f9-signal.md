# rquant F-9 signal 实盘通路（paper trading）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `rquant signal` 双口径：单标的 paper-sim（state 快照 + 增量重放 + 悬挂决策 = 今日信号）与组合清单（横截面 top-N vs 持仓 diff）；`--commit` 落盘、`--out` JSON、单口径 `--fetch` 一条命令日常化。

**Architecture:** 在 master(HEAD `14a3409`)上：`backtest/sim.rs` 加账户快照/恢复（trip 私有 → 转换住在 sim.rs）；新 `src/signal/mod.rs`（state IO + 单/组合两引擎 + print）；CLI `Cmd::Signal` + fetch 逻辑抽函数复用。**语义权威=spec §2**（`docs/superpowers/specs/2026-06-12-rquant-f9-signal-design.md`，实现者先通读——尤其"悬挂决策/last_time 落后一根 bar/不 finalize"）。

**Tech Stack:** Rust 2024 + 既有。

> ⚠️ git 纪律：`git add` 点名文件；提交前 `git status --porcelain`。

---

## 文件结构
```
改动: src/backtest/sim.rs   # TripSnapshot/AccountSnapshot + SimAccount::{snapshot,restore}
新增: src/signal/mod.rs     # PaperState/HoldingsState IO + run_signal_single/run_signal_portfolio + 报告 + print
改动: src/lib.rs            # + pub mod signal;
改动: src/cli/mod.rs        # Cmd::Signal + run_fetch_to_csv 抽取（Fetch 臂行为零变）
改动: tests/e2e.rs、docs/cli-reference.md、README.md
```

---

## Task 1: SimAccount 快照/恢复

**Files:**
- Modify: `src/backtest/sim.rs`

- [ ] **Step 1: RED 测试**

```rust
    #[test]
    fn account_snapshot_roundtrip_preserves_everything() {
        // 持仓中账户（含 open trip）
        let mut acc = SimAccount::default();
        sim_step(&mut acc, 10.0, 10.0, 10.5, t("2024-01-02 10:00:00"), 0.7, 0.001, "tree");
        let snap = acc.snapshot();
        assert_eq!(snap.entry_price, Some(10.0));
        let json = serde_json::to_string(&snap).unwrap(); // NaN 不出现 → 序列化成功
        let back: AccountSnapshot = serde_json::from_str(&json).unwrap();
        let acc2 = SimAccount::restore(&back);
        // 恢复后继续走一步，与原账户走同一步结果一致
        let mut a1 = acc;
        let mut a2 = acc2;
        let r1 = sim_step(&mut a1, 10.5, 10.6, 10.4, t("2024-01-03 10:00:00"), 0.0, 0.001, "tree");
        let r2 = sim_step(&mut a2, 10.5, 10.6, 10.4, t("2024-01-03 10:00:00"), 0.0, 0.001, "tree");
        assert_eq!(r1.is_some(), r2.is_some());
        assert!((a1.nav - a2.nav).abs() < 1e-15 && (a1.pos - a2.pos).abs() < 1e-15);
        assert_eq!(a1.bars_held, a2.bars_held);
        // 空仓账户：entry NaN → snapshot None → restore NaN
        let flat = SimAccount::default();
        let s = flat.snapshot();
        assert!(s.entry_price.is_none() && s.trip.is_none());
        assert!(SimAccount::restore(&s).entry_price.is_nan());
    }
```

- [ ] **Step 2: 实现（追加到 sim.rs；OpenTrip 字段对照现有定义）**

```rust
/// 开仓回合快照（持久化用；OpenTrip 为私有故转换住本文件）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TripSnapshot {
    pub entry_t: NaiveDateTime,
    pub entry_px: f64,
    pub open_nav: f64,
    pub max_abs_pos: f64,
}

/// SimAccount 可序列化快照（entry_price NaN ↔ None：serde_json 不允许 NaN）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountSnapshot {
    pub pos: f64,
    pub entry_price: Option<f64>,
    pub bars_held: usize,
    pub nav: f64,
    pub peak_nav: f64,
    pub max_drawdown: f64,
    pub turnover: f64,
    pub last_increase_date: Option<NaiveDate>,
    pub trip: Option<TripSnapshot>,
}

impl SimAccount {
    pub fn snapshot(&self) -> AccountSnapshot {
        AccountSnapshot {
            pos: self.pos,
            entry_price: if self.entry_price.is_nan() { None } else { Some(self.entry_price) },
            bars_held: self.bars_held,
            nav: self.nav,
            peak_nav: self.peak_nav,
            max_drawdown: self.max_drawdown,
            turnover: self.turnover,
            last_increase_date: self.last_increase_date,
            trip: self.trip.as_ref().map(|t| TripSnapshot {
                entry_t: t.entry_t,
                entry_px: t.entry_px,
                open_nav: t.open_nav,
                max_abs_pos: t.max_abs_pos,
            }),
        }
    }

    pub fn restore(s: &AccountSnapshot) -> SimAccount {
        SimAccount {
            pos: s.pos,
            entry_price: s.entry_price.unwrap_or(f64::NAN),
            bars_held: s.bars_held,
            nav: s.nav,
            peak_nav: s.peak_nav,
            max_drawdown: s.max_drawdown,
            turnover: s.turnover,
            last_increase_date: s.last_increase_date,
            trip: s.trip.as_ref().map(|t| OpenTrip {
                entry_t: t.entry_t,
                entry_px: t.entry_px,
                open_nav: t.open_nav,
                max_abs_pos: t.max_abs_pos,
            }),
        }
    }
}
```

- [ ] **Step 3: GREEN + clippy + Commit**

```bash
git add src/backtest/sim.rs
git commit -m "feat(backtest): SimAccount snapshot/restore for paper-trading persistence" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: 单标的重放引擎 + 黄金不变量

**Files:**
- Create: `src/signal/mod.rs`；Modify: `src/lib.rs`（+ `pub mod signal;`）

- [ ] **Step 1: 类型 + state IO + run_signal_single（READ `run_sim` 主循环先——重放体与之逐字同口径）**

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct PaperState {
    pub version: u32,            // =1
    pub tree_name: String,
    pub last_time: Option<NaiveDateTime>,
    pub account: AccountSnapshot,
}

pub fn read_paper_state(path: &Path, tree_name: &str) -> Result<Option<PaperState>>
// 不存在 → Ok(None)；JSON 损坏 → Err("signal state corrupt: …（如需重建请删除该文件）")；
// version != 1 → Err；tree_name 不符 → Err（防串树）。
pub fn write_paper_state(path: &Path, st: &PaperState) -> Result<()>

#[derive(Debug, Serialize, Deserialize)]
pub struct PaperStats { pub nav: f64, pub total_return: f64, pub max_drawdown: f64, pub bars_replayed: usize }

#[derive(Debug, Serialize, Deserialize)]
pub struct SingleSignal {
    pub t: NaiveDateTime,        // 悬挂 bar 时间
    pub target: f64,
    pub current_pos: f64,
    pub delta: f64,
    pub reason: String,          // tree/stop/tp/max_hold
    pub leaf: Option<String>,    // 硬遍历叶 id；soft → None
    pub paper: PaperStats,
}

pub struct SignalSingleConfig { /* tree_path, primary_path, context_path, news_path: Option, aux_paths, window, warmup, cost_bps, soft, state_path */ }

/// 返回 (信号, 更新后 state)；落盘由调用方按 --commit 决定。
pub async fn run_signal_single(cfg: &SignalSingleConfig, llm: &LlmEvaluator) -> Result<(SingleSignal, PaperState)>
```
逻辑（spec §2 权威）：
1. 加载树；`read_paper_state`（None → fresh：`SimAccount::default().snapshot()`、last_time None）。
2. 加载 primary/context/news/aux；`len < warmup + 1` → Error::Data("not enough bars")。
3. `acc = SimAccount::restore(&state.account)`；`rate = cost_bps/2/1e4`。
4. **重放**：`for i in warmup..len-1`，跳过 `time_i <= last_time` 的；其余执行与 run_sim 循环体逐字同口径（SimState 注入含 unreal、风控覆盖 stop→tp→max_hold、树目标硬/软、`sim_step(acc, close_i, open_{i+1}, close_{i+1}, t_{i+1}, target, rate, reason)`）；计 bars_replayed；记录已记账最后决策时间 → new_last_time（无新记账 → 沿用旧 last_time）。**不调用 finalize**。
5. **悬挂决策** i = len−1：build_context(t = time_{len−1}) + SimState 注入（unreal 用 close_{len−1}）→ 风控覆盖优先否则树目标（硬保留 trace.leaf）→ `(target, reason, leaf)`；`delta = target − acc.pos`。
6. 组装 SingleSignal{ paper: PaperStats{ nav: acc.nav, total_return: acc.nav−1, max_drawdown: acc.max_drawdown, bars_replayed } } + PaperState{ version 1, tree_name, last_time: new_last_time, account: acc.snapshot() }。

- [ ] **Step 2: 黄金不变量测试（tokio + tempfile）**

```rust
    // 合成 ~32 bar 跨多日上行 + 入/出/持有树（pos 条件，同 sim e2e 形态）。
    // A：一次 run_signal_single(全量数据, fresh state) → state_a
    // B：run_signal_single(前 k bar, fresh) → state_b1 → 以 state_b1 为起点跑全量 → state_b2
    // 断言 state_a == state_b2 逐字段（serde_json::to_value 相等即可），k 取 {warmup+3, len-5} 两个切分点。
    // 幂等：以 state_a 再跑全量 → bars_replayed == 0 且 state 不变、信号同 t 同 target。
```

- [ ] **Step 3: 悬挂风控测试**

入场后构造末 bar 大幅浮亏超 `risk.stop_loss` 的数据 → 悬挂决策 `reason == "stop"`、`target == 0.0`，且 state.account 与重放后（未记账悬挂）一致。state 损坏文件 → Err 含 "corrupt"；版本/树名不符 → Err。

- [ ] **Step 4: GREEN + clippy + Commit**

```bash
git add src/signal/mod.rs src/lib.rs
git commit -m "feat(signal): single-symbol paper-sim replay with pending-decision signal (split==full invariant)" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: 组合清单引擎

**Files:**
- Modify: `src/signal/mod.rs`

- [ ] **Step 1: 类型 + run_signal_portfolio（READ `backtest/portfolio.rs` 的加载/打分段先）**

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct HoldingsState {
    pub version: u32,            // =1
    pub tree_name: String,
    pub last_time: Option<NaiveDateTime>,
    pub holdings: BTreeMap<String, f64>,
}
// read/write 同 PaperState 风格（损坏/版本/树名报错；缺省 fresh 空持仓）。

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub enum TradeAction { Buy, Sell, Adjust, Hold }

#[derive(Debug, Serialize, Deserialize)]
pub struct TradeInstr { pub symbol: String, pub action: TradeAction, pub from_w: f64, pub to_w: f64 }

#[derive(Debug, Serialize, Deserialize)]
pub struct PortfolioSignal {
    pub t: NaiveDateTime,
    pub n_fresh: usize,
    pub targets: Vec<(String, f64)>,
    pub trades: Vec<TradeInstr>,
}

pub struct SignalPortfolioConfig { /* tree_path, universe_path, top, window, warmup(未用打分但保留一致), cost_bps(报告口径无成本——清单不记账), soft, aux_paths, state_path */ }

pub async fn run_signal_portfolio(cfg: &SignalPortfolioConfig, llm: &LlmEvaluator) -> Result<(PortfolioSignal, HoldingsState)>
```
逻辑：universe 加载 → `build_timeline` → `t_last = *timeline.last()`（空 → Err）→ 逐标的 `score_symbol`（soft 同义；None=不新鲜跳过）→ `select_top(scores, top)` → 等权 targets → trades = state.holdings ∪ targets 的并集逐 symbol：`from_w` vs `to_w`（缺省 0）→ Buy(0→w)/Sell(w→0)/Adjust(变)/Hold(|差|<1e-12)，按 symbol 字典序。`n_fresh < universe 数` → eprintln 提示。新 HoldingsState{ holdings: targets 转 map, last_time: Some(t_last) }。

- [ ] **Step 2: 测试**

四象限：state 持 {A:0.5, B:0.5}，新目标 {A:1/3, C:1/3, D:1/3}（top=3）→ A=Adjust、B=Sell、C/D=Buy；持仓不变场景全 Hold。新鲜度：一标的时间错开 → 不入候选。

- [ ] **Step 3: GREEN + clippy + Commit**

```bash
git add src/signal/mod.rs
git commit -m "feat(signal): portfolio target diff (buy/sell/adjust/hold instructions)" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: CLI（互斥/--fetch 抽取/--commit/print/JSON）

**Files:**
- Modify: `src/cli/mod.rs`、`src/signal/mod.rs`（print fns）

- [ ] **Step 1: fetch 逻辑抽函数（行为零变——全量测试回归是保护网）**

现 `Cmd::Fetch` 臂主体抽为：
```rust
pub(crate) async fn run_fetch_to_csv(
    symbol: &str, scale: u32, datalen: u32, base_url: &str, adjust: &str, out: &std::path::Path,
) -> anyhow::Result<usize> /* 写出 bar 数 */
```
Fetch 臂改调它（打印行为保持）。

- [ ] **Step 2: Cmd::Signal**

```rust
    /// Generate today's trading signal (single-symbol paper-sim or portfolio target list)
    Signal {
        #[arg(long)]
        tree: PathBuf,
        #[arg(long)]
        state: PathBuf,
        #[arg(long)]
        primary: Option<PathBuf>,
        #[arg(long)]
        context: Option<PathBuf>,
        #[arg(long)]
        news: Option<PathBuf>,
        #[arg(long)]
        universe: Option<PathBuf>,
        #[arg(long, default_value_t = 5)]
        top: usize,
        /// Optional: refresh --primary from network first (single mode only)
        #[arg(long)]
        fetch: Option<String>,
        #[arg(long, default_value_t = 60)]
        scale: u32,
        #[arg(long, default_value_t = 1023)]
        datalen: u32,
        #[arg(long, default_value = "none")]
        adjust: String,
        #[arg(long, default_value_t = false)]
        soft: bool,
        #[arg(long, default_value_t = false)]
        commit: bool,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long, default_value_t = 100)]
        warmup: usize,
        #[arg(long, default_value_t = 100)]
        window: usize,
        #[arg(long, default_value_t = 10.0)]
        cost_bps: f64,
        #[arg(long = "aux", value_name = "NAME=PATH")]
        aux: Vec<String>,
        #[arg(long, default_value = "")]
        llm_model: String,
        #[arg(long, default_value = "")]
        llm_base_url: String,
        #[arg(long, default_value = ".rquant-cache/llm")]
        llm_cache_dir: PathBuf,
    },
```
分流：`primary.is_some() == universe.is_some()` → anyhow 错误（恰一）；`fetch.is_some() && primary.is_none()` → 错误；fetch Some → `run_fetch_to_csv(..., adjust, primary 路径)`（sina 默认 base_url 常量沿用 Fetch 臂的默认）；单：context 缺省 = primary；LLM/aux 构造 mirror Backtest；调 `run_signal_single`/`run_signal_portfolio` → `print_single_signal`/`print_portfolio_signal` → `out` Some → 写 JSON pretty → `commit` → 写 state，否则打印 `[DRY RUN] 未落盘 state；加 --commit 提交`。

- [ ] **Step 3: print fns（signal/mod.rs）**

单（样式）：
```
=== rquant SIGNAL (single) @ {t} ===
目标仓位: {target:.2}   当前: {current:.2}   Δ: {delta:+.2}
reason: {reason}{ (leaf)}
纸面账户: nav {:.4}  总收益 {:+.2%}  回撤 {:.2%}  本次重放 {bars_replayed} bar
```
组合：targets 表 + trades 逐行（`BUY  sh600000  0.00 → 0.33` 式）+ n_fresh 行。

- [ ] **Step 4: 全绿 + clippy + `signal --help` + Commit**

```bash
git add src/cli/mod.rs src/signal/mod.rs
git commit -m "feat(cli): signal subcommand (mode mutex, --fetch reuse, --commit gating, JSON out)" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 5: e2e 两天模拟 + 文档 + 真数据 smoke

**Files:**
- Modify: `tests/e2e.rs`、`docs/cli-reference.md`、`README.md`

- [ ] **Step 1: e2e**

- `signal_two_day_paper_flow`：合成数据写"第一天"前缀 CSV → `run_signal_single` + 手动 `write_paper_state`（模拟 --commit）→ 全量 CSV 再跑 → state 与"全量一次跑"的 state 相等（json value 比对）；第二跑 `bars_replayed` == 新增可记账决策数。
- `signal_portfolio_diff_chain`：3 标的 universe → 空持仓 state → 跑出 Buy 清单 → commit 后再跑同数据 → 全 Hold。
- CLI 互斥：构造 `primary+universe 同给` 经 clap/分流报错（直接调分流逻辑或子进程 --help 级验证，取实现可测的层面）。

- [ ] **Step 2: 文档**

- cli-reference：signal 全旗标表 + **state 文件语义**（版本/树名守卫、损坏报错、last_time 落后一根 bar 的悬挂语义）+ 纸面边界（次开盘可成交/无滑点/假设历史信号全部执行）。
- README：「每日一条命令」节——
  ```powershell
  rquant signal --tree my.yaml --fetch sh600519 --scale 60 --adjust qfq `
    --primary data\p.csv --state paper.json --commit --out signal.json
  ```
  + Windows 任务计划程序示例一行（`schtasks /create /sc daily /st 15:30 /tn rquant-signal /tr "..."`）+ 研究闭环终图：factor → 入树 → optimize → backtest/sim → **signal 纸面盘** → （人工下单）。

- [ ] **Step 3: 真数据 smoke（手动不入库）**

`signal --tree examples/regime_adaptive_1.yaml --fetch sh600519 --scale 60 --adjust qfq --primary tmps/p.csv --state tmps/paper.json --commit --warmup 80` 两连跑：首跑记录信号与 bars_replayed；**第二跑 bars_replayed=0 且信号同值（幂等）**；组合：6 真股 universe top-2 清单一跑。数字记入报告；清理。

- [ ] **Step 4: 全绿 + clippy + Commit**

```bash
git add tests/e2e.rs docs/cli-reference.md README.md
git commit -m "test+docs: two-day paper flow e2e, daily-command guide, real-data smoke" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## 附录 A：Spec 覆盖对照（自检）
| Spec § | 实现于 |
|---|---|
| §2 重放/悬挂/last_time/不 finalize/增量≡全量 | T2（黄金不变量）|
| §3 state 文件（快照/版本/树名/损坏报错）| T1/T2/T3 |
| §4 组合 diff | T3 |
| §5 输出（bar 时间确定性/print/JSON）| T2-T4 |
| §6 CLI（互斥/--fetch 抽取/--news）| T4 |
| §7 测试/文档/smoke | T1-T5 |

## 附录 B：明确不在范围（YAGNI）
- 券商/vnpy；webhook 推送；内置调度；组合 --fetch；多 state 并管；悬挂决策的 T+1 预判提示。
