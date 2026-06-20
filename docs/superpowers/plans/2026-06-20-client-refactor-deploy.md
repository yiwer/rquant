# 客户端重构 sub-3a「value 部署」实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 给桌面端加 value 选股纸面盘第 4 本 + 月度 `as-of→diff→下单清单→确认→滚动 NAV` 闭环(screen 驱动、手动触发、纸面只跟踪不下真单)。

**Architecture:** 复用 sub-1/2a 桌面范式。月度选股直调 `rquant::screen::run_screen`(冻结配置 `deploy/value_pb_deploy_frozen.yaml`);diff/NAV 滚动纯 Rust 算术(读 kday close + 沪深300);两步 preview→commit(无自动副作用);状态原子写 `.rquant-desktop/deploy_book/value.json`。驾驶舱第 4 卡 + 新 `部署` 顶层页。

**Tech Stack:** Rust 2024 + Tauri 2.11 + ts-rs 10 / React 18 + antd 6 + ECharts 6 + Vitest 4。

## Global Constraints

- DTO `#[derive(Debug,Clone,Serialize,TS)] #[ts(export)]`(可读回加 `Deserialize`);bindings→`desktop/src-tauri/bindings/`,前端 `@bindings/<Name>`;**新 DTO 名全局唯一**(注:sub-2a 已有 `DeployDto`[部署加固分析器]、sub-1 已有 `NavPointDto`/`DiffRowDto` —— 本子项用 `DeployBookDto`/`DeployNavPointDto` 等不同名,**复用** sub-1 `DiffRowDto`)。
- 命令同步壳;重计算(screen as-of)走 `state.tasks.start(kind, heavy, |ctx|…)`;screen 直调库用 `tokio::runtime::Runtime::new()?.block_on(...)` + `rquant::cli::build_llm`(仿 sub-1 `screen_asof`)。
- **纸面盘:只跟踪 NAV、不下真单**;**手动触发、确认后才落账**(`deploy_run_month` 预览不写,`deploy_commit_month` 才写);screen 用**冻结**部署配置;数据假定已刷新,as-of 超数据覆盖→友好警示。
- 全中文 UI(保留 PB/夏普/沪深300 等);英文 commit(`git commit -F -`);`git add` 显式列文件;收尾 `cargo test --workspace` + `cd desktop/ui` build/vitest。
- 数据:`kday/<sym>.csv` close=col4;`index/csi300.csv` `time,close`;冻结配置 `deploy/value_pb_deploy_frozen.yaml`(screen 配置,top-50,λ=0)。`ScreenResult.rows[{symbol,combined_score,selected,...}]`,picks=selected==true 的行。`run_screen(cfg,&llm)` 异步。

## 文件结构

- Create `desktop/src-tauri/src/deploy_book.rs` — 状态模型 + `diff` + `ew_return` + 状态读写 + 单测。
- Create `desktop/src-tauri/src/deploy_cmds.rs` — `deploy_book_read` / `deploy_run_month` / `deploy_commit_month`。
- Create `desktop/src-tauri/src/dto_deploy.rs` — 部署 DTO。
- Modify `paths.rs`(`deploy_book_path`)、`lib.rs`(mod + 3 handler)。
- Modify `desktop/ui/src/`:`api/ipc.ts`、`App.tsx`(部署路由)、`labels.ts`、`pages/Cockpit.tsx`(第4卡)。
- Create `desktop/ui/src/pages/Deploy.tsx`、`components/ValueBookCard.tsx`、`stores/deploy.ts`(+ 测试)。复用 `components/{DiffTable,NavChart}`。

---

## Task 1: paths + deploy_book 状态模型 + diff(纯逻辑 TDD)

**Files:** Create `desktop/src-tauri/src/deploy_book.rs`;Modify `paths.rs`、`lib.rs`(`pub mod deploy_book;`)

**Interfaces — Produces:** `paths::Workspace::deploy_book_path()->.rquant-desktop/deploy_book/value.json`;`deploy_book::{DeployState, NavPoint, MonthRec, diff}`。`diff(prev:&[String], next:&[String]) -> Vec<crate::dto::DiffRowDto>`(EW 权重;action "Buy"/"Sell"/"Hold")。

- [ ] **Step 1: paths**(`paths.rs` impl):
```rust
pub fn deploy_book_path(&self) -> PathBuf { self.desktop_data_dir().join("deploy_book").join("value.json") }
```

