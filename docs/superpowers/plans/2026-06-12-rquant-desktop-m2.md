# rquant 桌面端 M2（回测中心 + 数据工作台）实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 交付 C 回测中心（配置→运行→留档→五视图→对比，含初始资金 10w 展示层）与 A 数据工作台（CSV 清单/拉取/K线浏览+因子叠加/universe 管理），并清掉 M1 终审挂账（引擎状态原子写、CSP 基线）。

**Architecture:** 沿用 M1 形态——桥接层零业务逻辑调 `rquant` 库；新增引擎配合改动三项（原子写、sim 决策轨迹可选输出、因子表达式解析助手提升），全部默认关/行为冻结 + 锁测试。留档=每次运行一个 `.rquant-desktop/runs/<id>/` 目录（engine 自写 result/traces，桥接写 config/meta）。spec：`docs/superpowers/specs/2026-06-12-rquant-desktop-design.md` §5.2/§5.3/§7/§9。

**Tech Stack:** 同 M1（Tauri 2 / React 18 + antd 6 + zustand + echarts 6 / ts-rs 10）。

**分支：** `desktop-m2`（从 master 切出）。

---

## 工程师必读上下文（零仓库背景假设；事实已逐条核对源码）

**M1 已就绪的桥接层资产（直接复用）：**
- `crate::paths::Workspace`（root/paper_dir/deploy_dir/desktop_data_dir/journal_path/run_log_path + detect）
- `crate::tasks::TaskRegistry::start(kind, heavy, body) -> Result<String,String>`、`TaskCtx{cancelled(), progress(pct,stage,detail)}`（heavy 槽独占）
- `crate::dto`（ts-rs 全量导出到 `desktop/src-tauri/bindings/`，**bindings 已 .gitattributes 钉 LF**，regen 不脏树）
- `crate::error::ErrorDto::from_anyhow`
- UI：`api/ipc.ts` typed invoke、`stores/`、antd 6 + echarts 6 组件先例（NavChart 的 echarts init/dispose 模式、TaskDrawer、matchMedia polyfill 已配）
- 开发启动 = 两进程：`cd desktop/ui && npm run dev` + 仓库根 `cargo run -p rquant-desktop`（`npx tauri dev` 不可用——conf 在兄弟目录）

**引擎事实（已核对）：**
1. `rquant::backtest::runner::BacktestConfig` 字段：`tree_path/primary_path/context_path/news_path:Option/out_path/traces_path:Option/cost_bps/warmup/window/concurrency/holidays_path:Option/folds/aux_paths:Vec<(String,PathBuf)>`。`pub async fn run(cfg,llm)->Result<Report>`（打分硬）；`run_soft` 在 `backtest::soft`；`rquant::backtest::sim::run_sim(cfg, llm, soft: bool) -> Result<SimReport>`。
2. `SimReport{tree_name,cost_bps,total_return,max_drawdown,n_round_trips,win_rate,avg_hold_bars,turnover,buy_and_hold,trades:Vec<RoundTrip>,risk:Option<RiskMetrics>}`；`RoundTrip{entry_t,exit_t,entry_px,exit_px,max_abs_pos,trip_return,bars_held,reason}`；sim traces JSONL 行 = `SimStepRecord{t,target,pos,nav}`。`RiskMetrics` 含 `sharpe` 等 Option 字段。
3. 打分硬 traces JSONL 行 = `rquant::engine::trace::Trace{t,path:Vec<StepRecord>,leaf,stance}`、`StepRecord{node_id,label,confidence,rationale}`——**路径级回放数据打分模式已天然存在**。
4. `run_sim` 内部逐 bar 调 `traverse(tree,ctx,llm)` 得完整 `Trace`（src/backtest/sim.rs:564 附近），现仅取 `trace.leaf` 计算 target、路径被丢弃——E2 把它顺手写出。
5. `rquant::signal::{write_paper_state,write_holdings_state}` 目前是裸 `fs::write`（src/signal/mod.rs:139-143 与 499-501 附近）——E1 改原子写。
6. `rquant::features::context::build_context(primary:&[Bar],context:&[Bar],news:&[NewsRecord],aux:&BTreeMap<String,AuxTable>,t,window)->Context` 已 pub；`rquant::dsl::parser::parse_str(&str)->Result<Expr>` 已 pub；`rquant::dsl::eval::eval(&Expr,&Context)->Result<Value>` 已 pub（Value 有 Series/Scalar/Bool 三态，标量上下文取 Series 末位）。`Bar{time,open,high,low,close,volume}`。
7. 树 YAML 顶层 `params:`（名→f64）与 `factors:`（名→DSL，**文档序**、可引用先定义因子）在 `tree::loader` 加载期 AST 内联替换（搜 `substitute`）——E3 把这段解析/代入逻辑提升为可独立调用的 pub 助手（spec §4-2"提升不复制"）。
8. `rquant::cli::{run_fetch_to_csv, SINA_BASE_URL, build_llm}` 已 pub（M1）。fetch 落盘 CSV 表头与 `read_bars_csv` 兼容。
9. 基线测试数：**引擎 321（299 lib + 22 e2e）、桥接 47**。每任务后必须保持/递增，引擎黄金不变量（signal golden_invariant 系列）是底线闸。

**关键产品决策（已与用户确认，写死）：**
- **初始资金 = 纯展示层**：`initial_capital` 默认 **100000**，进 config 留档；引擎 nav 语义零改动。金额口径：资产曲线 = nav×资金（数学严格）、期末资产/净盈亏 = 资金×(1+总收益)/资金×总收益（严格）、**每笔盈亏额 = 资金×trip_return（单利近似口径，UI 注明）**。
- 运行模式四态：`sim_hard` / `sim_soft` / `score_hard` / `score_soft`。**五视图完整支持 sim_hard**；sim_soft 无路径回放（软遍历无单一路径，回放 tab 显示提示）；score_* 提供 概览(原样关键字段)+回放(score 的 traces 即 Trace)+原始 三视图。
- 留档目录契约：`.rquant-desktop/runs/<id>/{config.json, meta.json, result.json, traces.jsonl[, decision_traces.jsonl]}`——result/traces 由**引擎自写**（out_path/traces_path 指进 run 目录），config/meta 由桥接原子写。
- run id 格式 `YYYYMMDD-HHMMSS-<pid四位hex>-<seq两位>`，**delete 前必须正则校验 id**（防路径穿越）。
- 数据工作台拉取落 `.rquant-desktop/data/{symbol}_{scale}_{adjust}.csv`（paper/ 归账本所有，不混写）；universe 自定义清单在 `.rquant-desktop/universes/*.csv`，`deploy/*.csv` 只读展示。
- CSV/树路径入参一律经 workspace 归一与**越界守卫**（canonicalize 后必须以 ws.root 为前缀）。

**纪律红线（同 M1）：** 重放/回测语义冻结——引擎改动仅 E1/E2/E3 三项且各带行为零变锁；git add 点名文件；提交信息英文；不碰 deploy 冻结树。

**验证命令（每任务通用）：** `cargo test`（引擎全量）/ `cargo test -p rquant-desktop` / `cargo clippy --workspace --all-targets -- -D warnings` / `cd desktop/ui && npx tsc --noEmit && npx vitest run && npm run build`。

---

### Task E1: 引擎状态文件原子写（TDD，M1 终审挂账）

**Files:**
- Modify: `src/signal/mod.rs`（write_paper_state / write_holdings_state + 新私有助手）

- [ ] **Step 1: 切分支**

```bash
git checkout -b desktop-m2
```

- [ ] **Step 2: 写失败测试**（加在 `src/signal/mod.rs` 既有 `#[cfg(test)]` 模块内；先找到该模块再追加）

```rust
    #[test]
    fn write_paper_state_is_atomic_and_overwrites() {
        let td = tempfile::tempdir().unwrap();
        let path = td.path().join("st.json");
        let tree_name = "t".to_string();
        let mk = |nav: f64| {
            let mut acc = SimAccount::default();
            acc.nav = nav;
            PaperState {
                version: 1,
                tree_name: tree_name.clone(),
                last_time: None,
                account: acc.snapshot(),
            }
        };
        write_paper_state(&path, &mk(1.0)).unwrap();
        write_paper_state(&path, &mk(1.5)).unwrap(); // 覆盖既有文件(Windows rename 替换语义)
        let back = read_paper_state(&path, &tree_name).unwrap().unwrap();
        assert!((back.account.nav - 1.5).abs() < 1e-12);
        // 临时文件不得残留
        let leftovers: Vec<_> = std::fs::read_dir(td.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|x| x == "tmp").unwrap_or(false))
            .collect();
        assert!(leftovers.is_empty(), "tmp leftover: {:?}", leftovers);
    }

    #[test]
    fn write_holdings_state_is_atomic_and_overwrites() {
        let td = tempfile::tempdir().unwrap();
        let path = td.path().join("h.json");
        let mk = |w: f64| {
            let mut holdings = std::collections::BTreeMap::new();
            holdings.insert("sh600000".to_string(), w);
            HoldingsState { version: 1, tree_name: "t".into(), last_time: None, holdings }
        };
        write_holdings_state(&path, &mk(0.5)).unwrap();
        write_holdings_state(&path, &mk(1.0)).unwrap();
        let back = read_holdings_state(&path, "t").unwrap().unwrap();
        assert!((back.holdings["sh600000"] - 1.0).abs() < 1e-12);
    }
```

- [ ] **Step 3: 跑测试确认失败形态**

Run: `cargo test -p rquant write_paper_state_is_atomic`
Expected: 编译过但测试可能已绿（裸 write 也能覆盖）——本任务的"失败形态"是 tmp 残留断言对**新实现**的约束；先确认两测试能跑，再改实现。

- [ ] **Step 4: 实现原子写**（spec §7：temp + rename；`.json` → `.json.tmp` 同目录保证同卷 rename）

在 State IO 区加私有助手并改两个写函数：

```rust
/// 原子落盘:同目录写 .json.tmp 再 rename 替换(Windows MoveFileEx 替换语义,std 文档保证)。
/// spec §7——半写状态文件不可能被 read_paper_state 观测为 corrupt。
fn write_json_atomic(path: &Path, json: &str) -> Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}
```

`write_paper_state` / `write_holdings_state` 的 `std::fs::write(path, json)?` 替换为 `write_json_atomic(path, &json)?`（序列化行不动）。

- [ ] **Step 5: 全量验证**

Run: `cargo test`
Expected: 321+2 全绿——尤其 `golden_invariant_*` 系列与 e2e（写路径换了但内容字节不变）。

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean。

- [ ] **Step 6: Commit**

```bash
git status --porcelain
git add src/signal/mod.rs
git commit -m "fix(signal): atomic temp+rename state writes (spec 7, M1 final-review carry-over)"
```

---

### Task E2: 引擎 sim 决策轨迹可选输出（默认 None 行为零变）

**Files:**
- Modify: `src/backtest/runner.rs`（BacktestConfig 加字段）
- Modify: `src/backtest/sim.rs`（run_sim 硬分支写 Trace JSONL）
- Modify: 所有 `BacktestConfig { ... }` 字面量构造点（cli/optimize/测试助手——编译器指路，全部补 `decision_traces_path: None`）

- [ ] **Step 1: 写失败测试**（`src/backtest/sim.rs` 测试模块内；构造方式照抄既有 sim 测试的 `make_cfg`/fixture 模式——先读测试模块找到现成的树+CSV fixture 助手并复用，下面代码里的 `make_cfg`/fixture 名以现场为准微调）

```rust
    #[tokio::test]
    async fn decision_traces_written_when_path_set_and_report_unchanged() {
        // 复用本模块既有 sim e2e fixture(树文件+合成 bars CSV+out tempfile)
        // 跑两次:一次 decision_traces_path=None,一次 Some(tmp)
        // 断言:1) Some 跑生成文件,行数>0,每行可反序列化为 engine::trace::Trace 且 path 非空
        //      2) 两次 SimReport serde_json 串完全相等(字段级行为零变)
        let llm = LlmEvaluator::stub();
        let (tree_f, bars_f, out_f) = sim_fixture(); // ← 以测试模块现成助手为准
        let mut cfg = make_cfg(&tree_f, &bars_f, &out_f, None);
        cfg.decision_traces_path = None;
        let r1 = run_sim(&cfg, &llm, false).await.unwrap();

        let dt = tempfile::Builder::new().suffix(".jsonl").tempfile().unwrap();
        let mut cfg2 = make_cfg(&tree_f, &bars_f, &out_f, None);
        cfg2.decision_traces_path = Some(dt.path().to_path_buf());
        let r2 = run_sim(&cfg2, &llm, false).await.unwrap();

        assert_eq!(
            serde_json::to_string(&r1).unwrap(),
            serde_json::to_string(&r2).unwrap(),
            "report must be bit-identical regardless of decision trace emission"
        );
        let txt = std::fs::read_to_string(dt.path()).unwrap();
        let lines: Vec<_> = txt.lines().collect();
        assert!(!lines.is_empty());
        for l in &lines {
            let tr: crate::engine::trace::Trace = serde_json::from_str(l).unwrap();
            assert!(!tr.path.is_empty(), "trace path must be recorded");
        }
    }
```

若本模块没有可直接复用的 fixture 助手，照 `src/backtest/sim.rs` 既有 e2e 测试（搜 `make_cfg`，929 行附近）的构造方式内联写一份最小树（quant 两分支→long/flat 叶）+ 30 根上升 bars CSV。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p rquant decision_traces_written`
Expected: 编译失败（字段不存在）。

- [ ] **Step 3: 实现**

`runner.rs` BacktestConfig 末尾加字段（带文档注释）：

```rust
    /// 可选:sim 硬模式逐 bar 完整决策轨迹(Trace JSONL)输出路径。
    /// None(默认,CLI 不暴露)=零行为变化;桌面端决策回放消费(spec §4-3)。
    pub decision_traces_path: Option<std::path::PathBuf>,
}
```

编译器会指出所有字面量构造点（cli/mod.rs 的 backtest/optimize 臂、optimize 模块内部、runner/sim/soft 各测试助手）——逐一补 `decision_traces_path: None,`。**只补字段，不动任何其他行。**

`sim.rs` 的 `run_sim`：在既有 `if let Some(tp) = &cfg.traces_path`（536 行附近）同样式样，硬分支（`traverse` 调用处，564 附近）把拿到的 `trace` 序列化追加：

```rust
        // 决策轨迹(硬模式专属;软遍历无单一路径,见计划决策)
        let mut decision_w = match (&cfg.decision_traces_path, soft) {
            (Some(p), false) => Some(std::io::BufWriter::new(std::fs::File::create(p)?)),
            _ => None,
        };
```

循环内拿到 `trace` 后（在取 `trace.leaf` 之前或之后均可，**不得改动既有任何一行的语义**）：

```rust
        if let Some(w) = decision_w.as_mut() {
            use std::io::Write;
            serde_json::to_writer(&mut *w, &trace)?;
            writeln!(w)?;
        }
```

循环结束后 flush（`if let Some(mut w) = decision_w { use std::io::Write; w.flush()?; }`）。

- [ ] **Step 4: 验证**

Run: `cargo test`
Expected: 全绿（321+2+1）。锁点 = 新测试的 report 串相等断言 + 全部既有 sim/golden 测试未动。

Run: `cargo clippy --workspace --all-targets -- -D warnings` → clean。

- [ ] **Step 5: Commit**

```bash
git status --porcelain
git add src/backtest/runner.rs src/backtest/sim.rs src/cli/mod.rs src/optimize
git status --porcelain   # 若测试助手在别的文件也补了字段,一并点名 add
git commit -m "feat(backtest): optional sim decision-trace jsonl, default off bit-identical (spec 4-3)"
```

---

### Task E3: 因子表达式解析助手提升（resolve_factor_exprs）

**Files:**
- Modify: `src/tree/loader.rs`（抽取/提升，零行为变化）

- [ ] **Step 1: 读现场**

打开 `src/tree/loader.rs`，定位 params/factors 的物化段（搜 `substitute` 与 `factors`）。现状：加载时把 `params` 名代入为常量、把 `factors` 按**文档序**逐个代入（后定义因子可引用先定义者），最终因子被内联进节点条件（可能再包 `Expr::Cached(slot, ..)` 做 memoize）。**注意：本任务提取的是"解析 yaml → 代入后的因子 Expr 列表"这一段，不含 Cached 槽位分配**（槽位是全树唯一资源，独立求值不需要——Context.eval_cache 对未包 Cached 的表达式天然无感）。

若现场结构与上述理解不符（如 factors 不是独立物化、或代入逻辑无法无损抽取），**STOP 报告 BLOCKED** 并贴出现场代码段——不要硬改。

- [ ] **Step 2: 写失败测试**（loader.rs 测试模块内）

```rust
    #[test]
    fn resolve_factor_exprs_substitutes_params_and_prior_factors() {
        let yaml = r#"
meta: { name: "t", forward_window: 4, stances: [long, flat] }
params: { n: 3.0 }
factors:
  base: "sma(close, n)"
  derived: "base / close"
root: r
nodes:
  r:
    type: quant
    branches:
      - when: "derived > 1.0"
        goto: l
    default: { goto: f }
leaves:
  l: { stance: long }
  f: { stance: flat }
"#;
        let factors = resolve_factor_exprs(yaml).unwrap();
        assert_eq!(factors.len(), 2);
        assert_eq!(factors[0].0, "base");
        assert_eq!(factors[1].0, "derived");
        // 求值验证代入正确:5 根 close=10 → sma(close,3)=10 → derived=1.0
        let bars: Vec<crate::data::bar::Bar> = (0..5)
            .map(|i| crate::data::bar::Bar {
                time: chrono::NaiveDate::from_ymd_opt(2026, 1, 1)
                    .unwrap()
                    .and_hms_opt(9, 30 + i, 0)
                    .unwrap(),
                open: 10.0, high: 10.0, low: 10.0, close: 10.0, volume: 1.0,
            })
            .collect();
        let ctx = crate::features::context::build_context(
            &bars, &bars, &[], &Default::default(), bars[4].time, 5,
        );
        let v = crate::dsl::eval::eval(&factors[1].1, &ctx).unwrap();
        let last = match v {
            crate::dsl::eval::Value::Scalar(x) => x,
            crate::dsl::eval::Value::Series(s) => *s.last().unwrap(),
            _ => panic!("unexpected value kind"),
        };
        assert!((last - 1.0).abs() < 1e-12);
    }

    #[test]
    fn resolve_factor_exprs_empty_factors_ok() {
        let yaml = r#"
meta: { name: "t", forward_window: 4, stances: [long, flat] }
root: r
nodes:
  r:
    type: quant
    branches:
      - when: "close > open"
        goto: l
    default: { goto: f }
leaves:
  l: { stance: long }
  f: { stance: flat }
"#;
        assert!(resolve_factor_exprs(yaml).unwrap().is_empty());
    }
