# 客户端重构 — 选股 & 迭代研究台 实现计划 (子项 1/3)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 给桌面客户端新增 `选股` 与 `研究` 两个顶层页,把 `rquant screen`(as-of 选股榜 + 选股回测)、迭代 harness(轮次台账/round card/一键跑轮)与指数相对评估搬上 GUI。

**Architecture:** 沿用现有 Tauri2(同步命令壳 + `TaskRegistry` 线程任务)+ React/antd + ts-rs + Zustand + `api/ipc.ts` 接缝。选股 as-of/回测**直接调 rquant 库**(`tokio` runtime,仿 `backtest_run`,非子进程);跑轮**外壳调 `python scripts/iterate.py`**(verdict 唯一真源);指数相对在 Rust 桥重算(端口 `to_index_relative`,可即时切基准)。新域命令/DTO 进**新文件**(不动现有 `commands.rs`/`dto.rs`)。

**Tech Stack:** Rust 2024 + Tauri 2.11 + ts-rs 10 / React 18 + antd 6 + Vite 8 + Zustand 5 + ECharts 6 + Vitest 4。

## Global Constraints

- DTO 一律 `#[derive(Debug, Clone, Serialize, TS)]` + `#[ts(export)]`(可读回的加 `Deserialize`);ts-rs 导出到 `desktop/src-tauri/bindings/`,前端按 `@bindings/<Name>` 引入。
- 命令是**同步** `#[tauri::command] pub fn`;重计算走 `state.tasks.start(kind, heavy=true, |ctx| -> Result<serde_json::Value,String>)`,返回 task id;库调用用 `tokio::runtime::Runtime::new()?.block_on(...)`。
- 库错误 `.map_err(|e| e.to_string())`;命令返回 `Result<T, String>`。
- **verdict 裁决永远只在 Python `iterate.py`**;Rust/前端只读取 ledger 产物并展示,**不得**二次实现 PASS/FALSIFIED 门槛。`break_even`、`index-relative` 仅是良性算术,可在 Rust 重算。
- 前端 store 持 `api: realApi`,动作调 `get().api.X()`;测试以 `useStore.setState({ api: {...realApi, X: async()=>mock} })` 注入。`invoke<T>("cmd", {args})` 来自 `@tauri-apps/api/core`,集中在 `api/ipc.ts`。
- 路由经 `desktop/ui/src/App.tsx` 的 `MODULES` 数组 + `<Routes>` 注册(HashRouter)。
- 单位铁律(roe/np_yoy/…=百分数;eps/bps=元)、point-in-time 铁律:仅展示,不重算引擎语义。
- git:`git add` 显式列文件(不用 `-A`);commit 前 `git status --porcelain`;**英文** commit(`git commit -F -` heredoc);收尾 `cargo test --workspace` + `cd desktop/ui && npm run build && npx vitest run`。
- 指数 CSV:`data/baostock/index/{csi300,csi500,csi1000}.csv`,列 `time,close`,`time` 形如 `2018-01-02 15:00:00`。

## 文件结构(创建/修改)

**Rust 桥(`desktop/src-tauri/src/`):**
- Create `index_relative.rs` — 纯函数:从 holdings nav + 指数 CSV 重算超额(端口 `to_index_relative`)。
- Create `dto_screen.rs` — 选股相关 DTO。
- Create `dto_iter.rs` — 迭代相关 DTO。
- Create `screen_cmds.rs` — 选股命令(配置枚举/as-of/回测/归档读取/指数相对)。
- Create `screen_runs.rs` — 选股回测归档(meta + gross/net 报告 JSON;仿 `runs.rs`)。
- Create `iter_cmds.rs` — 迭代命令(ledger/round-card/queue/跑轮)。
- Create `iter_read.rs` — 纯函数:解析 `.iter/ledger.jsonl`、`iteration-ledger.md` 队列段、round-card 门槛映射。
- Modify `paths.rs` — 加 screen 配置目录、`.iter` 目录、指数目录、screen-runs 目录、`python_exe()` 探测。
- Modify `dto.rs` — 不动(新 DTO 进新文件)。仅 `lib.rs`/模块声明引入新模块。
- Modify `lib.rs` — `mod` 声明 + `generate_handler!` 追加新命令。
- Modify `error.rs` — 不动(命令直接 `.map_err`)。

**Python harness(`scripts/`):**
- Modify `iterate.py` — 跑完一轮额外写 `.iter/round_<n>.json` sidecar(tier2 cells + 门槛明细;**不改 judge 逻辑**)。

**前端(`desktop/ui/src/`):**
- Modify `App.tsx` — `MODULES` 加 `选股`/`研究`,注册两路由。
- Modify `api/ipc.ts` — 追加新命令方法。
- Modify `labels.ts` / `errors.ts` — 追加术语与报错规则。
- Create `stores/screen.ts` / `stores/research.ts`。
- Create `pages/Screen.tsx` / `pages/Research.tsx`。
- Create `components/ScreenPickTable.tsx`、`ScreenBacktestResult.tsx`、`LedgerTable.tsx`、`RoundCard.tsx`、`RunRoundForm.tsx`(+ 各 `.test.tsx`)。
- 复用 `components/NavChart.tsx`(净值/超额曲线)。

---

## Task 1: 路径助手 + Python 探测

**Files:**
- Modify: `desktop/src-tauri/src/paths.rs`
- Test: 同文件 `#[cfg(test)]`

**Interfaces:**
- Produces: `Workspace::screen_iter_dir()->PathBuf`(`examples/screen/iter`)、`deploy_dir()`(已存在)、`iter_dir()->PathBuf`(`.iter`)、`ledger_jsonl()->PathBuf`(`.iter/ledger.jsonl`)、`ledger_md()->PathBuf`(`docs/superpowers/iteration-ledger.md`)、`index_dir()->PathBuf`(`data/baostock/index`)、`screen_runs_dir()->PathBuf`(`.rquant-desktop/screen_runs`);自由函数 `python_exe()->String`。

- [ ] **Step 1: 加路径方法**(`paths.rs` 的 `impl Workspace` 内,仿现有 `runs_dir`):
```rust
pub fn screen_iter_dir(&self) -> PathBuf { self.root.join("examples").join("screen").join("iter") }
pub fn iter_dir(&self) -> PathBuf { self.root.join(".iter") }
pub fn ledger_jsonl(&self) -> PathBuf { self.iter_dir().join("ledger.jsonl") }
pub fn ledger_md(&self) -> PathBuf { self.root.join("docs").join("superpowers").join("iteration-ledger.md") }
pub fn index_dir(&self) -> PathBuf { self.root.join("data").join("baostock").join("index") }
pub fn screen_runs_dir(&self) -> PathBuf { self.desktop_data_dir().join("screen_runs") }
```

- [ ] **Step 2: 加 Python 探测自由函数**(文件末,`valid_symbol` 旁):
```rust
/// 解析 Python 可执行:优先 env RQUANT_PYTHON,否则 "python"(Windows venv 已在 PATH)。
pub fn python_exe() -> String {
    std::env::var("RQUANT_PYTHON").unwrap_or_else(|_| "python".to_string())
}
```

- [ ] **Step 3: 测试 + 跑**
```rust
#[test]
fn screen_paths_resolve_under_root() {
    let ws = Workspace::detect(&std::env::current_dir().unwrap()).unwrap();
    assert!(ws.ledger_jsonl().ends_with(".iter/ledger.jsonl") || ws.ledger_jsonl().ends_with(".iter\\ledger.jsonl"));
    assert!(ws.index_dir().ends_with("index"));
}
```
Run: `cargo test -p rquant-desktop paths:: -- --nocapture` → PASS(若 `detect` 需在仓库内运行,测试已满足)。

- [ ] **Step 4: Commit**
```bash
git add desktop/src-tauri/src/paths.rs
git commit -F - <<'EOF'
feat(desktop): screen/iter/index path helpers + python_exe probe
EOF
```

---

## Task 2: 指数相对重算模块(纯逻辑,TDD)

**Files:**
- Create: `desktop/src-tauri/src/index_relative.rs`
- Modify: `desktop/src-tauri/src/lib.rs`(加 `mod index_relative;`)
- Test: 同文件

**Interfaces:**
- Consumes: 一条 screen 报告的 holdings(`[{t, nav}]`)+ 指数 CSV。
- Produces:
  - `load_index(path:&Path)->Result<BTreeMap<String,f64>,String>`(键=`time[..10]` 日期)
  - `idx_at(m:&BTreeMap<String,f64>, day:&str)->Option<f64>`(≤day 的最近收盘)
  - `compute(holdings:&[(String,f64)], regimes:&[(String,String,String)], idx:&BTreeMap<String,f64>) -> IndexRel`,其中 `holdings=(day,nav)`、`regimes=(label,from,to)`,返回 `IndexRel { excess_cum:Option<f64>, curve:Vec<(String,f64)>, per_regime:Vec<(String,Option<f64>)> }`。

- [ ] **Step 1: 写失败测试**(`index_relative.rs`):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    fn idx() -> BTreeMap<String, f64> {
        BTreeMap::from([
            ("2024-01-02".into(), 100.0),
            ("2024-06-28".into(), 110.0),
            ("2024-12-31".into(), 120.0),
        ])
    }
    #[test]
    fn excess_is_strat_minus_index() {
        // 组合 1.0→1.5 = +50%;指数 100→120 = +20% → 超额 +30%
        let h = vec![("2024-01-02".into(), 1.0), ("2024-12-31".into(), 1.5)];
        let r = compute(&h, &[], &idx());
        assert!((r.excess_cum.unwrap() - 0.30).abs() < 1e-9);
    }
    #[test]
    fn per_regime_excess_windowed() {
        let h = vec![
            ("2024-01-02".into(), 1.0),
            ("2024-06-28".into(), 1.2),  // 组合 +20% vs 指数 +10% → +10%
            ("2024-12-31".into(), 1.5),
        ];
        let reg = vec![("H1".to_string(), "2024-01-02".to_string(), "2024-06-28".to_string())];
        let r = compute(&h, &reg, &idx());
        assert_eq!(r.per_regime.len(), 1);
        assert!((r.per_regime[0].1.unwrap() - 0.10).abs() < 1e-9);
    }
    #[test]
    fn idx_at_uses_last_on_or_before() {
        let m = idx();
        assert_eq!(idx_at(&m, "2024-03-01"), Some(100.0)); // 取 ≤ 的最近
        assert_eq!(idx_at(&m, "2023-01-01"), None);
    }
}
```

- [ ] **Step 2: 跑测试确认失败**
Run: `cargo test -p rquant-desktop index_relative::` → FAIL(`compute` 未定义)。

- [ ] **Step 3: 实现**(`index_relative.rs` 顶部):
```rust
//! 指数相对超额重算:端口自 scripts/iterate.py::to_index_relative。
//! 仅算术(超额=组合累计 − 指数累计),非裁决——可安全在 Rust 重算以支持即时切基准。
use std::collections::BTreeMap;
use std::path::Path;