- [ ] **Step 2: 写失败测试**(`deploy_book.rs`):
```rust
#[cfg(test)]
mod diff_tests {
    use super::*;
    #[test]
    fn diff_buy_sell_hold() {
        let prev = vec!["A".to_string(), "B".to_string()];
        let next = vec!["B".to_string(), "C".to_string()];
        let d = diff(&prev, &next);
        let get = |s: &str| d.iter().find(|r| r.symbol == s).unwrap();
        assert_eq!(get("A").action, "Sell");   // 在 prev 不在 next
        assert_eq!(get("C").action, "Buy");     // 在 next 不在 prev
        assert_eq!(get("B").action, "Hold");    // 两者都在
        assert!((get("C").to_w - 0.5).abs() < 1e-9); // next EW = 1/2
        assert!((get("A").to_w - 0.0).abs() < 1e-9);
    }
}
```

- [ ] **Step 3: 跑确认失败** `cargo test -p rquant-desktop diff_tests` → FAIL。

- [ ] **Step 4: 实现**(`deploy_book.rs`):
```rust
//! value 部署纸面盘:状态模型 + diff + NAV 滚动(纯算术,纸面只跟踪不下真单)。
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavPoint { pub t: String, pub nav: f64, pub bench_nav: f64 }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonthRec { pub as_of: String, pub picks: Vec<String>, pub nav: f64, pub bench_nav: f64, pub n_buy: u32, pub n_sell: u32 }
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DeployState {
    pub holdings: Vec<String>,
    pub last_date: Option<String>,
    pub nav: f64,
    pub bench_base: Option<f64>,        // 沪深300 close at go-live (归一基)
    pub nav_history: Vec<NavPoint>,
    pub months: Vec<MonthRec>,
}

/// EW 调仓 diff:买(新进)/卖(移出)/持(都在)。权重=1/N。
pub fn diff(prev: &[String], next: &[String]) -> Vec<crate::dto::DiffRowDto> {
    let pw = if prev.is_empty() { 0.0 } else { 1.0 / prev.len() as f64 };
    let nw = if next.is_empty() { 0.0 } else { 1.0 / next.len() as f64 };
    let pset: BTreeSet<&str> = prev.iter().map(|s| s.as_str()).collect();
    let nset: BTreeSet<&str> = next.iter().map(|s| s.as_str()).collect();
    let mut all: BTreeSet<&str> = BTreeSet::new();
    all.extend(pset.iter().copied()); all.extend(nset.iter().copied());
    all.into_iter().map(|s| {
        let inp = pset.contains(s); let inn = nset.contains(s);
        let action = if inn && !inp { "Buy" } else if inp && !inn { "Sell" } else { "Hold" };
        crate::dto::DiffRowDto {
            symbol: s.to_string(), action: action.to_string(),
            from_w: if inp { pw } else { 0.0 }, to_w: if inn { nw } else { 0.0 },
        }
    }).collect()
}
```
加 `pub mod deploy_book;` 到 lib.rs。(确认 `crate::dto::DiffRowDto` 字段为 `symbol:String, action:String, from_w:f64, to_w:f64` —— sub-1 cockpit 定义;若字段名不同,读 `dto.rs` 对齐。)

- [ ] **Step 5: 跑确认通过** `cargo test -p rquant-desktop diff_tests` → PASS。

- [ ] **Step 6: Commit**
```bash
git add desktop/src-tauri/src/deploy_book.rs desktop/src-tauri/src/paths.rs desktop/src-tauri/src/lib.rs
git commit -F - <<'EOF'
feat(desktop): deploy_book state model + EW rebalance diff + path
EOF
```

---

## Task 2: deploy_book NAV 滚动 + 状态读写(纯逻辑 TDD)

**Files:** Modify `desktop/src-tauri/src/deploy_book.rs`

**Interfaces — Produces:** `ew_return(holdings:&[String], price:&dyn Fn(&str,&str)->Option<f64>, d0:&str, d1:&str)->f64`(EW 持有期收益,跳缺失,空→0);`read_state(path)->DeployState`(缺省 Default+nav=0/empty);`write_state(path,&DeployState)->Result<(),String>`(原子写)。

- [ ] **Step 1: 写失败测试**:
```rust
#[cfg(test)]
mod nav_tests {
    use super::*;
    use std::collections::HashMap;
    #[test]
    fn ew_return_mean_of_holdings() {
        let px: HashMap<(&str,&str),f64> = HashMap::from([
            (("A","2024-01-31"),10.0),(("A","2024-02-29"),11.0),  // +10%
            (("B","2024-01-31"),10.0),(("B","2024-02-29"),13.0)]);// +30%
        let price = |s:&str,d:&str| px.get(&(s,d)).copied();
        let r = ew_return(&["A".to_string(),"B".to_string()], &price, "2024-01-31", "2024-02-29");
        assert!((r - 0.20).abs() < 1e-9); // (.1+.3)/2
        assert!((ew_return(&[], &price, "2024-01-31", "2024-02-29")).abs() < 1e-9);
    }
}
```