```

- [ ] **Step 3: 跑测试确认失败**

Run: `cargo test -p rquant resolve_factor_exprs`
Expected: 编译失败（函数不存在）。

- [ ] **Step 4: 实现**——把加载路径里"解析 params/factors + 逐因子代入"的现有内部逻辑抽为：

```rust
/// 解析树 YAML 的 params/factors 并完成代入(params→常量、先序因子→内联),
/// 返回文档序 (因子名, 已代入 Expr) 列表——**不分配 Cached 槽**,供独立求值
/// (桌面端决策回放因子表/K线因子叠加,spec §4-2)。
/// 与 load_tree_str 的物化语义同源:本函数被其复用或与其共享同一内部助手,禁止复制粘贴两份代入逻辑。
pub fn resolve_factor_exprs(yaml: &str) -> Result<Vec<(String, crate::dsl::ast::Expr)>> {
    // 实现要求:复用现场已有的 RawTree/serde 结构与 substitute 链;
    // load_tree_str 改为调用同一助手(行为零变,既有全量测试为锁)。
}
```

重构铁律：`load_tree_file`/`load_tree_str` 对外行为零变化（树结构、错误信息、Cached 槽位分配全都不变）——**引擎全量 321 测试是判据**。

- [ ] **Step 5: 验证**

Run: `cargo test` → 321+2+1+2 全绿；clippy clean。

- [ ] **Step 6: Commit**

```bash
git status --porcelain
git add src/tree/loader.rs
git commit -m "feat(tree): pub resolve_factor_exprs - shared factor materialization for desktop replay (spec 4-2)"
```

---

### Task B1: runs 留档基建 + M2 全量 DTO

**Files:**
- Create: `desktop/src-tauri/src/runs.rs`
- Modify: `desktop/src-tauri/src/paths.rs`（+runs_dir/data_dir/universes_dir）
- Modify: `desktop/src-tauri/src/dto.rs`（M2 全部 DTO 一次定义——后续任务签名以此为准）
- Modify: `desktop/src-tauri/src/lib.rs`（`pub mod runs;`）
- Modify（生成物）: `desktop/src-tauri/bindings/*.ts`

- [ ] **Step 1: paths.rs 加三个目录访问器**（紧随 journal_path 之后）

```rust
    pub fn runs_dir(&self) -> PathBuf {
        self.desktop_data_dir().join("runs")
    }
    pub fn data_dir(&self) -> PathBuf {
        self.desktop_data_dir().join("data")
    }
    pub fn universes_dir(&self) -> PathBuf {
        self.desktop_data_dir().join("universes")
    }
```

并在 paths 测试 `workspace_paths_join_correctly` 里补三行断言（`ends_with(".rquant-desktop/runs")` 等同款式）。

- [ ] **Step 2: dto.rs 追加 M2 DTO**（全部 `#[derive(Debug, Clone, Serialize, TS)] #[ts(export)]`；标注 Deserialize 的额外加）

```rust
// ───────────────────────── M2: 回测中心 / 数据工作台 ─────────────────────────

/// 回测运行配置(留档 config.json 原文;Deserialize 供读回与重跑)。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BacktestConfigDto {
    /// 工作区相对路径(examples/.. 或 deploy/..)。
    pub tree_path: String,
    /// 主行情 CSV(工作区相对)。fetch 置时由任务先拉取生成。
    pub primary_path: String,
    /// "sim_hard" | "sim_soft" | "score_hard" | "score_soft"
    pub mode: String,
    pub cost_bps: f64,
    pub warmup: u32,
    pub window: u32,
    /// 展示层初始资金(元);默认 100000。引擎 nav 语义不感知此值。
    pub initial_capital: f64,
    /// 可选:运行前刷新行情。
    pub fetch: Option<FetchSpecDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FetchSpecDto {
    pub symbol: String,
    /// 分钟:15/60;日线:240。
    pub scale: u32,
    pub datalen: u32,
    /// "qfq" | "none"
    pub adjust: String,
}

/// 留档条目(meta.json;Deserialize 供列表读回)。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RunMetaDto {
    pub id: String,
    /// 同 BacktestConfigDto.mode。
    pub kind: String,
    /// 用户可改名;默认 "<树名> × <primary 文件名>"。
    pub name: String,
    pub tree_name: String,
    pub created: String,
    pub ok: bool,
    pub error: Option<String>,
}

/// 概览指标卡(sim 全量;score 仅 kind/tree_name + raw)。
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RunSummaryDto {
    pub meta: RunMetaDto,
    pub config: BacktestConfigDto,
    pub total_return: Option<f64>,
    pub max_drawdown: Option<f64>,
    pub n_round_trips: Option<u32>,
    pub win_rate: Option<f64>,
    pub avg_hold_bars: Option<f64>,
    pub turnover: Option<f64>,
    pub buy_and_hold: Option<f64>,
    pub sharpe: Option<f64>,
    /// 资金换算(严格口径):initial_capital×(1+total_return) / ×total_return。
    pub final_equity: Option<f64>,
    pub net_pnl: Option<f64>,
    /// score 模式:result.json 原样(UI 原始视图/简版概览用)。sim 模式 None。
    #[ts(type = "unknown")]
    pub raw: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct EquityPointDto {
    pub t: String,
    pub nav: f64,
    /// nav × initial_capital。
    pub equity: f64,
    pub pos: f64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TradeDto {
    pub entry_t: String,
    pub exit_t: String,
    pub entry_px: f64,
    pub exit_px: f64,
    pub max_abs_pos: f64,
    pub trip_return: f64,
    pub bars_held: u32,
    pub reason: String,
    /// 资金×trip_return——单利近似口径(UI 注明)。
    pub pnl_amount: f64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ReplayStepDto {
    pub node_id: String,
    pub label: String,
    pub confidence: f64,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ReplayFrameDto {
    pub t: String,
    pub leaf: String,
    pub stance: String,
    pub path: Vec<ReplayStepDto>,
    /// sim 模式由 SimStepRecord 对齐补充;score 模式 None。
    pub target: Option<f64>,
    pub pos: Option<f64>,
    pub nav: Option<f64>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct FactorValueDto {
    pub name: String,
    /// 非有限→None(NaN 弃权语义如实呈现)。
    pub value: Option<f64>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct BarDto {
    pub t: String,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct FactorPointDto {
    pub t: String,
    pub value: Option<f64>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct CsvInfoDto {
    /// 工作区相对路径。
    pub path: String,
    /// 解析失败→None(坏文件如实列出)。
    pub rows: Option<u32>,
    pub first_t: Option<String>,
    pub last_t: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UniverseEntryDto {
    pub symbol: String,
    pub primary: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct UniverseInfoDto {
    pub path: String,
    pub name: String,
    /// deploy/ 下=true(只读)。
    pub frozen: bool,
    pub entries: Vec<UniverseEntryDto>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TreeInfoDto {
    pub path: String,
    /// load 失败→None + error。
    pub name: Option<String>,
    pub frozen: bool,
    pub error: Option<String>,
}
```

- [ ] **Step 3: 写 runs.rs 失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Workspace;

    fn ws() -> (tempfile::TempDir, Workspace) {
        let td = tempfile::tempdir().unwrap();
        (td, Workspace::new(td.path().to_path_buf()))
    }

    fn meta(id: &str) -> crate::dto::RunMetaDto {
        crate::dto::RunMetaDto {
            id: id.into(),
            kind: "sim_hard".into(),
            name: "n".into(),
            tree_name: "t".into(),
            created: "2026-06-12T21:00:00".into(),
            ok: true,
            error: None,
        }
    }

    #[test]
    fn run_id_format_and_uniqueness() {
        let a = new_run_id();
        let b = new_run_id();
        assert!(is_valid_run_id(&a), "{}", a);
        assert_ne!(a, b, "seq must disambiguate same-second ids");
    }

    #[test]
    fn id_validation_rejects_traversal() {
        assert!(!is_valid_run_id("../../etc"));
        assert!(!is_valid_run_id("20260612-210000-abcd-01/.."));
        assert!(!is_valid_run_id(""));
        assert!(is_valid_run_id("20260612-210000-0a1b-07"));
    }

    #[test]
    fn meta_roundtrip_and_listing_desc() {
        let (_td, w) = ws();
        let m1 = meta("20260612-210000-0a1b-01");
        let m2 = meta("20260612-210001-0a1b-02");
        write_meta(&w, &m1).unwrap();
        write_meta(&w, &m2).unwrap();
        let all = list_runs(&w);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].id, m2.id, "desc by id");
    }

    #[test]
    fn delete_refuses_bad_id_and_removes_good() {
        let (_td, w) = ws();
        let m = meta("20260612-210000-0a1b-03");
        write_meta(&w, &m).unwrap();
        assert!(delete_run(&w, "../x").is_err());
        delete_run(&w, &m.id).unwrap();
        assert!(list_runs(&w).is_empty());
    }
}
```

Run: `cargo test -p rquant-desktop runs` → 编译失败（模块不存在）。

- [ ] **Step 4: 实现 runs.rs**

```rust
//! 回测留档:每次运行一个 runs/<id>/ 目录。result/traces 由引擎自写,
//! 桥接只写 config.json + meta.json(原子)。id 经正则校验防路径穿越。
use crate::dto::RunMetaDto;
use crate::paths::Workspace;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

static SEQ: AtomicU32 = AtomicU32::new(1);

pub fn new_run_id() -> String {
    let now = chrono::Local::now().naive_local();
    let seq = SEQ.fetch_add(1, Ordering::Relaxed) % 100;
    format!("{}-{:04x}-{:02}", now.format("%Y%m%d-%H%M%S"), std::process::id() % 0x10000, seq)
}

pub fn is_valid_run_id(id: &str) -> bool {
    // YYYYMMDD-HHMMSS-xxxx-nn(全小写 hex);手写校验避免 regex 依赖
    let b = id.as_bytes();
    if b.len() != 24 {
        return false;
    }
    let digit = |r: std::ops::Range<usize>| b[r].iter().all(|c| c.is_ascii_digit());
    let hex = |r: std::ops::Range<usize>| b[r].iter().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase());
    digit(0..8) && b[8] == b'-' && digit(9..15) && b[15] == b'-' && hex(16..20) && b[20] == b'-' && digit(21..23) == false || {
        // 上式难读——改用直白写法
        false
    }
}
```

**注意**：上面 `is_valid_run_id` 的一行式难读且易错——实现时用直白写法（计划在此给出最终版，照抄这版）：

```rust
pub fn is_valid_run_id(id: &str) -> bool {
    let parts: Vec<&str> = id.split('-').collect();
    if parts.len() != 4 {
        return false;
    }
    let (d, t, pid, seq) = (parts[0], parts[1], parts[2], parts[3]);
    d.len() == 8 && d.bytes().all(|c| c.is_ascii_digit())
        && t.len() == 6 && t.bytes().all(|c| c.is_ascii_digit())
        && pid.len() == 4 && pid.bytes().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        && seq.len() == 2 && seq.bytes().all(|c| c.is_ascii_digit())
}
```

（`new_run_id` 的 pid 段用 `{:04x}` 小写 hex，与校验一致。）

```rust
pub struct RunPaths {
    pub dir: PathBuf,
    pub config_json: PathBuf,
    pub meta_json: PathBuf,
    pub result_json: PathBuf,
    pub traces_jsonl: PathBuf,
    pub decision_jsonl: PathBuf,
}

pub fn run_paths(ws: &Workspace, id: &str) -> RunPaths {
    let dir = ws.runs_dir().join(id);
    RunPaths {
        config_json: dir.join("config.json"),
        meta_json: dir.join("meta.json"),
        result_json: dir.join("result.json"),
        traces_jsonl: dir.join("traces.jsonl"),
        decision_jsonl: dir.join("decision_traces.jsonl"),
        dir,
    }
}

fn write_json_atomic(path: &PathBuf, json: &str) -> anyhow::Result<()> {
    std::fs::create_dir_all(path.parent().expect("run file has parent"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn write_meta(ws: &Workspace, meta: &RunMetaDto) -> anyhow::Result<()> {
    let rp = run_paths(ws, &meta.id);
    write_json_atomic(&rp.meta_json, &serde_json::to_string_pretty(meta)?)
}

pub fn read_meta(ws: &Workspace, id: &str) -> Option<RunMetaDto> {
    let rp = run_paths(ws, id);
    let txt = std::fs::read_to_string(rp.meta_json).ok()?;
    serde_json::from_str(&txt).ok()
}

/// 列出全部留档(按 id 降序=时间降序);meta 损坏的目录跳过。
pub fn list_runs(ws: &Workspace) -> Vec<RunMetaDto> {
    let Ok(rd) = std::fs::read_dir(ws.runs_dir()) else {
        return Vec::new();
    };
    let mut v: Vec<RunMetaDto> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
        .filter(|id| is_valid_run_id(id))
        .filter_map(|id| read_meta(ws, &id))
        .collect();
    v.sort_by(|a, b| b.id.cmp(&a.id));
    v
}

pub fn delete_run(ws: &Workspace, id: &str) -> anyhow::Result<()> {
    if !is_valid_run_id(id) {
        anyhow::bail!("invalid run id: {}", id);
    }
    std::fs::remove_dir_all(run_paths(ws, id).dir)?;
    Ok(())
}

pub fn write_config(ws: &Workspace, id: &str, cfg: &crate::dto::BacktestConfigDto) -> anyhow::Result<()> {
    write_json_atomic(&run_paths(ws, id).config_json, &serde_json::to_string_pretty(cfg)?)
}

pub fn read_config(ws: &Workspace, id: &str) -> anyhow::Result<crate::dto::BacktestConfigDto> {
    let txt = std::fs::read_to_string(run_paths(ws, id).config_json)?;
    Ok(serde_json::from_str(&txt)?)
}
```

`lib.rs` 加 `pub mod runs;`（字母序）。

- [ ] **Step 5: 验证 + bindings regen**

Run: `cargo test -p rquant-desktop` → 47+4 绿（含 ts-rs export 新 DTO 测试自动出现，总数以实际为准——export_bindings 系列会从 12 涨到 ~26）。
Run: `cd desktop/ui && npx tsc --noEmit` → clean（paths 别名连到新 bindings）。
Run: clippy workspace → clean。

- [ ] **Step 6: Commit**

```bash
git status --porcelain
git add desktop/src-tauri/src/runs.rs desktop/src-tauri/src/paths.rs desktop/src-tauri/src/dto.rs desktop/src-tauri/src/lib.rs desktop/src-tauri/bindings
git commit -m "feat(desktop): run archive infra + full m2 dto set with bindings"
```

---

### Task B2: backtest_run 重任务（fetch→引擎→留档）

**Files:**
- Create: `desktop/src-tauri/src/backtest_run.rs`
- Modify: `desktop/src-tauri/src/lib.rs`（`pub mod backtest_run;`）

- [ ] **Step 1: 先读现场一处事实**——`run()`/`run_sim` 是否自写 out_path：打开 `src/backtest/runner.rs` 与 `sim.rs`，确认 report 落盘发生在引擎内（搜 `write_report`/`out_path` 的使用）。两种情况都已覆盖：引擎自写→桥接不重写；引擎只返回→桥接在任务体里 `serde_json::to_string_pretty` 写到 `rp.result_json`（原子）。把实情写进报告。

- [ ] **Step 2: 写失败测试**（backtest_run.rs `#[cfg(test)]`；直测任务体函数，不过 tauri）

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Workspace;

    /// 合成 40 根上升 60m bar CSV(表头与 read_bars_csv 兼容:time,open,high,low,close,volume)。
    fn write_bars_csv(path: &std::path::Path, n: usize) {
        let mut s = String::from("time,open,high,low,close,volume\n");
        for i in 0..n {
            let day = 1 + i / 4;
            let hour = 10 + (i % 4);
            let px = 10.0 + i as f64 * 0.1;
            s.push_str(&format!(
                "2026-01-{:02} {:02}:00:00,{:.2},{:.2},{:.2},{:.2},1000\n",
                day, hour, px, px + 0.05, px - 0.05, px
            ));
        }
        std::fs::write(path, s).unwrap();
    }

    const MINI_TREE: &str = r#"
meta: { name: "m2-mini", forward_window: 4, stances: [long, flat] }
root: r
nodes:
  r:
    type: quant
    branches:
      - when: "close > sma(close, 5)"
        goto: l
    default: { goto: f }
leaves:
  l: { stance: long, weight: 1.0 }
  f: { stance: flat }
"#;

    fn fixture_ws() -> (tempfile::TempDir, Workspace) {
        let td = tempfile::tempdir().unwrap();
        let root = td.path().to_path_buf();
        std::fs::create_dir_all(root.join("examples")).unwrap();
        std::fs::create_dir_all(root.join("data_in")).unwrap();
        std::fs::write(root.join("examples/mini.yaml"), MINI_TREE).unwrap();
        write_bars_csv(&root.join("data_in/bars.csv"), 40);
        (td, Workspace::new(root))
    }

    fn cfg(mode: &str) -> crate::dto::BacktestConfigDto {
        crate::dto::BacktestConfigDto {
            tree_path: "examples/mini.yaml".into(),
            primary_path: "data_in/bars.csv".into(),
            mode: mode.into(),
            cost_bps: 10.0,
            warmup: 10,
            window: 20,
            initial_capital: 100000.0,
            fetch: None,
        }
    }

    /// 测试用 NullCtx:不经 TaskRegistry 直接构造 TaskCtx 不可行(私有字段)——
    /// 任务体签名设计为接受 &dyn RunProgress(本模块定义的小 trait),
    /// TaskCtx 在 commands 侧适配。测试用 NoopProgress。
    struct NoopProgress;
    impl RunProgress for NoopProgress {
        fn progress(&self, _pct: f32, _stage: &str, _detail: &str) {}
        fn cancelled(&self) -> bool {
            false
        }
    }

    #[test]
    fn sim_hard_run_produces_full_archive() {
        let (_td, w) = fixture_ws();
        let out = execute_backtest(&w, &NoopProgress, &cfg("sim_hard")).unwrap();
        let id = out["run_id"].as_str().unwrap();
        let rp = crate::runs::run_paths(&w, id);
        assert!(rp.config_json.exists());
        assert!(rp.meta_json.exists());
        assert!(rp.result_json.exists());
        assert!(rp.traces_jsonl.exists());
        assert!(rp.decision_jsonl.exists(), "sim_hard must emit decision traces");
        let meta = crate::runs::read_meta(&w, id).unwrap();
        assert!(meta.ok);
        assert_eq!(meta.kind, "sim_hard");
        assert_eq!(meta.tree_name, "m2-mini");
    }

    #[test]
    fn score_hard_run_archives_without_decision_file() {
        let (_td, w) = fixture_ws();
        let out = execute_backtest(&w, &NoopProgress, &cfg("score_hard")).unwrap();
        let id = out["run_id"].as_str().unwrap();
        let rp = crate::runs::run_paths(&w, id);
        assert!(rp.result_json.exists());
        assert!(rp.traces_jsonl.exists(), "score traces are Trace jsonl");
        assert!(!rp.decision_jsonl.exists());
    }

    #[test]
    fn bad_mode_rejected() {
        let (_td, w) = fixture_ws();
        assert!(execute_backtest(&w, &NoopProgress, &cfg("nonsense")).is_err());
    }
}
```

Run: `cargo test -p rquant-desktop backtest_run` → 编译失败。

- [ ] **Step 3: 实现 backtest_run.rs**

```rust
//! 回测执行任务体:可选 fetch → 构造 BacktestConfig(out/traces 指进 run 目录) →
//! 按 mode 调引擎 → 桥接写 config/meta。引擎语义零触碰。
use crate::dto::BacktestConfigDto;
use crate::paths::Workspace;
use crate::runs;

/// 进度抽象:TaskCtx 在 commands 侧适配;测试用 Noop。
pub trait RunProgress {
    fn progress(&self, pct: f32, stage: &str, detail: &str);
    fn cancelled(&self) -> bool;
}

impl RunProgress for crate::tasks::TaskCtx {
    fn progress(&self, pct: f32, stage: &str, detail: &str) {
        crate::tasks::TaskCtx::progress(self, pct, stage, detail)
    }
    fn cancelled(&self) -> bool {
        crate::tasks::TaskCtx::cancelled(self)
    }
}

pub fn execute_backtest(
    ws: &Workspace,
    p: &dyn RunProgress,
    cfg: &BacktestConfigDto,
) -> Result<serde_json::Value, String> {
    match cfg.mode.as_str() {
        "sim_hard" | "sim_soft" | "score_hard" | "score_soft" => {}
        m => return Err(format!("unknown mode: {}", m)),
    }
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let llm = rquant::cli::build_llm(String::new(), String::new(), ws.root().join(".rquant-cache").join("llm"))
        .map_err(|e| e.to_string())?;

    let id = runs::new_run_id();
    let rp = runs::run_paths(ws, &id);
    std::fs::create_dir_all(&rp.dir).map_err(|e| e.to_string())?;

    // ── 可选 fetch ───────────────────────────────────────────────────────
    let mut effective = cfg.clone();
    if let Some(f) = &cfg.fetch {
        if p.cancelled() {
            return Err("cancelled by user".into());
        }
        p.progress(0.05, "fetch", &f.symbol);
        let out_rel = format!("{}/{}_{}_{}.csv",
            ".rquant-desktop/data", f.symbol, f.scale, f.adjust);
        let out_abs = ws.root().join(&out_rel);
        std::fs::create_dir_all(out_abs.parent().expect("data dir has parent")).map_err(|e| e.to_string())?;
        rt.block_on(rquant::cli::run_fetch_to_csv(
            &f.symbol, f.scale, f.datalen, rquant::cli::SINA_BASE_URL, &f.adjust, &out_abs,
        ))
        .map_err(|e| e.to_string())?;
        effective.primary_path = out_rel;
    }

    // ── 构造引擎配置(out/traces 指进 run 目录) ────────────────────────────
    let primary_abs = ws.root().join(&effective.primary_path);
    let engine_cfg = rquant::backtest::runner::BacktestConfig {
        tree_path: ws.root().join(&effective.tree_path),
        primary_path: primary_abs.clone(),
        context_path: primary_abs,
        news_path: None,
        out_path: rp.result_json.clone(),
        traces_path: Some(rp.traces_jsonl.clone()),
        cost_bps: effective.cost_bps,
        warmup: effective.warmup as usize,
        window: effective.window as usize,
        concurrency: 4,
        holidays_path: None,
        folds: 0,
        aux_paths: Vec::new(),
        decision_traces_path: if effective.mode == "sim_hard" {
            Some(rp.decision_jsonl.clone())
        } else {
            None
        },
    };

    if p.cancelled() {
        return Err("cancelled by user".into());
    }
    p.progress(0.3, "run", &effective.mode);

    // ── 调引擎;tree_name 从结果取 ────────────────────────────────────────
    let run_outcome: Result<String, String> = (|| {
        let tree_name = match effective.mode.as_str() {
            "sim_hard" => rt
                .block_on(rquant::backtest::sim::run_sim(&engine_cfg, &llm, false))
                .map(|r| persist_if_needed(&rp.result_json, &r).map(|_| r.tree_name))
                .map_err(|e| e.to_string())??,
            "sim_soft" => rt
                .block_on(rquant::backtest::sim::run_sim(&engine_cfg, &llm, true))
                .map(|r| persist_if_needed(&rp.result_json, &r).map(|_| r.tree_name))
                .map_err(|e| e.to_string())??,
            "score_hard" => rt
                .block_on(rquant::backtest::runner::run(&engine_cfg, &llm))
                .map(|r| persist_if_needed(&rp.result_json, &r).map(|_| r.tree_name))
                .map_err(|e| e.to_string())??,
            "score_soft" => rt
                .block_on(rquant::backtest::soft::run_soft(&engine_cfg, &llm))
                .map(|r| persist_if_needed(&rp.result_json, &r).map(|_| r.tree_name))
                .map_err(|e| e.to_string())??,
            _ => unreachable!("validated above"),
        };
        Ok(tree_name)
    })();

    // ── 落 config + meta(成败都留痕) ─────────────────────────────────────
    runs::write_config(ws, &id, &effective).map_err(|e| e.to_string())?;
    let primary_file = std::path::Path::new(&effective.primary_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("?")
        .to_string();
    let meta = crate::dto::RunMetaDto {
        id: id.clone(),
        kind: effective.mode.clone(),
        name: String::new(), // 下方按结果补
        tree_name: String::new(),
        created: chrono::Local::now().naive_local().format("%Y-%m-%dT%H:%M:%S").to_string(),
        ok: run_outcome.is_ok(),
        error: run_outcome.as_ref().err().cloned(),
    };
    let meta = match &run_outcome {
        Ok(tree_name) => crate::dto::RunMetaDto {
            name: format!("{} × {}", tree_name, primary_file),
            tree_name: tree_name.clone(),
            ..meta
        },
        Err(_) => crate::dto::RunMetaDto { name: format!("(失败) × {}", primary_file), ..meta },
    };
    runs::write_meta(ws, &meta).map_err(|e| e.to_string())?;

    run_outcome?;
    p.progress(0.95, "archive", &id);
    Ok(serde_json::json!({ "run_id": id }))
}

/// 引擎自写 out_path 的模式下本函数是幂等覆盖;只返回不写的模式下这是唯一落盘。
/// (Step 1 调研后:若证实引擎全部自写,可把本函数简化为存在性校验——以现场为准,报告里说明。)
fn persist_if_needed<T: serde::Serialize>(path: &std::path::PathBuf, r: &T) -> Result<(), String> {
    let json = serde_json::to_string_pretty(r).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())?;
    Ok(())
}
```

`lib.rs` 加 `pub mod backtest_run;`。

注意：`run_soft` 的返回类型是 `SoftReport`（字段名以现场为准——若无 `tree_name` 字段，取 `r.tree_name` 改为从 result.json 读回或用 load_tree_file 的 meta.name，**任选一种并在报告说明**；不要猜字段）。

- [ ] **Step 4: 验证**

Run: `cargo test -p rquant-desktop` → 既有 + 3 新全绿。
Run: `cargo test` → 引擎全量绿。clippy clean。

- [ ] **Step 5: Commit**

```bash
git status --porcelain
git add desktop/src-tauri/src/backtest_run.rs desktop/src-tauri/src/lib.rs
git commit -m "feat(desktop): backtest execution task - fetch/engine/archive pipeline"
```

---

### Task B3: 结果读取（摘要/资产曲线/交易明细）

**Files:**
- Create: `desktop/src-tauri/src/results.rs`
- Modify: `desktop/src-tauri/src/lib.rs`

- [ ] **Step 1: 写失败测试**（依赖 B2 fixture——把 B2 测试模块的 `fixture_ws`/`MINI_TREE`/`write_bars_csv`/`NoopProgress` 提为 `pub(crate)`（移入 `backtest_run.rs` 顶部 `#[cfg(test)] pub(crate) mod test_fixtures`），本模块复用；改造时 B2 测试同步改引用）

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::backtest_run::test_fixtures::{cfg, fixture_ws, NoopProgress};

    fn run_one(mode: &str) -> (tempfile::TempDir, crate::paths::Workspace, String) {
        let (td, w) = fixture_ws();
        let out = crate::backtest_run::execute_backtest(&w, &NoopProgress, &cfg(mode)).unwrap();
        let id = out["run_id"].as_str().unwrap().to_string();
        (td, w, id)
    }

    #[test]
    fn sim_summary_has_metrics_and_money() {
        let (_td, w, id) = run_one("sim_hard");
        let s = run_summary(&w, &id).unwrap();
        assert_eq!(s.meta.kind, "sim_hard");
        let tr = s.total_return.unwrap();
        assert!((s.final_equity.unwrap() - 100000.0 * (1.0 + tr)).abs() < 1e-6);
        assert!((s.net_pnl.unwrap() - 100000.0 * tr).abs() < 1e-6);
        assert!(s.raw.is_none());
    }

    #[test]
    fn score_summary_is_raw_passthrough() {
        let (_td, w, id) = run_one("score_hard");
        let s = run_summary(&w, &id).unwrap();
        assert!(s.total_return.is_none());
        assert!(s.raw.is_some());
        assert_eq!(s.raw.unwrap()["tree_name"], "m2-mini");
    }

    #[test]
    fn equity_series_scales_nav_by_capital() {
        let (_td, w, id) = run_one("sim_hard");
        let pts = equity_series(&w, &id).unwrap();
        assert!(!pts.is_empty());
        for p in &pts {
            assert!((p.equity - p.nav * 100000.0).abs() < 1e-6);
        }
        // 升序
        assert!(pts.windows(2).all(|w2| w2[0].t <= w2[1].t));
    }

    #[test]
    fn trades_have_amount_column() {
        let (_td, w, id) = run_one("sim_hard");
        let ts = trades(&w, &id).unwrap();
        for t in &ts {
            assert!((t.pnl_amount - 100000.0 * t.trip_return).abs() < 1e-6);
        }
    }
}
```

Run: `cargo test -p rquant-desktop results` → 编译失败。

- [ ] **Step 2: 实现 results.rs**

```rust
//! 留档读取:摘要(指标卡+资金换算)/资产曲线/交易明细。sim 解析强类型;score 原样透传。
use crate::dto::{EquityPointDto, RunSummaryDto, TradeDto};
use crate::paths::Workspace;
use crate::runs;
use rquant::backtest::sim::{SimReport, SimStepRecord};

fn iso(t: &chrono::NaiveDateTime) -> String {
    t.format("%Y-%m-%dT%H:%M:%S").to_string()
}

pub fn run_summary(ws: &Workspace, id: &str) -> Result<RunSummaryDto, String> {
    let meta = runs::read_meta(ws, id).ok_or_else(|| format!("run {} not found", id))?;
    let config = runs::read_config(ws, id).map_err(|e| e.to_string())?;
    let rp = runs::run_paths(ws, id);
    let txt = std::fs::read_to_string(&rp.result_json).map_err(|e| e.to_string())?;
    let cap = config.initial_capital;
    let mut s = RunSummaryDto {
        meta,
        config,
        total_return: None,
        max_drawdown: None,
        n_round_trips: None,
        win_rate: None,
        avg_hold_bars: None,
        turnover: None,
        buy_and_hold: None,
        sharpe: None,
        final_equity: None,
        net_pnl: None,
        raw: None,
    };
    if s.meta.kind.starts_with("sim") {
        let r: SimReport = serde_json::from_str(&txt).map_err(|e| e.to_string())?;
        s.total_return = Some(r.total_return);
        s.max_drawdown = Some(r.max_drawdown);
        s.n_round_trips = Some(r.n_round_trips as u32);
        s.win_rate = Some(r.win_rate);
        s.avg_hold_bars = Some(r.avg_hold_bars);
        s.turnover = Some(r.turnover);
        s.buy_and_hold = Some(r.buy_and_hold);
        s.sharpe = r.risk.as_ref().and_then(|k| k.sharpe);
        s.final_equity = Some(cap * (1.0 + r.total_return));
        s.net_pnl = Some(cap * r.total_return);
    } else {
        s.raw = Some(serde_json::from_str(&txt).map_err(|e| e.to_string())?);
    }
    Ok(s)
}

pub fn equity_series(ws: &Workspace, id: &str) -> Result<Vec<EquityPointDto>, String> {
    let config = runs::read_config(ws, id).map_err(|e| e.to_string())?;
    let rp = runs::run_paths(ws, id);
    let txt = std::fs::read_to_string(&rp.traces_jsonl).map_err(|e| e.to_string())?;
    let cap = config.initial_capital;
    Ok(txt
        .lines()
        .filter_map(|l| serde_json::from_str::<SimStepRecord>(l).ok())
        .map(|r| EquityPointDto { t: iso(&r.t), nav: r.nav, equity: r.nav * cap, pos: r.pos })
        .collect())
}

pub fn trades(ws: &Workspace, id: &str) -> Result<Vec<TradeDto>, String> {
    let config = runs::read_config(ws, id).map_err(|e| e.to_string())?;
    let rp = runs::run_paths(ws, id);
    let txt = std::fs::read_to_string(&rp.result_json).map_err(|e| e.to_string())?;
    let r: SimReport = serde_json::from_str(&txt).map_err(|e| e.to_string())?;
    let cap = config.initial_capital;
    Ok(r.trades
        .iter()
        .map(|t| TradeDto {
            entry_t: iso(&t.entry_t),
            exit_t: iso(&t.exit_t),
            entry_px: t.entry_px,
            exit_px: t.exit_px,
            max_abs_pos: t.max_abs_pos,
            trip_return: t.trip_return,
            bars_held: t.bars_held as u32,
            reason: t.reason.clone(),
            pnl_amount: cap * t.trip_return,
        })
        .collect())
}
```

注意：`RiskMetrics.sharpe` 若是 `f64` 而非 `Option<f64>`（以现场为准），把 `.and_then(|k| k.sharpe)` 改 `.map(|k| k.sharpe)`，并在报告说明。`SimStepRecord`/`SimReport`/`RiskMetrics` 若任一非 pub，按 spec §4-2 在引擎做仅可见性提升（同 M1 纪律：零逻辑改动 + 引擎全量回归）。

`lib.rs` 加 `pub mod results;`。

- [ ] **Step 3: 验证**

Run: `cargo test -p rquant-desktop` → 全绿；`cargo test` 引擎绿；clippy clean。

- [ ] **Step 4: Commit**

```bash
git status --porcelain
git add desktop/src-tauri/src/results.rs desktop/src-tauri/src/backtest_run.rs desktop/src-tauri/src/lib.rs
git status --porcelain   # 若做了引擎可见性提升,补 add src/...
git commit -m "feat(desktop): run summary/equity/trades readers with capital conversion"
```

---

### Task B4: 决策回放数据（frames + 因子值表）

**Files:**
- Create: `desktop/src-tauri/src/replay.rs`
- Modify: `desktop/src-tauri/src/lib.rs`

- [ ] **Step 1: 写失败测试**（复用 B2 的 test_fixtures）

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::backtest_run::test_fixtures::{cfg, fixture_ws, NoopProgress};

    fn run_one(mode: &str) -> (tempfile::TempDir, crate::paths::Workspace, String) {
        let (td, w) = fixture_ws();
        let out = crate::backtest_run::execute_backtest(&w, &NoopProgress, &cfg(mode)).unwrap();
        (td, w, out["run_id"].as_str().unwrap().to_string())
    }

    #[test]
    fn sim_hard_frames_align_path_with_account() {
        let (_td, w, id) = run_one("sim_hard");
        let frames = replay_frames(&w, &id).unwrap();
        assert!(!frames.is_empty());
        for f in &frames {
            assert!(!f.path.is_empty(), "decision path recorded");
            assert!(f.nav.is_some(), "sim aligns SimStepRecord");
        }
        // 时间升序
        assert!(frames.windows(2).all(|w2| w2[0].t <= w2[1].t));
    }

    #[test]
    fn score_hard_frames_have_path_without_account() {
        let (_td, w, id) = run_one("score_hard");
        let frames = replay_frames(&w, &id).unwrap();
        assert!(!frames.is_empty());
        assert!(frames.iter().all(|f| f.nav.is_none() && !f.path.is_empty()));
    }

    #[test]
    fn replay_factors_evaluates_tree_factors_at_t() {
        // mini 树没有 factors 块 → 空表也合法;换带因子的树验证求值
        let (_td, w) = fixture_ws();
        const FACTOR_TREE: &str = r#"
meta: { name: "m2-fct", forward_window: 4, stances: [long, flat] }
params: { n: 5.0 }
factors:
  ma: "sma(close, n)"
root: r
nodes:
  r:
    type: quant
    branches:
      - when: "close > ma"
        goto: l
    default: { goto: f }
leaves:
  l: { stance: long }
  f: { stance: flat }
"#;
        std::fs::write(w.root().join("examples/fct.yaml"), FACTOR_TREE).unwrap();
        let mut c = cfg("sim_hard");
        c.tree_path = "examples/fct.yaml".into();
        let out = crate::backtest_run::execute_backtest(&w, &NoopProgress, &c).unwrap();
        let id = out["run_id"].as_str().unwrap();
        let frames = replay_frames(&w, id).unwrap();
        let t = frames.last().unwrap().t.clone();
        let vals = replay_factors(&w, id, &t).unwrap();
        assert_eq!(vals.len(), 1);
        assert_eq!(vals[0].name, "ma");
        assert!(vals[0].value.unwrap() > 0.0);
    }
}
```

Run: `cargo test -p rquant-desktop replay` → 编译失败。

- [ ] **Step 2: 实现 replay.rs**

```rust
//! 决策回放:frames=Trace(±SimStepRecord 对齐);因子值=resolve_factor_exprs+build_context 现算。
use crate::dto::{FactorValueDto, ReplayFrameDto, ReplayStepDto};
use crate::paths::Workspace;
use crate::runs;
use rquant::backtest::sim::SimStepRecord;
use rquant::engine::trace::Trace;
use std::collections::BTreeMap;

fn iso(t: &chrono::NaiveDateTime) -> String {
    t.format("%Y-%m-%dT%H:%M:%S").to_string()
}

fn read_traces<T: serde::de::DeserializeOwned>(path: &std::path::Path) -> Vec<T> {
    std::fs::read_to_string(path)
        .map(|txt| txt.lines().filter_map(|l| serde_json::from_str(l).ok()).collect())
        .unwrap_or_default()
}

pub fn replay_frames(ws: &Workspace, id: &str) -> Result<Vec<ReplayFrameDto>, String> {
    let meta = runs::read_meta(ws, id).ok_or_else(|| format!("run {} not found", id))?;
    let rp = runs::run_paths(ws, id);
    // 路径来源:sim_hard=decision_traces.jsonl;score_*=traces.jsonl(本身就是 Trace 行)
    let traces: Vec<Trace> = if meta.kind == "sim_hard" {
        read_traces(&rp.decision_jsonl)
    } else if meta.kind.starts_with("score") {
        read_traces(&rp.traces_jsonl)
    } else {
        return Err("replay paths unavailable for sim_soft (no single-path traversal)".into());
    };
    if traces.is_empty() {
        return Err("no decision traces archived for this run".into());
    }
    // sim 账户线按 t 对齐
    let steps: BTreeMap<String, SimStepRecord> = if meta.kind.starts_with("sim") {
        read_traces::<SimStepRecord>(&rp.traces_jsonl)
            .into_iter()
            .map(|s| (iso(&s.t), s))
            .collect()
    } else {
        BTreeMap::new()
    };
    let mut frames: Vec<ReplayFrameDto> = traces
        .into_iter()
        .map(|tr| {
            let key = iso(&tr.t);
            let st = steps.get(&key);
            ReplayFrameDto {
                t: key,
                leaf: tr.leaf,
                stance: format!("{:?}", tr.stance),
                path: tr
                    .path
                    .into_iter()
                    .map(|s| ReplayStepDto {
                        node_id: s.node_id,
                        label: s.label,
                        confidence: s.confidence,
                        rationale: s.rationale,
                    })
                    .collect(),
                target: st.map(|s| s.target),
                pos: st.map(|s| s.pos),
                nav: st.map(|s| s.nav),
            }
        })
        .collect();
    frames.sort_by(|a, b| a.t.cmp(&b.t));
    Ok(frames)
}

/// 在 t 时刻现算树的全部因子值(spec §5.2 回放因子表)。
pub fn replay_factors(ws: &Workspace, id: &str, t: &str) -> Result<Vec<FactorValueDto>, String> {
    let config = runs::read_config(ws, id).map_err(|e| e.to_string())?;
    let yaml = std::fs::read_to_string(ws.root().join(&config.tree_path)).map_err(|e| e.to_string())?;
    let factors = rquant::tree::loader::resolve_factor_exprs(&yaml).map_err(|e| e.to_string())?;
    if factors.is_empty() {
        return Ok(Vec::new());
    }
    let bars = rquant::data::reader::read_bars_csv(&ws.root().join(&config.primary_path))
        .map_err(|e| e.to_string())?;
    let t_parsed = chrono::NaiveDateTime::parse_from_str(t, "%Y-%m-%dT%H:%M:%S").map_err(|e| e.to_string())?;
    let ctx = rquant::features::context::build_context(
        &bars, &bars, &[], &Default::default(), t_parsed, config.window as usize,
    );
    Ok(factors
        .iter()
        .map(|(name, expr)| {
            let v = rquant::dsl::eval::eval(expr, &ctx).ok().and_then(|val| match val {
                rquant::dsl::eval::Value::Scalar(x) => Some(x),
                rquant::dsl::eval::Value::Series(s) => s.last().copied(),
                rquant::dsl::eval::Value::Bool(b) => Some(if b { 1.0 } else { 0.0 }),
            });
            FactorValueDto { name: name.clone(), value: v.filter(|x| x.is_finite()) }
        })
        .collect())
}
```

注意：`Value` 枚举若有第四变体（如 BoolSeries），match 补臂取末位 bool→1/0；以现场为准并报告。`Stance` 的 `{:?}` 输出 "Long"/"Flat"/"Short"——UI 按此三值映射。

`lib.rs` 加 `pub mod replay;`。

- [ ] **Step 3: 验证**

Run: `cargo test -p rquant-desktop` 全绿 + `cargo test` 引擎绿 + clippy clean。

- [ ] **Step 4: Commit**

```bash
git status --porcelain
git add desktop/src-tauri/src/replay.rs desktop/src-tauri/src/lib.rs
git commit -m "feat(desktop): decision replay frames + on-demand factor table"
```

---

### Task B5: 数据工作台桥接（CSV 清单/K线/因子叠加/universe/批量拉取）

**Files:**
- Create: `desktop/src-tauri/src/data_bench.rs`
- Modify: `desktop/src-tauri/src/lib.rs`

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::backtest_run::test_fixtures::write_bars_csv;
    use crate::paths::Workspace;

    fn ws() -> (tempfile::TempDir, Workspace) {
        let td = tempfile::tempdir().unwrap();
        let root = td.path().to_path_buf();
        std::fs::create_dir_all(root.join("paper")).unwrap();
        std::fs::create_dir_all(root.join(".rquant-desktop/data")).unwrap();
        std::fs::create_dir_all(root.join(".rquant-desktop/universes")).unwrap();
        std::fs::create_dir_all(root.join("deploy")).unwrap();
        (td, Workspace::new(root))
    }

    #[test]
    fn csv_list_scans_both_dirs_and_reports_freshness() {
        let (_td, w) = ws();
        write_bars_csv(&w.paper_dir().join("p_x.csv"), 10);
        write_bars_csv(&w.data_dir().join("sh600000_60_qfq.csv"), 20);
        std::fs::write(w.paper_dir().join("broken.csv"), "not,a,bar\n1,2,3\n").unwrap();
        let list = csv_list(&w);
        assert_eq!(list.len(), 3);
        let good = list.iter().find(|c| c.path.ends_with("p_x.csv")).unwrap();
        assert_eq!(good.rows, Some(10));
        assert!(good.last_t.as_deref().unwrap() > good.first_t.as_deref().unwrap());
        let bad = list.iter().find(|c| c.path.ends_with("broken.csv")).unwrap();
        assert!(bad.rows.is_none());
    }

    #[test]
    fn read_bars_rejects_path_escape() {
        let (_td, w) = ws();
        assert!(read_bars(&w, "../outside.csv", 100).is_err());
        assert!(read_bars(&w, "C:/Windows/system.ini", 100).is_err());
    }

    #[test]
    fn read_bars_tails_and_converts() {
        let (_td, w) = ws();
        write_bars_csv(&w.paper_dir().join("p_y.csv"), 30);
        let bars = read_bars(&w, "paper/p_y.csv", 10).unwrap();
        assert_eq!(bars.len(), 10);
        assert!(bars[0].t < bars[9].t);
    }

    #[test]
    fn eval_factor_over_tail() {
        let (_td, w) = ws();
        write_bars_csv(&w.paper_dir().join("p_z.csv"), 30);
        let pts = eval_factor(&w, "paper/p_z.csv", "sma(close, 5)", 20, 10).unwrap();
        assert_eq!(pts.len(), 10);
        assert!(pts.last().unwrap().value.unwrap() > 0.0);
        assert!(eval_factor(&w, "paper/p_z.csv", "not a (((expr", 20, 10).is_err());
    }

    #[test]
    fn universe_write_only_custom_dir_and_roundtrip() {
        let (_td, w) = ws();
        std::fs::write(w.deploy_dir().join("universe_10.csv"), "symbol,primary\nsh1,paper/a.csv\n").unwrap();
        let entries = vec![crate::dto::UniverseEntryDto { symbol: "sh600000".into(), primary: "paper/p.csv".into() }];
        universe_write(&w, "my_list", &entries).unwrap();
        let all = universe_list(&w);
        assert_eq!(all.len(), 2);
        let frozen = all.iter().find(|u| u.frozen).unwrap();
        assert!(frozen.path.starts_with("deploy"));
        let custom = all.iter().find(|u| !u.frozen).unwrap();
        assert_eq!(custom.entries.len(), 1);
        assert!(universe_write(&w, "../evil", &entries).is_err(), "name sanitized");
    }
}
```

Run: `cargo test -p rquant-desktop data_bench` → 编译失败。

- [ ] **Step 2: 实现 data_bench.rs**

```rust
//! 数据工作台:CSV 清单/新鲜度、K线读取(tail 上限)、因子叠加现算、universe 管理、批量拉取任务。
//! 一切路径经 resolve_under_root 越界守卫(spec §9 fs 收敛)。
use crate::dto::{BarDto, CsvInfoDto, FactorPointDto, UniverseEntryDto, UniverseInfoDto};
use crate::paths::Workspace;
use rquant::data::bar::Bar;

const MAX_TAIL: usize = 2000;

fn iso(t: &chrono::NaiveDateTime) -> String {
    t.format("%Y-%m-%dT%H:%M:%S").to_string()
}

/// 工作区相对路径 → 绝对路径,拒绝越界(canonicalize 前缀校验;文件须存在)。
fn resolve_under_root(ws: &Workspace, rel: &str) -> Result<std::path::PathBuf, String> {
    let joined = ws.root().join(rel);
    let canon = joined.canonicalize().map_err(|e| format!("{}: {}", rel, e))?;
    let root = ws.root().canonicalize().map_err(|e| e.to_string())?;
    if !canon.starts_with(&root) {
        return Err(format!("path escapes workspace: {}", rel));
    }
    Ok(canon)
}

fn rel_of(ws: &Workspace, abs: &std::path::Path) -> String {
    abs.strip_prefix(ws.root())
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| abs.to_string_lossy().to_string())
}

fn scan_dir(ws: &Workspace, dir: &std::path::Path, out: &mut Vec<CsvInfoDto>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.filter_map(|e| e.ok()) {
        let p = e.path();
        if p.extension().map(|x| x == "csv").unwrap_or(false) {
            let info = match rquant::data::reader::read_bars_csv(&p) {
                Ok(bars) if !bars.is_empty() => CsvInfoDto {
                    path: rel_of(ws, &p),
                    rows: Some(bars.len() as u32),
                    first_t: Some(iso(&bars[0].time)),
                    last_t: Some(iso(&bars[bars.len() - 1].time)),
                },
                _ => CsvInfoDto { path: rel_of(ws, &p), rows: None, first_t: None, last_t: None },
            };
            out.push(info);
        }
    }
}

pub fn csv_list(ws: &Workspace) -> Vec<CsvInfoDto> {
    let mut v = Vec::new();
    scan_dir(ws, &ws.paper_dir(), &mut v);
    scan_dir(ws, &ws.data_dir(), &mut v);
    v.sort_by(|a, b| a.path.cmp(&b.path));
    v
}

pub fn read_bars(ws: &Workspace, rel: &str, tail: usize) -> Result<Vec<BarDto>, String> {
    let abs = resolve_under_root(ws, rel)?;
    let bars = rquant::data::reader::read_bars_csv(&abs).map_err(|e| e.to_string())?;
    let take = tail.clamp(1, MAX_TAIL);
    let start = bars.len().saturating_sub(take);
    Ok(bars[start..]
        .iter()
        .map(|b: &Bar| BarDto {
            t: iso(&b.time),
            open: b.open,
            high: b.high,
            low: b.low,
            close: b.close,
            volume: b.volume,
        })
        .collect())
}

/// 因子叠加:对尾部 tail 根 bar 逐点 build_context+eval(标量取值/序列取末位)。
pub fn eval_factor(ws: &Workspace, rel: &str, expr_src: &str, window: usize, tail: usize) -> Result<Vec<FactorPointDto>, String> {
    let abs = resolve_under_root(ws, rel)?;
    let expr = rquant::dsl::parser::parse_str(expr_src).map_err(|e| e.to_string())?;
    let bars = rquant::data::reader::read_bars_csv(&abs).map_err(|e| e.to_string())?;
    let take = tail.clamp(1, MAX_TAIL).min(bars.len());
    let start = bars.len() - take;
    let aux = Default::default();
    Ok(bars[start..]
        .iter()
        .map(|b| {
            let ctx = rquant::features::context::build_context(&bars, &bars, &[], &aux, b.time, window);
            let v = rquant::dsl::eval::eval(&expr, &ctx).ok().and_then(|val| match val {
                rquant::dsl::eval::Value::Scalar(x) => Some(x),
                rquant::dsl::eval::Value::Series(s) => s.last().copied(),
                rquant::dsl::eval::Value::Bool(x) => Some(if x { 1.0 } else { 0.0 }),
            });
            FactorPointDto { t: iso(&b.time), value: v.filter(|x| x.is_finite()) }
        })
        .collect())
}

fn read_universe_file(ws: &Workspace, abs: &std::path::Path, frozen: bool) -> Option<UniverseInfoDto> {
    let txt = std::fs::read_to_string(abs).ok()?;
    let mut lines = txt.lines();
    let header = lines.next()?;
    if !header.trim_start().starts_with("symbol,primary") {
        return None; // 非 universe 形状的 csv 不算
    }
    let entries = lines
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| {
            let mut it = l.split(',');
            Some(UniverseEntryDto { symbol: it.next()?.trim().to_string(), primary: it.next()?.trim().to_string() })
        })
        .collect();
    Some(UniverseInfoDto {
        path: rel_of(ws, abs),
        name: abs.file_stem().and_then(|s| s.to_str()).unwrap_or("?").to_string(),
        frozen,
        entries,
    })
}

pub fn universe_list(ws: &Workspace) -> Vec<UniverseInfoDto> {
    let mut v = Vec::new();
    for (dir, frozen) in [(ws.deploy_dir(), true), (ws.universes_dir(), false)] {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.filter_map(|e| e.ok()) {
            let p = e.path();
            if p.extension().map(|x| x == "csv").unwrap_or(false) {
                if let Some(u) = read_universe_file(ws, &p, frozen) {
                    v.push(u);
                }
            }
        }
    }
    v.sort_by(|a, b| a.path.cmp(&b.path));
    v
}

/// 自定义清单写入(.rquant-desktop/universes/<name>.csv,原子);name 白名单防穿越。
pub fn universe_write(ws: &Workspace, name: &str, entries: &[UniverseEntryDto]) -> Result<(), String> {
    if name.is_empty() || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-') {
        return Err(format!("invalid universe name: {}", name));
    }
    let dir = ws.universes_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.csv", name));
    let mut s = String::from("symbol,primary\n");
    for e in entries {
        s.push_str(&format!("{},{}\n", e.symbol, e.primary));
    }
    let tmp = path.with_extension("csv.tmp");
    std::fs::write(&tmp, &s).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(())
}

/// 批量拉取任务体(重任务;串行+节流;落 .rquant-desktop/data/)。
pub fn fetch_batch(
    ws: &Workspace,
    p: &dyn crate::backtest_run::RunProgress,
    symbols: &[String],
    scale: u32,
    datalen: u32,
    adjust: &str,
) -> Result<serde_json::Value, String> {
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(ws.data_dir()).map_err(|e| e.to_string())?;
    let mut written = Vec::new();
    for (i, sym) in symbols.iter().enumerate() {
        if p.cancelled() {
            return Err("cancelled by user".into());
        }
        p.progress(i as f32 / symbols.len() as f32, "fetch", sym);
        let out = ws.data_dir().join(format!("{}_{}_{}.csv", sym, scale, adjust));
        rt.block_on(rquant::cli::run_fetch_to_csv(sym, scale, datalen, rquant::cli::SINA_BASE_URL, adjust, &out))
            .map_err(|e| e.to_string())?;
        written.push(rel_of(ws, &out));
        std::thread::sleep(std::time::Duration::from_millis(500)); // sina 节流
    }
    Ok(serde_json::json!({ "written": written }))
}
```

`lib.rs` 加 `pub mod data_bench;`。

- [ ] **Step 3: 验证**

Run: `cargo test -p rquant-desktop` 全绿（注意 `write_bars_csv` 需在 B2 的 test_fixtures 中为 `pub(crate)`）；clippy clean。

- [ ] **Step 4: Commit**

```bash
git status --porcelain
git add desktop/src-tauri/src/data_bench.rs desktop/src-tauri/src/lib.rs
git commit -m "feat(desktop): data workbench bridge - csv scan/kline/factor-overlay/universe/batch-fetch"
```

---

### Task B6: 命令注册装配（M2 全部 tauri 命令 + tree_list）

**Files:**
- Modify: `desktop/src-tauri/src/commands.rs`（追加 M2 命令薄壳 + tree_list 装配函数）
- Modify: `desktop/src-tauri/src/lib.rs`（invoke_handler 注册）

- [ ] **Step 1: 写失败测试**（commands.rs 测试模块追加；tree_list 装配函数直测）

```rust
    #[test]
    fn tree_list_scans_examples_and_deploy() {
        let td = tempfile::tempdir().unwrap();
        let root = td.path().to_path_buf();
        std::fs::create_dir_all(root.join("examples")).unwrap();
        std::fs::create_dir_all(root.join("deploy")).unwrap();
        std::fs::write(
            root.join("examples/ok.yaml"),
            crate::backtest_run::test_fixtures::MINI_TREE,
        )
        .unwrap();
        std::fs::write(root.join("deploy/bad.yaml"), "not: a tree").unwrap();
        let ws = Workspace::new(root);
        let list = assemble_tree_list(&ws);
        assert_eq!(list.len(), 2);
        let ok = list.iter().find(|t| t.path.ends_with("ok.yaml")).unwrap();
        assert_eq!(ok.name.as_deref(), Some("m2-mini"));
        assert!(!ok.frozen);
        let bad = list.iter().find(|t| t.path.ends_with("bad.yaml")).unwrap();
        assert!(bad.name.is_none() && bad.error.is_some() && bad.frozen);
    }
```

（`MINI_TREE` 须在 test_fixtures 中 `pub(crate) const`。）

Run: `cargo test -p rquant-desktop tree_list` → 编译失败。

- [ ] **Step 2: commands.rs 追加**

```rust
// ───────────────────────── M2: 回测中心 / 数据工作台 ─────────────────────────

pub fn assemble_tree_list(ws: &Workspace) -> Vec<crate::dto::TreeInfoDto> {
    let mut v = Vec::new();
    for (dir, frozen) in [(ws.root().join("examples"), false), (ws.deploy_dir(), true)] {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.filter_map(|e| e.ok()) {
            let p = e.path();
            let is_yaml = p.extension().map(|x| x == "yaml" || x == "yml").unwrap_or(false);
            if !is_yaml {
                continue;
            }
            let rel = p
                .strip_prefix(ws.root())
                .map(|x| x.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| p.to_string_lossy().to_string());
            match rquant::tree::loader::load_tree_file(&p) {
                Ok(t) => v.push(crate::dto::TreeInfoDto { path: rel, name: Some(t.meta.name), frozen, error: None }),
                Err(e) => v.push(crate::dto::TreeInfoDto { path: rel, name: None, frozen, error: Some(e.to_string()) }),
            }
        }
    }
    v.sort_by(|a, b| a.path.cmp(&b.path));
    v
}

// ---- M2 tauri 薄壳 ----

#[tauri::command]
pub fn tree_list(state: tauri::State<AppState>) -> Vec<crate::dto::TreeInfoDto> {
    assemble_tree_list(&state.ws)
}

#[tauri::command]
pub fn backtest_run(state: tauri::State<AppState>, config: crate::dto::BacktestConfigDto) -> Result<String, String> {
    let ws = state.ws.clone();
    state
        .tasks
        .start("backtest", true, move |ctx| crate::backtest_run::execute_backtest(&ws, ctx, &config))
}

#[tauri::command]
pub fn runs_list(state: tauri::State<AppState>) -> Vec<crate::dto::RunMetaDto> {
    crate::runs::list_runs(&state.ws)
}

#[tauri::command]
pub fn run_delete(state: tauri::State<AppState>, id: String) -> Result<(), String> {
    crate::runs::delete_run(&state.ws, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn run_summary(state: tauri::State<AppState>, id: String) -> Result<crate::dto::RunSummaryDto, String> {
    crate::results::run_summary(&state.ws, &id)
}

#[tauri::command]
pub fn run_equity(state: tauri::State<AppState>, id: String) -> Result<Vec<crate::dto::EquityPointDto>, String> {
    crate::results::equity_series(&state.ws, &id)
}

#[tauri::command]
pub fn run_trades(state: tauri::State<AppState>, id: String) -> Result<Vec<crate::dto::TradeDto>, String> {
    crate::results::trades(&state.ws, &id)
}

#[tauri::command]
pub fn run_replay_frames(state: tauri::State<AppState>, id: String) -> Result<Vec<crate::dto::ReplayFrameDto>, String> {
    crate::replay::replay_frames(&state.ws, &id)
}

#[tauri::command]
pub fn run_replay_factors(
    state: tauri::State<AppState>,
    id: String,
    t: String,
) -> Result<Vec<crate::dto::FactorValueDto>, String> {
    crate::replay::replay_factors(&state.ws, &id, &t)
}

#[tauri::command]
pub fn data_csv_list(state: tauri::State<AppState>) -> Vec<crate::dto::CsvInfoDto> {
    crate::data_bench::csv_list(&state.ws)
}

#[tauri::command]
pub fn data_read_bars(state: tauri::State<AppState>, path: String, tail: u32) -> Result<Vec<crate::dto::BarDto>, String> {
    crate::data_bench::read_bars(&state.ws, &path, tail as usize)
}

#[tauri::command]
pub fn data_eval_factor(
    state: tauri::State<AppState>,
    path: String,
    expr: String,
    window: u32,
    tail: u32,
) -> Result<Vec<crate::dto::FactorPointDto>, String> {
    crate::data_bench::eval_factor(&state.ws, &path, &expr, window as usize, tail as usize)
}

#[tauri::command]
pub fn universe_list(state: tauri::State<AppState>) -> Vec<crate::dto::UniverseInfoDto> {
    crate::data_bench::universe_list(&state.ws)
}

#[tauri::command]
pub fn universe_write(
    state: tauri::State<AppState>,
    name: String,
    entries: Vec<crate::dto::UniverseEntryDto>,
) -> Result<(), String> {
    crate::data_bench::universe_write(&state.ws, &name, &entries)
}

#[tauri::command]
pub fn fetch_batch(
    state: tauri::State<AppState>,
    symbols: Vec<String>,
    scale: u32,
    datalen: u32,
    adjust: String,
) -> Result<String, String> {
    let ws = state.ws.clone();
    state
        .tasks
        .start("fetch_batch", true, move |ctx| {
            crate::data_bench::fetch_batch(&ws, ctx, &symbols, scale, datalen, &adjust)
        })
}
```

`lib.rs` 的 `invoke_handler` 追加 16 个命令（保持 M1 七个不动）：

```rust
            commands::tree_list,
            commands::backtest_run,
            commands::runs_list,
            commands::run_delete,
            commands::run_summary,
            commands::run_equity,
            commands::run_trades,
            commands::run_replay_frames,
            commands::run_replay_factors,
            commands::data_csv_list,
            commands::data_read_bars,
            commands::data_eval_factor,
            commands::universe_list,
            commands::universe_write,
            commands::fetch_batch,
```

- [ ] **Step 3: 验证**

Run: `cargo test -p rquant-desktop` 全绿；`cargo build -p rquant-desktop`（tauri 宏全链路）；clippy clean。

- [ ] **Step 4: Commit**

```bash
git status --porcelain
git add desktop/src-tauri/src/commands.rs desktop/src-tauri/src/backtest_run.rs desktop/src-tauri/src/lib.rs
git commit -m "feat(desktop): register m2 command surface - backtest/runs/replay/data/universe"
```

---

### Task U1: 回测中心页骨架（配置表单 + 运行 + 历史列表）

**Files:**
- Create: `desktop/ui/src/pages/Backtest.tsx`、`desktop/ui/src/stores/backtest.ts`、`desktop/ui/src/components/BacktestConfigForm.tsx`、`desktop/ui/src/components/RunHistoryList.tsx`
- Create: `desktop/ui/src/pages/Backtest.test.tsx`
- Modify: `desktop/ui/src/api/ipc.ts`（M2 全部 api 一次加齐）、`desktop/ui/src/App.tsx`（/backtest 换真页）

- [ ] **Step 1: ipc.ts 追加**（M2 全量，后续任务不再改此文件）

```ts
import type { TreeInfoDto } from "@bindings/TreeInfoDto";
import type { BacktestConfigDto } from "@bindings/BacktestConfigDto";
import type { RunMetaDto } from "@bindings/RunMetaDto";
import type { RunSummaryDto } from "@bindings/RunSummaryDto";
import type { EquityPointDto } from "@bindings/EquityPointDto";
import type { TradeDto } from "@bindings/TradeDto";
import type { ReplayFrameDto } from "@bindings/ReplayFrameDto";
import type { FactorValueDto } from "@bindings/FactorValueDto";
import type { BarDto } from "@bindings/BarDto";
import type { FactorPointDto } from "@bindings/FactorPointDto";
import type { CsvInfoDto } from "@bindings/CsvInfoDto";
import type { UniverseInfoDto } from "@bindings/UniverseInfoDto";
import type { UniverseEntryDto } from "@bindings/UniverseEntryDto";

// api 对象内追加:
  treeList: () => invoke<TreeInfoDto[]>("tree_list"),
  backtestRun: (config: BacktestConfigDto) => invoke<string>("backtest_run", { config }),
  runsList: () => invoke<RunMetaDto[]>("runs_list"),
  runDelete: (id: string) => invoke<void>("run_delete", { id }),
  runSummary: (id: string) => invoke<RunSummaryDto>("run_summary", { id }),
  runEquity: (id: string) => invoke<EquityPointDto[]>("run_equity", { id }),
  runTrades: (id: string) => invoke<TradeDto[]>("run_trades", { id }),
  runReplayFrames: (id: string) => invoke<ReplayFrameDto[]>("run_replay_frames", { id }),
  runReplayFactors: (id: string, t: string) => invoke<FactorValueDto[]>("run_replay_factors", { id, t }),
  dataCsvList: () => invoke<CsvInfoDto[]>("data_csv_list"),
  dataReadBars: (path: string, tail: number) => invoke<BarDto[]>("data_read_bars", { path, tail }),
  dataEvalFactor: (path: string, expr: string, window: number, tail: number) =>
    invoke<FactorPointDto[]>("data_eval_factor", { path, expr, window, tail }),
  universeList: () => invoke<UniverseInfoDto[]>("universe_list"),
  universeWrite: (name: string, entries: UniverseEntryDto[]) => invoke<void>("universe_write", { name, entries }),
  fetchBatch: (symbols: string[], scale: number, datalen: number, adjust: string) =>
    invoke<string>("fetch_batch", { symbols, scale, datalen, adjust }),
```

- [ ] **Step 2: stores/backtest.ts**

```ts
import { create } from "zustand";
import type { RunMetaDto } from "@bindings/RunMetaDto";
import type { RunSummaryDto } from "@bindings/RunSummaryDto";
import { api as realApi, type Api } from "../api/ipc";

interface BacktestState {
  api: Api;
  runs: RunMetaDto[];
  selectedId: string | null;
  summary: RunSummaryDto | null;
  compareIds: string[];
  loadRuns: () => Promise<void>;
  select: (id: string) => Promise<void>;
  toggleCompare: (id: string) => void;
  remove: (id: string) => Promise<void>;
}

export const useBacktest = create<BacktestState>((set, get) => ({
  api: realApi,
  runs: [],
  selectedId: null,
  summary: null,
  compareIds: [],
  loadRuns: async () => {
    try {
      set({ runs: await get().api.runsList() });
    } catch {
      /* 启动早期 invoke 不可用时静默 */
    }
  },
  select: async (id) => {
    set({ selectedId: id, summary: null });
    try {
      set({ summary: await get().api.runSummary(id) });
    } catch (e) {
      set({ summary: null });
      throw e;
    }
  },
  toggleCompare: (id) =>
    set((s) => ({
      compareIds: s.compareIds.includes(id)
        ? s.compareIds.filter((x) => x !== id)
        : [...s.compareIds, id].slice(-2), // 至多两个,后选顶替
    })),
  remove: async (id) => {
    await get().api.runDelete(id);
    const s = get();
    set({
      runs: s.runs.filter((r) => r.id !== id),
      selectedId: s.selectedId === id ? null : s.selectedId,
      summary: s.selectedId === id ? null : s.summary,
      compareIds: s.compareIds.filter((x) => x !== id),
    });
  },
}));
```

- [ ] **Step 3: BacktestConfigForm.tsx**

```tsx
import { Button, Form, InputNumber, Select, Space, Switch, Typography } from "antd";
import { App as AntApp } from "antd";
import { useEffect, useState } from "react";
import type { TreeInfoDto } from "@bindings/TreeInfoDto";
import type { CsvInfoDto } from "@bindings/CsvInfoDto";
import type { BacktestConfigDto } from "@bindings/BacktestConfigDto";
import { api } from "../api/ipc";