pub struct IndexRel {
    pub excess_cum: Option<f64>,
    pub curve: Vec<(String, f64)>,          // (day, 组合累计 − 指数累计)
    pub per_regime: Vec<(String, Option<f64>)>, // (label, 窗口超额)
}

pub fn load_index(path: &Path) -> Result<BTreeMap<String, f64>, String> {
    let txt = std::fs::read_to_string(path).map_err(|e| format!("读指数失败 {}: {e}", path.display()))?;
    let mut m = BTreeMap::new();
    for line in txt.lines().skip(1) {
        let mut it = line.split(',');
        let (Some(t), Some(c)) = (it.next(), it.next()) else { continue };
        if let Ok(v) = c.trim().parse::<f64>() {
            m.insert(t.get(..10).unwrap_or(t).to_string(), v);
        }
    }
    if m.is_empty() { return Err("指数数据为空".into()); }
    Ok(m)
}

pub fn idx_at(m: &BTreeMap<String, f64>, day: &str) -> Option<f64> {
    m.range(..=day.to_string()).next_back().map(|(_, v)| *v)
}

pub fn compute(
    holdings: &[(String, f64)],
    regimes: &[(String, String, String)],
    idx: &BTreeMap<String, f64>,
) -> IndexRel {
    let nav: Vec<(String, f64)> = holdings.iter().filter(|(_, v)| *v > 0.0).cloned().collect();
    if nav.len() < 2 {
        return IndexRel { excess_cum: None, curve: vec![], per_regime: vec![] };
    }
    let base_nav = nav[0].1;
    let base_idx = idx_at(idx, &nav[0].0);
    // 逐点超额曲线:组合相对首日累计 − 指数相对首日累计
    let curve = nav.iter().map(|(d, v)| {
        let strat = v / base_nav - 1.0;
        let ex = match (base_idx, idx_at(idx, d)) {
            (Some(i0), Some(i)) if i0 != 0.0 => strat - (i / i0 - 1.0),
            _ => strat,
        };
        (d.clone(), ex)
    }).collect::<Vec<_>>();
    let excess_cum = curve.last().map(|(_, e)| *e);
    let per_regime = regimes.iter().map(|(label, from, to)| {
        let sub: Vec<&(String, f64)> = nav.iter().filter(|(d, _)| from <= d && d <= to).collect();
        let ex = if sub.len() >= 2 {
            let sr = sub.last().unwrap().1 / sub[0].1 - 1.0;
            match (idx_at(idx, &sub[0].0), idx_at(idx, &sub.last().unwrap().0)) {
                (Some(x0), Some(x1)) if x0 != 0.0 => Some(sr - (x1 / x0 - 1.0)),
                _ => None,
            }
        } else { None };
        (label.clone(), ex)
    }).collect();
    IndexRel { excess_cum, curve, per_regime }
}
```
加 `mod index_relative;` 到 `lib.rs` 模块声明区。

- [ ] **Step 4: 跑测试确认通过**
Run: `cargo test -p rquant-desktop index_relative::` → PASS(3 测试)。

- [ ] **Step 5: Commit**
```bash
git add desktop/src-tauri/src/index_relative.rs desktop/src-tauri/src/lib.rs
git commit -F - <<'EOF'
feat(desktop): index-relative excess recompute (port of to_index_relative)
EOF
```

---

## Task 3: 选股 DTO

**Files:**
- Create: `desktop/src-tauri/src/dto_screen.rs`
- Modify: `desktop/src-tauri/src/lib.rs`(加 `mod dto_screen;`)

**Interfaces:**
- Produces(均 `#[derive(Debug,Clone,Serialize,TS)] #[ts(export)]`,可读回的加 `Deserialize`):

- [ ] **Step 1: 写 DTO**(`dto_screen.rs`):
```rust
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ScreenConfigDto { pub path: String, pub name: Option<String>, pub frozen: bool, pub error: Option<String> }

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ScreenReasonDto { pub tree: String, pub leaf: String, pub score: f64 }

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ScreenPickDto {
    pub rank: usize, pub symbol: String,
    pub quality_score: f64, pub speculative_score: f64, pub combined_score: f64,
    pub tags: Vec<String>, pub selected: bool, pub reasons: Vec<ScreenReasonDto>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ScreenResultDto {
    pub config: String, pub as_of: String,
    pub n_universe: usize, pub top: usize, pub rows: Vec<ScreenPickDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ScreenRunMetaDto {
    pub id: String, pub config: String, pub from: String, pub to: String,
    pub top: u32, pub rebalance: u32, pub created: String, pub ok: bool, pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct NavPointDto { pub t: String, pub nav: f64, pub benchmark_nav: f64 }
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TagAttribDto { pub tag: String, pub n_picks: usize, pub hit_rate: f64, pub mean_fwd_return: f64 }
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RegimeSliceDto { pub label: String, pub from: String, pub to: String, pub picks_return: f64, pub benchmark_return: f64, pub excess: f64 }
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct QualityLayerDto { pub layer: usize, pub n: usize, pub mean_quality: f64, pub mean_fwd_return: f64 }

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ScreenBacktestReportDto {
    pub meta: ScreenRunMetaDto,
    pub net_total_return: f64, pub gross_total_return: f64,
    pub abs_sharpe: Option<f64>, pub max_drawdown: f64, pub turnover: f64,
    pub break_even: Option<f64>,
    pub nav: Vec<NavPointDto>,
    pub tag_attribution: Vec<TagAttribDto>,
    pub regime_slices: Vec<RegimeSliceDto>,
    pub quality_layers: Vec<QualityLayerDto>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ExcessPointDto { pub t: String, pub excess: f64 }
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RegimeExcessDto { pub label: String, pub excess: Option<f64> }
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct IndexRelativeDto {
    pub benchmark: String, pub excess_cum: Option<f64>,
    pub curve: Vec<ExcessPointDto>, pub per_regime: Vec<RegimeExcessDto>,
}
```

- [ ] **Step 2: 声明模块 + 生成 bindings**
加 `mod dto_screen;` 到 `lib.rs`。Run: `cargo test -p rquant-desktop export_bindings` (ts-rs 导出测试)→ 确认 `desktop/src-tauri/bindings/ScreenResultDto.ts` 等生成。
Expected: 新 `.ts` 文件出现在 `bindings/`。

- [ ] **Step 3: Commit**
```bash
git add desktop/src-tauri/src/dto_screen.rs desktop/src-tauri/src/lib.rs desktop/src-tauri/bindings/
git commit -F - <<'EOF'
feat(desktop): screen DTOs + ts-rs bindings
EOF
```

---

## Task 4: 迭代 DTO

**Files:**
- Create: `desktop/src-tauri/src/dto_iter.rs`
- Modify: `desktop/src-tauri/src/lib.rs`(加 `mod dto_iter;`)

- [ ] **Step 1: 写 DTO**(`dto_iter.rs`):
```rust
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 镜像 .iter/ledger.jsonl 的一行;数值键缺省为 None(老轮次可能缺)。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LedgerRoundDto {
    pub round: i64, pub label: String,
    #[serde(default)] pub axis: String,
    #[serde(default)] pub note: String,
    #[serde(default)] pub benchmark: String,
    #[serde(default = "one")] pub rebalance: i64,
    pub verdict: String,
    #[serde(default)] pub flags: Vec<String>,
    #[serde(default)] pub gross_ex: Option<f64>,
    #[serde(default)] pub net_ex: Option<f64>,
    #[serde(default)] pub net_oos_ex: Option<f64>,
    #[serde(default)] pub net_train_ex: Option<f64>,
    #[serde(default)] pub net_sharpe: Option<f64>,
    #[serde(default)] pub break_even: Option<f64>,
}
fn one() -> i64 { 1 }

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct GateDto { pub name: String, pub pass: bool, pub value: Option<f64>, pub threshold: Option<f64>, pub note: String }
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Tier2CellDto { pub top: i64, pub rebalance: i64, pub net_excess: f64 }

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RoundCardDto {
    pub round: i64, pub label: String, pub benchmark: String, pub rebalance: i64,
    pub verdict: String, pub gates: Vec<GateDto>, pub tier2: Vec<Tier2CellDto>,
    pub flags: Vec<String>, pub config_path: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct IterQueueDto { pub queue: Vec<String>, pub falsified: Vec<String> }
```

- [ ] **Step 2: 声明模块 + bindings**:加 `mod dto_iter;`。Run: `cargo test -p rquant-desktop export_bindings` → `bindings/LedgerRoundDto.ts` 等生成。

- [ ] **Step 3: Commit**
```bash
git add desktop/src-tauri/src/dto_iter.rs desktop/src-tauri/src/lib.rs desktop/src-tauri/bindings/
git commit -F - <<'EOF'
feat(desktop): iteration ledger / round-card DTOs + bindings
EOF
```

---

## Task 5: 迭代读取层(纯逻辑,TDD)

**Files:**
- Create: `desktop/src-tauri/src/iter_read.rs`
- Modify: `desktop/src-tauri/src/lib.rs`(加 `mod iter_read;`)
- Test: 同文件

**Interfaces:**
- Consumes: `LedgerRoundDto`(Task 4)、ledger.jsonl 文本、ledger.md 文本、sidecar JSON。
- Produces:
  - `parse_ledger(jsonl:&str)->Vec<LedgerRoundDto>`(逐行 serde,跳过坏行)
  - `parse_queue(md:&str)->IterQueueDto`(取两节标题下的 `•`/`-` 列表项)
  - `gates_from(r:&LedgerRoundDto)->Vec<GateDto>`(由 flags+metrics 映射门槛 ✓/✗,**不重新裁决**,仅展示)
  - `round_card(r:&LedgerRoundDto, tier2:Vec<Tier2CellDto>, config_path:String)->RoundCardDto`