- [ ] **Step 2: 跑确认失败** `cargo test -p rquant-desktop nav_tests` → FAIL。

- [ ] **Step 3: 实现**(`deploy_book.rs` 追加):
```rust
pub fn ew_return(holdings: &[String], price: &dyn Fn(&str, &str) -> Option<f64>, d0: &str, d1: &str) -> f64 {
    let rets: Vec<f64> = holdings.iter().filter_map(|s| {
        match (price(s, d0), price(s, d1)) { (Some(p0), Some(p1)) if p0 > 0.0 => Some(p1 / p0 - 1.0), _ => None }
    }).collect();
    if rets.is_empty() { 0.0 } else { rets.iter().sum::<f64>() / rets.len() as f64 }
}

pub fn read_state(path: &std::path::Path) -> DeployState {
    std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}
pub fn write_state(path: &std::path::Path, st: &DeployState) -> Result<(), String> {
    std::fs::create_dir_all(path.parent().expect("deploy_book has parent")).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(st).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}
```

- [ ] **Step 4: 跑确认通过** `cargo test -p rquant-desktop nav_tests` → PASS;`cargo test -p rquant-desktop deploy_book` 两组全过。

- [ ] **Step 5: Commit**
```bash
git add desktop/src-tauri/src/deploy_book.rs
git commit -F - <<'EOF'
feat(desktop): deploy_book EW return roll + atomic state read/write
EOF
```

---

## Task 3: 部署 DTO + bindings

**Files:** Create `dto_deploy.rs`;Modify `lib.rs`(`pub mod dto_deploy;`)

- [ ] **Step 1: 写 DTO**(`dto_deploy.rs`):
```rust
use serde::Serialize; use ts_rs::TS;
#[derive(Debug,Clone,Serialize,TS)] #[ts(export)]
pub struct DeployHoldingDto { pub symbol: String, pub weight: f64, pub since: String }
#[derive(Debug,Clone,Serialize,TS)] #[ts(export)]
pub struct DeployNavPointDto { pub t: String, pub nav: f64, pub bench_nav: f64 }
#[derive(Debug,Clone,Serialize,TS)] #[ts(export)]
pub struct DeployMonthRecDto { pub as_of: String, pub nav: f64, pub excess: f64, pub n_holdings: u32, pub n_buy: u32, pub n_sell: u32 }
#[derive(Debug,Clone,Serialize,TS)] #[ts(export)]
pub struct DeployBookDto { pub status: String, pub nav: Option<f64>, pub excess_total: Option<f64>, pub last_rebalance: Option<String>, pub holdings: Vec<DeployHoldingDto>, pub nav_history: Vec<DeployNavPointDto>, pub months: Vec<DeployMonthRecDto> }
#[derive(Debug,Clone,Serialize,TS)] #[ts(export)]
pub struct DeployMonthDto { pub as_of: String, pub picks: Vec<DeployHoldingDto>, pub diff: Vec<crate::dto::DiffRowDto>, pub proj_nav: f64, pub proj_excess: f64, pub realized_ret: f64 }
```

- [ ] **Step 2: 声明 + bindings** 加 `pub mod dto_deploy;` 到 lib.rs;`cargo test -p rquant-desktop 2>&1 | tail -3`;确认 `bindings/{DeployBookDto,DeployMonthDto,DeployHoldingDto}.ts` 生成。

- [ ] **Step 3: Commit**
```bash
git add desktop/src-tauri/src/dto_deploy.rs desktop/src-tauri/src/lib.rs desktop/src-tauri/bindings/
git commit -F - <<'EOF'
feat(desktop): deploy book DTOs + ts-rs bindings
EOF
```

---

## Task 4: 部署命令(read / run_month 预览 / commit_month 落账)

**Files:** Create `deploy_cmds.rs`;Modify `lib.rs`(mod + 3 handler)

**Interfaces — Consumes:** `deploy_book::*`、`dto_deploy::*`、`rquant::screen::{run_screen, ScreenRunConfig}`、`rquant::cli::build_llm`、`index_relative::{load_index, idx_at}`、`paths`。