export default function BacktestConfigForm({ onStarted }: { onStarted: (taskId: string) => void }) {
  const { message } = AntApp.useApp();
  const [trees, setTrees] = useState<TreeInfoDto[]>([]);
  const [csvs, setCsvs] = useState<CsvInfoDto[]>([]);
  const [useFetch, setUseFetch] = useState(false);
  const [starting, setStarting] = useState(false);
  const [form] = Form.useForm();

  useEffect(() => {
    api.treeList().then(setTrees).catch(() => {});
    api.dataCsvList().then(setCsvs).catch(() => {});
  }, []);

  const submit = async () => {
    const v = await form.validateFields();
    const config: BacktestConfigDto = {
      tree_path: v.tree_path,
      primary_path: useFetch ? "" : v.primary_path,
      mode: v.mode,
      cost_bps: v.cost_bps,
      warmup: v.warmup,
      window: v.window,
      initial_capital: v.initial_capital,
      fetch: useFetch
        ? { symbol: v.symbol, scale: v.scale, datalen: 1023, adjust: "qfq" }
        : null,
    };
    setStarting(true);
    try {
      const taskId = await api.backtestRun(config);
      message.success(`回测已启动(任务 ${taskId})`);
      onStarted(taskId);
    } catch (e) {
      message.error(String(e));
    } finally {
      setStarting(false);
    }
  };

  return (
    <Form
      form={form}
      layout="vertical"
      size="small"
      initialValues={{ mode: "sim_hard", cost_bps: 10, warmup: 80, window: 100, initial_capital: 100000, scale: 60 }}
    >
      <Form.Item name="tree_path" label="决策树" rules={[{ required: true }]}>
        <Select
          showSearch
          options={trees.map((t) => ({
            value: t.path,
            label: `${t.name ?? "(加载失败)"} · ${t.path}${t.frozen ? " 🔒" : ""}`,
            disabled: !t.name,
          }))}
        />
      </Form.Item>
      <Form.Item label="数据来源">
        <Space>
          <Switch checked={useFetch} onChange={setUseFetch} checkedChildren="拉取" unCheckedChildren="本地CSV" />
          <Typography.Text type="secondary">{useFetch ? "新浪 qfq" : "工作区内 CSV"}</Typography.Text>
        </Space>
      </Form.Item>
      {useFetch ? (
        <Space.Compact block>
          <Form.Item name="symbol" rules={[{ required: useFetch }]} style={{ flex: 1 }}>
            <Select
              showSearch
              placeholder="sh600030"
              options={["sh600030", "sh600036", "sh600519", "sz000858"].map((s) => ({ value: s }))}
              popupMatchSelectWidth={false}
              // 允许自由输入
              mode={undefined}
              optionFilterProp="value"
            />
          </Form.Item>
          <Form.Item name="scale">
            <Select options={[{ value: 15 }, { value: 60 }, { value: 240, label: "240(日线)" }]} />
          </Form.Item>
        </Space.Compact>
      ) : (
        <Form.Item name="primary_path" label="行情 CSV" rules={[{ required: !useFetch }]}>
          <Select
            showSearch
            options={csvs.map((c) => ({
              value: c.path,
              label: `${c.path}${c.rows != null ? ` (${c.rows}根,至${c.last_t})` : " (解析失败)"}`,
              disabled: c.rows == null,
            }))}
          />
        </Form.Item>
      )}
      <Space wrap>
        <Form.Item name="mode" label="模式">
          <Select
            style={{ width: 130 }}
            options={[
              { value: "sim_hard", label: "sim·硬" },
              { value: "sim_soft", label: "sim·软" },
              { value: "score_hard", label: "打分·硬" },
              { value: "score_soft", label: "打分·软" },
            ]}
          />
        </Form.Item>
        <Form.Item name="cost_bps" label="成本bps">
          <InputNumber min={0} />
        </Form.Item>
        <Form.Item name="warmup" label="warmup">
          <InputNumber min={0} />
        </Form.Item>
        <Form.Item name="window" label="window">
          <InputNumber min={10} />
        </Form.Item>
        <Form.Item name="initial_capital" label="初始资金(元)">
          <InputNumber min={1} step={10000} />
        </Form.Item>
      </Space>
      <Button type="primary" loading={starting} onClick={() => void submit()}>
        运行回测
      </Button>
    </Form>
  );
}
```

- [ ] **Step 4: RunHistoryList.tsx**

```tsx
import { Checkbox, List, Popconfirm, Tag, Typography } from "antd";
import type { RunMetaDto } from "@bindings/RunMetaDto";

