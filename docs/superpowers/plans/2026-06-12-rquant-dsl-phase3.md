# DSL Phase-3（节流状态量 + 日内锚定族 + percentrank/corr + at_entry 惯用法）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 落地缺口评估 #4/#6（状态量族）与 #5（日内锚定族）+ #7/#9 小件（percentrank/corr）。重大简化：**at_entry 快照族无需任何新机制**——`ref(expr, bars_held)` 即信号 bar 锚定（已验证：ref 的 k 实参经 as_usize(eval) 接受表达式、saturating_sub 越窗 → 空 → NaN 弃权），缺口收窄为两个真状态字段 + 文档化惯用法。

**Architecture:** T1 走极值字段的成熟 playbook（SimAccount + sim_step + SimState + AccountSnapshot serde(default) + roundtrip 扩展 + signal 黄金不变量变体）；T2 日内锚定 5 标识符纯 Context 派生（无状态、无前视：只读可见窗内当日 bar）；T3 两个滚动窗函数沿 std_roll 模式；T4 文档（含 at_entry 惯用法与**冷却写法纪律**）+ 冻结闸。

**Tech Stack:** Rust 2024 + 既有。无新依赖。

**铁律与守则（实现者必读）：**
- **状态字段 NaN 纪律**（同极值）：SimAccount 存 f64（NaN=尚无），SimState 注入同形，AccountSnapshot 用 `Option<f64>` + `#[serde(default)]`（旧 state → None → NaN，弃权降级不报错）。
- **冷却写法纪律（关键语义陷阱，文档必须讲透）**：打分/portfolio 模式下 `bars_since_exit`/`last_trip_return` 恒 NaN。冷却条件**必须写成独立的阻断分支**（`when: "bars_since_exit < cooldown" → flat`，NaN → false → 自然落空、打分模式零影响），**绝不可**写成入场条件的 AND 子句（`pos == 0 and bars_since_exit >= cooldown` 在打分模式恒弃权 → 树在打分口径退化为纯 flat，评分/优化全废）。
- 冻结闸基准（v2 树不引用新状态量/新标识符 → 五指标必须精确相等）：总收益 −0.0641 / 回撤 0.0885 / 回合 36 / 胜率 38.9% / 换手 41.6；窗口移位降级规则同前。
- ⚠️ git 纪律照旧；杂物不碰；分支 `dsl-phase3`。

---

## 文件结构

```
改动: src/backtest/sim.rs       # SimAccount/sim_step/finalize + 2 字段；AccountSnapshot + 2 Option；roundtrip 扩展
改动: src/features/context.rs   # SimState + 2 字段；session 锚定计算助手（或放 eval.rs，见 T2）
改动: src/dsl/eval.rs           # Ident 臂 + 7 个标识符（2 状态 + 5 日内锚定）；percentrank/corr 臂
改动: src/features/indicators.rs# percentrank_roll / corr_roll
改动: src/tree/loader.rs        # RESERVED_IDENTS 14→21；RESERVED_FNS 29→31
改动: src/tree/lint.rs          # 标量 Ident 表 +7；SERIES_FNS +2（同步锁等长断言会强制测试补例）
改动: src/signal/mod.rs         # 黄金不变量变体（冷却树）测试
改动: src/backtest/{runner,soft,portfolio,sim}.rs 及 optimize/mod.rs 的 SimState 字面量补字段（编译器驱动，逐个补）
改动: docs/{dsl-reference,tree-yaml-schema,cli-reference}.md
```

---

## Task 1: 节流状态量 bars_since_exit / last_trip_return（state playbook 全套）

**Files:**
- Modify: `src/backtest/sim.rs`、`src/features/context.rs`（SimState）、`src/dsl/eval.rs`（Ident 臂）、`src/tree/loader.rs`（RESERVED_IDENTS 14→16）、`src/tree/lint.rs`（标量 Ident +2）、`src/signal/mod.rs`（黄金变体）+ 全仓 SimState 字面量补字段