- [ ] **Step 1: 实现**(`deploy_cmds.rs`;helper:读冻结配置选股 top-50 + kday close + 沪深300):
```rust
use crate::commands::AppState;
use crate::dto_deploy::*;
use std::collections::HashMap;

const DEPLOY_CONFIG: &str = "deploy/value_pb_deploy_frozen.yaml";

fn load_close(ws: &crate::paths::Workspace, sym: &str) -> HashMap<String, f64> {
    let mut m = HashMap::new();
    if let Ok(txt) = std::fs::read_to_string(ws.kday_dir().join(format!("{sym}.csv"))) {
        for line in txt.lines().skip(1) { let c: Vec<&str> = line.split(',').collect();
            if c.len() >= 5 { if let Ok(close) = c[4].parse::<f64>() { m.insert(c[0].get(..10).unwrap_or(c[0]).to_string(), close); } } }
    }
    m
}

// 跑 as-of screen(冻结配置) → top-50 选中 symbols(按 combined 降序)
fn screen_picks(ws: &crate::paths::Workspace, as_of: &str) -> Result<Vec<String>, String> {
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let llm = rquant::cli::build_llm(String::new(), String::new(), ws.root().join(".rquant-cache").join("llm")).map_err(|e| e.to_string())?;
    let cfg = rquant::screen::ScreenRunConfig {
        config_path: ws.root().join(DEPLOY_CONFIG),
        universe_path: ws.root().join("data/baostock/universe_baostock_day.csv"),
        as_of: chrono::NaiveDate::parse_from_str(as_of, "%Y-%m-%d").ok(),
        top: Some(50), window: 260, out_path: None, membership_path: None, sectors_path: None,
    };
    let res = rt.block_on(rquant::screen::run_screen(&cfg, &llm)).map_err(|e| e.to_string())?;
    Ok(res.rows.iter().filter(|r| r.selected).map(|r| r.symbol.clone()).collect())
}

// 共享:算一个月的预览(选股 + diff + 滚动 NAV),不写
fn compute_month(ws: &crate::paths::Workspace, as_of: &str) -> Result<(DeployMonthDto, crate::deploy_book::DeployState, f64, f64), String> {
    let st = crate::deploy_book::read_state(&ws.deploy_book_path());
    let picks = screen_picks(ws, as_of)?;
    if picks.is_empty() { return Err("该日无选股(数据未刷新或配置异常)".into()); }
    let dlist = crate::deploy_book::diff(&st.holdings, &picks);
    let n_buy = dlist.iter().filter(|d| d.action == "Buy").count() as u32;
    let n_sell = dlist.iter().filter(|d| d.action == "Sell").count() as u32;
    // 实现收益:上月持仓 last_date→as_of 的 EW 收益(首月=0)
    let syms: std::collections::BTreeSet<String> = st.holdings.iter().cloned().collect();
    let mut px: HashMap<String, HashMap<String,f64>> = HashMap::new();
    for s in &syms { px.insert(s.clone(), load_close(ws, s)); }
    let price = |s: &str, d: &str| px.get(s).and_then(|m| m.get(d)).copied();
    let realized = match &st.last_date { Some(d0) => crate::deploy_book::ew_return(&st.holdings, &price, d0, as_of), None => 0.0 };
    let prev_nav = if st.nav > 0.0 { st.nav } else { 1.0 };
    let proj_nav = prev_nav * (1.0 + realized);
    // 沪深300 归一 bench_nav
    let idx = crate::index_relative::load_index(&ws.index_dir().join("csi300.csv"))?;
    let bench_at = crate::index_relative::idx_at(&idx, as_of);
    let bench_base = st.bench_base.or(bench_at);
    let bench_nav = match (bench_base, bench_at) { (Some(b0), Some(b1)) if b0 > 0.0 => b1 / b0, _ => 1.0 };
    let proj_excess = (proj_nav - 1.0) - (bench_nav - 1.0);
    let picks_dto: Vec<DeployHoldingDto> = picks.iter().map(|s| DeployHoldingDto { symbol: s.clone(), weight: 1.0 / picks.len() as f64, since: as_of.to_string() }).collect();
    let dto = DeployMonthDto { as_of: as_of.to_string(), picks: picks_dto, diff: dlist, proj_nav, proj_excess, realized_ret: realized };
    Ok((dto, st, proj_nav, bench_nav))
}

#[tauri::command]
pub fn deploy_book_read(state: tauri::State<AppState>) -> DeployBookDto {
    let st = crate::deploy_book::read_state(&state.ws.deploy_book_path());
    let status = if st.months.is_empty() { "empty" } else { "ok" }.to_string();
    let excess_total = st.nav_history.last().map(|p| (p.nav - 1.0) - (p.bench_nav - 1.0));
    DeployBookDto {
        status,
        nav: if st.nav > 0.0 { Some(st.nav) } else { None },
        excess_total,
        last_rebalance: st.last_date.clone(),
        holdings: st.holdings.iter().map(|s| DeployHoldingDto { symbol: s.clone(), weight: if st.holdings.is_empty() {0.0} else {1.0/st.holdings.len() as f64}, since: st.last_date.clone().unwrap_or_default() }).collect(),
        nav_history: st.nav_history.iter().map(|p| DeployNavPointDto { t: p.t.clone(), nav: p.nav, bench_nav: p.bench_nav }).collect(),
        months: st.months.iter().map(|m| DeployMonthRecDto { as_of: m.as_of.clone(), nav: m.nav, excess: (m.nav - 1.0) - (m.bench_nav - 1.0), n_holdings: m.picks.len() as u32, n_buy: m.n_buy, n_sell: m.n_sell }).collect(),
    }
}

#[tauri::command]
pub fn deploy_run_month(state: tauri::State<AppState>, as_of: String) -> Result<String, String> {
    let ws = state.ws.clone();
    state.tasks.start("deploy_month", true, move |ctx| {
        ctx.progress(0.3, "选股", &as_of);
        let (dto, _st, _nav, _b) = compute_month(&ws, &as_of)?;
        serde_json::to_value(dto).map_err(|e| e.to_string())
    })
}

#[tauri::command]
pub fn deploy_commit_month(state: tauri::State<AppState>, as_of: String) -> Result<(), String> {
    let (dto, mut st, proj_nav, bench_nav) = compute_month(&state.ws, &as_of)?;
    let picks: Vec<String> = dto.picks.iter().map(|h| h.symbol.clone()).collect();
    let n_buy = dto.diff.iter().filter(|d| d.action == "Buy").count() as u32;
    let n_sell = dto.diff.iter().filter(|d| d.action == "Sell").count() as u32;
    if st.bench_base.is_none() { let idx = crate::index_relative::load_index(&state.ws.index_dir().join("csi300.csv"))?; st.bench_base = crate::index_relative::idx_at(&idx, &as_of); }
    st.nav = proj_nav;
    st.nav_history.push(crate::deploy_book::NavPoint { t: as_of.clone(), nav: proj_nav, bench_nav });
    st.months.push(crate::deploy_book::MonthRec { as_of: as_of.clone(), picks: picks.clone(), nav: proj_nav, bench_nav, n_buy, n_sell });
    st.holdings = picks;
    st.last_date = Some(as_of);
    crate::deploy_book::write_state(&state.ws.deploy_book_path(), &st)
}
```
*(注:`deploy_run_month` task result = DeployMonthDto;前端经 `task://progress` 取。`deploy_commit_month` 同步重算并落账——幂等于同 as_of 重复点会重复追加,前端在 commit 后禁用/刷新避免重复点。)*