const KIND_TAG: Record<string, string> = {
  sim_hard: "blue", sim_soft: "geekblue", score_hard: "purple", score_soft: "magenta",
};

export default function RunHistoryList({
  runs, selectedId, compareIds, onSelect, onToggleCompare, onDelete,
}: {
  runs: RunMetaDto[];
  selectedId: string | null;
  compareIds: string[];
  onSelect: (id: string) => void;
  onToggleCompare: (id: string) => void;
  onDelete: (id: string) => void;
}) {
  return (
    <List
      size="small"
      dataSource={runs}
      locale={{ emptyText: "暂无留档——跑一次回测吧" }}
      renderItem={(r) => (
        <List.Item
          style={{ cursor: "pointer", background: r.id === selectedId ? "rgba(22,119,255,.08)" : undefined }}
          onClick={() => onSelect(r.id)}
          actions={[
            <Checkbox
              key="c"
              checked={compareIds.includes(r.id)}
              onClick={(e) => e.stopPropagation()}
              onChange={() => onToggleCompare(r.id)}
            >
              对比
            </Checkbox>,
            <Popconfirm key="d" title="删除该留档?" onConfirm={() => onDelete(r.id)}>
              <Typography.Link onClick={(e) => e.stopPropagation()}>删除</Typography.Link>
            </Popconfirm>,
          ]}
        >
          <List.Item.Meta
            title={
              <>
                <Tag color={r.ok ? KIND_TAG[r.kind] ?? "default" : "red"}>{r.ok ? r.kind : "失败"}</Tag>
                {r.name}
              </>
            }
            description={`${r.id} · ${r.created}`}
          />
        </List.Item>
      )}
    />
  );
}
```

- [ ] **Step 5: Backtest.tsx 页骨架**（结果区 tabs 本任务先放占位，U2/U3/U4 逐个换真）

```tsx
import { useEffect } from "react";
import { Card, Col, Row, Tabs, Typography } from "antd";
import { useBacktest } from "../stores/backtest";
import BacktestConfigForm from "../components/BacktestConfigForm";
import RunHistoryList from "../components/RunHistoryList";

