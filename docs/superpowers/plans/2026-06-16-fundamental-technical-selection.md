# 基本面×技术选股方法学（子项③）实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把②的价值轴（PB）与横截面动量组合成「价值闸 + 动量排序」选股法，在 survivorship-free top-2000 上做 time-slice OOS + 跨regime + 敏感面验证。

**Architecture:** 扩 `screen` 引擎——把 fundamentals + membership 穿进打分/回测（缺省冻结），加横截面价值闸阶段（`value_frac` 时两段 `select_top`）。PB-alone 基线 = 既有 combine 路径（value 树作 quality、λ=0、select top-N 最便宜），无需新选择码；+动量 = 新 value_frac 两段路径。

**Tech Stack:** Rust（screen/portfolio/data 模块，DSL fund.* + Membership 已在位）；YAML 树/配置；data/*.csv（②已生成，gitignored）。

**Spec:** `docs/superpowers/specs/2026-06-16-fundamental-technical-selection-design.md`

## Global Constraints

- 提交信息英文 + footer `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`。
- 闸：`cargo test --workspace` + `cargo clippy --workspace --all-targets`（screen/portfolio 是引擎公共路径，桥接 crate 依赖）。
- **行为冻结铁律**：fundamentals/membership/value_frac 缺省时，screen 与 portfolio 逐字同改造前（现有 screener/portfolio 测试不破）。
- **point-in-time**：membership 排名≤d + `fund.as_of(t)`≤t（首报前 NaN 弃权）。
- 数据在主仓 `data/`（绝对路径或 cwd=主仓）；产物 gitignored。
- **Worktree**：执行起手 `git worktree add .claude/worktrees/worktree-fundtech -b worktree-fundtech HEAD` 然后 `EnterWorktree(path=...)`（从本地 HEAD，**非** fresh）。用户消息边界会还原 cwd → 每次 re-EnterWorktree。
- **§5.3 反过拟合**：敏感面须普遍正、OOS 不调参；falsification 是有效产出。

## 文件结构

| 文件 | 职责 |
|---|---|
| `src/backtest/portfolio.rs`（改）| `score_symbol` 加 `fundamentals: Option<&FundamentalSeries>` 参；portfolio 自身调用点穿 None（冻结）|
| `src/screen/mod.rs`（改）| `score_and_leaf` 加 fund 参；`run_screen` 加载 funds + membership + 价值闸 |
| `src/screen/backtest.rs`（改）| `eval_symbol`/loop 穿 funds + membership mask + value_frac 两段选择 |
| `src/screen/combine.rs`（改）| `CombineOutput` 加 `tilt` 字段（暴露已算的倾斜量）|
| `src/screen/config.rs`（改）| `ScreenConfig` 加 `value_frac: Option<f64>`（serde default None）|
| `src/cli/mod.rs`（改）| `Cmd::Screen` 加 `--membership`（--from/--to 已有）|
| `examples/trees/screen/value_pb.yaml`（新）| 价值/cheapness 树（fund.bps）|
| `examples/trees/screen/momentum_xs.yaml`（新）| 横截面动量 setup 树 |
| `examples/screen/value_momentum_v1.yaml`（新）| 价值闸+动量配置 |
| `examples/screen/value_baseline_v1.yaml`（新）| PB-alone 基线配置（λ=0、无 value_frac）|
| `docs/{cli,dsl}-reference.md`（改）| screen `--membership`/`value_frac` + fund. 在 screen 可用 |
| `docs/superpowers/2026-06-16-fundamental-technical-selection-findings.md`（新）| 两里程碑诚实判定 |

---

## Task 1: fundamentals 穿进 screen 打分（Rust，TDD）

**Files:** Modify `src/backtest/portfolio.rs`（score_symbol 加参 + 自身调用点穿 None）、`src/screen/mod.rs`（score_and_leaf 加参 + run_screen 加载 funds）、`src/screen/backtest.rs`（eval_symbol 加参 + loop 加载 funds）。

**Interfaces:**
- Consumes: `crate::data::fundamentals::{FundamentalSeries, load_fundamentals_csv}`（①已建）；`UniverseEntry.fundamentals: Option<PathBuf>`（①已建）。
- Produces: `score_symbol(..., fundamentals: Option<&FundamentalSeries>, ...)`；screen 打分链可见 `fund.*`。

- [ ] **Step 1: 写失败测试**（screen backtest 用 fund.bps 价值树能打出分）

在 `src/screen/backtest.rs` 的 `#[cfg(test)]` 加（仿现有 `backtest_*` 测试的 wf/bars 辅助）：
```rust
    #[tokio::test]
    async fn backtest_reads_fundamentals_for_value_tree() {
        // 价值树用 fund.bps；若 fundamentals 没穿进来，fund.bps=NaN → 全弃权 → 无选中。
        let vtree = wf(".yaml", r#"
meta: { name: v, forward_window: 1, stances: [long, flat] }
root: g
nodes:
  g: { type: quant, branches: [ { when: "fund.bps > 0", goto: l, label: cheap } ], default: { goto: f, label: flat } }
leaves: { l: { stance: long, weight: "1 / (1 + close/fund.bps)" }, f: { stance: flat } }
"#);
        let m = wf(".yaml", M_SIMPLE);
        let cfg_yaml = format!(
            "quality_trees: [{}]\nsetup_trees:\n  动量延续: [{}]\nmerge: {{ q_floor: 0.0, top: 1 }}\n",
            vtree.path().to_str().unwrap().replace('\\', "/"),
            m.path().to_str().unwrap().replace('\\', "/"),
        );
        let cfg_f = wf(".yaml", &cfg_yaml);
        let up = bars(0.01);
        // 逐股财务 CSV：bps=5（公告日早于回测窗）
        let fund = wf(".csv", "time,roe,np_yoy,rev_yoy,gross_margin,eps,bps\n2023-12-01,10,5,5,30,1.0,5.0\n");
        let mut univ = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        // universe 第4列 fundamentals
        writeln!(univ, "symbol,primary,context,fundamentals\nUP,{p},,{f}",
            p = up.path().to_str().unwrap().replace('\\', "/"),
            f = fund.path().to_str().unwrap().replace('\\', "/")).unwrap();
        univ.flush().unwrap();
        let cfg = ScreenBacktestConfig {
            config_path: cfg_f.path().to_path_buf(), universe_path: univ.path().to_path_buf(),
            from: None, to: None, rebalance: 4, top: None, warmup: 5, window: 10, cost_bps: 0.0, soft: false, out_path: None,
        };
        let r = run_screen_backtest(&cfg, &LlmEvaluator::Disabled).await.unwrap();
        // fund.bps 已穿进来 → 价值树出分 → UP 被选中（avg_members>0）
        assert!(r.avg_members > 0.0, "value tree using fund.bps must score (fundamentals threaded)");
    }
```
（`bars(0.01)` 的日期是 2024-01-01 起，fund 公告日 2023-12-01 在前 → as_of 命中 bps=5。）

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --lib screen::backtest::tests::backtest_reads_fundamentals_for_value_tree`
Expected: FAIL（`avg_members=0`——fund.bps=NaN 因 fundamentals 未穿进，价值树全弃权）。

- [ ] **Step 3: 实现 fundamentals 穿线**

(a) `src/backtest/portfolio.rs` `score_symbol`（:83-96）加参 + 用之：
```rust
pub async fn score_symbol(
    primary: &[Bar],
    context: &[Bar],
    aux: &BTreeMap<String, crate::data::aux_table::AuxTable>,
    tree: &crate::tree::loader::Tree,
    llm: &LlmEvaluator,
    soft: bool,
    t: NaiveDateTime,
    window: usize,
    fundamentals: Option<&crate::data::fundamentals::FundamentalSeries>,
) -> crate::Result<Option<f64>> {
    if !is_fresh(primary, t) {
        return Ok(None);
    }
    let ctx = crate::features::context::build_context(primary, context, &[], aux, fundamentals, t, window);
    // ...（其余不变）
```
(b) portfolio 内 `score_symbol` 调用点（grep `score_symbol(` in portfolio.rs）末参补 `None`（冻结）。

(c) `src/screen/mod.rs` `score_and_leaf`（:72-88）加 `fundamentals: Option<&FundamentalSeries>` 参，`build_context(..., fundamentals, t, window)`；`run_screen` 加载 funds 并传（见 (e)）。

(d) `src/screen/backtest.rs` `eval_symbol`（:106）加 `funds: Option<&crate::data::fundamentals::FundamentalSeries>` 参，传给两处 `score_symbol(...)`（quality + setups）末参。

(e) 两处 universe 加载（`run_screen` mod.rs:128-130、`run_screen_backtest` backtest.rs:175-178）后加载 funds：
```rust
    let mut funds: Vec<Option<crate::data::fundamentals::FundamentalSeries>> = Vec::with_capacity(universe.len());
    for e in &universe {
        funds.push(e.fundamentals.as_ref().map(|p| crate::data::fundamentals::load_fundamentals_csv(p)).transpose()?);
    }
```
backtest 调用点（:221）`eval_symbol(&primaries[i], &contexts[i], &aux, &quality, &setups, llm, cfg.soft, t_rb, cfg.window, &mp, funds[i].as_ref())`。
run_screen 同理把 `funds[i].as_ref()` 传进 score_and_leaf。

- [ ] **Step 4: 跑测试确认通过 + 冻结回归**

Run: `cargo test --lib screen backtest portfolio`
Expected: 新测试 PASS；**所有现有 screen/portfolio 测试仍 PASS**（funds=None 冻结）。

- [ ] **Step 5: 提交**

```bash
git add src/backtest/portfolio.rs src/screen/mod.rs src/screen/backtest.rs
git commit -F - <<'EOF'
feat(screen): thread fundamentals into screen scoring (fund.* in trees)

score_symbol/score_and_leaf/eval_symbol gain an Option<&FundamentalSeries>;
run_screen + backtest load per-symbol funds from the universe and pass them, so
screen trees can use fund.bps/fund.eps. None preserves portfolio/screen behavior.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task 2: membership mask 穿进 screen（Rust，TDD）

**Files:** Modify `src/screen/backtest.rs`（ScreenBacktestConfig + loop mask）、`src/screen/mod.rs`（ScreenRunConfig + as-of mask）、`src/cli/mod.rs`（`Cmd::Screen` 加 `--membership` + 透传）。

**Interfaces:**
- Consumes: `crate::data::membership::Membership::{load_csv, effective_at}`（②已建）。
- Produces: `ScreenBacktestConfig.membership_path: Option<PathBuf>`；每再平衡 eligible ∩ membership-at-t。

- [ ] **Step 1: 写失败测试**（membership 限制候选）

在 `src/screen/backtest.rs` 测试加：
```rust
    #[tokio::test]
    async fn backtest_membership_restricts_candidates() {
        let q = wf(".yaml", Q_SIMPLE);
        let m = wf(".yaml", M_SIMPLE);
        let cfg_yaml = format!(
            "quality_trees: [{}]\nsetup_trees:\n  动量延续: [{}]\nmerge: {{ q_floor: 0.0, top: 5 }}\n",
            q.path().to_str().unwrap().replace('\\', "/"),
            m.path().to_str().unwrap().replace('\\', "/"),
        );
        let cfg_f = wf(".yaml", &cfg_yaml);
        let up = bars(0.01);
        let up2 = bars(0.012);
        let mut univ = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        writeln!(univ, "symbol,primary\nUP,{}\nUP2,{}", up.path().to_str().unwrap().replace('\\',"/"), up2.path().to_str().unwrap().replace('\\',"/")).unwrap();
        univ.flush().unwrap();
        // membership：只含 UP（不含 UP2），日期覆盖全程
        let mem = wf(".csv", "date,symbol\n2024-01-01,UP\n");
        let cfg = ScreenBacktestConfig {
            config_path: cfg_f.path().to_path_buf(), universe_path: univ.path().to_path_buf(),
            from: None, to: None, rebalance: 4, top: None, warmup: 5, window: 10, cost_bps: 0.0, soft: false, out_path: None,
            membership_path: Some(mem.path().to_path_buf()),
        };
        let r = run_screen_backtest(&cfg, &LlmEvaluator::Disabled).await.unwrap();
        // 仅 UP 可入选；UP2 被 membership 挡
        for h in &r.holdings {
            for (s, _) in &h.selected { assert_eq!(s, "UP", "UP2 must be masked out by membership"); }
        }
    }
```

- [ ] **Step 2: 跑确认失败**

Run: `cargo test --lib screen::backtest::tests::backtest_membership_restricts_candidates`
Expected: 编译失败（`ScreenBacktestConfig` 无 `membership_path`）。

- [ ] **Step 3: 实现 membership mask**

(a) `ScreenBacktestConfig`（backtest.rs:83-95）末加 `pub membership_path: Option<PathBuf>,`。
(b) `run_screen_backtest`：universe 加载后：
```rust
    let membership = cfg.membership_path.as_ref()
        .map(|p| crate::data::membership::Membership::load_csv(p)).transpose()?;
```
(c) 再平衡 eval 循环（:220 `for (i, e) in universe.iter().enumerate()`）体首加 mask 闸：
```rust
        for (i, e) in universe.iter().enumerate() {
            if let Some(m) = &membership {
                match m.effective_at(t_rb) {
                    Some(set) if set.contains(&e.symbol) => {}
                    _ => continue, // 非当期成员（或早于首期）→ 跳过
                }
            }
            if let Some(ev) = eval_symbol(/* ... */).await? { /* ... */ }
        }