- [ ] **Step 2: 注册 + 编译** lib.rs 加 `mod deploy_cmds;` + handler `deploy_cmds::deploy_book_read, deploy_cmds::deploy_run_month, deploy_cmds::deploy_commit_month`。`cargo build -p rquant-desktop 2>&1 | tail -15`;若 `ScreenRunConfig` 字段/`run_screen` 签名不符,读 `src/screen/mod.rs` 对齐(本计划据 sub-1 真实签名)。`cargo test -p rquant-desktop 2>&1 | tail -3` 绿。

- [ ] **Step 3: Commit**
```bash
git add desktop/src-tauri/src/deploy_cmds.rs desktop/src-tauri/src/lib.rs
git commit -F - <<'EOF'
feat(desktop): deploy commands (read book / run-month preview / commit-month)
EOF
```

---

## Task 5: 前端 IPC + 部署路由 + labels + 空壳 + 驾驶舱第4卡

**Files:** Modify `api/ipc.ts`、`App.tsx`、`labels.ts`、`pages/Cockpit.tsx`;Create `pages/Deploy.tsx`(空壳)、`components/ValueBookCard.tsx`

- [ ] **Step 1: api/ipc.ts 追加**:
```typescript
  // 部署
  deployBookRead: () => invoke<import("@bindings/DeployBookDto").DeployBookDto>("deploy_book_read"),
  deployRunMonth: (asOf: string) => invoke<string>("deploy_run_month", { asOf }),
  deployCommitMonth: (asOf: string) => invoke<void>("deploy_commit_month", { asOf }),
```