export default function Backtest() {
  const st = useBacktest();

  useEffect(() => {
    void st.loadRuns();
    // 任务完成后列表会过时——简单轮询(驾驶舱模式一致,8s 足够)
    const timer = setInterval(() => void st.loadRuns(), 8000);
    return () => clearInterval(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <Row gutter={12}>
      <Col span={7}>
        <Card size="small" title="回测配置" style={{ marginBottom: 12 }}>
          <BacktestConfigForm onStarted={() => void st.loadRuns()} />
        </Card>
        <Card size="small" title={`历史留档(${st.runs.length})`}>
          <RunHistoryList
            runs={st.runs}
            selectedId={st.selectedId}
            compareIds={st.compareIds}
            onSelect={(id) => void st.select(id)}
            onToggleCompare={st.toggleCompare}
            onDelete={(id) => void st.remove(id)}
          />
        </Card>
      </Col>
      <Col span={17}>
        {st.selectedId == null ? (
          <Typography.Text type="secondary">从左侧选择一次留档查看结果</Typography.Text>
        ) : (
          <Tabs
            items={[
              { key: "overview", label: "概览", children: <Typography.Text type="secondary">U2 交付</Typography.Text> },
              { key: "kline", label: "K线信号", children: <Typography.Text type="secondary">U3 交付</Typography.Text> },
              { key: "trades", label: "交易明细", children: <Typography.Text type="secondary">U2 交付</Typography.Text> },
              { key: "replay", label: "决策回放", children: <Typography.Text type="secondary">U4 交付</Typography.Text> },
              { key: "raw", label: "原始", children: <Typography.Text type="secondary">U2 交付</Typography.Text> },
            ]}
          />
        )}
      </Col>
    </Row>
  );
}
```

App.tsx：`/backtest` 路由由 Placeholder 换 `<Backtest />`（import 加一行；其余路由不动）。

- [ ] **Step 6: Backtest.test.tsx**

```tsx
import { render, screen, waitFor } from "@testing-library/react";
import "@testing-library/jest-dom";
import { HashRouter } from "react-router-dom";
import { App as AntApp } from "antd";
import Backtest from "./Backtest";
import { useBacktest } from "../stores/backtest";
import type { RunMetaDto } from "@bindings/RunMetaDto";

const RUNS: RunMetaDto[] = [
  { id: "20260612-210000-0a1b-01", kind: "sim_hard", name: "m2-mini × bars.csv",
    tree_name: "m2-mini", created: "2026-06-12T21:00:00", ok: true, error: null },
];

const realApi = useBacktest.getState().api;
afterEach(() => useBacktest.setState({ api: realApi, runs: [], selectedId: null, summary: null, compareIds: [] }));

test("backtest page lists archived runs", async () => {
  useBacktest.setState({
    api: { ...realApi, runsList: async () => RUNS, treeList: async () => [], dataCsvList: async () => [] },
  });
  render(
    <AntApp><HashRouter><Backtest /></HashRouter></AntApp>
  );
  await waitFor(() => expect(screen.getByText(/m2-mini × bars.csv/)).toBeInTheDocument());
  expect(screen.getByText(/历史留档\(1\)/)).toBeInTheDocument();
});
```

注意：BacktestConfigForm 内部直接 import api（treeList/dataCsvList）——测试里组件挂载会调真 invoke 并被 catch 吞掉（jsdom 无 tauri），不影响断言；若出现 unhandled rejection 噪声，在两处 `.catch(() => {})` 已兜住（保持）。

- [ ] **Step 7: 验证**

Run: `cd desktop/ui && npx tsc --noEmit && npx vitest run` → 全绿（5 个测试文件）。
Run: `npm run build` → OK。

- [ ] **Step 8: Commit**

```bash
git status --porcelain
git add desktop/ui/src/pages/Backtest.tsx desktop/ui/src/pages/Backtest.test.tsx desktop/ui/src/stores/backtest.ts desktop/ui/src/components/BacktestConfigForm.tsx desktop/ui/src/components/RunHistoryList.tsx desktop/ui/src/api/ipc.ts desktop/ui/src/App.tsx
git commit -m "feat(desktop): backtest page skeleton - config form, run trigger, archive list"
```

---

### Task U2: 概览 + 交易明细 + 原始视图

**Files:**
- Create: `desktop/ui/src/components/RunOverview.tsx`、`desktop/ui/src/components/TradesTable.tsx`、`desktop/ui/src/components/RawJsonView.tsx`
- Create: `desktop/ui/src/components/RunOverview.test.tsx`
- Modify: `desktop/ui/src/pages/Backtest.tsx`（三个 tab 换真）

- [ ] **Step 1: RunOverview.tsx**（指标卡 + 资产/净值曲线切换；曲线复用 echarts 模式）

```tsx
import { useEffect, useRef, useState } from "react";
import { Card, Col, Row, Segmented, Statistic, Typography } from "antd";
import * as echarts from "echarts";
import type { RunSummaryDto } from "@bindings/RunSummaryDto";
import type { EquityPointDto } from "@bindings/EquityPointDto";
import { api } from "../api/ipc";

function EquityChart({ points, money }: { points: EquityPointDto[]; money: boolean }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!ref.current) return;
    const chart = echarts.init(ref.current);
    chart.setOption({
      tooltip: { trigger: "axis" },
      xAxis: { type: "category", data: points.map((p) => p.t) },
      yAxis: { type: "value", scale: true, axisLabel: { formatter: money ? "¥{value}" : "{value}" } },
      series: [
        { name: money ? "资产" : "净值", type: "line", showSymbol: false,
          data: points.map((p) => (money ? p.equity : p.nav)) },
        { name: "仓位", type: "line", showSymbol: false, yAxisIndex: 0, lineStyle: { opacity: 0 },
          areaStyle: { opacity: 0.08 }, data: points.map((p) => (money ? p.pos * points[0].equity : p.pos)) },
      ],
      grid: { left: 72, right: 16, top: 24, bottom: 24 },
    });
    const onResize = () => chart.resize();
    window.addEventListener("resize", onResize);
    return () => {
      window.removeEventListener("resize", onResize);
      chart.dispose();
    };
  }, [points, money]);
  return <div ref={ref} style={{ height: 300 }} />;
}