```
(d) `ScreenRunConfig`（mod.rs:53-60）+ `run_screen` 同加 `membership_path` + as-of mask（as_of 时点 `m.effective_at(picked_t)`）。
(e) `src/cli/mod.rs` `Cmd::Screen`（:242-261）加 `#[arg(long)] membership: Option<PathBuf>,`；match 臂（:670）解构 + 两处配置构造（ScreenBacktestConfig:684 / ScreenRunConfig:700）填 `membership_path: membership.clone()`（backtest）/`membership`（as-of）。

- [ ] **Step 4: 跑测试 + 全量编译**

Run: `cargo test --lib screen` 然后 `cargo test --workspace`
Expected: 新测试 PASS；现有全绿；workspace 编译（CLI 改动 + 桥接 crate）。

- [ ] **Step 5: 提交**

```bash
git add src/screen/backtest.rs src/screen/mod.rs src/cli/mod.rs
git commit -F - <<'EOF'
feat(screen): point-in-time membership mask (--membership)

Each rebalance restricts candidates to members effective at t (survivorship-free
top-2000). None preserves prior behavior.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task 3: value_frac 横截面价值闸两段选择（Rust，TDD）

**Files:** Modify `src/screen/combine.rs`（CombineOutput.tilt）、`src/screen/backtest.rs`（SymbolEval.tilt + 两段选择）、`src/screen/config.rs`（ScreenConfig.value_frac）。

**Interfaces:**
- Consumes: `select_top`（portfolio，已在 backtest 用）。
- Produces: `ScreenConfig.value_frac: Option<f64>`；value_frac=Some 时两段选择（价值闸→动量 top-N）。

- [ ] **Step 1: 暴露 combine 的 tilt**

`src/screen/combine.rs` `CombineOutput`（:19-28）加字段 `pub tilt: f64,`；`combine`（:82-88）返回处加 `tilt,`（`tilt` 局部已在 :74-78 算好）。同步修 combine.rs 内构造 CombineOutput 的测试断言（若有逐字段构造则补；现有测试用字段访问不受影响）。

- [ ] **Step 2: 写失败测试**（value_frac 价值闸：贵的股被闸掉）

`src/screen/backtest.rs` 测试加（两只都涨[动量同向]，但 PB 不同 → 价值闸只留便宜的）：
```rust
    #[tokio::test]
    async fn backtest_value_gate_keeps_cheapest() {
        // 价值树 weight=1/(1+close/fund.bps)：CHEAP bps 大(PB小,分高) / RICH bps 小(PB大,分低)
        let vtree = wf(".yaml", r#"
meta: { name: v, forward_window: 1, stances: [long, flat] }
root: g
nodes: { g: { type: quant, branches: [ { when: "fund.bps > 0", goto: l, label: c } ], default: { goto: f, label: flat } } }
leaves: { l: { stance: long, weight: "1 / (1 + close/fund.bps)" }, f: { stance: flat } }
"#);
        let mtree = wf(".yaml", r#"
meta: { name: m, forward_window: 1, stances: [long, flat] }
root: g
nodes: { g: { type: quant, branches: [ { when: "close > ref(close, 2)", goto: l, label: up } ], default: { goto: f, label: flat } } }
leaves: { l: { stance: long, weight: "sigmoid((close/ref(close,2) - 1) * 50)" }, f: { stance: flat } }
"#);
        // value_frac: 0.5 → 两只里留最便宜 1 只
        let cfg_yaml = format!(
            "quality_trees: [{}]\nsetup_trees:\n  动量延续: [{}]\nmerge: {{ q_floor: 0.0, top: 5 }}\nvalue_frac: 0.5\n",
            vtree.path().to_str().unwrap().replace('\\',"/"), mtree.path().to_str().unwrap().replace('\\',"/"));
        let cfg_f = wf(".yaml", &cfg_yaml);
        let px = bars(0.01); // 两只同价同涨
        // CHEAP: bps=50（PB 低，价值分高）；RICH: bps=1（PB 高，价值分低）
        let fc = wf(".csv", "time,roe,np_yoy,rev_yoy,gross_margin,eps,bps\n2023-12-01,10,5,5,30,1,50\n");
        let fr = wf(".csv", "time,roe,np_yoy,rev_yoy,gross_margin,eps,bps\n2023-12-01,10,5,5,30,1,1\n");
        let mut univ = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        writeln!(univ, "symbol,primary,context,fundamentals\nCHEAP,{p},,{fc}\nRICH,{p2},,{fr}",
            p=px.path().to_str().unwrap().replace('\\',"/"), p2=bars(0.01).path().to_str().unwrap().replace('\\',"/"),
            fc=fc.path().to_str().unwrap().replace('\\',"/"), fr=fr.path().to_str().unwrap().replace('\\',"/")).unwrap();
        univ.flush().unwrap();
        let cfg = ScreenBacktestConfig {
            config_path: cfg_f.path().to_path_buf(), universe_path: univ.path().to_path_buf(),
            from: None, to: None, rebalance: 4, top: None, warmup: 5, window: 10, cost_bps: 0.0, soft: false, out_path: None,
            membership_path: None,
        };
        let r = run_screen_backtest(&cfg, &LlmEvaluator::Disabled).await.unwrap();
        // 价值闸留最便宜 1 只 → 选中恒为 CHEAP，绝不含 RICH
        for h in &r.holdings {
            for (s, _) in &h.selected { assert_eq!(s, "CHEAP", "value gate must drop the expensive (high-PB) RICH"); }
        }
        assert!(r.avg_members > 0.0);
    }
```
（注意上方 `bars(0.01)` 调两次得两个独立文件——CHEAP/RICH 同涨幅、仅 bps 不同。）

- [ ] **Step 3: 跑确认失败**

Run: `cargo test --lib screen::backtest::tests::backtest_value_gate_keeps_cheapest`
Expected: 编译失败（`ScreenConfig`/`SymbolEval` 无相关字段；`value_frac` 未解析）。

- [ ] **Step 4: 实现 value_frac 两段选择**

(a) `src/screen/config.rs` `ScreenConfig`（:62-71）加 `#[serde(default)] pub value_frac: Option<f64>,`（None 冻结）。`validate` 加：`if let Some(f)=self.value_frac { if !(f>0.0 && f<=1.0) { return Err(...) } }`。
(b) `src/screen/backtest.rs` `SymbolEval`（:99-103）加 `tilt: f64,`；`eval_symbol`（:139-140）`SymbolEval { combined: out.combined_score, quality: out.quality_score, tilt: out.tilt, tags: out.tags }`。
(c) `run_screen_backtest`：读 `let value_frac = sc.value_frac;`（sc 已加载）。把选择（:228 `let selected = select_top(&scores, top);`）改为：
```rust
        let selected = if let Some(f) = value_frac {
            // 价值闸：按 quality（cheapness）取最便宜 ceil(f×n) 只
            let qv: Vec<(String, f64)> = evals.iter().map(|(s, e)| (s.clone(), e.quality)).collect();
            let keep = ((f * qv.len() as f64).ceil() as usize).max(1);
            let cheap = select_top(&qv, keep);
            let cheap_set: std::collections::BTreeSet<String> = cheap.iter().map(|(s, _)| s.clone()).collect();
            // 池内按动量 tilt 取 top-N
            let mv: Vec<(String, f64)> = evals.iter()
                .filter(|(s, _)| cheap_set.contains(*s))
                .map(|(s, e)| (s.clone(), e.tilt)).collect();
            select_top(&mv, top)
        } else {
            select_top(&scores, top)
        };
```
（注：`select_top` 过滤 s>0；quality=1/(1+PB)∈(0,1)>0、tilt=sigmoid∈(0,1)>0，均通过。）

- [ ] **Step 5: 跑测试 + 冻结 + clippy**

Run: `cargo test --lib screen` 然后 `cargo clippy --lib`
Expected: 新测试 PASS；现有 screen 测试全绿（value_frac=None 冻结）；clippy 净。

- [ ] **Step 6: 提交**

```bash
git add src/screen/combine.rs src/screen/backtest.rs src/screen/config.rs
git commit -F - <<'EOF'
feat(screen): cross-sectional value gate (value_frac) — cheapest fraction then momentum top-N

value_frac=Some(f): each rebalance keeps the cheapest ceil(f*n) by quality
(value score), then select_top by momentum tilt within. None = existing
combine-based select (frozen). CombineOutput exposes tilt.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task 4: 两棵树 value_pb + momentum_xs（YAML + 加载/lint 测试）

**Files:** Create `examples/trees/screen/value_pb.yaml`、`examples/trees/screen/momentum_xs.yaml`。

- [ ] **Step 1: 写树**

`examples/trees/screen/value_pb.yaml`：
```yaml
meta: { name: value_pb, forward_window: 20, stances: [long, flat] }
root: gate
nodes:
  gate:
    type: quant
    branches:
      - { when: "fund.bps > 0", goto: cheap, label: has_book }
    default: { goto: flat, label: no_book }
leaves:
  cheap: { stance: long, weight: "1 / (1 + close / fund.bps)" }   # PB↓ → 分↑，(0,1) 单调不饱和
  flat: { stance: flat }
```
`examples/trees/screen/momentum_xs.yaml`：
```yaml
meta: { name: momentum_xs, forward_window: 20, stances: [long, flat] }
params: { mom_n: 20, mom_scale: 5 }
root: gate
nodes:
  gate:
    type: quant
    branches:
      - { when: "close > ref(close, mom_n)", goto: up, label: up }
    default: { goto: flat, label: flat }
leaves:
  up: { stance: long, weight: "sigmoid((close / ref(close, mom_n) - 1) * mom_scale)" }  # 动量↑ → 分↑，(0,1)
  flat: { stance: flat }
```

- [ ] **Step 2: 加载 + lint 测试**

确认两树过既有 `all_example_trees_lint_clean`（tree/loader 或 lint 测试 glob `examples/trees/**`）。Run:
```
cargo test --lib all_example_trees_lint_clean
```
Expected: PASS（含两新树；若 lint 报恒假/空转 → 调表达式，但上式为单比较+单调权重，应净）。

- [ ] **Step 3: 真数据冒烟（加载不报错）**

Run（确认树能加载 + fund. 解析；cwd=主仓，有 data/）：
```
cargo run --release -- screen --universe data/universe_membership.csv --config examples/screen/value_momentum_v1.yaml --as-of 2024-06-28 --top 5 2>&1 | head -20
```
（此步依赖 Task 5 的配置；若配置未就位，先跳到 Task 5 再回跑。）Expected: 打印 as-of 选股清单、无加载错。

- [ ] **Step 4: 提交**

```bash
git add examples/trees/screen/value_pb.yaml examples/trees/screen/momentum_xs.yaml
git commit -F - <<'EOF'
feat(screen): value_pb + momentum_xs trees for value-gate + momentum selection

value_pb: cheapness = 1/(1+PB) monotone-unsaturated in (0,1). momentum_xs:
sigmoid-mapped mom_n-day return. Both rank-preserving under weight clamp[0,1].

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task 5: 配置 + 文档（YAML + docs）

**Files:** Create `examples/screen/value_momentum_v1.yaml`、`examples/screen/value_baseline_v1.yaml`；Modify `docs/cli-reference.md`、`docs/dsl-reference.md`。

- [ ] **Step 1: 写两配置**

`examples/screen/value_baseline_v1.yaml`（PB-alone 基线：value 树作 quality、λ=0、无 value_frac、无动量倾斜）：
```yaml
quality_trees: [examples/trees/screen/value_pb.yaml]
setup_trees:
  动量延续: [examples/trees/screen/momentum_xs.yaml]   # 仅标注，λ=0 不倾斜
merge: { q_floor: 0.0, top: 30, lambda: 0.0, tilt_setups: ["动量延续"], quality_layers: 5 }
regimes:
  - { label: "2018熊", from: 2018-01-02, to: 2018-12-28 }
  - { label: "2019-21牛", from: 2019-01-02, to: 2021-12-31 }
  - { label: "2022回调", from: 2022-01-04, to: 2022-12-30 }
  - { label: "2023-25", from: 2023-01-03, to: 2026-06-12 }
```
`examples/screen/value_momentum_v1.yaml`（价值闸+动量：value_frac 闸 + 动量 top-N）：
```yaml
quality_trees: [examples/trees/screen/value_pb.yaml]
setup_trees:
  动量延续: [examples/trees/screen/momentum_xs.yaml]
merge: { q_floor: 0.0, top: 30, lambda: 1.0, tilt_setups: ["动量延续"], quality_layers: 5 }
value_frac: 0.3   # 横截面最便宜 30% → 池内动量 top-N
regimes:
  - { label: "2018熊", from: 2018-01-02, to: 2018-12-28 }
  - { label: "2019-21牛", from: 2019-01-02, to: 2021-12-31 }
  - { label: "2022回调", from: 2022-01-04, to: 2022-12-30 }
  - { label: "2023-25", from: 2023-01-03, to: 2026-06-12 }
```

- [ ] **Step 2: 配置解析测试**

Run: `cargo test --lib screen::config`（确认 value_frac 解析 + 既有解析不破）。补一条解析测试若无：
```rust
    #[test]
    fn parses_value_frac() {
        let yaml = "quality_trees: [q.yaml]\nsetup_trees:\n  动量延续: [a.yaml]\nvalue_frac: 0.3\n";
        let cfg: ScreenConfig = serde_yaml::from_str(yaml).unwrap();
        cfg.validate().unwrap();
        assert_eq!(cfg.value_frac, Some(0.3));
    }
```
Expected: PASS。

- [ ] **Step 3: 文档**

`docs/cli-reference.md` screen 节加 `--membership <PATH>`（点时成员 mask）说明 + 指 value_momentum 配置；`docs/dsl-reference.md` 注明 **screen 树现可用 `fund.*`**（同 factor，point-in-time）。

- [ ] **Step 4: 提交**

```bash
git add examples/screen/value_momentum_v1.yaml examples/screen/value_baseline_v1.yaml docs/cli-reference.md docs/dsl-reference.md
git commit -F - <<'EOF'
feat(screen): value-momentum + PB-alone baseline configs + docs

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task 6: 里程碑1 — PB-alone 基线回测（联网/计算，控制器跑）

- [ ] **Step 1: 全期 + OOS 切片回测**

cwd=主仓。Run（基线 = value_baseline_v1，frozen 路径、λ=0 → 最便宜 top-30）：
```powershell
# 全期
cargo run --release -- screen --backtest --universe data/universe_membership.csv --membership data/membership_top2000.csv --config examples/screen/value_baseline_v1.yaml --top 30 --rebalance 20 --warmup 60 --window 120 --cost-bps 20 --out base_full.json
# IS 定参窗
cargo run --release -- screen --backtest ... --from 2018-01-01 --to 2022-12-31 --out base_is.json
# OOS 验证窗
cargo run --release -- screen --backtest ... --from 2023-01-01 --to 2026-06-12 --out base_oos.json
```
Expected: 各出 total/benchmark/excess/Sharpe/maxDD + regime 切片。**记录**：基线 OOS 超额是否 >0（②的 PB IC 扣费后可交易？）。

- [ ] **Step 2: 记录中间结论**（findings 草稿，Task 8 汇总）。无 commit（产物 gitignored）。

---

## Task 7: 里程碑2 — 价值闸+动量 + 敏感面（计算，控制器跑）

- [ ] **Step 1: 主配置回测（全期 + OOS）**

Run（value_momentum_v1，value_frac 路径）：
```powershell
cargo run --release -- screen --backtest --universe data/universe_membership.csv --membership data/membership_top2000.csv --config examples/screen/value_momentum_v1.yaml --top 30 --rebalance 20 --warmup 60 --window 120 --cost-bps 20 --from 2023-01-01 --to 2026-06-12 --out vm_oos.json
```
Expected: vs 基线 + vs benchmark；**记录**动量是否在价值之上再添 alpha（OOS）。

- [ ] **Step 2: 敏感面（防尖峰）**

扫 `value_frac{0.2,0.3,0.5} × mom_n{20,60} × top{20,30,50} × rebalance{10,20}`（mom_n 改 momentum_xs 的 params 或多配置；value_frac/top/rebalance 走 CLI/配置）。逐组记 OOS 超额。**判据：须普遍正、无单点尖峰**（尖峰=过拟合，§5.3 弃）。
Expected: 一张敏感面表（findings 用）。

- [ ] **Step 3: 记录**（findings 草稿）。无 commit。

---

## Task 8: findings + 全量闸 + finishing + 记忆（控制器）

- [ ] **Step 1: 写 findings**

`docs/superpowers/2026-06-16-fundamental-technical-selection-findings.md`：两里程碑（基线 / +动量）全期+OOS+regime+敏感面表 + **works/inconclusive/falsified 诚实判定**。重点诚实问：基线 OOS 超额？动量增益？敏感面是否普遍正（vs 尖峰）？2018/2022 熊扛跌？对比 screener 弧线（纯技术证伪）+ ②（PB borderline）。**不调参凑超额（§5.3）**。

- [ ] **Step 2: 全量闸**

Run: `cargo test --workspace` + `cargo clippy --workspace --all-targets`
Expected: 全绿（含新 screen 测试 + 冻结回归）、clippy 净。

- [ ] **Step 3: 提交 findings + finishing**

```bash
git add docs/superpowers/2026-06-16-fundamental-technical-selection-findings.md
git commit -F - <<'EOF'
docs(fund-tech): value-gate + momentum selection findings (honest verdict)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
```
调用 `superpowers:finishing-a-development-branch`：核验测试 → ExitWorktree(keep) → master → `git merge --no-ff worktree-fundtech`（temp 文件 -F，英文；合并前 `git log master..worktree-fundtech` + 查并行提交）→ 清理 worktree（remove --force + prune + branch -d）。

- [ ] **Step 4: 更新记忆**

`memory/rquant-project.md` 加子项③ bullet（价值闸+动量机制、screen 接基本面+membership、两里程碑结论、对比②/screener）。仅记非显然、跨会话有用的。

---

## 自审（writing-plans）

**1. Spec 覆盖：**
- §3.1 基本面+membership 进 screen → Task 1 + Task 2 ✓
- §3.2 横截面价值闸两段（select_top×2）+ value_frac → Task 3 ✓；PB-alone 基线（frozen 路径 λ=0）→ Task 5 config + Task 6 ✓
- §3.3 两树（value_pb 1/(1+PB) / momentum_xs sigmoid）→ Task 4 ✓
- §4 验证（time-slice OOS〔--from/--to 已有〕+ regime〔config〕+ 敏感面 + 基准 top-2000 等权〔backtest 内置〕）→ Task 6/7 ✓
- §5 文件全覆盖 ✓；§6 诚实边界（冻结/point-in-time/survivorship/§5.3）→ 各步嵌入 + Task 8 findings ✓
- 闸 --workspace → Task 2/8 ✓

**2. 占位符扫描：** 无 TBD；代码步含完整 diff/signature；联网步给确切命令 + expected。✓

**3. 类型一致性：** `score_symbol(...fundamentals: Option<&FundamentalSeries>)`（T1 定义）= T1 portfolio 调用点 + T1 eval_symbol 传递一致；`ScreenBacktestConfig.membership_path`（T2）= T2 CLI 构造一致；`ScreenConfig.value_frac`（T3 config）= T3 backtest 读取 + T5 配置 + T5 测试一致；`CombineOutput.tilt`（T3 step1）= `SymbolEval.tilt`（T3 step4b）= 两段选择读取一致；树 `fund.bps`/`ref(close,mom_n)`/`sigmoid`（T4）= ① fund. 通道 + DSL 既有一致。✓