- [ ] **Step 1: 写失败测试**(`iter_read.rs`):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_ledger_skips_bad_lines() {
        let j = r#"{"round":4,"label":"value_pb","verdict":"PASS","flags":[],"net_oos_ex":0.64,"net_sharpe":1.13,"gross_ex":3.1,"break_even":164.0}
not-json
{"round":1,"label":"corr","verdict":"FALSIFIED","flags":["gross-excess<=0"]}"#;
        let v = parse_ledger(j);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].round, 4);
        assert_eq!(v[1].verdict, "FALSIFIED");
    }
    #[test]
    fn gates_map_flags_to_pass_fail_without_rejudging() {
        let r = LedgerRoundDto { round:1, label:"x".into(), axis:String::new(), note:String::new(),
            benchmark:"csi300".into(), rebalance:1, verdict:"FALSIFIED".into(),
            flags: vec!["gross-excess<=0".into(), "break-even<40bps".into()],
            gross_ex:Some(-0.1), net_ex:None, net_oos_ex:Some(0.2), net_train_ex:None, net_sharpe:Some(0.5), break_even:Some(10.0) };
        let g = gates_from(&r);
        let gross = g.iter().find(|x| x.name=="gross 超额>0").unwrap();
        assert!(!gross.pass); // flag 在 → 不过
        let oos = g.iter().find(|x| x.name=="net-OOS 超额>0").unwrap();
        assert!(oos.pass);    // 无对应 flag → 过
        let sf = g.iter().find(|x| x.name=="无 sign-flip").unwrap();
        assert!(sf.pass);     // 无 sign-flip flag → 过
    }
    #[test]
    fn parse_queue_extracts_two_sections() {
        let md = "## 已证伪角度（勿重试）\n- 动量\n- 反转\n\n## 待试角度（候选队列，新 baostock 数据集解锁；Claude 维护）\n- 股息率\n- 低波\n## 其他\n- 噪声\n";
        let q = parse_queue(md);
        assert_eq!(q.falsified, vec!["动量", "反转"]);
        assert_eq!(q.queue, vec!["股息率", "低波"]);
    }
}
```

- [ ] **Step 2: 跑确认失败**
Run: `cargo test -p rquant-desktop iter_read::` → FAIL。

- [ ] **Step 3: 实现**(`iter_read.rs`):
```rust
//! 迭代 ledger 只读解析:JSONL 轮次、md 队列段、门槛展示映射。
//! gates_from 仅把 Python judge 已下的 flags/metrics 映射成展示行,绝不在 Rust 重新裁决。
use crate::dto_iter::{GateDto, IterQueueDto, LedgerRoundDto, RoundCardDto, Tier2CellDto};

pub fn parse_ledger(jsonl: &str) -> Vec<LedgerRoundDto> {
    jsonl.lines().filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<LedgerRoundDto>(l).ok())
        .collect()
}

pub fn parse_queue(md: &str) -> IterQueueDto {
    fn items_after(md: &str, marker: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut in_sec = false;
        for line in md.lines() {
            if line.starts_with("## ") {
                in_sec = line.contains(marker);
                continue;
            }
            if in_sec {
                let t = line.trim_start_matches(['-', '•', '*', ' ']).trim();
                if !t.is_empty() && (line.trim_start().starts_with('-') || line.trim_start().starts_with('•') || line.trim_start().starts_with('*')) {
                    out.push(t.to_string());
                }
            }
        }
        out
    }
    IterQueueDto { falsified: items_after(md, "已证伪角度"), queue: items_after(md, "待试角度") }
}

pub fn gates_from(r: &LedgerRoundDto) -> Vec<GateDto> {
    let has = |f: &str| r.flags.iter().any(|x| x == f);
    let be_flag = r.flags.iter().any(|x| x.starts_with("break-even<"));
    vec![
        GateDto { name: "gross 超额>0".into(), pass: !has("gross-excess<=0"), value: r.gross_ex, threshold: Some(0.0), note: "源头有 alpha".into() },
        GateDto { name: "net-OOS 超额>0".into(), pass: !has("net-OOS<=0"), value: r.net_oos_ex, threshold: Some(0.0), note: "金标准".into() },
        GateDto { name: "net Sharpe>0".into(), pass: !has("net-sharpe<=0"), value: r.net_sharpe, threshold: Some(0.0), note: String::new() },
        GateDto { name: "break-even≥40bps".into(), pass: !be_flag, value: r.break_even, threshold: Some(40.0), note: "≥2×成本".into() },
        GateDto { name: "无 sign-flip".into(), pass: !has("sign-flip"), value: None, threshold: None, note: "Tier-2 敏感扫".into() },
    ]
}

pub fn round_card(r: &LedgerRoundDto, tier2: Vec<Tier2CellDto>, config_path: String) -> RoundCardDto {
    RoundCardDto {
        round: r.round, label: r.label.clone(), benchmark: r.benchmark.clone(),
        rebalance: r.rebalance, verdict: r.verdict.clone(),
        gates: gates_from(r), tier2, flags: r.flags.clone(), config_path,
    }
}
```
加 `mod iter_read;` 到 `lib.rs`。

- [ ] **Step 4: 跑确认通过**
Run: `cargo test -p rquant-desktop iter_read::` → PASS(3 测试)。

- [ ] **Step 5: Commit**
```bash
git add desktop/src-tauri/src/iter_read.rs desktop/src-tauri/src/lib.rs
git commit -F - <<'EOF'
feat(desktop): iteration ledger/queue/round-card readers (display-only, no re-judge)
EOF
```

---

## Task 6: 选股回测归档(仿 runs.rs)

**Files:**
- Create: `desktop/src-tauri/src/screen_runs.rs`
- Modify: `desktop/src-tauri/src/lib.rs`(加 `mod screen_runs;`)

**Interfaces:**
- Produces: `new_id()->String`、`run_dir(ws,id)->PathBuf`、`write_meta(ws,&ScreenRunMetaDto)`、`write_report(ws,id,kind:&str,json:&str)`(kind=`net`/`gross`)、`read_meta(ws,id)->Result<ScreenRunMetaDto,String>`、`read_report(ws,id,kind)->Result<serde_json::Value,String>`、`list_meta(ws)->Vec<ScreenRunMetaDto>`。复用 `runs.rs` 的原子写习惯。

- [ ] **Step 1: 实现**(`screen_runs.rs`,仿 `runs.rs` 的 `write_json_atomic`/`run_paths`):
```rust
use crate::dto_screen::ScreenRunMetaDto;
use crate::paths::Workspace;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
static SEQ: AtomicU64 = AtomicU64::new(0);

pub fn new_id() -> String {
    let now = chrono::Local::now().naive_local();
    let seq = SEQ.fetch_add(1, Ordering::Relaxed) % 100;
    format!("scr-{}-{:02}", now.format("%Y%m%d-%H%M%S"), seq)
}
pub fn run_dir(ws: &Workspace, id: &str) -> PathBuf { ws.screen_runs_dir().join(id) }

fn write_atomic(path: &Path, s: &str) -> Result<(), String> {
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, s).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}
pub fn write_meta(ws: &Workspace, m: &ScreenRunMetaDto) -> Result<(), String> {
    write_atomic(&run_dir(ws, &m.id).join("meta.json"), &serde_json::to_string_pretty(m).map_err(|e| e.to_string())?)
}
pub fn write_report(ws: &Workspace, id: &str, kind: &str, json: &str) -> Result<(), String> {
    write_atomic(&run_dir(ws, id).join(format!("{kind}.json")), json)
}
pub fn read_meta(ws: &Workspace, id: &str) -> Result<ScreenRunMetaDto, String> {
    let s = std::fs::read_to_string(run_dir(ws, id).join("meta.json")).map_err(|e| e.to_string())?;
    serde_json::from_str(&s).map_err(|e| e.to_string())
}
pub fn read_report(ws: &Workspace, id: &str, kind: &str) -> Result<serde_json::Value, String> {
    let s = std::fs::read_to_string(run_dir(ws, id).join(format!("{kind}.json"))).map_err(|e| e.to_string())?;
    serde_json::from_str(&s).map_err(|e| e.to_string())
}
pub fn list_meta(ws: &Workspace) -> Vec<ScreenRunMetaDto> {
    let dir = ws.screen_runs_dir();
    let Ok(rd) = std::fs::read_dir(&dir) else { return vec![] };
    let mut out: Vec<ScreenRunMetaDto> = rd.filter_map(|e| e.ok())
        .filter_map(|e| read_meta(ws, e.file_name().to_str()?).ok()).collect();
    out.sort_by(|a, b| b.created.cmp(&a.created));
    out
}
```

- [ ] **Step 2: 编译**
Run: `cargo build -p rquant-desktop` → OK。加 `mod screen_runs;` 到 `lib.rs`。

- [ ] **Step 3: Commit**
```bash
git add desktop/src-tauri/src/screen_runs.rs desktop/src-tauri/src/lib.rs
git commit -F - <<'EOF'
feat(desktop): screen backtest run archive (meta + gross/net reports)
EOF
```

---

## Task 7: 选股命令(配置枚举 / as-of / 回测 / 报告 / 指数相对)

**Files:**
- Create: `desktop/src-tauri/src/screen_cmds.rs`
- Modify: `desktop/src-tauri/src/lib.rs`(加 `mod screen_cmds;` + 注册命令)

**Interfaces:**
- Consumes: `AppState`(`commands::AppState`,字段 `ws`/`tasks`)、Task1 路径、Task2 index_relative、Task3 DTO、Task6 归档。
- Produces 命令:`screen_configs_list`、`screen_asof`、`screen_backtest_run`、`screen_runs_list`、`screen_run_report`、`index_list`、`screen_index_relative`。

- [ ] **Step 1: 配置枚举 + 指数列表(同步命令)**(`screen_cmds.rs`):
```rust
use crate::commands::AppState;
use crate::dto_screen::*;
use std::path::PathBuf;