- [ ] **Step 2: 空壳 + 卡** `pages/Deploy.tsx`:`export default function Deploy(){return <div>部署(开发中)</div>;}`。`components/ValueBookCard.tsx`:
```tsx
import { useEffect, useState } from "react";
import { Card, Statistic, Row, Col } from "antd";
import { useNavigate } from "react-router-dom";
import { api } from "../api/ipc";
import type { DeployBookDto } from "@bindings/DeployBookDto";
export default function ValueBookCard() {
  const nav = useNavigate(); const [d, setD] = useState<DeployBookDto | null>(null);
  useEffect(() => { api.deployBookRead().then(setD).catch(() => {}); }, []);
  const pct = (v?: number | null) => (v == null ? "—" : `${(v*100).toFixed(1)}%`);
  return (
    <Card size="small" title="价值选股盘(纸面)" hoverable onClick={() => nav("/deploy")}
      extra={d?.status === "empty" ? "未建仓" : "跟踪中"}>
      {d?.status === "empty" || !d ? <span style={{ opacity: .6 }}>去部署页跑首月建仓 →</span> : (
        <Row gutter={12}>
          <Col><Statistic title="NAV" value={d.nav?.toFixed(3) ?? "—"} /></Col>
          <Col><Statistic title="超额(沪深300)" value={pct(d.excess_total)} /></Col>
          <Col><Statistic title="持仓" value={d.holdings.length} /></Col>
          <Col><Statistic title="上次调仓" value={d.last_rebalance ?? "—"} /></Col>
        </Row>
      )}
    </Card>
  );
}
```

- [ ] **Step 3: App.tsx** MODULES 加 `{ key: "deploy", label: "部署" }`(放 `data` 后);import Deploy;`<Route path="/deploy" element={<Deploy/>}/>`;占位过滤排除 `deploy`。

- [ ] **Step 4: Cockpit.tsx** READ it;在三本账本卡之后插入 `<ValueBookCard />`(import 之)。仅追加一卡,不动现有三本装配。

- [ ] **Step 5: labels.ts 追加**:`export const DEPLOY_TERM = { paper: "纸面盘", rebalance: "调仓", holdings: "持仓", excess: "超额", run: "跑本月", commit: "确认调仓" } as const;`

- [ ] **Step 6: 验证** `node desktop/ui/node_modules/typescript/bin/tsc --noEmit -p desktop/ui/tsconfig.app.json 2>&1 | tail -10` → 0;`npm --prefix desktop/ui run test -- --run 2>&1 | tail -6` → 全过。

- [ ] **Step 7: Commit**
```bash
git add desktop/ui/src/api/ipc.ts desktop/ui/src/App.tsx desktop/ui/src/labels.ts desktop/ui/src/pages/Cockpit.tsx desktop/ui/src/pages/Deploy.tsx desktop/ui/src/components/ValueBookCard.tsx
git commit -F - <<'EOF'
feat(ui): wire deploy IPC, 部署 route, cockpit value book card, labels
EOF
```

---

## Task 6: stores/deploy.ts + 测试

**Files:** Create `stores/deploy.ts`、`stores/deploy.test.ts`

- [ ] **Step 1: store**:
```typescript
import { create } from "zustand";
import type { DeployBookDto } from "@bindings/DeployBookDto";
import type { DeployMonthDto } from "@bindings/DeployMonthDto";
import { api as realApi, type Api } from "../api/ipc";
import { friendlyError } from "../errors";
interface DeployState {
  api: Api; book: DeployBookDto | null; preview: DeployMonthDto | null; error: string | null;
  load: () => Promise<void>; setPreview: (p: DeployMonthDto | null) => void;
  commit: (asOf: string) => Promise<void>;
}
export const useDeploy = create<DeployState>((set, get) => ({
  api: realApi, book: null, preview: null, error: null,
  load: async () => { try { set({ book: await get().api.deployBookRead() }); } catch { /* 静默 */ } },
  setPreview: (preview) => set({ preview }),
  commit: async (asOf) => {
    set({ error: null });
    try { await get().api.deployCommitMonth(asOf); set({ preview: null }); await get().load(); }
    catch (e) { set({ error: friendlyError(String(e)).title }); }
  },
}));
```