**语义定义（权威）：**
- `last_trip_return`：最近一次平仓回合的 `trip_return`（净值口径）；从未平仓 → NaN。Turtle S1 跳过规则：`last_trip_return > 0` → 跳过本次突破。
- `bars_since_exit`：距最近一次平仓**执行 bar** 的 bar 数；平仓执行 bar 收盘记 1（镜像 bars_held 口径），其后每执行 bar +1（**不论当前是否持仓**——它是"距上次离场事件"的单调计数）；从未平仓 → NaN。翻向（flip）也是一次平仓事件。
- finalize（期末清算）同样更新两者（signal 模式不调 finalize，不受影响）。

- [ ] **Step 1: RED——sim 单元测试 + 黄金不变量变体**

sim.rs `mod tests` 追加：

```rust
    /// 节流状态量：平仓回合记账与逐 bar 计数（含翻向与从未平仓 NaN）。
    #[test]
    fn throttle_state_tracks_exits() {
        let mut acc = SimAccount::default();
        assert!(acc.bars_since_exit.is_nan() && acc.last_trip_return.is_nan());
        // 开仓（执行 bar1）：仍无平仓事件
        sim_step(&mut acc, 10.0, 10.0, 10.2, 9.9, 10.1, t("2024-01-02 10:00:00"), 1.0, 0.0, "tree");
        assert!(acc.bars_since_exit.is_nan());
        // 平仓（执行 bar2，开 10.1 → 平在开盘）：bars_since_exit=1，last_trip_return 记账
        sim_step(&mut acc, 10.1, 10.1, 10.3, 10.0, 10.2, t("2024-01-03 10:00:00"), 0.0, 0.0, "tree");
        assert!((acc.bars_since_exit - 1.0).abs() < 1e-12);
        let r1 = acc.last_trip_return;
        assert!((r1 - (10.1 / 10.0 - 1.0)).abs() < 1e-12); // 零成本：入 10.0 出 10.1
        // 空仓再走一根：+1
        sim_step(&mut acc, 10.2, 10.2, 10.4, 10.1, 10.3, t("2024-01-04 10:00:00"), 0.0, 0.0, "tree");
        assert!((acc.bars_since_exit - 2.0).abs() < 1e-12);
        // 再开仓后计数继续单调（不重置）
        sim_step(&mut acc, 10.3, 10.3, 10.5, 10.2, 10.4, t("2024-01-05 10:00:00"), 1.0, 0.0, "tree");
        assert!((acc.bars_since_exit - 3.0).abs() < 1e-12);
        assert!((acc.last_trip_return - r1).abs() < 1e-12); // 未再平仓，不变
    }
```

roundtrip 测试 `account_snapshot_roundtrip_preserves_everything` 扩展：持仓中账户经历一次平仓+再开仓后 snapshot → 断言 `snap.bars_since_exit == Some(实算值)`、`snap.last_trip_return == Some(实算值)`；restore 后位级一致；flat 初始账户两字段 `is_none()`、restore 后 `is_nan()`。

signal/mod.rs 黄金不变量变体（咬合设计——**平仓事件在切分点前、冷却阻断在切分点后**：字段若没进 state，B2 重播时 NaN → 阻断分支落空 → 提前再入场 → 状态分叉变红）：

```rust
    /// 冷却树：离场后 3 根 bar 内不再入场（阻断分支形态——冷却写法纪律）。
    fn cooldown_signal_tree() -> String {
        r#"
meta: { name: cooldown_sig, forward_window: 1, stances: [long, flat] }
root: gate
nodes:
  gate:
    type: quant
    branches:
      - when: "pos > 0 and bars_held >= 2"
        goto: leaf_flat
        label: exit_after_2
      - when: "pos > 0"
        goto: leaf_long
        label: hold
      - when: "bars_since_exit < 3"
        goto: leaf_flat
        label: cooldown_block
      - when: "close > 0"
        goto: leaf_long
        label: enter
    default: { goto: leaf_flat, label: idle }
leaves:
  leaf_long: { stance: long }
  leaf_flat: { stance: flat }
"#
        .to_string()
    }

    /// 节流状态量经 state 往返的黄金不变量（playbook 第三例）。
    /// 数据 16 根一天一根；fresh 入场→持 2 根平仓→冷却 3 根→再入场……循环。
    /// k 取"平仓后冷却期内"的切分点：B1 末态 bars_since_exit ∈ {1,2}，
    /// 该值若没进 AccountSnapshot，B2 重播 NaN → cooldown_block 落空 → 提前再入场 → 分叉。
    #[tokio::test]
    async fn golden_invariant_with_throttle_state() {
        // 形态同 golden_invariant_with_position_extremes：A 全量 fresh vs
        // B 前 k 根 fresh→write_paper_state→全量续跑，serde_json::to_value 全字段相等；
        // k 至少取一个落在冷却期中段的值（按数据节奏推演选定，并在注释写明推演）；
        // 另断言 turnover > 2.5（确实发生了多次入出场——非空转）+ 幂等 bars_replayed == 0。
        todo!("实现者按 extremes 变体的既有写法克隆并替换树/数据/k——本文件内有完整范本")
    }
```