const pct = (v: number | null) => (v == null ? "—" : `${(v * 100).toFixed(2)}%`);
const yuan = (v: number | null) =>
  v == null ? "—" : v.toLocaleString("zh-CN", { style: "currency", currency: "CNY", maximumFractionDigits: 0 });

export default function RunOverview({ summary }: { summary: RunSummaryDto }) {
  const [points, setPoints] = useState<EquityPointDto[]>([]);
  const [money, setMoney] = useState(true);
  const sim = summary.meta.kind.startsWith("sim");

  useEffect(() => {
    setPoints([]);
    if (sim) api.runEquity(summary.meta.id).then(setPoints).catch(() => {});
  }, [summary.meta.id, sim]);

  if (!sim) {
    return (
      <Card size="small" title={`打分结果 · ${summary.meta.tree_name}`}>
        <Typography.Paragraph type="secondary">打分模式概览为原样关键字段(完整内容见"原始"标签)。</Typography.Paragraph>
        <pre style={{ fontSize: 12, maxHeight: 360, overflow: "auto" }}>
          {JSON.stringify(summary.raw, null, 2)?.slice(0, 4000)}
        </pre>
      </Card>
    );
  }

  return (
    <div>
      <Row gutter={8} style={{ marginBottom: 12 }}>
        <Col span={4}><Card size="small"><Statistic title="期末资产" value={yuan(summary.final_equity)} /></Card></Col>
        <Col span={4}><Card size="small"><Statistic title="净盈亏" value={yuan(summary.net_pnl)} /></Card></Col>
        <Col span={4}><Card size="small"><Statistic title="总收益" value={pct(summary.total_return)} /></Card></Col>
        <Col span={4}><Card size="small"><Statistic title="最大回撤" value={pct(summary.max_drawdown)} /></Card></Col>
        <Col span={4}><Card size="small"><Statistic title="Sharpe" value={summary.sharpe?.toFixed(2) ?? "—"} /></Card></Col>
        <Col span={4}><Card size="small"><Statistic title="bh对照" value={pct(summary.buy_and_hold)} /></Card></Col>
      </Row>
      <Card
        size="small"
        title="资产曲线"
        extra={<Segmented options={[{ label: "金额", value: 1 }, { label: "净值", value: 0 }]}
          value={money ? 1 : 0} onChange={(v) => setMoney(v === 1)} />}
      >
        {points.length ? <EquityChart points={points} money={money} /> :
          <Typography.Text type="secondary">无曲线数据(traces 缺失)</Typography.Text>}
      </Card>
      <Typography.Text type="secondary" style={{ fontSize: 12 }}>
        初始资金 {yuan(summary.config.initial_capital)};交易 {summary.n_round_trips ?? "—"} 笔,
        胜率 {pct(summary.win_rate)},换手 {summary.turnover?.toFixed(1) ?? "—"}
      </Typography.Text>
    </div>
  );
}
```

- [ ] **Step 2: TradesTable.tsx**

```tsx
import { useEffect, useState } from "react";
import { Table, Tooltip, Typography } from "antd";
import type { TradeDto } from "@bindings/TradeDto";
import { api } from "../api/ipc";

export default function TradesTable({ runId }: { runId: string }) {
  const [rows, setRows] = useState<TradeDto[]>([]);
  useEffect(() => {
    setRows([]);
    api.runTrades(runId).then(setRows).catch(() => {});
  }, [runId]);
  return (
    <Table
      size="small"
      rowKey={(r) => `${r.entry_t}-${r.exit_t}`}
      dataSource={rows}
      pagination={{ pageSize: 20 }}
      locale={{ emptyText: "无交易(打分模式或全程空仓)" }}
      columns={[
        { title: "入场", dataIndex: "entry_t" },
        { title: "出场", dataIndex: "exit_t" },
        { title: "入场价", dataIndex: "entry_px", render: (v: number) => v.toFixed(2) },
        { title: "出场价", dataIndex: "exit_px", render: (v: number) => v.toFixed(2) },
        { title: "持有bars", dataIndex: "bars_held" },
        { title: "收益率", dataIndex: "trip_return",
          render: (v: number) => <span style={{ color: v >= 0 ? "#3f8600" : "#cf1322" }}>{(v * 100).toFixed(2)}%</span> },
        { title: <Tooltip title="资金×trip_return,单利近似口径">盈亏额*</Tooltip>, dataIndex: "pnl_amount",
          render: (v: number) => v.toLocaleString("zh-CN", { maximumFractionDigits: 0 }) },
        { title: "原因", dataIndex: "reason", render: (v: string) => <Typography.Text type="secondary">{v}</Typography.Text> },
      ]}
    />
  );
}
```

- [ ] **Step 3: RawJsonView.tsx**

```tsx
import { useEffect, useState } from "react";
import { Typography } from "antd";
import { api } from "../api/ipc";