- [ ] **Step 2: 测试**`stores/deploy.test.ts`:
```typescript
import { test, expect, afterEach } from "vitest";
import { useDeploy } from "./deploy";
const real = useDeploy.getState().api;
afterEach(() => useDeploy.setState({ api: real, book: null, preview: null, error: null }));
test("commit clears preview and reloads book", async () => {
  let committed = ""; 
  useDeploy.setState({ api: { ...real, deployCommitMonth: async (a: string) => { committed = a; },
    deployBookRead: async () => ({ status: "ok", nav: 1.05, excess_total: 0.02, last_rebalance: "2026-06-30", holdings: [], nav_history: [], months: [] }) },
    preview: { as_of: "2026-06-30" } as any });
  await useDeploy.getState().commit("2026-06-30");
  expect(committed).toBe("2026-06-30");
  expect(useDeploy.getState().preview).toBeNull();
  expect(useDeploy.getState().book?.nav).toBe(1.05);
});
```

- [ ] **Step 3: 跑** `npm --prefix desktop/ui run test -- --run src/stores/deploy.test.ts 2>&1 | tail -6` → PASS;`tsc --noEmit` → 0。

- [ ] **Step 4: Commit**
```bash
git add desktop/ui/src/stores/deploy.ts desktop/ui/src/stores/deploy.test.ts
git commit -F - <<'EOF'
feat(ui): deploy store (book/preview/commit) + test
EOF
```

---

## Task 7: 部署页(Deploy + 跑本月 preview/confirm + journal + 持仓)

**Files:** Create `components/DeployHardeningNote`? 否。替换 `pages/Deploy.tsx`;Create `components/Deploy*` 内联即可。复用 `DiffTable`/`NavChart`。Test: `pages/Deploy.test.tsx`(可选,见下)。

**Interfaces — Consumes:** `useDeploy`、`DiffTable`(sub-1,props 见其源)、`NavChart`、`task://progress` 取 `deployRunMonth` 结果。

- [ ] **Step 1: Deploy.tsx**:
```tsx
import { useEffect, useState } from "react";
import { App as AntApp, Button, Card, Col, DatePicker, Row, Statistic, Table, Tabs } from "antd";
import { listen } from "@tauri-apps/api/event";
import { useDeploy } from "../stores/deploy";
import type { DeployMonthDto } from "@bindings/DeployMonthDto";
import DiffTable from "../components/DiffTable";
import NavChart from "../components/NavChart";

export default function Deploy() {
  const st = useDeploy(); const { message } = AntApp.useApp();
  const [asOf, setAsOf] = useState(""); const [running, setRunning] = useState(false);
  useEffect(() => { void st.load(); }, []);
  const pct = (v?: number | null) => (v == null ? "—" : `${(v*100).toFixed(1)}%`);

  async function runMonth() {
    if (!asOf) { message.warning("请选月末日期"); return; }
    setRunning(true);
    try {
      const taskId = await st.api.deployRunMonth(asOf);
      const un = await listen<{ id: string; status: string; result: DeployMonthDto | null }>("task://progress", (e) => {
        if (e.payload.id !== taskId) return;
        if (e.payload.status === "done") { st.setPreview(e.payload.result); setRunning(false); void un(); }
        else if (e.payload.status === "failed") { message.error("跑本月失败"); setRunning(false); void un(); } });
    } catch (e) { message.error(String(e)); setRunning(false); }
  }
  async function confirm() { if (st.preview) { await st.commit(st.preview.as_of); message.success("已调仓落账"); } }

  const b = st.book; const pv = st.preview;
  return (
    <Row gutter={12}>
      <Col span={9}>
        <Card size="small" title="价值选股盘(纸面 · 不下真单)">
          {b && b.status !== "empty" ? (
            <Row gutter={12}>
              <Col><Statistic title="NAV" value={b.nav?.toFixed(3) ?? "—"} /></Col>
              <Col><Statistic title="累计超额" value={pct(b.excess_total)} valueStyle={{ color: "#16a34a" }} /></Col>
              <Col><Statistic title="持仓" value={b.holdings.length} /></Col>
            </Row>
          ) : <span style={{ opacity: .6 }}>未建仓——选月末日期跑首月</span>}
          <div style={{ marginTop: 12 }}>
            <DatePicker onChange={(_, s) => setAsOf(((Array.isArray(s) ? s[0] : s) ?? "") as string)} />
            <Button type="primary" loading={running} style={{ marginLeft: 8 }} onClick={runMonth}>跑本月(预览)</Button>
          </div>
          {pv && (<Card size="small" title={`预览 ${pv.as_of}:拟 NAV ${pv.proj_nav.toFixed(3)} · 超额 ${pct(pv.proj_excess)} · 实现 ${pct(pv.realized_ret)}`} style={{ marginTop: 8 }}>
            <DiffTable rows={pv.diff} />
            <Button type="primary" danger block style={{ marginTop: 8 }} onClick={confirm}>确认调仓(落账)</Button>
          </Card>)}
        </Card>
      </Col>
      <Col span={15}>
        <Card size="small" title="NAV vs 沪深300">
          {b && b.nav_history.length ? <NavChart series={b.nav_history.map((p) => ({ t: p.t, v: p.nav }))} title="价值盘 NAV" /> : <span style={{ opacity: .6 }}>暂无净值,跑首月后显示</span>}
        </Card>
        <Card size="small" title="月度调仓" style={{ marginTop: 8 }}>
          <Table size="small" rowKey="as_of" pagination={false} dataSource={b?.months ?? []}
            columns={[{ title: "日期", dataIndex: "as_of" }, { title: "NAV", dataIndex: "nav", render: (v: number) => v.toFixed(3) },
              { title: "超额", dataIndex: "excess", render: pct }, { title: "持仓", dataIndex: "n_holdings" },
              { title: "买", dataIndex: "n_buy" }, { title: "卖", dataIndex: "n_sell" }]} />
        </Card>
      </Col>
    </Row>
  );
}
```
*(读 `components/DiffTable.tsx` 确认其 props=`{rows: DiffRowDto[]}`——sub-1 cockpit 用它渲染组合 diff;`NavChart` props 按其实际签名适配,若是 `{points}` 而非 `{series}` 则对齐。)*