（`todo!` 仅为计划占位标记——实现时必须写成完整可跑测试，范本就在同文件 `golden_invariant_with_position_extremes`。）

- [ ] **Step 2: 实现**

sim.rs：
```rust
// SimAccount 新增（NaN 纪律注释同极值字段）：
    pub bars_since_exit: f64,
    pub last_trip_return: f64,
// Default：均 f64::NAN。
```

sim_step 末段（极值更新块附近，顺序：先记平仓事件，再统一 +1）：
```rust
    // 节流状态量：平仓事件（含翻向）重置计数并记账回合收益；其后每执行 bar 单调 +1。
    if let Some(rt) = &closed {
        acc.last_trip_return = rt.trip_return;
        acc.bars_since_exit = 1.0; // 平仓执行 bar 收盘记 1（镜像 bars_held 口径）
    } else if !acc.bars_since_exit.is_nan() {
        acc.bars_since_exit += 1.0;
    }
```
（插入位置必须在 `closed` 已定且本步其余记账完成后、return 前；注意翻向场景 closed=Some 且新仓已开——计数仍按平仓事件重置 ✓。）

finalize：close_trip 之后同样 `last_trip_return = rt.trip_return; bars_since_exit = 1.0`（取 closed 的 Some 分支）。

AccountSnapshot：+ `bars_since_exit: Option<f64>` / `last_trip_return: Option<f64>`（`#[serde(default)]`、is_finite ↔ None 映射进 snapshot()/restore()，doc 注释同极值——旧 state 缺字段 → NaN，引用它们的条件弃权至下次平仓事件）。

SimState（context.rs）：+ 两字段（f64，Default NAN）；**全仓 SimState 字面量编译器驱动补齐**（run_sim、signal 重放+悬挂、optimize evaluate_sim、loader 测试 mini ctx、eval 测试若有——逐个补 `bars_since_exit: acc.bars_since_exit, last_trip_return: acc.last_trip_return,`，测试 ctx 用 `..Default::default()` 或显式 NAN）。

eval.rs Ident 臂：+ `"bars_since_exit" => Ok(Value::Scalar(ctx.sim.bars_since_exit)),`、`"last_trip_return" => ...`。

loader.rs RESERVED_IDENTS 14→16；lint.rs expr_shape 标量 Ident 表 +2。

- [ ] **Step 3: GREEN + 全量 + clippy + Commit**

```bash
cargo test && cargo clippy --all-targets -- -D warnings
git add src/backtest/sim.rs src/features/context.rs src/dsl/eval.rs src/tree/loader.rs src/tree/lint.rs src/signal/mod.rs src/backtest/runner.rs src/backtest/soft.rs src/backtest/portfolio.rs src/optimize/mod.rs
git status --porcelain
git commit -m "feat(sim): throttle state vars bars_since_exit/last_trip_return through full state pipeline" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```
（实际 add 清单以真实触碰为准——SimState 字面量在哪些文件由编译器揭示，点名列全。）

---

## Task 2: 日内锚定族 session_open / session_high / session_low / session_vwap / bars_today