export default function RawJsonView({ runId }: { runId: string }) {
  const [txt, setTxt] = useState("");
  useEffect(() => {
    setTxt("");
    api.runSummary(runId)
      .then((s) => setTxt(JSON.stringify(s.raw ?? s, null, 2)))
      .catch((e) => setTxt(String(e)));
  }, [runId]);
  return (
    <div>
      <Typography.Text type="secondary" style={{ fontSize: 12 }}>
        sim 模式显示摘要 DTO;score 模式显示 result.json 原样。完整文件在 .rquant-desktop/runs/{runId}/
      </Typography.Text>
      <pre style={{ fontSize: 12, maxHeight: 480, overflow: "auto" }}>{txt}</pre>
    </div>
  );
}
```

- [ ] **Step 4: Backtest.tsx 接线**——overview/trades/raw 三个 tab 的占位换为：

```tsx
{ key: "overview", label: "概览", children: st.summary ? <RunOverview summary={st.summary} /> : <Spin /> },
{ key: "trades", label: "交易明细", children: <TradesTable runId={st.selectedId} /> },
{ key: "raw", label: "原始", children: <RawJsonView runId={st.selectedId} /> },
```

（import 三个组件 + antd Spin。）

- [ ] **Step 5: RunOverview.test.tsx**

```tsx
import { render, screen } from "@testing-library/react";
import "@testing-library/jest-dom";
import { vi } from "vitest";
import type { RunSummaryDto } from "@bindings/RunSummaryDto";

vi.mock("../api/ipc", () => ({ api: { runEquity: async () => [] } }));
vi.mock("echarts", () => ({ init: () => ({ setOption: () => {}, resize: () => {}, dispose: () => {} }) }));

import RunOverview from "./RunOverview";

const SUMMARY: RunSummaryDto = {
  meta: { id: "20260612-210000-0a1b-01", kind: "sim_hard", name: "n", tree_name: "t",
    created: "2026-06-12T21:00:00", ok: true, error: null },
  config: { tree_path: "examples/x.yaml", primary_path: "paper/p.csv", mode: "sim_hard",
    cost_bps: 10, warmup: 80, window: 100, initial_capital: 100000, fetch: null },
  total_return: 0.246, max_drawdown: 0.137, n_round_trips: 33, win_rate: 0.52,
  avg_hold_bars: 9.1, turnover: 21.4, buy_and_hold: -0.232, sharpe: 1.21,
  final_equity: 124600, net_pnl: 24600, raw: null,
};

test("overview shows money metrics from initial capital", () => {
  render(<RunOverview summary={SUMMARY} />);
  expect(screen.getByText("期末资产")).toBeInTheDocument();
  expect(screen.getByText(/124,600/)).toBeInTheDocument();
  expect(screen.getByText("24.60%")).toBeInTheDocument();
});
```

- [ ] **Step 6: 验证**

Run: `cd desktop/ui && npx tsc --noEmit && npx vitest run && npm run build` → 全绿。

- [ ] **Step 7: Commit**

```bash
git status --porcelain
git add desktop/ui/src/components/RunOverview.tsx desktop/ui/src/components/RunOverview.test.tsx desktop/ui/src/components/TradesTable.tsx desktop/ui/src/components/RawJsonView.tsx desktop/ui/src/pages/Backtest.tsx
git commit -m "feat(desktop): run overview with capital display, trades table, raw view"
```

---

### Task U3: K线组件 + K线信号视图

**Files:**
- Create: `desktop/ui/src/components/KlineChart.tsx`（通用：数据工作台复用）、`desktop/ui/src/components/KlineSignalsView.tsx`
- Create: `desktop/ui/src/components/KlineChart.test.tsx`
- Modify: `desktop/ui/src/pages/Backtest.tsx`（kline tab 换真）

- [ ] **Step 1: KlineChart.tsx**（candlestick+volume 副图；可选 markers/overlay）

```tsx
import { useEffect, useRef } from "react";
import * as echarts from "echarts";
import type { BarDto } from "@bindings/BarDto";

export interface TradeMarker {
  t: string;
  price: number;
  kind: "entry" | "exit";
  label: string;
}

export interface Overlay {
  name: string;
  points: { t: string; value: number | null }[];
}

/** 通用 K 线:主图 candlestick(+overlay 线/markers),副图 volume。 */
export default function KlineChart({
  bars, markers = [], overlays = [], height = 420,
}: {
  bars: BarDto[];
  markers?: TradeMarker[];
  overlays?: Overlay[];
  height?: number;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!ref.current || !bars.length) return;
    const chart = echarts.init(ref.current);
    const times = bars.map((b) => b.t);
    const idx = new Map(times.map((t, i) => [t, i]));
    chart.setOption({
      tooltip: { trigger: "axis", axisPointer: { type: "cross" } },
      axisPointer: { link: [{ xAxisIndex: "all" }] },
      grid: [
        { left: 64, right: 16, top: 24, height: height - 200 },
        { left: 64, right: 16, top: height - 150, height: 80 },
      ],
      xAxis: [
        { type: "category", data: times, gridIndex: 0 },
        { type: "category", data: times, gridIndex: 1, axisLabel: { show: false } },
      ],
      yAxis: [
        { type: "value", scale: true, gridIndex: 0 },
        { type: "value", gridIndex: 1, axisLabel: { show: false } },
      ],
      dataZoom: [{ type: "inside", xAxisIndex: [0, 1] }, { type: "slider", xAxisIndex: [0, 1], top: height - 44 }],
      series: [
        {
          name: "K", type: "candlestick", xAxisIndex: 0, yAxisIndex: 0,
          data: bars.map((b) => [b.open, b.close, b.low, b.high]),
          itemStyle: { color: "#cf1322", color0: "#3f8600", borderColor: "#cf1322", borderColor0: "#3f8600" },
          markPoint: markers.length
            ? {
                data: markers
                  .filter((m) => idx.has(m.t))
                  .map((m) => ({
                    coord: [idx.get(m.t), m.price],
                    value: m.label,
                    symbol: m.kind === "entry" ? "arrow" : "pin",
                    symbolRotate: m.kind === "entry" ? 0 : 180,
                    itemStyle: { color: m.kind === "entry" ? "#1677ff" : "#fa8c16" },
                  })),
                label: { fontSize: 10 },
              }
            : undefined,
        },
        ...overlays.map((o) => ({
          name: o.name, type: "line" as const, xAxisIndex: 0, yAxisIndex: 0, showSymbol: false,
          data: times.map((t) => o.points.find((p) => p.t === t)?.value ?? null),
          connectNulls: false,
        })),
        {
          name: "成交量", type: "bar", xAxisIndex: 1, yAxisIndex: 1,
          data: bars.map((b) => b.volume),
          itemStyle: { color: "rgba(22,119,255,.45)" },
        },
      ],
      legend: overlays.length ? { top: 0 } : undefined,
    });
    const onResize = () => chart.resize();
    window.addEventListener("resize", onResize);
    return () => {
      window.removeEventListener("resize", onResize);
      chart.dispose();
    };
  }, [bars, markers, overlays, height]);
  return <div ref={ref} style={{ height }} />;
}
```

（overlay 逐点 `find` 是 O(n²)——tail≤2000、overlay≤2 条，最坏 ~8M 次比较可感知卡顿；实现时**用 Map 替换**：`const om = new Map(o.points.map(p=>[p.t,p.value]))` 后 `om.get(t) ?? null`。此为计划内定稿，照 Map 版写。）

- [ ] **Step 2: KlineSignalsView.tsx**

```tsx
import { useEffect, useState } from "react";
import { Typography } from "antd";
import type { BarDto } from "@bindings/BarDto";
import type { TradeDto } from "@bindings/TradeDto";
import { api } from "../api/ipc";
import KlineChart, { type TradeMarker } from "./KlineChart";

export default function KlineSignalsView({ runId, primaryPath, isSim }: { runId: string; primaryPath: string; isSim: boolean }) {
  const [bars, setBars] = useState<BarDto[]>([]);
  const [trades, setTrades] = useState<TradeDto[]>([]);

  useEffect(() => {
    setBars([]);
    setTrades([]);
    api.dataReadBars(primaryPath, 2000).then(setBars).catch(() => {});
    if (isSim) api.runTrades(runId).then(setTrades).catch(() => {});
  }, [runId, primaryPath, isSim]);

  if (!bars.length) return <Typography.Text type="secondary">行情 CSV 不可读({primaryPath})</Typography.Text>;

  const markers: TradeMarker[] = trades.flatMap((t) => [
    { t: t.entry_t, price: t.entry_px, kind: "entry" as const, label: "买" },
    { t: t.exit_t, price: t.exit_px, kind: "exit" as const, label: t.reason },
  ]);

  return (
    <div>
      <KlineChart bars={bars} markers={markers} />
      <Typography.Text type="secondary" style={{ fontSize: 12 }}>
        {isSim ? `${trades.length} 笔交易标注(箭头=入场,旗标=出场)` : "打分模式无交易标注"}
        ;显示末 2000 根
      </Typography.Text>
    </div>
  );
}
```

- [ ] **Step 3: Backtest.tsx 接线**——kline tab：

```tsx
{ key: "kline", label: "K线信号",
  children: st.summary
    ? <KlineSignalsView runId={st.selectedId} primaryPath={st.summary.config.primary_path}
        isSim={st.summary.meta.kind.startsWith("sim")} />
    : <Spin /> },
```

- [ ] **Step 4: KlineChart.test.tsx**

```tsx
import { render } from "@testing-library/react";
import "@testing-library/jest-dom";
import { vi } from "vitest";

const setOption = vi.fn();
vi.mock("echarts", () => ({ init: () => ({ setOption, resize: () => {}, dispose: () => {} }) }));

import KlineChart from "./KlineChart";
import type { BarDto } from "@bindings/BarDto";

const BARS: BarDto[] = [
  { t: "2026-01-01T10:00:00", open: 10, high: 10.5, low: 9.8, close: 10.2, volume: 100 },
  { t: "2026-01-01T11:00:00", open: 10.2, high: 10.8, low: 10.1, close: 10.6, volume: 120 },
];

test("kline builds candlestick + volume series with markers", () => {
  render(
    <KlineChart bars={BARS} markers={[{ t: "2026-01-01T11:00:00", price: 10.2, kind: "entry", label: "买" }]} />
  );
  expect(setOption).toHaveBeenCalled();
  const opt = setOption.mock.calls[0][0];
  expect(opt.series[0].type).toBe("candlestick");
  expect(opt.series.at(-1).type).toBe("bar");
  expect(opt.series[0].markPoint.data).toHaveLength(1);
});
```

- [ ] **Step 5: 验证**

Run: `cd desktop/ui && npx tsc --noEmit && npx vitest run && npm run build` → 全绿。

- [ ] **Step 6: Commit**

```bash
git status --porcelain
git add desktop/ui/src/components/KlineChart.tsx desktop/ui/src/components/KlineChart.test.tsx desktop/ui/src/components/KlineSignalsView.tsx desktop/ui/src/pages/Backtest.tsx
git commit -m "feat(desktop): shared kline chart + trade-marker signals view"
```

---

### Task U4: 决策回放视图

**Files:**
- Create: `desktop/ui/src/components/ReplayView.tsx`、`desktop/ui/src/components/ReplayView.test.tsx`
- Modify: `desktop/ui/src/pages/Backtest.tsx`（replay tab 换真）

- [ ] **Step 1: ReplayView.tsx**

```tsx
import { useEffect, useState } from "react";
import { Alert, Card, Col, Descriptions, Row, Slider, Table, Tag, Typography } from "antd";
import type { ReplayFrameDto } from "@bindings/ReplayFrameDto";
import type { FactorValueDto } from "@bindings/FactorValueDto";
import { api } from "../api/ipc";

const STANCE_COLOR: Record<string, string> = { Long: "green", Short: "red", Flat: "default" };

export default function ReplayView({ runId }: { runId: string }) {
  const [frames, setFrames] = useState<ReplayFrameDto[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [i, setI] = useState(0);
  const [factors, setFactors] = useState<FactorValueDto[]>([]);

  useEffect(() => {
    setFrames([]);
    setError(null);
    setI(0);
    api.runReplayFrames(runId)
      .then((f) => {
        setFrames(f);
        setI(f.length ? f.length - 1 : 0);
      })
      .catch((e) => setError(String(e)));
  }, [runId]);

  const f = frames[i];

  useEffect(() => {
    setFactors([]);
    if (f) api.runReplayFactors(runId, f.t).then(setFactors).catch(() => {});
  }, [runId, f?.t]); // eslint-disable-line react-hooks/exhaustive-deps

  if (error) return <Alert type="info" message={error} />;
  if (!frames.length) return <Typography.Text type="secondary">加载回放帧…</Typography.Text>;

  return (
    <div>
      <Slider min={0} max={frames.length - 1} value={i} onChange={setI}
        tooltip={{ formatter: (v) => frames[v ?? 0]?.t }} />
      <Row gutter={12}>
        <Col span={14}>
          <Card size="small" title={`决策路径 @ ${f.t}`}
            extra={<Tag color={STANCE_COLOR[f.stance] ?? "default"}>{f.leaf} · {f.stance}</Tag>}>
            <Table
              size="small"
              rowKey={(r) => r.node_id}
              pagination={false}
              dataSource={f.path}
              columns={[
                { title: "节点", dataIndex: "node_id" },
                { title: "分支", dataIndex: "label" },
                { title: "置信", dataIndex: "confidence", render: (v: number) => v.toFixed(3) },
                { title: "依据", dataIndex: "rationale", ellipsis: true },
              ]}
            />
            {f.nav != null && (
              <Descriptions size="small" column={3} style={{ marginTop: 8 }}>
                <Descriptions.Item label="target">{f.target?.toFixed(2)}</Descriptions.Item>
                <Descriptions.Item label="pos">{f.pos?.toFixed(2)}</Descriptions.Item>
                <Descriptions.Item label="nav">{f.nav?.toFixed(6)}</Descriptions.Item>
              </Descriptions>
            )}
          </Card>
        </Col>
        <Col span={10}>
          <Card size="small" title="因子值(现算)">
            <Table
              size="small"
              rowKey={(r) => r.name}
              pagination={false}
              dataSource={factors}
              locale={{ emptyText: "该树无 factors 块" }}
              columns={[
                { title: "因子", dataIndex: "name" },
                { title: "值", dataIndex: "value",
                  render: (v: number | null) => (v == null ? <Tag>NaN/弃权</Tag> : v.toFixed(6)) },
              ]}
            />
          </Card>
        </Col>
      </Row>
    </div>
  );
}
```

- [ ] **Step 2: Backtest.tsx 接线**——replay tab：

```tsx
{ key: "replay", label: "决策回放", children: <ReplayView runId={st.selectedId} /> },
```

- [ ] **Step 3: ReplayView.test.tsx**

```tsx
import { render, screen, waitFor } from "@testing-library/react";
import "@testing-library/jest-dom";
import { vi } from "vitest";
import type { ReplayFrameDto } from "@bindings/ReplayFrameDto";

const FRAMES: ReplayFrameDto[] = [
  { t: "2026-01-05T11:00:00", leaf: "l", stance: "Long",
    path: [{ node_id: "r", label: "up", confidence: 1, rationale: "close>sma" }],
    target: 1, pos: 0, nav: 1.0 },
  { t: "2026-01-05T12:00:00", leaf: "f", stance: "Flat",
    path: [{ node_id: "r", label: "default", confidence: 1, rationale: "" }],
    target: 0, pos: 1, nav: 1.01 },
];

vi.mock("../api/ipc", () => ({
  api: {
    runReplayFrames: async () => FRAMES,
    runReplayFactors: async () => [{ name: "ma", value: 10.2 }],
  },
}));

import ReplayView from "./ReplayView";

test("replay shows latest frame path and factors", async () => {
  render(<ReplayView runId="20260612-210000-0a1b-01" />);
  await waitFor(() => expect(screen.getByText(/决策路径 @ 2026-01-05T12:00:00/)).toBeInTheDocument());
  expect(screen.getByText("default")).toBeInTheDocument();
  expect(screen.getByText("ma")).toBeInTheDocument();
  expect(screen.getByText("10.200000")).toBeInTheDocument();
});
```

- [ ] **Step 4: 验证 + Commit**

Run: `cd desktop/ui && npx tsc --noEmit && npx vitest run && npm run build` → 全绿。

```bash
git status --porcelain
git add desktop/ui/src/components/ReplayView.tsx desktop/ui/src/components/ReplayView.test.tsx desktop/ui/src/pages/Backtest.tsx
git commit -m "feat(desktop): decision replay view - path table + on-demand factor values"
```

---

### Task U5: 对比视图

**Files:**
- Create: `desktop/ui/src/components/CompareView.tsx`
- Modify: `desktop/ui/src/pages/Backtest.tsx`（compareIds 满 2 时显示对比按钮/区域）

- [ ] **Step 1: CompareView.tsx**

```tsx
import { useEffect, useRef, useState } from "react";
import { Card, Table, Typography } from "antd";
import * as echarts from "echarts";
import type { RunSummaryDto } from "@bindings/RunSummaryDto";
import type { EquityPointDto } from "@bindings/EquityPointDto";
import { api } from "../api/ipc";

function OverlayChart({ a, b, an, bn }: { a: EquityPointDto[]; b: EquityPointDto[]; an: string; bn: string }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!ref.current) return;
    const chart = echarts.init(ref.current);
    // 各自时间轴可能不同——以并集为类目轴,缺位 null 断线
    const times = Array.from(new Set([...a.map((p) => p.t), ...b.map((p) => p.t)])).sort();
    const ma = new Map(a.map((p) => [p.t, p.nav]));
    const mb = new Map(b.map((p) => [p.t, p.nav]));
    chart.setOption({
      tooltip: { trigger: "axis" },
      legend: { top: 0 },
      xAxis: { type: "category", data: times },
      yAxis: { type: "value", scale: true },
      series: [
        { name: an, type: "line", showSymbol: false, connectNulls: false, data: times.map((t) => ma.get(t) ?? null) },
        { name: bn, type: "line", showSymbol: false, connectNulls: false, data: times.map((t) => mb.get(t) ?? null) },
      ],
      grid: { left: 56, right: 16, top: 28, bottom: 24 },
    });
    const onResize = () => chart.resize();
    window.addEventListener("resize", onResize);
    return () => {
      window.removeEventListener("resize", onResize);
      chart.dispose();
    };
  }, [a, b, an, bn]);
  return <div ref={ref} style={{ height: 280 }} />;
}