- [ ] **Step 2: 验证** `node desktop/ui/node_modules/typescript/bin/tsc --noEmit -p desktop/ui/tsconfig.app.json 2>&1 | tail -12` → 0;`npm --prefix desktop/ui run test -- --run 2>&1 | tail -6` → 全过。

- [ ] **Step 3: Commit**
```bash
git add desktop/ui/src/pages/Deploy.tsx
git commit -F - <<'EOF'
feat(ui): 部署 page — value paper book, run-month preview/confirm, NAV vs index, journal
EOF
```

---

## Task 8: 收尾闸 + 文档 + 记忆 + finishing

- [ ] **Step 1: 全量后端闸** `cargo test --workspace 2>&1 | grep "test result"` → 全 ok。
- [ ] **Step 2: 前端闸** `npm --prefix desktop/ui run build` 成功 + `npm --prefix desktop/ui run test -- --run` 全过。
- [ ] **Step 3: 真数据冒烟** 启 `npm run tauri dev`:部署页选最新数据日 → 跑本月(预览)→ 下单清单(首月全 Buy 50)→ 确认 → NAV=1 建仓 + journal 一条;核对预览选股 top-50 与 `rquant screen --as-of <date> --config deploy/value_pb_deploy_frozen.yaml` CLI 一致(诚实对账)。
- [ ] **Step 4: 文档 + 记忆** `docs/desktop-screen-research.md` 加部署页一节;更新记忆 `rquant-project.md`(sub-3a 落地)。
- [ ] **Step 5: Commit**
```bash
git add docs/ && git commit -F - <<'EOF'
docs(desktop): value deploy book (sub-3a) usage; finalize
EOF
```
- [ ] **Step 6: finishing** 调用 superpowers:finishing-a-development-branch 收口。

---

## 自审备忘(写计划时已校)

- **类型一致**:`DeployBookDto`/`DeployMonthDto`/`DeployHoldingDto`/`DeployNavPointDto`/`DeployMonthRecDto` 新名全局唯一(≠ sub-2a `DeployDto`、≠ sub-1 `NavPointDto`);**复用** sub-1 `crate::dto::DiffRowDto`(diff)与前端 `DiffTable`/`NavChart`。命令名 snake↔camel 一致(asOf↔as_of)。
- **纪律**:纸面只跟踪 NAV、不下真单;`deploy_run_month` 预览不写、`deploy_commit_month` 才落账;screen 用冻结配置 `deploy/value_pb_deploy_frozen.yaml`;数据缺/as-of 超覆盖→友好报错。
- **复用**:`screen::run_screen`(冻结配置)、`index_relative`(沪深300 超额)、`TaskRegistry`、Cockpit 卡范式、DiffTable/NavChart。
- **范围**:数据管线监控留 3b;无自动排程(手动按钮);无真实交易;NAV go-live 前向(历史看选股回测)。
- **已知取舍**:同 as_of 重复 commit 会重复追加月度记录(前端 commit 后清 preview+刷新缓解;严格幂等留后续);NavChart 单序列展示价值盘 NAV(vs 沪深300 叠加留实现期按 NavChart 能力)。