#[tauri::command]
pub fn screen_configs_list(state: tauri::State<AppState>) -> Vec<ScreenConfigDto> {
    let mut out = Vec::new();
    for (dir, frozen) in [(state.ws.screen_iter_dir(), false), (state.ws.deploy_dir(), true)] {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("yaml") { continue }
            let rel = p.strip_prefix(state.ws.root()).unwrap_or(&p).to_string_lossy().replace('\\', "/");
            let (name, error) = match std::fs::read_to_string(&p)
                .ok().and_then(|s| serde_yaml::from_str::<serde_yaml::Value>(&s).ok()) {
                Some(_) => (p.file_stem().and_then(|s| s.to_str()).map(String::from), None),
                None => (None, Some("配置解析失败".to_string())),
            };
            out.push(ScreenConfigDto { path: rel, name, frozen, error });
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

#[tauri::command]
pub fn index_list(state: tauri::State<AppState>) -> Vec<String> {
    let dir = state.ws.index_dir();
    let Ok(rd) = std::fs::read_dir(&dir) else { return vec![] };
    let mut v: Vec<String> = rd.flatten()
        .filter_map(|e| { let p = e.path();
            (p.extension().and_then(|s| s.to_str()) == Some("csv"))
                .then(|| p.file_stem().and_then(|s| s.to_str()).map(String::from)).flatten() })
        .collect();
    v.sort();
    v
}
```

- [ ] **Step 2: as-of 选股命令(task,调库)**:
```rust
#[tauri::command]
pub fn screen_asof(state: tauri::State<AppState>, config: String, as_of: String, top: u32) -> Result<String, String> {
    let ws = state.ws.clone();
    state.tasks.start("screen_asof", true, move |ctx| {
        ctx.progress(0.1, "load", &config);
        let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
        let llm = rquant::cli::build_llm(String::new(), String::new(), ws.root().join(".rquant-cache").join("llm")).map_err(|e| e.to_string())?;
        let cfg = rquant::screen::ScreenRunConfig {
            config_path: ws.root().join(&config),
            universe_path: ws.root().join("data/baostock/universe_baostock_day.csv"),
            as_of: chrono::NaiveDate::parse_from_str(&as_of, "%Y-%m-%d").ok(),
            top: Some(top as usize), window: 260, out_path: None,
            membership_path: None, sectors_path: None,
        };
        ctx.progress(0.4, "screen", "");
        let res = rt.block_on(rquant::screen::run_screen(&cfg, &llm)).map_err(|e| e.to_string())?;
        let rows = res.rows.iter().map(|r| ScreenPickDto {
            rank: r.rank, symbol: r.symbol.clone(),
            quality_score: r.quality_score, speculative_score: r.speculative_score, combined_score: r.combined_score,
            tags: r.tags.clone(), selected: r.selected,
            reasons: r.reasons.iter().map(|x| ScreenReasonDto { tree: x.tree.clone(), leaf: x.leaf.clone(), score: x.score }).collect(),
        }).collect();
        let dto = ScreenResultDto { config, as_of: res.as_of.format("%Y-%m-%d").to_string(),
            n_universe: res.n_universe, top: res.top, rows };
        serde_json::to_value(dto).map_err(|e| e.to_string())
    })
}
```
*注:逐股"逐树打分"由 `ScreenPickDto.reasons`(tree/leaf/score)直接提供,前端展开行渲染即可——无需单独 `screen_pick_detail` 命令(YAGNI,完整决策路径 replay 留 future)。*

- [ ] **Step 3: 选股回测命令(task,gross+net 两跑,归档)**:
```rust
#[tauri::command]
pub fn screen_backtest_run(state: tauri::State<AppState>, config: String, from: String, to: String, top: u32, rebalance: u32, cost_bps: f64) -> Result<String, String> {
    let ws = state.ws.clone();
    state.tasks.start("screen_backtest", true, move |ctx| {
        let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
        let llm = rquant::cli::build_llm(String::new(), String::new(), ws.root().join(".rquant-cache").join("llm")).map_err(|e| e.to_string())?;
        let mk = |cost: f64| rquant::screen::backtest::ScreenBacktestConfig {
            config_path: ws.root().join(&config),
            universe_path: ws.root().join("data/baostock/universe_baostock_day.csv"),
            from: chrono::NaiveDate::parse_from_str(&from, "%Y-%m-%d").ok(),
            to: chrono::NaiveDate::parse_from_str(&to, "%Y-%m-%d").ok(),
            rebalance: rebalance as usize, top: Some(top as usize),
            warmup: 260, window: 260, cost_bps: cost, soft: false,
            out_path: None, membership_path: None, sectors_path: None,
        };
        let id = crate::screen_runs::new_id();
        ctx.progress(0.2, "gross", "cost=0");
        let gross = rt.block_on(rquant::screen::backtest::run_screen_backtest(&mk(0.0), &llm)).map_err(|e| e.to_string())?;
        if ctx.cancelled() { return Err("cancelled".into()); }
        ctx.progress(0.6, "net", &format!("cost={cost_bps}"));
        let net = rt.block_on(rquant::screen::backtest::run_screen_backtest(&mk(cost_bps), &llm)).map_err(|e| e.to_string())?;
        let created = chrono::Local::now().naive_local().format("%Y-%m-%dT%H:%M:%S").to_string();
        let meta = ScreenRunMetaDto { id: id.clone(), config: config.clone(), from: from.clone(), to: to.clone(),
            top, rebalance, created, ok: true, error: None };
        crate::screen_runs::write_meta(&ws, &meta)?;
        crate::screen_runs::write_report(&ws, &id, "gross", &serde_json::to_string(&gross).map_err(|e| e.to_string())?)?;
        crate::screen_runs::write_report(&ws, &id, "net", &serde_json::to_string(&net).map_err(|e| e.to_string())?)?;
        ctx.progress(0.95, "archive", &id);
        Ok(serde_json::json!({ "run_id": id }))
    })
}
```

- [ ] **Step 4: 报告读取 + 指数相对(同步命令)**:
```rust
#[tauri::command]
pub fn screen_runs_list(state: tauri::State<AppState>) -> Vec<ScreenRunMetaDto> { crate::screen_runs::list_meta(&state.ws) }

#[tauri::command]
pub fn screen_run_report(state: tauri::State<AppState>, id: String) -> Result<ScreenBacktestReportDto, String> {
    let meta = crate::screen_runs::read_meta(&state.ws, &id)?;
    let net = crate::screen_runs::read_report(&state.ws, &id, "net")?;
    let gross = crate::screen_runs::read_report(&state.ws, &id, "gross")?;
    let f = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_f64());
    let net_total = f(&net, "total_return").unwrap_or(0.0);
    let gross_total = f(&gross, "total_return").unwrap_or(0.0);
    // break-even = cost·gross/(gross−net),仅 gross>0 且有衰减时有意义(良性算术,非裁决)
    let cost = 20.0; // 展示用单边成本基点;与 harness 一致
    let break_even = if gross_total > 0.0 && gross_total > net_total {
        Some(cost * gross_total / (gross_total - net_total)) } else { None };
    let nav = net.get("holdings").and_then(|h| h.as_array()).map(|a| a.iter().map(|h| NavPointDto {
        t: h.get("t").and_then(|x| x.as_str()).unwrap_or("").chars().take(10).collect(),
        nav: h.get("nav").and_then(|x| x.as_f64()).unwrap_or(0.0),
        benchmark_nav: h.get("benchmark_nav").and_then(|x| x.as_f64()).unwrap_or(0.0),
    }).collect()).unwrap_or_default();
    let tag_attribution = serde_json::from_value(net.get("tag_attribution").cloned().unwrap_or(serde_json::json!([]))).unwrap_or_default();
    let regime_slices = serde_json::from_value(net.get("regime_slices").cloned().unwrap_or(serde_json::json!([]))).unwrap_or_default();
    let quality_layers = serde_json::from_value(net.get("quality_layers").cloned().unwrap_or(serde_json::json!([]))).unwrap_or_default();
    Ok(ScreenBacktestReportDto {
        meta, net_total_return: net_total, gross_total_return: gross_total,
        abs_sharpe: net.get("risk").and_then(|r| r.get("sharpe")).and_then(|x| x.as_f64()),
        max_drawdown: f(&net, "max_drawdown").unwrap_or(0.0), turnover: f(&net, "turnover").unwrap_or(0.0),
        break_even, nav, tag_attribution, regime_slices, quality_layers,
    })
}

#[tauri::command]
pub fn screen_index_relative(state: tauri::State<AppState>, id: String, benchmark: String) -> Result<IndexRelativeDto, String> {
    let net = crate::screen_runs::read_report(&state.ws, &id, "net")?;
    let holdings: Vec<(String, f64)> = net.get("holdings").and_then(|h| h.as_array()).map(|a| a.iter().filter_map(|h| {
        Some((h.get("t")?.as_str()?.chars().take(10).collect(), h.get("nav")?.as_f64()?))
    }).collect()).unwrap_or_default();
    let regimes: Vec<(String, String, String)> = net.get("regime_slices").and_then(|s| s.as_array()).map(|a| a.iter().filter_map(|s| {
        Some((s.get("label")?.as_str()?.to_string(), s.get("from")?.as_str()?.to_string(), s.get("to")?.as_str()?.to_string()))
    }).collect()).unwrap_or_default();
    let idx = crate::index_relative::load_index(&state.ws.index_dir().join(format!("{benchmark}.csv")))?;
    let r = crate::index_relative::compute(&holdings, &regimes, &idx);
    Ok(IndexRelativeDto {
        benchmark, excess_cum: r.excess_cum,
        curve: r.curve.into_iter().map(|(t, excess)| ExcessPointDto { t, excess }).collect(),
        per_regime: r.per_regime.into_iter().map(|(label, excess)| RegimeExcessDto { label, excess }).collect(),
    })
}
```

- [ ] **Step 5: 注册 + 编译**:`lib.rs` 加 `mod screen_cmds;` 与 `generate_handler!` 追加 7 命令(`screen_cmds::screen_configs_list` 等)。
Run: `cargo build -p rquant-desktop` → OK。

- [ ] **Step 6: 冒烟测试(真数据)**:临时单测或手测 —— 启动应用后调 `screen_configs_list` 应列出 9 个 iter 配置;`index_list` 应返回 `[csi300,csi500,csi1000]`。Run: `cargo test -p rquant-desktop` → 既有测试全绿。

- [ ] **Step 7: Commit**
```bash
git add desktop/src-tauri/src/screen_cmds.rs desktop/src-tauri/src/lib.rs
git commit -F - <<'EOF'
feat(desktop): screen commands (configs/as-of/backtest gross+net/report/index-relative)
EOF
```

---

## Task 8: iterate.py round sidecar(持久化 tier2 + 配置路径)

**Files:**
- Modify: `scripts/iterate.py`
- Test: `scripts/test_iterate.py`

**Interfaces:**
- Consumes: 现有 `judge` 返回的 metrics/flags、`tier2_sweep` 的扫描结果、`append_ledger` 的轮号。
- Produces: 文件 `.iter/round_<round>.json`,内容 `{round, label, benchmark, rebalance, config_path, tier2:[{top,rebalance,net_excess}]}`。**不改 judge/裁决**。

- [ ] **Step 1: 写失败测试**(`scripts/test_iterate.py` 加):
```python
def test_round_sidecar_shape(tmp_path):
    import iterate
    cells = [{"top": 50, "rebalance": 1, "net_excess": 0.64}]
    p = iterate.write_round_sidecar(tmp_path, 4, "value_pb", "csi300", 1, "examples/screen/iter/value_pb_base.yaml", cells)
    import json
    d = json.loads(open(p, encoding="utf-8").read())
    assert d["round"] == 4 and d["config_path"].endswith("value_pb_base.yaml")
    assert d["tier2"][0]["net_excess"] == 0.64
```

- [ ] **Step 2: 跑确认失败**
Run: `python -m pytest scripts/test_iterate.py::test_round_sidecar_shape -q` → FAIL(`write_round_sidecar` 不存在)。

- [ ] **Step 3: 实现**(`iterate.py` 加纯函数,并在 `main` 跑完一轮后调用):
```python
def write_round_sidecar(iter_dir, rnd, label, bench, reb, config_path, tier2_cells):
    """写 .iter/round_<rnd>.json 供 GUI round card 读取(tier2 cells + 配置路径)。纯持久化,不影响裁决。"""
    import json, os
    os.makedirs(iter_dir, exist_ok=True)
    path = os.path.join(iter_dir, f"round_{rnd}.json")
    rec = {"round": rnd, "label": label, "benchmark": bench or "EW", "rebalance": reb,
           "config_path": config_path, "tier2": tier2_cells}
    with open(path, "w", encoding="utf-8") as fp:
        json.dump(rec, fp, ensure_ascii=False, indent=2)
    return path
```
在 `main()` 中 `append_ledger(...)` 之后,用 tier2 扫描结果(若 Tier-2 已跑,收集成 `[{"top":t,"rebalance":r,"net_excess":x}]`;未跑则空列表)调用 `write_round_sidecar(os.path.dirname(LEDGER_JSONL), rnd, label, args.config, bench, args.benchmark, reb, cells)`。配置路径用 `args.config`(传入的 yaml 相对路径)。

- [ ] **Step 4: 跑确认通过**
Run: `python -m pytest scripts/test_iterate.py -q` → 全 PASS(含新测试)。

- [ ] **Step 5: Commit**
```bash
git add scripts/iterate.py scripts/test_iterate.py
git commit -F - <<'EOF'
feat(iterate): persist per-round sidecar (tier2 cells + config path) for GUI round card
EOF
```

---

## Task 9: 迭代命令(ledger / round-card / queue / 跑轮)

**Files:**
- Create: `desktop/src-tauri/src/iter_cmds.rs`
- Modify: `desktop/src-tauri/src/lib.rs`(加 `mod iter_cmds;` + 注册)

**Interfaces:**
- Consumes: Task1 路径、Task4 DTO、Task5 读取层、Task8 sidecar、`python_exe`、`TaskRegistry`。
- Produces 命令:`iter_ledger`、`iter_queue`、`iter_round_card`、`iter_run_round`。

- [ ] **Step 1: 只读命令**(`iter_cmds.rs`):
```rust
use crate::commands::AppState;
use crate::dto_iter::*;

#[tauri::command]
pub fn iter_ledger(state: tauri::State<AppState>) -> Vec<LedgerRoundDto> {
    let txt = std::fs::read_to_string(state.ws.ledger_jsonl()).unwrap_or_default();
    let mut v = crate::iter_read::parse_ledger(&txt);
    v.sort_by(|a, b| b.round.cmp(&a.round)); // 新轮在前
    v
}

#[tauri::command]
pub fn iter_queue(state: tauri::State<AppState>) -> IterQueueDto {
    let md = std::fs::read_to_string(state.ws.ledger_md()).unwrap_or_default();
    crate::iter_read::parse_queue(&md)
}

#[tauri::command]
pub fn iter_round_card(state: tauri::State<AppState>, round: i64) -> Result<RoundCardDto, String> {
    let txt = std::fs::read_to_string(state.ws.ledger_jsonl()).map_err(|e| e.to_string())?;
    let r = crate::iter_read::parse_ledger(&txt).into_iter().find(|x| x.round == round)
        .ok_or_else(|| format!("ledger 无轮次 {round}"))?;
    // sidecar:tier2 cells + 配置路径(老轮次可能缺)
    let side = std::fs::read_to_string(state.ws.iter_dir().join(format!("round_{round}.json"))).ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
    let tier2: Vec<Tier2CellDto> = side.as_ref()
        .and_then(|v| v.get("tier2").cloned())
        .and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
    let config_path = side.as_ref().and_then(|v| v.get("config_path")).and_then(|x| x.as_str())
        .map(String::from)
        .unwrap_or_else(|| format!("examples/screen/iter/{}.yaml", r.label));
    Ok(crate::iter_read::round_card(&r, tier2, config_path))
}
```

- [ ] **Step 2: 跑轮命令(spawn Python 子进程)**:
```rust
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

#[tauri::command]
pub fn iter_run_round(state: tauri::State<AppState>, config: String, note: String, axis: String, top: u32, benchmark: String, rebalance: u32) -> Result<String, String> {
    let ws = state.ws.clone();
    state.tasks.start("iter_round", true, move |ctx| {
        let mut cmd = Command::new(crate::paths::python_exe());
        cmd.current_dir(ws.root())
            .arg("scripts/iterate.py").arg(&config)
            .arg("--note").arg(&note)
            .arg("--axis").arg(&axis)
            .arg("--top").arg(top.to_string())
            .arg("--benchmark").arg(&benchmark)
            .arg("--rebalance").arg(rebalance.to_string())
            .stdout(Stdio::piped()).stderr(Stdio::piped());
        ctx.progress(0.05, "spawn", "iterate.py");
        let mut child = cmd.spawn().map_err(|e| format!("启动 Python 失败(确认已装 Python 与依赖): {e}"))?;
        // 流式读 stdout 当进度细节(round card 由 iterate.py 打到 stdout)
        if let Some(out) = child.stdout.take() {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if ctx.cancelled() { let _ = child.kill(); return Err("cancelled".into()); }
                ctx.progress(0.5, "run", &line);
            }
        }
        let status = child.wait().map_err(|e| e.to_string())?;
        if !status.success() {
            let err = child.stderr.take().map(|e| {
                let mut s = String::new(); let _ = BufReader::new(e).read_line(&mut s); s
            }).unwrap_or_default();
            return Err(format!("iterate.py 退出码 {:?}: {err}", status.code()));
        }
        // 读 ledger 尾行返回新轮次
        let txt = std::fs::read_to_string(ws.ledger_jsonl()).unwrap_or_default();
        let last = crate::iter_read::parse_ledger(&txt).into_iter().max_by_key(|r| r.round);
        ctx.progress(0.98, "done", "");
        Ok(serde_json::to_value(last).map_err(|e| e.to_string())?)
    })
}
```

- [ ] **Step 3: 注册 + 编译**:`lib.rs` 加 `mod iter_cmds;` + `generate_handler!` 追加 4 命令。
Run: `cargo build -p rquant-desktop && cargo test -p rquant-desktop` → OK + 既有测试全绿。

- [ ] **Step 4: Commit**
```bash
git add desktop/src-tauri/src/iter_cmds.rs desktop/src-tauri/src/lib.rs
git commit -F - <<'EOF'
feat(desktop): iteration commands (ledger/queue/round-card readers + iterate.py round launcher)
EOF
```

---

## Task 10: 前端 api/ipc + 路由 + labels/errors

**Files:**
- Modify: `desktop/ui/src/api/ipc.ts`、`App.tsx`、`labels.ts`、`errors.ts`

**Interfaces:**
- Produces:`api.screenConfigsList()` 等 11 方法;路由 `/screen`、`/research`;术语 + 报错规则。

- [ ] **Step 1: api/ipc.ts 追加方法**(在 `export const api = {` 内,仿现有):
```typescript
  // 选股
  screenConfigsList: () => invoke<import("@bindings/ScreenConfigDto").ScreenConfigDto[]>("screen_configs_list"),
  indexList: () => invoke<string[]>("index_list"),
  screenAsof: (config: string, asOf: string, top: number) => invoke<string>("screen_asof", { config, asOf, top }),
  screenBacktestRun: (config: string, from: string, to: string, top: number, rebalance: number, costBps: number) =>
    invoke<string>("screen_backtest_run", { config, from, to, top, rebalance, costBps }),
  screenRunsList: () => invoke<import("@bindings/ScreenRunMetaDto").ScreenRunMetaDto[]>("screen_runs_list"),
  screenRunReport: (id: string) => invoke<import("@bindings/ScreenBacktestReportDto").ScreenBacktestReportDto>("screen_run_report", { id }),
  screenIndexRelative: (id: string, benchmark: string) => invoke<import("@bindings/IndexRelativeDto").IndexRelativeDto>("screen_index_relative", { id, benchmark }),
  // 迭代
  iterLedger: () => invoke<import("@bindings/LedgerRoundDto").LedgerRoundDto[]>("iter_ledger"),
  iterQueue: () => invoke<import("@bindings/IterQueueDto").IterQueueDto>("iter_queue"),
  iterRoundCard: (round: number) => invoke<import("@bindings/RoundCardDto").RoundCardDto>("iter_round_card", { round }),
  iterRunRound: (config: string, note: string, axis: string, top: number, benchmark: string, rebalance: number) =>
    invoke<string>("iter_run_round", { config, note, axis, top, benchmark, rebalance }),
```
*注:Tauri 把 snake_case 命令参数按 camelCase 自动映射(`as_of`↔`asOf`、`cost_bps`↔`costBps`),与现有命令一致。*

- [ ] **Step 2: App.tsx 注册两页**:`MODULES` 在 `data` 后插入 `{ key: "screen", label: "选股" }, { key: "research", label: "研究" }`;import `Screen`/`Research`;`<Routes>` 加 `<Route path="/screen" element={<Screen />} />` 与 `<Route path="/research" element={<Research />} />`;并把这两 key 从 placeholder 过滤列表排除(改 `.filter` 条件追加 `&& m.key !== "screen" && m.key !== "research"`)。

- [ ] **Step 3: labels.ts 追加**:
```typescript
export const VERDICT_ZH: Record<string, string> = { PASS: "通过", FALSIFIED: "证伪" };
export const SCREEN_TERM = {
  combined: "综合分", quality: "质量分", speculative: "投机分", excess: "超额",
  oos: "OOS 超额", breakEven: "盈亏平衡", indexRel: "指数相对", ewRef: "等权 · 不可投·参考",
} as const;
```

- [ ] **Step 4: errors.ts 追加规则**(在 `RULES` 数组,放网络规则前):
```typescript
  [/python|iterate\.py|no module|modulenotfound/i, "未找到 Python 或 harness 依赖（确认已装 Python 与依赖）"],
  [/index|csi\d+|指数数据/i, "缺少基准指数数据（运行 scripts/fetch_index.py）"],
  [/universe|无可选标的|empty/i, "该日无可选标的（检查成分/数据范围）"],
```

- [ ] **Step 5: 编译验证**
Run: `cd desktop/ui && npx tsc --noEmit` → 0 error(bindings 已存在;`Screen`/`Research` 占位组件下一任务建,本步可先建空组件占位 `export default function Screen(){return null}` 以过编译)。

- [ ] **Step 6: Commit**
```bash
git add desktop/ui/src/api/ipc.ts desktop/ui/src/App.tsx desktop/ui/src/labels.ts desktop/ui/src/errors.ts
git commit -F - <<'EOF'
feat(ui): wire screen/iter IPC methods, routes, labels & error rules
EOF
```

---

## Task 11: 选股 store + 研究 store

**Files:**
- Create: `desktop/ui/src/stores/screen.ts`、`stores/research.ts`
- Test: `stores/screen.test.ts`

**Interfaces:**
- Produces:`useScreen`(configs/asof/backtest/report/indexRel 状态 + 动作)、`useResearch`(ledger/queue/roundCard/runRound)。

- [ ] **Step 1: 写 screen store**(`stores/screen.ts`,仿 `stores/backtest.ts`):
```typescript
import { create } from "zustand";
import type { ScreenConfigDto } from "@bindings/ScreenConfigDto";
import type { ScreenResultDto } from "@bindings/ScreenResultDto";
import type { ScreenRunMetaDto } from "@bindings/ScreenRunMetaDto";
import type { ScreenBacktestReportDto } from "@bindings/ScreenBacktestReportDto";
import type { IndexRelativeDto } from "@bindings/IndexRelativeDto";
import { api as realApi, type Api } from "../api/ipc";
import { friendlyError } from "../errors";

interface ScreenState {
  api: Api;
  configs: ScreenConfigDto[];
  indices: string[];
  asof: ScreenResultDto | null;
  runs: ScreenRunMetaDto[];
  report: ScreenBacktestReportDto | null;
  indexRel: IndexRelativeDto | null;
  benchmark: string;
  error: string | null;
  loadConfigs: () => Promise<void>;
  loadRuns: () => Promise<void>;
  selectRun: (id: string) => Promise<void>;
  setBenchmark: (id: string, b: string) => Promise<void>;
}

export const useScreen = create<ScreenState>((set, get) => ({
  api: realApi, configs: [], indices: [], asof: null, runs: [], report: null, indexRel: null,
  benchmark: "csi300", error: null,
  loadConfigs: async () => {
    try { set({ configs: await get().api.screenConfigsList(), indices: await get().api.indexList() }); }
    catch { /* 启动早期静默 */ }
  },
  loadRuns: async () => { try { set({ runs: await get().api.screenRunsList() }); } catch {} },
  selectRun: async (id) => {
    set({ report: null, indexRel: null, error: null });
    try {
      const report = await get().api.screenRunReport(id);
      const indexRel = await get().api.screenIndexRelative(id, get().benchmark);
      set({ report, indexRel });
    } catch (e) { set({ error: friendlyError(String(e)).title }); }
  },
  setBenchmark: async (id, b) => {
    set({ benchmark: b });
    try { set({ indexRel: await get().api.screenIndexRelative(id, b) }); }
    catch (e) { set({ error: friendlyError(String(e)).title }); }
  },
}));
```

- [ ] **Step 2: 写 research store**(`stores/research.ts`):
```typescript
import { create } from "zustand";
import type { LedgerRoundDto } from "@bindings/LedgerRoundDto";
import type { IterQueueDto } from "@bindings/IterQueueDto";
import type { RoundCardDto } from "@bindings/RoundCardDto";
import { api as realApi, type Api } from "../api/ipc";
import { friendlyError } from "../errors";

interface ResearchState {
  api: Api;
  ledger: LedgerRoundDto[];
  queue: IterQueueDto | null;
  card: RoundCardDto | null;
  error: string | null;
  load: () => Promise<void>;
  selectRound: (round: number) => Promise<void>;
}

export const useResearch = create<ResearchState>((set, get) => ({
  api: realApi, ledger: [], queue: null, card: null, error: null,
  load: async () => {
    try { set({ ledger: await get().api.iterLedger(), queue: await get().api.iterQueue() }); } catch {}
  },
  selectRound: async (round) => {
    set({ card: null, error: null });
    try { set({ card: await get().api.iterRoundCard(round) }); }
    catch (e) { set({ error: friendlyError(String(e)).title }); }
  },
}));
```

- [ ] **Step 3: 写 store 测试**(`stores/screen.test.ts`,仿现有 store/page 测试注入 api):
```typescript
import { test, expect, afterEach } from "vitest";
import { useScreen } from "./screen";
const real = useScreen.getState().api;
afterEach(() => useScreen.setState({ api: real, configs: [], runs: [], report: null, indexRel: null }));

test("setBenchmark refetches index-relative", async () => {
  let lastBench = "";
  useScreen.setState({ api: { ...real,
    screenIndexRelative: async (_id, b) => { lastBench = b; return { benchmark: b, excess_cum: 0.3, curve: [], per_regime: [] }; },
  } });
  await useScreen.getState().setBenchmark("scr-1", "csi500");
  expect(lastBench).toBe("csi500");
  expect(useScreen.getState().indexRel?.benchmark).toBe("csi500");
});
```

- [ ] **Step 4: 跑测试**
Run: `cd desktop/ui && npx vitest run src/stores/screen.test.ts` → PASS。

- [ ] **Step 5: Commit**
```bash
git add desktop/ui/src/stores/screen.ts desktop/ui/src/stores/research.ts desktop/ui/src/stores/screen.test.ts
git commit -F - <<'EOF'
feat(ui): screen & research zustand stores + benchmark-switch test
EOF
```

---

## Task 12: 选股页 — 选股榜(as-of)+ ScreenPickTable

**Files:**
- Create: `desktop/ui/src/components/ScreenPickTable.tsx`、`ScreenPickTable.test.tsx`
- Create/Modify: `desktop/ui/src/pages/Screen.tsx`(替换占位)

**Interfaces:**
- Consumes:`useScreen`、`ScreenResultDto`/`ScreenPickDto`、antd `Table`。
- Produces:`<ScreenPickTable result={ScreenResultDto} />` 渲染排行榜(可排序 + 展开行显示 `reasons`);`Screen.tsx` 顶层 Tabs(选股榜 / 选股回测),左栏配置+as-of+top+运行。

- [ ] **Step 1: 写 ScreenPickTable**(antd Table,列=排名/代码/综合/质量/投机/标签/理由,`expandable` 渲染 reasons):
```tsx
import { Table, Tag } from "antd";
import type { ScreenResultDto } from "@bindings/ScreenResultDto";
import type { ScreenPickDto } from "@bindings/ScreenPickDto";

export default function ScreenPickTable({ result }: { result: ScreenResultDto }) {
  const cols = [
    { title: "#", dataIndex: "rank", width: 50 },
    { title: "代码", dataIndex: "symbol", width: 90 },
    { title: "综合分", dataIndex: "combined_score", width: 90, sorter: (a: ScreenPickDto, b: ScreenPickDto) => a.combined_score - b.combined_score, defaultSortOrder: "descend" as const, render: (v: number) => v.toFixed(2) },
    { title: "质量分", dataIndex: "quality_score", width: 80, render: (v: number) => v.toFixed(2) },
    { title: "投机分", dataIndex: "speculative_score", width: 80, render: (v: number) => v.toFixed(2) },
    { title: "标签", dataIndex: "tags", render: (t: string[]) => t.map((x) => <Tag key={x}>{x}</Tag>) },
  ];
  return (
    <Table<ScreenPickDto>
      size="small" rowKey="symbol" columns={cols} dataSource={result.rows}
      pagination={{ pageSize: 50 }}
      expandable={{ expandedRowRender: (r) => (
        <Table size="small" rowKey="tree" pagination={false}
          columns={[{ title: "树", dataIndex: "tree" }, { title: "命中叶子", dataIndex: "leaf" }, { title: "打分", dataIndex: "score", render: (v: number) => v.toFixed(3) }]}
          dataSource={r.reasons} />
      ) }}
    />
  );
}
```

- [ ] **Step 2: 写组件测试**(`ScreenPickTable.test.tsx`,仿 `RunOverview.test.tsx`):
```tsx
import { render, screen } from "@testing-library/react";
import "@testing-library/jest-dom";
import ScreenPickTable from "./ScreenPickTable";
import type { ScreenResultDto } from "@bindings/ScreenResultDto";

const R: ScreenResultDto = { config: "value_pb_base.yaml", as_of: "2026-06-12", n_universe: 1073, top: 50, rows: [
  { rank: 1, symbol: "sh601398", quality_score: 0.91, speculative_score: 0.05, combined_score: 0.91, tags: ["质量"], selected: true, reasons: [{ tree: "value_pb", leaf: "L2", score: 0.9 }] },
] };
test("renders ranked picks with scores", () => {
  render(<ScreenPickTable result={R} />);
  expect(screen.getByText("sh601398")).toBeInTheDocument();
  expect(screen.getByText("0.91")).toBeInTheDocument();
  expect(screen.getByText("质量")).toBeInTheDocument();
});
```

- [ ] **Step 3: 写 Screen.tsx**(左栏配置/as-of/top/运行 + Tabs;选股榜 tab 用 ScreenPickTable,任务完成后从 task result 取 ScreenResultDto;选股回测 tab 占位待 Task 13)。最小骨架:
```tsx
import { useEffect, useState } from "react";
import { App as AntApp, Button, Card, Col, DatePicker, InputNumber, Row, Select, Tabs } from "antd";
import { useScreen } from "../stores/screen";
import { listen } from "@tauri-apps/api/event";
import type { ScreenResultDto } from "@bindings/ScreenResultDto";
import ScreenPickTable from "../components/ScreenPickTable";
import ScreenBacktestResult from "../components/ScreenBacktestResult";

export default function Screen() {
  const st = useScreen();
  const { message } = AntApp.useApp();
  const [config, setConfig] = useState<string>("");
  const [asOf, setAsOf] = useState<string>("2026-06-12");
  const [top, setTop] = useState<number>(50);
  const [asofResult, setAsofResult] = useState<ScreenResultDto | null>(null);
  useEffect(() => { void st.loadConfigs(); void st.loadRuns(); }, []);

  async function runAsof() {
    try {
      const taskId = await st.api.screenAsof(config, asOf, top);
      const un = await listen<{ id: string; status: string; result: ScreenResultDto | null }>("task://progress", (e) => {
        if (e.payload.id === taskId && e.payload.status === "done") { setAsofResult(e.payload.result); un(); }
        if (e.payload.id === taskId && e.payload.status === "failed") { message.error("选股失败"); un(); }
      });
    } catch (e) { message.error(String(e)); }
  }

  const left = (
    <Card size="small" title="选股配置">
      <Select style={{ width: "100%" }} placeholder="配置" value={config || undefined}
        onChange={setConfig} options={st.configs.map((c) => ({ value: c.path, label: c.name ?? c.path }))} />
      <DatePicker style={{ width: "100%", marginTop: 8 }} onChange={(_, s) => setAsOf(s as string)} />
      <InputNumber style={{ width: "100%", marginTop: 8 }} addonBefore="Top" value={top} onChange={(v) => setTop(v ?? 50)} />
      <Button type="primary" block style={{ marginTop: 8 }} disabled={!config} onClick={runAsof}>运行选股</Button>
    </Card>
  );
  return (
    <Row gutter={12}>
      <Col span={6}>{left}</Col>
      <Col span={18}>
        <Tabs items={[
          { key: "asof", label: "选股榜 (as-of)", children: asofResult ? <ScreenPickTable result={asofResult} /> : <span>选配置并运行</span> },
          { key: "bt", label: "选股回测", children: <ScreenBacktestResult /> },
        ]} />
      </Col>
    </Row>
  );
}
```
*(`ScreenBacktestResult` 在 Task 13 实现;本步可先建空壳 `export default ()=> null` 过编译。)*

- [ ] **Step 4: 跑组件测试 + 类型检查**
Run: `cd desktop/ui && npx vitest run src/components/ScreenPickTable.test.tsx && npx tsc --noEmit` → PASS + 0 error。

- [ ] **Step 5: Commit**
```bash
git add desktop/ui/src/components/ScreenPickTable.tsx desktop/ui/src/components/ScreenPickTable.test.tsx desktop/ui/src/pages/Screen.tsx
git commit -F - <<'EOF'
feat(ui): screen page as-of pick table (sortable, expandable per-tree reasons)
EOF
```

---

## Task 13: 选股回测结果视图 ScreenBacktestResult

**Files:**
- Create: `desktop/ui/src/components/ScreenBacktestResult.tsx`、`ScreenBacktestResult.test.tsx`

**Interfaces:**
- Consumes:`useScreen`(`runs`/`report`/`indexRel`/`benchmark`/`selectRun`/`setBenchmark`)、`ScreenBacktestReportDto`、`IndexRelativeDto`、复用 `NavChart`。
- Produces:基准切换 + 一等指数相对带(OOS 高亮 + break-even)+ 次行绝对 + 净值/超额图 + regime/归因/分层三联。

- [ ] **Step 1: 写组件**(antd Statistic/Segmented/Table + NavChart;图表布局按 spec §4.2:净值图独立成行、三联等高网格、指标带紧凑):
```tsx
import { useEffect } from "react";
import { Card, Col, Row, Segmented, Statistic, Table, Tag } from "antd";
import { useScreen } from "../stores/screen";
import NavChart from "./NavChart";

export default function ScreenBacktestResult() {
  const st = useScreen();
  useEffect(() => { void st.loadRuns(); }, []);
  const rep = st.report, ir = st.indexRel;
  const sel = st.runs[0]?.id; // 简化:默认选最近;真实用列表选择
  useEffect(() => { if (sel) void st.selectRun(sel); }, [sel]);
  const pct = (v?: number | null) => (v == null ? "—" : `${(v * 100).toFixed(1)}%`);
  return (
    <div>
      <Segmented value={st.benchmark}
        options={[...st.indices.map((i) => ({ value: i, label: i.toUpperCase() })), { value: "EW", label: "等权·参考" }]}
        onChange={(b) => sel && st.setBenchmark(sel, b as string)} />
      {/* 一等口径:指数相对 */}
      <Card size="small" style={{ marginTop: 8, background: "rgba(59,130,246,.05)" }} title={`指数相对（vs ${st.benchmark.toUpperCase()}）`}>
        <Row gutter={16}>
          <Col><Statistic title="净超额(累计)" value={pct(ir?.excess_cum)} valueStyle={{ color: "#16a34a" }} /></Col>
          {ir?.per_regime.map((r) => (
            <Col key={r.label}><Statistic title={r.label} value={pct(r.excess)} valueStyle={r.label.includes("OOS") ? { color: "#16a34a" } : {}} /></Col>
          ))}
          <Col><Statistic title="盈亏平衡(bps)" value={rep?.break_even?.toFixed(0) ?? "—"} /></Col>
        </Row>
      </Card>
      {/* 次行:绝对口径 */}
      <div style={{ opacity: 0.8, fontSize: 12, margin: "8px 0" }}>
        绝对:净总 {pct(rep?.net_total_return)} · Sharpe {rep?.abs_sharpe?.toFixed(2) ?? "—"} · 回撤 {pct(rep?.max_drawdown)} · 换手 {rep ? rep.turnover.toFixed(1) : "—"}
      </div>
      {/* 净值/超额曲线(独立成行) */}
      {ir && <NavChart series={ir.curve.map((p) => ({ t: p.t, v: p.excess }))} title="累计超额" />}
      {/* 三联等高网格 */}
      <Row gutter={8} style={{ marginTop: 8 }}>
        <Col span={8}><Card size="small" title="regime 切片(超额)">
          {ir?.per_regime.map((r) => <div key={r.label}>{r.label}: {pct(r.excess)}</div>)}
        </Card></Col>
        <Col span={8}><Card size="small" title="标签归因">
          {rep?.tag_attribution.map((t) => <div key={t.tag}>{t.tag}: {pct(t.mean_fwd_return)}</div>)}
        </Card></Col>
        <Col span={8}><Card size="small" title="优质分分层">
          <Table size="small" pagination={false} rowKey="layer"
            columns={[{ title: "层", dataIndex: "layer" }, { title: "年化", dataIndex: "mean_fwd_return", render: pct }]}
            dataSource={rep?.quality_layers ?? []} />
        </Card></Col>
      </Row>
    </div>
  );
}
```
*(若 `NavChart` 的 props 与上面不符,按其实际签名适配 —— 读取 `components/NavChart.tsx` 确认 `series`/`title` 形参名后对齐。)*

- [ ] **Step 2: 写测试**(注入 store report/indexRel,断言默认显示指数相对 + OOS):
```tsx
import { render, screen } from "@testing-library/react";
import "@testing-library/jest-dom";
import { vi, afterEach } from "vitest";
vi.mock("./NavChart", () => ({ default: () => null }));
import ScreenBacktestResult from "./ScreenBacktestResult";
import { useScreen } from "../stores/screen";
const real = useScreen.getState().api;
afterEach(() => useScreen.setState({ api: real, runs: [], report: null, indexRel: null }));

test("shows index-relative excess and OOS by default", () => {
  useScreen.setState({
    api: { ...real, screenRunsList: async () => [], screenRunReport: async () => null as any, screenIndexRelative: async () => null as any },
    runs: [{ id: "scr-1", config: "c", from: "2018-01", to: "2026-06", top: 50, rebalance: 1, created: "x", ok: true, error: null }],
    benchmark: "csi300",
    report: { meta: {} as any, net_total_return: 3.24, gross_total_return: 3.5, abs_sharpe: 1.13, max_drawdown: 0.19, turnover: 2.4, break_even: 164, nav: [], tag_attribution: [], regime_slices: [], quality_layers: [] },
    indexRel: { benchmark: "csi300", excess_cum: 2.96, curve: [], per_regime: [{ label: "2024-26_OOS", excess: 0.64 }] },
  });
  render(<ScreenBacktestResult />);
  expect(screen.getByText(/指数相对/)).toBeInTheDocument();
  expect(screen.getByText("2024-26_OOS")).toBeInTheDocument();
});
```

- [ ] **Step 3: 跑测试 + 类型检查**
Run: `cd desktop/ui && npx vitest run src/components/ScreenBacktestResult.test.tsx && npx tsc --noEmit` → PASS。

- [ ] **Step 4: Commit**
```bash
git add desktop/ui/src/components/ScreenBacktestResult.tsx desktop/ui/src/components/ScreenBacktestResult.test.tsx
git commit -F - <<'EOF'
feat(ui): screen backtest result view (index-relative default + benchmark switch + attribution)
EOF
```

---

## Task 14: 研究页 — LedgerTable + RoundCard + RunRoundForm

**Files:**
- Create: `desktop/ui/src/components/LedgerTable.tsx`、`RoundCard.tsx`、`RunRoundForm.tsx`、`RoundCard.test.tsx`
- Create/Modify: `desktop/ui/src/pages/Research.tsx`(替换占位)

**Interfaces:**
- Consumes:`useResearch`、`LedgerRoundDto`/`RoundCardDto`/`IterQueueDto`、`useScreen`(配置/指数下拉)。
- Produces:研究页(左 launcher+记忆 / 右 台账+roundcard)。

- [ ] **Step 1: LedgerTable**(antd Table,verdict 着色 + 筛选;点行回调 `onSelect(round)`):
```tsx
import { Table, Tag } from "antd";
import type { LedgerRoundDto } from "@bindings/LedgerRoundDto";
import { VERDICT_ZH } from "../labels";
export default function LedgerTable({ rows, onSelect }: { rows: LedgerRoundDto[]; onSelect: (r: number) => void }) {
  const pct = (v?: number | null) => (v == null ? "—" : `${(v * 100).toFixed(0)}%`);
  return <Table<LedgerRoundDto> size="small" rowKey="round" dataSource={rows} pagination={false}
    onRow={(r) => ({ onClick: () => onSelect(r.round), style: { cursor: "pointer" } })}
    columns={[
      { title: "#", dataIndex: "round", width: 48 },
      { title: "label", dataIndex: "label" },
      { title: "假设", dataIndex: "note", ellipsis: true },
      { title: "net超额", dataIndex: "net_ex", render: pct },
      { title: "OOS超额", dataIndex: "net_oos_ex", render: pct },
      { title: "Sharpe", dataIndex: "net_sharpe", render: (v?: number | null) => v?.toFixed(2) ?? "—" },
      { title: "裁决", dataIndex: "verdict", render: (v: string) => <Tag color={v === "PASS" ? "green" : "red"}>{VERDICT_ZH[v] ?? v}</Tag> },
    ]} />;
}
```

- [ ] **Step 2: RoundCard**(逐条门槛 ✓/✗ + Tier-2 cells + flags):
```tsx
import { Card, Table, Tag } from "antd";
import type { RoundCardDto } from "@bindings/RoundCardDto";
export default function RoundCard({ card }: { card: RoundCardDto }) {
  return (
    <Card size="small" title={`Round ${card.round} · ${card.label} [${card.benchmark}]`}
      extra={<Tag color={card.verdict === "PASS" ? "green" : "red"}>{card.verdict}</Tag>}>
      <Table size="small" pagination={false} rowKey="name" title={() => "verdict 门槛"}
        columns={[
          { title: "门槛", dataIndex: "name" },
          { title: "", dataIndex: "pass", width: 40, render: (p: boolean) => <span style={{ color: p ? "#16a34a" : "#dc2626" }}>{p ? "✓" : "✗"}</span> },
          { title: "值", dataIndex: "value", render: (v?: number | null) => v?.toFixed(2) ?? "—" },
        ]} dataSource={card.gates} />
      {card.tier2.length > 0 && (
        <Table size="small" pagination={false} rowKey={(r) => `${r.top}-${r.rebalance}`} style={{ marginTop: 8 }} title={() => "Tier-2 敏感扫(net超额)"}
          columns={[{ title: "Top", dataIndex: "top" }, { title: "调仓", dataIndex: "rebalance" }, { title: "net超额", dataIndex: "net_excess", render: (v: number) => `${(v * 100).toFixed(0)}%` }]}
          dataSource={card.tier2} />
      )}
    </Card>
  );
}
```

- [ ] **Step 3: RunRoundForm**(launcher;选配置/note/axis/top/基准/调仓 → `iterRunRound`):
```tsx
import { useState } from "react";
import { App as AntApp, Button, Input, InputNumber, Select } from "antd";
import { useScreen } from "../stores/screen";
import { useResearch } from "../stores/research";
export default function RunRoundForm() {
  const sc = useScreen(); const rs = useResearch(); const { message } = AntApp.useApp();
  const [config, setConfig] = useState(""); const [note, setNote] = useState(""); const [bench, setBench] = useState("csi300");
  async function run() {
    try { await rs.api.iterRunRound(config, note, "daily", 50, bench, 1); message.success("已开始跑轮(后台)"); }
    catch (e) { message.error(String(e)); }
  }
  return <div>
    <Select style={{ width: "100%" }} placeholder="配置" onChange={setConfig}
      options={sc.configs.map((c) => ({ value: c.path, label: c.name ?? c.path }))} />
    <Input style={{ marginTop: 8 }} placeholder="假设 note" value={note} onChange={(e) => setNote(e.target.value)} />
    <Select style={{ width: "100%", marginTop: 8 }} value={bench} onChange={setBench}
      options={sc.indices.map((i) => ({ value: i, label: i.toUpperCase() }))} />
    <Button type="primary" block style={{ marginTop: 8 }} disabled={!config} onClick={run}>▶ 运行一轮</Button>
  </div>;
}
```

- [ ] **Step 4: Research.tsx**(组装):
```tsx
import { useEffect } from "react";
import { Card, Col, Row } from "antd";
import { useResearch } from "../stores/research";
import { useScreen } from "../stores/screen";
import LedgerTable from "../components/LedgerTable";
import RoundCard from "../components/RoundCard";
import RunRoundForm from "../components/RunRoundForm";
export default function Research() {
  const rs = useResearch(); const sc = useScreen();
  useEffect(() => { void rs.load(); void sc.loadConfigs(); }, []);
  return (
    <Row gutter={12}>
      <Col span={7}>
        <Card size="small" title="跑一轮"><RunRoundForm /></Card>
        <Card size="small" title="待试角度" style={{ marginTop: 12 }}>{rs.queue?.queue.map((q) => <div key={q}>• {q}</div>)}</Card>
        <Card size="small" title="已证伪角度(不再试)" style={{ marginTop: 12 }}>{rs.queue?.falsified.map((q) => <div key={q} style={{ opacity: .6 }}>• {q}</div>)}</Card>
      </Col>
      <Col span={17}>
        <Card size="small" title="轮次台账"><LedgerTable rows={rs.ledger} onSelect={(r) => void rs.selectRound(r)} /></Card>
        {rs.card && <div style={{ marginTop: 12 }}><RoundCard card={rs.card} /></div>}
      </Col>
    </Row>
  );
}
```

- [ ] **Step 5: RoundCard 测试**(`RoundCard.test.tsx`):
```tsx
import { render, screen } from "@testing-library/react";
import "@testing-library/jest-dom";
import RoundCard from "./RoundCard";
import type { RoundCardDto } from "@bindings/RoundCardDto";
const CARD: RoundCardDto = { round: 4, label: "value_pb", benchmark: "csi300", rebalance: 1, verdict: "PASS",
  gates: [{ name: "net-OOS 超额>0", pass: true, value: 0.64, threshold: 0, note: "金标准" }],
  tier2: [{ top: 50, rebalance: 1, net_excess: 0.64 }], flags: [], config_path: "examples/screen/iter/value_pb_base.yaml" };
test("round card shows verdict and gates", () => {
  render(<RoundCard card={CARD} />);
  expect(screen.getByText("PASS")).toBeInTheDocument();
  expect(screen.getByText("net-OOS 超额>0")).toBeInTheDocument();
});
```

- [ ] **Step 6: 跑测试 + 类型检查**
Run: `cd desktop/ui && npx vitest run src/components/RoundCard.test.tsx && npx tsc --noEmit` → PASS。

- [ ] **Step 7: Commit**
```bash
git add desktop/ui/src/components/LedgerTable.tsx desktop/ui/src/components/RoundCard.tsx desktop/ui/src/components/RunRoundForm.tsx desktop/ui/src/components/RoundCard.test.tsx desktop/ui/src/pages/Research.tsx
git commit -F - <<'EOF'
feat(ui): research page (ledger table + round card + run-round launcher + memory)
EOF
```

---

## Task 15: 收尾闸 + 文档 + 真数据冒烟

**Files:**
- Modify: `desktop/README.md` 或 `docs/`(新增页使用说明,简短)
- Modify: `C:\Users\Administrator\.claude\projects\E--rust-app-rquant\memory\rquant-project.md`(记客户端子项1落地)

- [ ] **Step 1: 全量后端闸**
Run: `cargo test --workspace` → 全绿(lib + e2e + bridge + desktop)。
Expected: 0 failed。

- [ ] **Step 2: 前端构建 + 测试**
Run: `cd desktop/ui && npm run build && npx vitest run` → build 成功 + 测试全绿。

- [ ] **Step 3: 真数据冒烟(GUI vs CLI 对账)**
手动:`npm run tauri dev` 启动;选股页跑 `value_pb_base.yaml` as-of 2026-06-12 top50 → 排行榜非空;研究页台账显示 ≥10 轮;对一个既有 PASS 轮的 round card,核对 OOS超额/verdict 与 `.iter/ledger.jsonl` 一致(诚实对账,数值须吻合)。
*若 `index-relative` 切基准:CSI300→CSI500 超额变化方向合理。*

- [ ] **Step 4: 文档 + 记忆**:在 `docs/` 增一节"选股/研究页用法"(配置来自 examples/screen/iter,跑轮调 Python harness,verdict 唯一真源);更新记忆 `rquant-project.md`。

- [ ] **Step 5: Commit**
```bash
git add docs/ desktop/README.md
git commit -F - <<'EOF'
docs(desktop): screen & research pages usage; finalize sub-project 1
EOF
```

- [ ] **Step 6: finishing**:调用 superpowers:finishing-a-development-branch 收口(merge/PR/keep 选择)。

---

## 自审备忘(写计划时已校)

- **类型一致**:DTO 字段名与 Rust 源结构体逐一对齐(`ScreenRow`→`ScreenPickDto`、`ScreenHolding.{t,nav,benchmark_nav}`、ledger.jsonl 13 键→`LedgerRoundDto`)。命令名 snake_case 与 `api/ipc.ts` camelCase 映射一致。
- **verdict 唯一真源**:Rust `gates_from` 仅由 flags/metrics 映射展示,从不重判;PASS/FALSIFIED 永远取自 Python 产出的 ledger.jsonl。
- **范围**:diff/导出/部署、optimize/factor/portfolio、数据管线 UI 均不在本计划(子项2/3)。
- **已知取舍**:`screen_pick_detail` 命令省去(reasons 随排行榜返回);完整决策路径 replay 留 future;`break_even` 展示成本固定 20bps(与 harness 默认一致),非可配。