**Files:**
- Modify: `src/dsl/eval.rs`（Ident 臂 + 计算助手）、`src/tree/loader.rs`（RESERVED_IDENTS 16→21）、`src/tree/lint.rs`（标量表 +5）

**语义定义（权威）：**
- 当日 = 可见窗内 `time.date() == ctx.t.date()` 的**尾部连续段**（从窗尾向前扫到日期变化为止）。
- `session_open` = 段首 bar 的 open；`session_high`/`session_low` = 段内 high 最大 / low 最小（**含当前 bar**——Brooks 日内高低点语义）；`session_vwap` = Σ(close×volume)/Σ(volume)（Σvolume ≤ 0 → NaN 弃权）；`bars_today` = 段长（≥1）。
- 可见窗截断（窗短于当日已有 bar 数）→ 按可见部分计算（文档诚实声明）；日线数据（一天一根）→ session_open=open、high/low=本根、bars_today=1（退化无害）。
- 全部 Scalar、纯 Context 派生（time ≤ t 闸门内）、无前视。

- [ ] **Step 1: RED 测试（eval.rs mod tests；需要带真实时间戳/OHLC 的 ctx 工厂——`ctx_from_closes` 时间戳是否同日？读后视情内联新工厂 `ctx_two_days(bars: &[(&str, f64, f64, f64, f64, f64)])` 式）**

用例：两天数据（day1 3 根 + day2 2 根，t = day2 第二根）→ `bars_today == 2`、`session_open == day2 首根 open`、`session_high == day2 两根 high 最大`、`session_low` 对称、`session_vwap` 手算断言；单日退化（t = day1 首根）→ bars_today == 1；volume 全 0 → `session_vwap > 0` 为 false（NaN 弃权）。

- [ ] **Step 2: 实现（eval.rs）**

```rust
/// 当日尾部连续段：可见窗内 date == t.date() 的末段切片范围。
fn session_range(ctx: &Context) -> std::ops::Range<usize> {
    let bars = &ctx.primary.bars;
    let today = ctx.t.date();
    let mut start = bars.len();
    while start > 0 && bars[start - 1].time.date() == today {
        start -= 1;
    }
    start..bars.len()
}
```
Ident 臂五个分支按定义计算（空段防御：range 空 → NaN——理论上 t 即窗尾 bar 时间不会空，防御为先）。`ctx.primary.bars` 可见性以 Window 实际定义为准（resolve_series 用的是 closes() 等访问器——若 bars 字段私有则加 pub(crate) 访问器或复用既有访问器组合，最小改动优先）。

loader/lint 同步：RESERVED_IDENTS 21、lint 标量表 +5。

- [ ] **Step 3: GREEN + 全量 + clippy + Commit**

```bash
git add src/dsl/eval.rs src/tree/loader.rs src/tree/lint.rs
git commit -m "feat(dsl): intraday session anchors - session_open/high/low/vwap, bars_today" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Task 3: percentrank / corr 滚动窗函数

**Files:**
- Modify: `src/features/indicators.rs`、`src/dsl/eval.rs`、`src/tree/loader.rs`（RESERVED_FNS 29→31）、`src/tree/lint.rs`（SERIES_FNS 13→15——等长锁会强制补同步锁用例）

**语义定义（权威）：**
- `percentrank(series, n)`：位 j = 窗口（含当前，长 n）内**严格小于** s[j] 的个数 / (n−1) ∈ [0,1]；n<2 或 j+1<n → NaN（严格头）；窗含 NaN → NaN。自归一化阈值惯用法：`percentrank(atr(14)/close, 250) > 0.95`。
- `corr(a, b, n)`：滚动 Pearson；两序列先尾对齐再逐位滚动；n<2 或头部不足 → NaN；窗含 NaN → NaN；任一侧零方差 → NaN。大盘相关惯用法：`corr(close, ctx.close, 60) > 0.7`。

- [ ] **Step 1: RED 测试**（indicators：手算小窗用例 + 严格头 NaN + NaN 传播 + corr 完全正/负相关 ±1 + 零方差 NaN；eval：进逐位条件 `count(percentrank(close, 3) > 0.9, 3) == ?` 手算、`corr(close, ctx.close, n)` 双序列路由）

- [ ] **Step 2: 实现**（indicators 两个 `_roll` 风格函数 O(len×n)；eval 臂：percentrank 单序列 + corr 双序列先 tail_align 再滚动；SERIES_FNS/RESERVED_FNS 同步——同步锁等长断言不补用例会红，按红补齐）

- [ ] **Step 3: GREEN + 全量 + clippy + Commit**

```bash
git add src/features/indicators.rs src/dsl/eval.rs src/tree/loader.rs src/tree/lint.rs
git commit -m "feat(dsl): rolling percentrank and pearson corr windows" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Task 4: 文档（at_entry 惯用法 + 冷却纪律）+ 冻结闸 + 收官