const pct = (v: number | null | undefined) => (v == null ? "—" : `${(v * 100).toFixed(2)}%`);

export default function CompareView({ ids }: { ids: [string, string] }) {
  const [sums, setSums] = useState<RunSummaryDto[]>([]);
  const [curves, setCurves] = useState<EquityPointDto[][]>([[], []]);

  useEffect(() => {
    setSums([]);
    setCurves([[], []]);
    void Promise.all(ids.map((id) => api.runSummary(id))).then(setSums).catch(() => {});
    void Promise.all(
      ids.map((id) => api.runEquity(id).catch(() => [] as EquityPointDto[]))
    ).then((c) => setCurves(c as EquityPointDto[][]));
  }, [ids[0], ids[1]]); // eslint-disable-line react-hooks/exhaustive-deps

  if (sums.length < 2) return <Typography.Text type="secondary">加载对比…</Typography.Text>;
  const [a, b] = sums;
  const rows = [
    { k: "总收益", a: pct(a.total_return), b: pct(b.total_return) },
    { k: "最大回撤", a: pct(a.max_drawdown), b: pct(b.max_drawdown) },
    { k: "Sharpe", a: a.sharpe?.toFixed(2) ?? "—", b: b.sharpe?.toFixed(2) ?? "—" },
    { k: "交易数", a: a.n_round_trips ?? "—", b: b.n_round_trips ?? "—" },
    { k: "胜率", a: pct(a.win_rate), b: pct(b.win_rate) },
    { k: "换手", a: a.turnover?.toFixed(1) ?? "—", b: b.turnover?.toFixed(1) ?? "—" },
    { k: "bh对照", a: pct(a.buy_and_hold), b: pct(b.buy_and_hold) },
  ];
  return (
    <div>
      <Card size="small" title="净值曲线叠加(nav 口径,资金无关)" style={{ marginBottom: 12 }}>
        {curves[0].length || curves[1].length ? (
          <OverlayChart a={curves[0]} b={curves[1]} an={a.meta.name} bn={b.meta.name} />
        ) : (
          <Typography.Text type="secondary">至少一侧无曲线(打分模式)</Typography.Text>
        )}
      </Card>
      <Table
        size="small"
        rowKey="k"
        pagination={false}
        dataSource={rows}
        columns={[
          { title: "指标", dataIndex: "k" },
          { title: a.meta.name, dataIndex: "a" },
          { title: b.meta.name, dataIndex: "b" },
        ]}
      />
    </div>
  );
}
```

- [ ] **Step 2: Backtest.tsx 接线**——右栏顶部加对比开关区：compareIds.length === 2 时显示一块 `<Card size="small" title="对比">` 含 `<CompareView ids={st.compareIds as [string,string]} />`，否则不渲染（选满两个自动出现；列表 checkbox 已在 U1 就位）。

- [ ] **Step 3: 验证 + Commit**

Run: `cd desktop/ui && npx tsc --noEmit && npx vitest run && npm run build` → 全绿。

```bash
git status --porcelain
git add desktop/ui/src/components/CompareView.tsx desktop/ui/src/pages/Backtest.tsx
git commit -m "feat(desktop): two-run comparison - nav overlay + metric diff table"
```

---

### Task U6: 数据工作台页

**Files:**
- Create: `desktop/ui/src/pages/DataBench.tsx`、`desktop/ui/src/pages/DataBench.test.tsx`
- Modify: `desktop/ui/src/App.tsx`（/data 换真页）

- [ ] **Step 1: DataBench.tsx**（左清单+universe；右 K线浏览+因子叠加+拉取）

```tsx
import { useEffect, useState } from "react";
import { App as AntApp, Button, Card, Col, Input, List, Row, Select, Space, Table, Tag, Typography } from "antd";
import type { CsvInfoDto } from "@bindings/CsvInfoDto";
import type { BarDto } from "@bindings/BarDto";
import type { UniverseInfoDto } from "@bindings/UniverseInfoDto";
import type { Overlay } from "../components/KlineChart";
import KlineChart from "../components/KlineChart";
import { api } from "../api/ipc";

export default function DataBench() {
  const { message } = AntApp.useApp();
  const [csvs, setCsvs] = useState<CsvInfoDto[]>([]);
  const [universes, setUniverses] = useState<UniverseInfoDto[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [bars, setBars] = useState<BarDto[]>([]);
  const [expr, setExpr] = useState("sma(close, 20)");
  const [overlays, setOverlays] = useState<Overlay[]>([]);
  const [fetchSyms, setFetchSyms] = useState("sh600030");
  const [fetchScale, setFetchScale] = useState(60);

  const refresh = () => {
    api.dataCsvList().then(setCsvs).catch(() => {});
    api.universeList().then(setUniverses).catch(() => {});
  };
  useEffect(refresh, []);

  const open = (path: string) => {
    setSelected(path);
    setOverlays([]);
    api.dataReadBars(path, 800).then(setBars).catch((e) => message.error(String(e)));
  };

  const addOverlay = async () => {
    if (!selected) return;
    try {
      const pts = await api.dataEvalFactor(selected, expr, 100, 800);
      setOverlays((o) => [...o.slice(-1), { name: expr, points: pts }]); // 至多 2 条
    } catch (e) {
      message.error(String(e));
    }
  };

  const startFetch = async () => {
    const symbols = fetchSyms.split(/[,\s]+/).filter(Boolean);
    if (!symbols.length) return;
    try {
      const id = await api.fetchBatch(symbols, fetchScale, 1023, "qfq");
      message.success(`拉取任务已启动(${id});完成后刷新清单`);
    } catch (e) {
      message.error(String(e));
    }
  };

  return (
    <Row gutter={12}>
      <Col span={8}>
        <Card size="small" title="行情 CSV(paper/ + .rquant-desktop/data/)" extra={<a onClick={refresh}>刷新</a>}
          style={{ marginBottom: 12 }}>
          <List
            size="small"
            dataSource={csvs}
            style={{ maxHeight: 320, overflow: "auto" }}
            renderItem={(c) => (
              <List.Item
                style={{ cursor: c.rows != null ? "pointer" : "not-allowed",
                  background: c.path === selected ? "rgba(22,119,255,.08)" : undefined }}
                onClick={() => c.rows != null && open(c.path)}
              >
                <List.Item.Meta
                  title={c.path}
                  description={c.rows != null ? `${c.rows} 根 · ${c.first_t} → ${c.last_t}` : "解析失败"}
                />
              </List.Item>
            )}
          />
        </Card>
        <Card size="small" title="批量拉取(新浪 qfq → .rquant-desktop/data/)" style={{ marginBottom: 12 }}>
          <Space.Compact block>
            <Input value={fetchSyms} onChange={(e) => setFetchSyms(e.target.value)} placeholder="sh600030, sz000333" />
            <Select value={fetchScale} onChange={setFetchScale} style={{ width: 110 }}
              options={[{ value: 15 }, { value: 60 }, { value: 240, label: "240(日线)" }]} />
            <Button type="primary" onClick={() => void startFetch()}>拉取</Button>
          </Space.Compact>
          <Typography.Text type="secondary" style={{ fontSize: 12 }}>串行+500ms 节流;进度见任务抽屉</Typography.Text>
        </Card>
        <Card size="small" title="universe 清单">
          <Table
            size="small"
            rowKey="path"
            pagination={false}
            dataSource={universes}
            columns={[
              { title: "清单", dataIndex: "name",
                render: (v: string, u) => (<>{v} {u.frozen && <Tag>deploy 只读</Tag>}</>) },
              { title: "成员", render: (_, u) => u.entries.length },
            ]}
            expandable={{
              expandedRowRender: (u) => (
                <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                  {u.entries.map((e) => e.symbol).join(" · ")}
                </Typography.Text>
              ),
            }}
          />
        </Card>
      </Col>
      <Col span={16}>
        <Card
          size="small"
          title={selected ? `K线 · ${selected}(末 800 根)` : "K线浏览器"}
          extra={
            <Space.Compact>
              <Input value={expr} onChange={(e) => setExpr(e.target.value)} style={{ width: 260 }}
                placeholder="DSL 表达式,如 sma(close,20)" onPressEnter={() => void addOverlay()} />
              <Button onClick={() => void addOverlay()} disabled={!selected}>叠加因子</Button>
            </Space.Compact>
          }
        >
          {bars.length ? (
            <KlineChart bars={bars} overlays={overlays} height={520} />
          ) : (
            <Typography.Text type="secondary">左侧选择 CSV 打开;因子叠加走引擎 DSL 同口径求值(NaN 断线=弃权)</Typography.Text>
          )}
        </Card>
      </Col>
    </Row>
  );
}
```

App.tsx：`/data` 路由换 `<DataBench />`。

- [ ] **Step 2: DataBench.test.tsx**

```tsx
import { render, screen, waitFor } from "@testing-library/react";
import "@testing-library/jest-dom";
import { vi } from "vitest";

vi.mock("../api/ipc", () => ({
  api: {
    dataCsvList: async () => [
      { path: "paper/p_sh600030.csv", rows: 942, first_t: "2025-06-01T10:00:00", last_t: "2026-06-12T15:00:00" },
      { path: "paper/broken.csv", rows: null, first_t: null, last_t: null },
    ],
    universeList: async () => [
      { path: "deploy/universe_10.csv", name: "universe_10", frozen: true,
        entries: [{ symbol: "sh600519", primary: "paper/pd_sh600519.csv" }] },
    ],
    dataReadBars: async () => [],
    dataEvalFactor: async () => [],
    fetchBatch: async () => "t1",
  },
}));
vi.mock("echarts", () => ({ init: () => ({ setOption: () => {}, resize: () => {}, dispose: () => {} }) }));

import { App as AntApp } from "antd";
import DataBench from "./DataBench";

test("data bench lists csvs with freshness and universes", async () => {
  render(<AntApp><DataBench /></AntApp>);
  await waitFor(() => expect(screen.getByText("paper/p_sh600030.csv")).toBeInTheDocument());
  expect(screen.getByText(/942 根/)).toBeInTheDocument();
  expect(screen.getByText("解析失败")).toBeInTheDocument();
  expect(screen.getByText("universe_10")).toBeInTheDocument();
  expect(screen.getByText("deploy 只读")).toBeInTheDocument();
});
```

- [ ] **Step 3: 验证 + Commit**

Run: `cd desktop/ui && npx tsc --noEmit && npx vitest run && npm run build` → 全绿。

```bash
git status --porcelain
git add desktop/ui/src/pages/DataBench.tsx desktop/ui/src/pages/DataBench.test.tsx desktop/ui/src/App.tsx
git commit -m "feat(desktop): data workbench page - csv freshness, kline browser with dsl overlay, batch fetch, universes"
```

---

### Task F1: CSP 基线 + 全量收尾闸 + 人工冒烟

**Files:**
- Modify: `desktop/src-tauri/tauri.conf.json`（csp）

- [ ] **Step 1: CSP 基线**（spec §9 M2 条款；antd 需 style 内联、echarts 用 canvas 无额外面）

`tauri.conf.json` 的 `"security": { "csp": null }` 改为：

```json
    "security": {
      "csp": "default-src 'self' ipc: http://ipc.localhost; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:"
    }
```

（Tauri 2 dev 模式会自动把 devUrl 源并入;若 dev 窗口出现样式丢失/白屏，把实际被阻止的源从 webview 控制台读出来补进列表——这是本任务唯一允许的调整面，调整结果写进报告。）

- [ ] **Step 2: 全量收尾闸（全绿才算完成）**

```bash
cargo test                                              # 引擎全量(E1-E3 后 ≥326)
cargo test -p rquant-desktop                            # 桥接(M1 47 + M2 新增)
cargo clippy --workspace --all-targets -- -D warnings
cd desktop/ui && npx tsc --noEmit && npx vitest run && npm run build
```

- [ ] **Step 3: 人工冒烟清单**（控制器执行；两进程启动）

```bash
cd desktop/ui && npm run dev          # 终端1
cargo run -p rquant-desktop           # 终端2(仓库根)
```

- [ ] 回测中心:树下拉列出 examples/+deploy/(冻结🔒标注);CSV 下拉带新鲜度
- [ ] 跑一次 sim_hard(tree_v4_frozen × paper/p_sh600030.csv,资金 100000)→任务抽屉进度→历史出现新留档
- [ ] 概览:期末资产/净盈亏为 ¥ 金额,曲线金额/净值切换工作
- [ ] K线信号:蜡烛图+进出场标记渲染
- [ ] 交易明细:盈亏额列带口径 tooltip
- [ ] 决策回放:滑块逐 bar,路径表+因子值表有数据
- [ ] 再跑一次不同参数(如 cost 20)→勾两次留档→对比区曲线叠加+指标差表
- [ ] 数据工作台:清单含新鲜度;打开 CSV 出 K线;叠加 sma(close,20) 出线;NaN 段断线
- [ ] universe 表显示 deploy 只读;拉取一只标的任务完成后刷新可见
- [ ] CSP 生效后窗口无样式异常(antd 正常)

- [ ] **Step 4: Commit + 收尾**

```bash
git status --porcelain
git add desktop/src-tauri/tauri.conf.json
git commit -m "feat(desktop): baseline csp (spec 9 m2 clause); m2 gate sweep + smoke passed"
```

REQUIRED SUB-SKILL: `superpowers:finishing-a-development-branch`——全量验证 → 选项 → 合并 master → 删分支。合并前贴近时点 `git log origin/master..master` 与 `git log master..desktop-m2` 查并行提交。

---

## 计划自审记录

- **Spec 覆盖（M2 范围）**：§5.2 配置面板(U1,含 initial_capital 默认 10w=用户确认)/运行即留档(B1,B2)/五视图(U2 概览+明细+原始、U3 K线信号、U4 回放——表格式,DAG 视图按 spec 期序属 M3)/对比(U5)；§5.3 universe 管理(B5,U6)/新鲜度(B5,U6)/批量拉取节流(B5)/K线浏览+DSL 因子叠加(B5,U6 复用 U3 组件)；§4-3 决策回放引擎依赖(E2,打分模式零改动直用现有 Trace)；§4-2 提升不复制(E3)；§7 原子写挂账(E1)+留档目录契约(B1)；§9 CSP 挂账(F1)+路径越界守卫(B5 resolve_under_root/B1 run id 校验/universe name 白名单)。
- **占位符扫描**：无 TBD/TODO；U1 结果区占位是任务间交接产物（U2/U3/U4 逐个换真，F1 冒烟为终验）。
- **类型一致性**：DTO 全集在 B1 一次定义；B2 `execute_backtest(ws,&dyn RunProgress,&BacktestConfigDto)` 与 B5/B6 调用一致；`RunProgress` trait 在 B2 定义、B5 引用 `crate::backtest_run::RunProgress`；test_fixtures(`MINI_TREE/write_bars_csv/fixture_ws/cfg/NoopProgress`) 在 B2 定义 pub(crate)、B3/B4/B6 复用；ipc.ts 在 U1 一次加齐、U2-U6 只消费。`is_valid_run_id` 给出定稿直白版（草稿一行式已显式作废标注）。
- **现场不确定点的处置纪律**（实现者按此行动，不猜）：run()/run_sim 是否自写 out_path（B2 Step1 调研后二选一）；`RiskMetrics.sharpe` Option 与否（B3 注明两种写法）；`SoftReport.tree_name` 缺失时的替代取法（B2 注明）；`Value` 第四变体（B4 注明补臂）；E3 loader 现场结构不符 → STOP。
- **引擎面总闸**：E1/E2/E3 各自带行为零变锁；全计划引擎语义红线与 M1 相同。