**Files:**
- Modify: `docs/dsl-reference.md`、`docs/tree-yaml-schema.md`、`docs/cli-reference.md`

- [ ] **Step 1: dsl-reference.md**

1. 状态标识符表 + 2 行（bars_since_exit/last_trip_return：语义、NaN 纪律、Turtle S1 例 `last_trip_return > 0`）+ 日内锚定 5 行（截断/日线退化诚实声明）。
2. **「入场时刻锚定」惯用法节（at_entry 之死）**：
```yaml
# 信号 bar 的 ATR（Turtle 原版 N）：开仓决策发生在 bars_held 根之前
n_at_entry: "ref(atr_v, bars_held)"
# 入场执行 bar 的高/低（信号 bar 止损位挂单）——安全形态（打分模式 bars_held=0 时 max 兜 0）
entry_bar_low: "ref(low, max(0, bars_held - 1))"
# Chandelier 原版（入场时 N 而非当前 N）
when: "pos > 0 and close < max_price_since_entry - 3 * ref(atr_v, bars_held)"
```
   附边界：长持仓 bars_held 超可见窗 → ref 空序列 → NaN 弃权（树侧保留固定止损兜底——与极值迁移注同纪律）；惯用法入文前实跑验证。
3. **「冷却写法纪律」专节**（铁律里的阻断分支形态 vs AND 子句反例，打分模式退化机理讲透）。
4. percentrank/corr 函数表行 + 自归一化/大盘相关惯用法。
5. session_vwap 与 phase-2 滚动 VWAP 的口径区分一句话（锚定日内 vs 滚动 n 根）。

- [ ] **Step 2: tree-yaml-schema.md + cli-reference.md**

保留名同步（idents 21 / fns 31）；cli-reference 的 state 文件语义节补一句：快照新增节流字段，旧 state 缺省 None→NaN 弃权（同极值迁移注挂一起）。

- [ ] **Step 3: 真数据冻结闸**（命令与基准同 phase-2 计划；v2 树不引用任何新标识符 → 期待强验收精确相等；降级规则同前；跑完删 tmps/）

- [ ] **Step 4: 全量 + clippy + Commit**

```bash
git add docs/dsl-reference.md docs/tree-yaml-schema.md docs/cli-reference.md
git commit -m "docs(dsl): throttle/session state reference, at-entry anchoring idioms, cooldown discipline" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## 附录 A：验收对照

| 缺口项 | 实现于 |
|---|---|
| #4 at_entry 快照族 | **零代码**——T4 惯用法文档（ref(expr, bars_held) 已验证可行）|
| #6 节流状态量（Turtle S1/再入场冷却） | T1（playbook 全套 + 黄金不变量第三例）|
| #5 日内锚定族（含锚定 VWAP） | T2 |
| #7 自归一化 percentrank / #9 corr | T3 |
| state 兼容铁律 | T1 serde default + roundtrip 扩展 + 冷却黄金变体 + T4 冻结闸 |

## 附录 B：明确不在本 phase

trades_today（T+1 已天然节流减仓侧，bars_since_exit 覆盖再入场——等真实树需要再加）；通用 cum/累积算子与完整 OBV（session_vwap 已覆盖最高频需求）；任意表达式 at_entry 持久化机制（惯用法已覆盖，YAGNI）；树镜像宏（tree schema 层，另立项）；比较逐位提升（维持 phase-2 边界）。
