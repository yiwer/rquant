# 客户端重构 sub-2a「认证 & 分析」实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 给桌面端加 `认证`(eval 5 门槛裁决,消费已有 optimize 报告)、`因子工作台`(factor IC 分析,填实 `/factor` 占位)、以及挂在选股回测结果上的三分析器(行业归因/两腿/部署加固,Rust 端口)。

**Architecture:** 复用 sub-1 桌面范式(同步命令壳 + `TaskRegistry` 重任务 + ts-rs DTO + `api/ipc` + Zustand store)。factor/eval **直调 rquant 库**(`run_factor`/`verdict::certify`,均同步);三分析器**纯 Rust 后验算术**(新 `analyze.rs`,数值对拍 `analyze_*.py`);新域代码进新文件,现有不动。

**Tech Stack:** Rust 2024 + Tauri 2.11 + ts-rs 10 / React 18 + antd 6 + Vite 8 + Zustand 5 + ECharts 6 + Vitest 4。

## Global Constraints

- DTO 一律 `#[derive(Debug, Clone, Serialize, TS)]` + `#[ts(export)]`(可读回的加 `Deserialize`);ts-rs 导出到 `desktop/src-tauri/bindings/`,前端 `@bindings/<Name>`。**跨模块 DTO 命名全局唯一**(sub-1 教训:同名→bindings 互相覆盖)。
- Rust `i32`/`u32`/`usize`→TS `number`、`i64`→`bigint`;**前端要 number 的整数字段用 `i32`/`u32`**。
- 命令同步 `#[tauri::command] pub fn`;重计算走 `state.tasks.start(kind, heavy=true, |ctx| -> Result<serde_json::Value,String>)`;库调用同步直调(factor/verdict 非 async,无需 tokio)。
- **全中文** UI(保留 PB/PE/ROE/夏普/IC 专业术语);新词用 `labels.ts`。Tauri v2 命令参数 JS camelCase 自动映射 Rust snake_case。
- **不重判、不改引擎语义**:eval 用 `certify` + `GateThresholds::default()`;分析器纯算术。
- 英文 commit(`git commit -F -` heredoc);`git add` 显式列文件(禁 `-A`);收尾 `cargo test --workspace` + `cd desktop/ui` build/vitest。
- 数据列(铁律):`kday/<sym>.csv` = `time,open,high,low,close,volume,amount,turn,pctChg`;`sector_membership.csv` = `symbol,industry,classification,update_date`;`sector/<行业>.csv` = `time,ret,index,n,breadth`(用 `index` 列);`index/<bench>.csv` = `time,close`。screen run 归档 `net.json` = 序列化 `ScreenBacktestReport`,其 `holdings:[{t,nav,benchmark_nav,selected:[[symbol,score]]}]`。

## 文件结构

**Rust 桥(`desktop/src-tauri/src/`)**
- Create `analyze.rs` — 三分析器纯算术(sector/twoleg/deploy)+ 单测。
- Create `analyze_cmds.rs` — 三分析命令(读 screen run + 数据 → 调 analyze.rs)。
- Create `factor_cmds.rs` — `factor_run`(调 `rquant::factor::run_factor`)。
- Create `eval_cmds.rs` — `eval_list_reports` + `eval_certify`。
- Create `dto_factor.rs` / `dto_eval.rs` / `dto_analyze.rs`。
- Modify `paths.rs` — 加 `daily_runs_dir()`/`sector_dir()`/`sector_membership_path()`/`kday_dir()`。
- Modify `lib.rs` — `mod` 声明 + `generate_handler!` 追加 8 命令。

**前端(`desktop/ui/src/`)**
- Modify `App.tsx`(加 `认证`/`因子工作台` 两路由)、`api/ipc.ts`、`labels.ts`。
- Create `stores/{factor,verdict}.ts`。
- Create `pages/{Factor,Verdict}.tsx`。
- Create `components/{FactorReport,VerdictMatrix,SectorAttrib,TwoLegBlend,DeployHardening}.tsx`(+ 测试)。
- Modify `components/ScreenBacktestResult.tsx`(加「分析」tab 组)。

---

## Task 1: 路径助手 + analyze.rs 骨架与 sector 归因(纯逻辑 TDD)

**Files:** Create `desktop/src-tauri/src/analyze.rs`；Modify `desktop/src-tauri/src/paths.rs`、`lib.rs`(`pub mod analyze;`)

**Interfaces — Produces:**
- `paths`: `Workspace::{daily_runs_dir()->.daily_runs, sector_dir()->data/baostock/sector, sector_membership_path()->data/baostock/sector_membership.csv, kday_dir()->data/baostock/kday}`。
- `analyze::sector_attribution(rebals: &[(String, Vec<String>)], price: &dyn Fn(&str,&str)->Option<f64>, sector_of: &HashMap<String,String>, sector_lvl: &dyn Fn(&str,&str)->Option<f64>, bench: &dyn Fn(&str)->Option<f64>) -> SectorAttrib`,`SectorAttrib { excess_total:f64, alloc_pct:f64, select_pct:f64, cum: Vec<(String,f64,f64,f64)> }`(每点 (day, r_p, r_alloc, r_bench) 累计减 1)。

- [ ] **Step 1: paths 方法**(`paths.rs` impl Workspace 内,仿现有):
```rust
pub fn daily_runs_dir(&self) -> PathBuf { self.root.join(".daily_runs") }
pub fn sector_dir(&self) -> PathBuf { self.root.join("data").join("baostock").join("sector") }
pub fn sector_membership_path(&self) -> PathBuf { self.root.join("data").join("baostock").join("sector_membership.csv") }
pub fn kday_dir(&self) -> PathBuf { self.root.join("data").join("baostock").join("kday") }
```

- [ ] **Step 2: 写失败测试**(`analyze.rs`,端口自 `analyze_sector.py`:EW 组合;r_p 用逐股价、r_alloc 用板块 index 按行业权、r_b 用基准;缺板块贡献 0):
```rust
#[cfg(test)]
mod sector_tests {
    use super::*;
    use std::collections::HashMap;
    #[test]
    fn brinson_alloc_select_split() {
        // 两个调仓点;两股 A(行业甲)/B(行业乙);EW=0.5 each。
        // 价: A 10→11(+10%), B 10→12(+20%) → r_p = .5*.1+.5*.2 = +15%
        // 板块 index: 甲 100→110(+10%), 乙 100→110(+10%);权 甲.5/乙.5 → r_a = +10%
        // 基准 100→105 → r_b = +5%
        let rebals = vec![("2024-01-02".to_string(), vec!["A".to_string(),"B".to_string()]),
                          ("2024-02-02".to_string(), vec!["A".to_string(),"B".to_string()])];
        let px: HashMap<(&str,&str),f64> = HashMap::from([
            (("A","2024-01-02"),10.0),(("A","2024-02-02"),11.0),
            (("B","2024-01-02"),10.0),(("B","2024-02-02"),12.0)]);
        let price = |s:&str,d:&str| px.get(&(s,d)).copied();
        let sector_of = HashMap::from([("A".to_string(),"甲".to_string()),("B".to_string(),"乙".to_string())]);
        let slv: HashMap<(&str,&str),f64> = HashMap::from([
            (("甲","2024-01-02"),100.0),(("甲","2024-02-02"),110.0),
            (("乙","2024-01-02"),100.0),(("乙","2024-02-02"),110.0)]);
        let sector_lvl = |s:&str,d:&str| slv.get(&(s,d)).copied();
        let bm: HashMap<&str,f64> = HashMap::from([("2024-01-02",100.0),("2024-02-02",105.0)]);
        let bench = |d:&str| bm.get(d).copied();
        let r = sector_attribution(&rebals, &price, &sector_of, &sector_lvl, &bench);
        assert!((r.excess_total - 0.10).abs() < 1e-9);   // r_p .15 - r_b .05
        assert!((r.alloc_pct - 0.5).abs() < 1e-9);       // (r_a-r_b)/(r_p-r_b) = .05/.10
        assert!((r.select_pct - 0.5).abs() < 1e-9);      // (r_p-r_a)/(r_p-r_b)
    }
}
```

- [ ] **Step 3: 跑确认失败** `cargo test -p rquant-desktop sector_tests` → FAIL(`sector_attribution` 未定义)。

- [ ] **Step 4: 实现**(`analyze.rs` 顶部):
```rust
//! 后验分析器(纯算术,无裁决):端口自 scripts/analyze_{sector,twoleg,deploy}.py。
use std::collections::HashMap;

pub struct SectorAttrib { pub excess_total: f64, pub alloc_pct: f64, pub select_pct: f64, pub cum: Vec<(String, f64, f64, f64)> }

pub fn sector_attribution(
    rebals: &[(String, Vec<String>)],
    price: &dyn Fn(&str, &str) -> Option<f64>,
    sector_of: &HashMap<String, String>,
    sector_lvl: &dyn Fn(&str, &str) -> Option<f64>,
    bench: &dyn Fn(&str) -> Option<f64>,
) -> SectorAttrib {
    let (mut nav_p, mut nav_a, mut nav_b) = (1.0_f64, 1.0_f64, 1.0_f64);
    let mut cum = Vec::new();
    for i in 0..rebals.len().saturating_sub(1) {
        let (t0, sel) = (&rebals[i].0, &rebals[i].1);
        let t1 = &rebals[i + 1].0;
        if sel.is_empty() { continue; }
        let w = 1.0 / sel.len() as f64;
        // r_p: 逐股等权
        let mut rp = 0.0;
        for s in sel {
            if let (Some(p0), Some(p1)) = (price(s, t0), price(s, t1)) {
                if p0 > 0.0 { rp += w * (p1 / p0 - 1.0); }
            }
        }
        // r_a: 行业权 × 板块 index 收益(缺板块贡献 0)
        let mut sec_w: HashMap<&str, f64> = HashMap::new();
        for s in sel { if let Some(ind) = sector_of.get(s) { *sec_w.entry(ind.as_str()).or_default() += w; } }
        let mut ra = 0.0;
        for (ind, sw) in &sec_w {
            if let (Some(l0), Some(l1)) = (sector_lvl(ind, t0), sector_lvl(ind, t1)) {
                if l0 > 0.0 { ra += sw * (l1 / l0 - 1.0); }
            }
        }
        // r_b
        let rb = match (bench(t0), bench(t1)) { (Some(b0), Some(b1)) if b0 > 0.0 => b1 / b0 - 1.0, _ => 0.0 };
        nav_p *= 1.0 + rp; nav_a *= 1.0 + ra; nav_b *= 1.0 + rb;
        cum.push((t1.clone(), nav_p - 1.0, nav_a - 1.0, nav_b - 1.0));
    }
    let (rp, ra, rb) = (nav_p - 1.0, nav_a - 1.0, nav_b - 1.0);
    let excess = rp - rb;
    let (alloc_pct, select_pct) = if excess.abs() > 1e-12 { ((ra - rb) / excess, (rp - ra) / excess) } else { (0.0, 0.0) };
    SectorAttrib { excess_total: excess, alloc_pct, select_pct, cum }
}
```
加 `pub mod analyze;` 到 `lib.rs`。

- [ ] **Step 5: 跑确认通过** `cargo test -p rquant-desktop sector_tests` → PASS。

- [ ] **Step 6: Commit**
```bash
git add desktop/src-tauri/src/analyze.rs desktop/src-tauri/src/paths.rs desktop/src-tauri/src/lib.rs
git commit -F - <<'EOF'
feat(desktop): analyze.rs sector attribution (Brinson alloc/selection) + paths
EOF
```

---

## Task 2: analyze.rs 两腿混合(纯逻辑 TDD)

**Files:** Modify `desktop/src-tauri/src/analyze.rs`

**Interfaces — Produces:** `analyze::two_leg(v_nav: &[(String,f64)], g_nav: &[(String,f64)], idx: &BTreeMap<String,f64>, regimes: &[(String,String,String)]) -> TwoLeg`,`TwoLeg { rows: Vec<TwoLegCell>, best_w: f64 }`,`TwoLegCell { w:f64, net_total:f64, excess:f64, oos_excess:Option<f64>, sharpe:f64, max_dd:f64 }`。端口自 `analyze_twoleg.py`:对齐两腿 nav(按日期交集)→ 段收益 `vseg/gseg` → 每 w 混合 `br=w·v+(1-w)·g` 累计 → 年化夏普(×√12)、最大回撤、净总、vs 指数全程超额、OOS 窗超额;best=minmax(sharpe)+minmax(oos) 等权打分最大。w∈{1.0,0.8,0.7,0.6,0.5,0.4,0.3,0.0}。

- [ ] **Step 1: 写失败测试**(对齐 + w=1 复现价值腿、w=0 复现成长腿):
```rust
#[cfg(test)]
mod twoleg_tests {
    use super::*;
    use std::collections::BTreeMap;
    #[test]
    fn endpoints_recover_each_leg() {
        let v = vec![("2024-01-02".into(),1.0),("2024-06-28".into(),1.2),("2024-12-31".into(),1.5)];
        let g = vec![("2024-01-02".into(),1.0),("2024-06-28".into(),1.1),("2024-12-31".into(),1.3)];
        let idx = BTreeMap::from([("2024-01-02".to_string(),100.0),("2024-12-31".to_string(),110.0)]);
        let r = two_leg(&v, &g, &idx, &[]);
        let w1 = r.rows.iter().find(|c| (c.w-1.0).abs()<1e-9).unwrap();
        let w0 = r.rows.iter().find(|c| c.w.abs()<1e-9).unwrap();
        assert!((w1.net_total - 0.5).abs() < 1e-9);  // 价值腿 1.0→1.5
        assert!((w0.net_total - 0.3).abs() < 1e-9);  // 成长腿 1.0→1.3
        assert!((w1.excess - 0.4).abs() < 1e-9);     // .5 - 指数 .1
    }
}
```

- [ ] **Step 2: 跑确认失败** `cargo test -p rquant-desktop twoleg_tests` → FAIL。

- [ ] **Step 3: 实现**(`analyze.rs` 追加;复用 `crate::index_relative::idx_at`):
```rust
use std::collections::BTreeMap;
pub struct TwoLegCell { pub w: f64, pub net_total: f64, pub excess: f64, pub oos_excess: Option<f64>, pub sharpe: f64, pub max_dd: f64 }
pub struct TwoLeg { pub rows: Vec<TwoLegCell>, pub best_w: f64 }

pub fn two_leg(v_nav: &[(String, f64)], g_nav: &[(String, f64)], idx: &BTreeMap<String, f64>, regimes: &[(String, String, String)]) -> TwoLeg {
    let gmap: BTreeMap<&str, f64> = g_nav.iter().map(|(d, v)| (d.as_str(), *v)).collect();
    let aligned: Vec<(String, f64, f64)> = v_nav.iter().filter_map(|(d, vv)| gmap.get(d.as_str()).map(|gv| (d.clone(), *vv, *gv))).collect();
    if aligned.len() < 2 { return TwoLeg { rows: vec![], best_w: 1.0 }; }
    let vseg: Vec<f64> = (0..aligned.len()-1).map(|i| aligned[i+1].1/aligned[i].1 - 1.0).collect();
    let gseg: Vec<f64> = (0..aligned.len()-1).map(|i| aligned[i+1].2/aligned[i].2 - 1.0).collect();
    let days: Vec<String> = aligned.iter().map(|(d,_,_)| d.clone()).collect();
    let oos_lbl = regimes.iter().find(|(l,_,_)| l.contains("OOS")).map(|(l,_,_)| l.clone());
    let win = |nav: &[(String,f64)], d0: &str, d1: &str| -> Option<f64> {
        let sub: Vec<&(String,f64)> = nav.iter().filter(|(d,_)| d0 <= d.as_str() && d.as_str() <= d1).collect();
        if sub.len() < 2 { return None; }
        let sr = sub.last().unwrap().1 / sub[0].1 - 1.0;
        match (crate::index_relative::idx_at(idx, &sub[0].0), crate::index_relative::idx_at(idx, &sub.last().unwrap().0)) {
            (Some(x0), Some(x1)) if x0 != 0.0 => Some(sr - (x1/x0 - 1.0)), _ => None }
    };
    let mut rows = Vec::new();
    for w in [1.0, 0.8, 0.7, 0.6, 0.5, 0.4, 0.3, 0.0] {
        let mut nav = vec![(days[0].clone(), 1.0)]; let mut cur = 1.0;
        let mut rets = Vec::new();
        for i in 0..vseg.len() { let r = w*vseg[i] + (1.0-w)*gseg[i]; rets.push(r); cur *= 1.0+r; nav.push((days[i+1].clone(), cur)); }
        let mean = rets.iter().sum::<f64>()/rets.len() as f64;
        let var = rets.iter().map(|r| (r-mean).powi(2)).sum::<f64>()/(rets.len()-1).max(1) as f64;
        let sd = var.sqrt();
        let sharpe = if sd > 0.0 { mean/sd*(12.0_f64).sqrt() } else { 0.0 };
        let (mut peak, mut dd) = (0.0_f64, 0.0_f64);
        for (_, vv) in &nav { peak = peak.max(*vv); dd = dd.max(1.0 - vv/peak); }
        let total = nav.last().unwrap().1 - 1.0;
        let excess = win(&nav, &days[0], days.last().unwrap()).unwrap_or(total);
        let oos = oos_lbl.as_ref().and_then(|l| regimes.iter().find(|(rl,_,_)| rl==l)).and_then(|(_,f,t)| win(&nav, f, t));
        rows.push(TwoLegCell { w, net_total: total, excess, oos_excess: oos, sharpe, max_dd: dd });
    }
    // best = minmax(sharpe)+minmax(oos) 等权
    let shs: Vec<f64> = rows.iter().map(|r| r.sharpe).collect();
    let ooss: Vec<f64> = rows.iter().map(|r| r.oos_excess.unwrap_or(0.0)).collect();
    let nz = |x:f64, lo:f64, hi:f64| if hi>lo {(x-lo)/(hi-lo)} else {0.5};
    let (slo,shi) = (shs.iter().cloned().fold(f64::INFINITY,f64::min), shs.iter().cloned().fold(f64::NEG_INFINITY,f64::max));
    let (olo,ohi) = (ooss.iter().cloned().fold(f64::INFINITY,f64::min), ooss.iter().cloned().fold(f64::NEG_INFINITY,f64::max));
    let best_w = rows.iter().max_by(|a,b| {
        let sa = nz(a.sharpe,slo,shi)+nz(a.oos_excess.unwrap_or(0.0),olo,ohi);
        let sb = nz(b.sharpe,slo,shi)+nz(b.oos_excess.unwrap_or(0.0),olo,ohi);
        sa.partial_cmp(&sb).unwrap() }).map(|c| c.w).unwrap_or(1.0);
    TwoLeg { rows, best_w }
}
```

- [ ] **Step 4: 跑确认通过** `cargo test -p rquant-desktop twoleg_tests` → PASS。

- [ ] **Step 5: Commit**
```bash
git add desktop/src-tauri/src/analyze.rs
git commit -F - <<'EOF'
feat(desktop): analyze.rs two-leg value×growth blend sweep
EOF
```

---

## Task 3: analyze.rs 部署加固 T+1 + 容量(纯逻辑 TDD)

**Files:** Modify `desktop/src-tauri/src/analyze.rs`

**Interfaces — Produces:** `analyze::deploy(rebals: &[(String, Vec<String>)], price: &dyn Fn(&str,&str)->Option<f64>, adv: &dyn Fn(&str,&str)->Option<f64>, bench: &dyn Fn(&str)->Option<f64>, build_days: f64) -> Deploy`,`Deploy { lag0_excess:f64, lag1_excess:f64, drag:f64, adv_median:f64, capacity: Vec<(f64,f64)> }`(capacity=(adv_pct, max_aum))。端口自 `analyze_deploy.py`:`replay(lag)` EW 再平衡 + 换手单边成本(RATE=0.001),lag0 用 close[T]/[T+1] 段、lag1 整体后移 1 bar;超额 vs 指数;容量 = N×%ADV×worst_min×build_days,%ADV∈{0.05,0.10,0.20};RATE=COST/2/1e4,COST=20。

- [ ] **Step 1: 写失败测试**(单调:lag1 拖累 = lag1−lag0;容量随 %ADV 线性):
```rust
#[cfg(test)]
mod deploy_tests {
    use super::*;
    use std::collections::HashMap;
    #[test]
    fn capacity_scales_with_adv_pct() {
        let rebals = vec![("2024-01-02".into(), vec!["A".to_string()]), ("2024-02-02".into(), vec!["A".to_string()]), ("2024-03-04".into(), vec!["A".to_string()])];
        let px: HashMap<(&str,&str),f64> = HashMap::from([(("A","2024-01-02"),10.0),(("A","2024-02-02"),11.0),(("A","2024-03-04"),12.0)]);
        let price = |s:&str,d:&str| px.get(&(s,d)).copied();
        let adv = |_s:&str,_d:&str| Some(1.0e8_f64); // 1亿/日
        let bm: HashMap<&str,f64> = HashMap::from([("2024-01-02",100.0),("2024-02-02",100.0),("2024-03-04",100.0)]);
        let bench = |d:&str| bm.get(d).copied();
        let r = deploy(&rebals, &price, &adv, &bench, 1.0);
        // N=1,worst_min=1e8;cap@10% = 1×0.10×1e8×1 = 1e7
        let c10 = r.capacity.iter().find(|(p,_)| (p-0.10).abs()<1e-9).unwrap();
        assert!((c10.1 - 1.0e7).abs() < 1.0);
        let c20 = r.capacity.iter().find(|(p,_)| (p-0.20).abs()<1e-9).unwrap();
        assert!((c20.1 - 2.0e7).abs() < 1.0); // 线性
    }
}
```

- [ ] **Step 2: 跑确认失败** `cargo test -p rquant-desktop deploy_tests` → FAIL。

- [ ] **Step 3: 实现**(`analyze.rs` 追加;`replay` 内 EW+换手成本;lag1 = 段用 [t1→t2] 价):
```rust
pub struct Deploy { pub lag0_excess: f64, pub lag1_excess: f64, pub drag: f64, pub adv_median: f64, pub capacity: Vec<(f64, f64)> }
const DEPLOY_RATE: f64 = 20.0 / 2.0 / 1.0e4; // 单边

fn replay_nav(rebals: &[(String, Vec<String>)], price: &dyn Fn(&str,&str)->Option<f64>, lag: usize) -> Vec<(String, f64)> {
    let mut nav = 1.0_f64; let mut out = Vec::new();
    let mut prev: Vec<String> = Vec::new();
    for i in 0..rebals.len().saturating_sub(1 + lag) {
        let sel = &rebals[i].1;
        // 换手成本(与上期权重差,EW)
        let wn = if sel.is_empty() {0.0} else {1.0/sel.len() as f64};
        let wp = if prev.is_empty() {0.0} else {1.0/prev.len() as f64};
        let mut names: std::collections::BTreeSet<&str> = sel.iter().map(|s| s.as_str()).collect();
        for s in &prev { names.insert(s.as_str()); }
        let tov: f64 = names.iter().map(|s| {
            let a = if sel.iter().any(|x| x==s) {wn} else {0.0};
            let b = if prev.iter().any(|x| x==s) {wp} else {0.0};
            (a-b).abs() }).sum();
        nav *= 1.0 - DEPLOY_RATE * tov;
        // 段收益:lag0 用 [t_i→t_{i+1}];lag1 用 [t_{i+1}→t_{i+2}]
        let (d0, d1) = (&rebals[i+lag].0, &rebals[i+1+lag].0);
        let w = wn;
        let mut r = 0.0;
        for s in sel { if let (Some(p0),Some(p1)) = (price(s,d0), price(s,d1)) { if p0>0.0 { r += w*(p1/p0-1.0); } } }
        nav *= 1.0 + r;
        out.push((rebals[i+1].0.clone(), nav));
        prev = sel.clone();
    }
    out
}

pub fn deploy(rebals: &[(String, Vec<String>)], price: &dyn Fn(&str,&str)->Option<f64>, adv: &dyn Fn(&str,&str)->Option<f64>, bench: &dyn Fn(&str)->Option<f64>, build_days: f64) -> Deploy {
    let excess_of = |nav: &[(String,f64)]| -> f64 {
        if nav.len() < 2 { return 0.0; }
        let sr = nav.last().unwrap().1 / 1.0 - 1.0;
        match (bench(&nav[0].0), bench(&nav.last().unwrap().0)) { (Some(b0),Some(b1)) if b0>0.0 => sr - (b1/b0-1.0), _ => sr }
    };
    let nav0 = replay_nav(rebals, price, 0);
    let nav1 = replay_nav(rebals, price, 1);
    let (e0, e1) = (excess_of(&nav0), excess_of(&nav1));
    // 容量:每调仓持仓名 worst ADV → 跨调仓取 min
    let mut per_reb_min = Vec::new(); let mut n_typ = 0usize;
    for (t, sel) in rebals { if sel.is_empty() { continue; } n_typ = n_typ.max(sel.len());
        let advs: Vec<f64> = sel.iter().filter_map(|s| adv(s, t)).collect();
        if let Some(m) = advs.iter().cloned().fold(None, |acc:Option<f64>,x| Some(acc.map_or(x, |a| a.min(x)))) { per_reb_min.push(m); } }
    let worst_min = per_reb_min.iter().cloned().fold(f64::INFINITY, f64::min);
    let worst_min = if worst_min.is_finite() { worst_min } else { 0.0 };
    let mut sorted = per_reb_min.clone(); sorted.sort_by(|a,b| a.partial_cmp(b).unwrap());
    let adv_median = if sorted.is_empty() {0.0} else { sorted[sorted.len()/2] };
    let capacity = [0.05, 0.10, 0.20].iter().map(|p| (*p, n_typ as f64 * p * worst_min * build_days)).collect();
    Deploy { lag0_excess: e0, lag1_excess: e1, drag: e1 - e0, adv_median, capacity }
}
```

- [ ] **Step 4: 跑确认通过** `cargo test -p rquant-desktop deploy_tests` → PASS;`cargo test -p rquant-desktop analyze` 三组全过。

- [ ] **Step 5: Commit**
```bash
git add desktop/src-tauri/src/analyze.rs
git commit -F - <<'EOF'
feat(desktop): analyze.rs deploy hardening (T+1 drag + capacity)
EOF
```

---

## Task 4: DTO(factor / eval / analyze)

**Files:** Create `dto_factor.rs` / `dto_eval.rs` / `dto_analyze.rs`;Modify `lib.rs`(三 `pub mod`)

- [ ] **Step 1: dto_factor.rs**(镜像 FactorReport/FactorStats/LayerStats/CorrMatrix 关键字段):
```rust
use serde::Serialize; use ts_rs::TS;
#[derive(Debug,Clone,Serialize,TS)] #[ts(export)]
pub struct DecayPointDto { pub horizon: u32, pub rank_ic: Option<f64> }
#[derive(Debug,Clone,Serialize,TS)] #[ts(export)]
pub struct LayerStatsDto { pub q: u32, pub ann_returns: Vec<Option<f64>>, pub spread_total: f64, pub spread_sharpe: Option<f64>, pub monotonicity: Option<f64> }
#[derive(Debug,Clone,Serialize,TS)] #[ts(export)]
pub struct FactorStatsDto { pub name: String, pub expr: String, pub n_periods: u32, pub ic_mean: Option<f64>, pub icir: Option<f64>, pub ic_t: Option<f64>, pub ic_pos_share: Option<f64>, pub rank_ic_mean: Option<f64>, pub rank_icir: Option<f64>, pub ic_decay: Vec<DecayPointDto>, pub layers: Option<LayerStatsDto> }
#[derive(Debug,Clone,Serialize,TS)] #[ts(export)]
pub struct CorrDto { pub names: Vec<String>, pub values: Vec<Vec<Option<f64>>> }
#[derive(Debug,Clone,Serialize,TS)] #[ts(export)]
pub struct FactorReportDto { pub n_symbols: u32, pub sample: u32, pub horizon: u32, pub layers_q: u32, pub factors: Vec<FactorStatsDto>, pub corr: Option<CorrDto> }
```

- [ ] **Step 2: dto_eval.rs**:
```rust
use serde::Serialize; use ts_rs::TS;
#[derive(Debug,Clone,Serialize,TS)] #[ts(export)]
pub struct OptimizeReportInfoDto { pub path: String, pub name: Option<String>, pub mode: Option<String>, pub n_combos: Option<u32>, pub folds: Option<u32>, pub error: Option<String> }
#[derive(Debug,Clone,Serialize,TS)] #[ts(export)]
pub struct GateOutcomeDto { pub gate: String, pub status: String, pub value: f64, pub threshold: f64, pub note: String }
#[derive(Debug,Clone,Serialize,TS)] #[ts(export)]
pub struct VerdictDto { pub strategy: String, pub n_symbols: u32, pub certified: bool, pub gates: Vec<GateOutcomeDto>, pub failed_gates: Vec<String> }
```

- [ ] **Step 3: dto_analyze.rs**:
```rust
use serde::Serialize; use ts_rs::TS;
#[derive(Debug,Clone,Serialize,TS)] #[ts(export)]
pub struct SectorCumDto { pub t: String, pub r_p: f64, pub r_alloc: f64, pub r_bench: f64 }
#[derive(Debug,Clone,Serialize,TS)] #[ts(export)]
pub struct SectorAttribDto { pub excess_total: f64, pub alloc_pct: f64, pub select_pct: f64, pub cum: Vec<SectorCumDto> }
#[derive(Debug,Clone,Serialize,TS)] #[ts(export)]
pub struct TwoLegCellDto { pub w: f64, pub net_total: f64, pub excess: f64, pub oos_excess: Option<f64>, pub sharpe: f64, pub max_dd: f64 }
#[derive(Debug,Clone,Serialize,TS)] #[ts(export)]
pub struct TwoLegDto { pub rows: Vec<TwoLegCellDto>, pub best_w: f64 }
#[derive(Debug,Clone,Serialize,TS)] #[ts(export)]
pub struct CapacityRowDto { pub adv_pct: f64, pub max_aum: f64 }
#[derive(Debug,Clone,Serialize,TS)] #[ts(export)]
pub struct DeployDto { pub lag0_excess: f64, pub lag1_excess: f64, pub drag: f64, pub adv_median: f64, pub capacity: Vec<CapacityRowDto> }
```

- [ ] **Step 4: 声明 + bindings** 加三 `pub mod` 到 lib.rs;`cargo test -p rquant-desktop 2>&1 | tail -3`(ts-rs 导出);确认 `bindings/{FactorReportDto,VerdictDto,SectorAttribDto,TwoLegDto,DeployDto}.ts` 生成。

- [ ] **Step 5: Commit**
```bash
git add desktop/src-tauri/src/dto_factor.rs desktop/src-tauri/src/dto_eval.rs desktop/src-tauri/src/dto_analyze.rs desktop/src-tauri/src/lib.rs desktop/src-tauri/bindings/
git commit -F - <<'EOF'
feat(desktop): factor/eval/analyze DTOs + ts-rs bindings
EOF
```

---

## Task 5: eval 命令(列报告 + 认证)

**Files:** Create `eval_cmds.rs`;Modify `lib.rs`(`mod` + 注册 2 命令)

**Interfaces — Consumes:** `dto_eval::*`、`rquant::optimize::OptimizeReport`(Deserialize)、`rquant::verdict::{certify, GateThresholds}`、`paths::Workspace::daily_runs_dir`。

- [ ] **Step 1: 实现**(`eval_cmds.rs`):
```rust
use crate::commands::AppState;
use crate::dto_eval::*;

#[tauri::command]
pub fn eval_list_reports(state: tauri::State<AppState>) -> Vec<OptimizeReportInfoDto> {
    let mut out = Vec::new();
    for dir in [state.ws.daily_runs_dir(), state.ws.root().to_path_buf()] {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("json") { continue }
            let Ok(txt) = std::fs::read_to_string(&p) else { continue };
            match serde_json::from_str::<rquant::optimize::OptimizeReport>(&txt) {
                Ok(r) => {
                    let rel = p.strip_prefix(state.ws.root()).unwrap_or(&p).to_string_lossy().replace('\\', "/");
                    out.push(OptimizeReportInfoDto { path: rel,
                        name: if r.primary.is_empty() { p.file_stem().and_then(|s| s.to_str()).map(String::from) } else { Some(r.primary.clone()) },
                        mode: Some(r.mode.clone()), n_combos: Some(r.n_combos as u32), folds: Some(r.folds as u32), error: None });
                }
                Err(_) => { /* 非 optimize 报告,跳过 */ }
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

#[tauri::command]
pub fn eval_certify(state: tauri::State<AppState>, paths: Vec<String>, name: String) -> Result<VerdictDto, String> {
    let mut loaded = Vec::new();
    for rel in &paths {
        let abs = state.ws.root().join(rel);
        let txt = std::fs::read_to_string(&abs).map_err(|e| e.to_string())?;
        let r: rquant::optimize::OptimizeReport = serde_json::from_str(&txt).map_err(|e| format!("非有效 optimize 报告 {rel}: {e}"))?;
        let sym = if r.primary.is_empty() { rel.clone() } else { r.primary.clone() };
        loaded.push((sym, r));
    }
    if loaded.is_empty() { return Err("未选择任何 optimize 报告".into()); }
    let strategy = if name.trim().is_empty() { loaded[0].0.clone() } else { name };
    let v = rquant::verdict::certify(&loaded, &strategy, &rquant::verdict::GateThresholds::default());
    Ok(VerdictDto {
        strategy: v.strategy, n_symbols: v.n_symbols as u32, certified: v.certified,
        gates: v.gates.iter().map(|g| GateOutcomeDto {
            gate: g.gate.clone(),
            status: serde_json::to_value(g.status).ok().and_then(|x| x.as_str().map(String::from)).unwrap_or_default(),
            value: g.value, threshold: g.threshold, note: g.note.clone() }).collect(),
        failed_gates: v.failed_gates.clone(),
    })
}
```
*(GateStatus 是 `#[serde(rename_all="lowercase")]` 枚举 → 经 serde_json 转 "pass"/"fail"/"indeterminate" 字符串入 DTO。)*

- [ ] **Step 2: 注册 + 编译** lib.rs 加 `mod eval_cmds;` + handler `eval_cmds::eval_list_reports, eval_cmds::eval_certify`。`cargo build -p rquant-desktop 2>&1 | tail -10`;若 `OptimizeReport`/`certify`/`GateThresholds` 路径或字段不符,读 `src/optimize/mod.rs`/`src/verdict/mod.rs` 对齐(本计划据真实签名编写,应直接通过)。`cargo test -p rquant-desktop 2>&1 | tail -3` 全绿。

- [ ] **Step 3: Commit**
```bash
git add desktop/src-tauri/src/eval_cmds.rs desktop/src-tauri/src/lib.rs
git commit -F - <<'EOF'
feat(desktop): eval commands (list optimize reports + certify via verdict)
EOF
```

---

## Task 6: factor 命令

**Files:** Create `factor_cmds.rs`;Modify `lib.rs`

**Interfaces — Consumes:** `rquant::factor::{run_factor, FactorConfig, FactorSpecItem}`(同步)、`dto_factor::*`。

- [ ] **Step 1: 实现**(`factor_cmds.rs`;任务跑——factor 在 1073 上较重):
```rust
use crate::commands::AppState;
use crate::dto_factor::*;

#[tauri::command]
pub fn factor_run(state: tauri::State<AppState>, factors: Vec<(String, String)>, horizon: u32, layers: u32, sample: u32) -> Result<String, String> {
    let ws = state.ws.clone();
    if factors.is_empty() { return Err("请至少添加一个因子表达式".into()); }
    state.tasks.start("factor", true, move |ctx| {
        ctx.progress(0.2, "因子", "");
        let tmp = ws.root().join(".rquant-desktop").join("factor_report.json");
        std::fs::create_dir_all(tmp.parent().unwrap()).map_err(|e| e.to_string())?;
        let cfg = rquant::factor::FactorConfig {
            universe_path: ws.root().join("data/baostock/universe_baostock_day.csv"),
            factors: factors.into_iter().map(|(name, expr)| rquant::factor::FactorSpecItem { name, expr }).collect(),
            sample: sample as usize, horizon: horizon as usize, layers: layers as usize,
            warmup: 260, window: 260, out_path: tmp, html_path: None, membership_path: None,
        };
        let rep = rquant::factor::run_factor(&cfg).map_err(|e| e.to_string())?;
        let dto = FactorReportDto {
            n_symbols: rep.n_symbols as u32, sample: rep.sample as u32, horizon: rep.horizon as u32, layers_q: rep.layers_q as u32,
            factors: rep.factors.iter().map(|f| FactorStatsDto {
                name: f.name.clone(), expr: f.expr.clone(), n_periods: f.n_periods as u32,
                ic_mean: f.ic_mean, icir: f.icir, ic_t: f.ic_t, ic_pos_share: f.ic_pos_share,
                rank_ic_mean: f.rank_ic_mean, rank_icir: f.rank_icir,
                ic_decay: f.ic_decay.iter().map(|(h, v)| DecayPointDto { horizon: *h as u32, rank_ic: *v }).collect(),
                layers: f.layers.as_ref().map(|l| LayerStatsDto { q: l.q as u32, ann_returns: l.ann_returns.clone(), spread_total: l.spread_total, spread_sharpe: l.spread_sharpe, monotonicity: l.monotonicity }),
            }).collect(),
            corr: rep.corr.as_ref().map(|c| CorrDto { names: c.names.clone(), values: c.values.clone() }),
        };
        serde_json::to_value(dto).map_err(|e| e.to_string())
    })
}
```

- [ ] **Step 2: 注册 + 编译** lib.rs 加 `mod factor_cmds;` + handler `factor_cmds::factor_run`。`cargo build -p rquant-desktop 2>&1 | tail -10`(若 `FactorConfig` 字段不符,读 `src/factor/mod.rs` 对齐)。

- [ ] **Step 3: Commit**
```bash
git add desktop/src-tauri/src/factor_cmds.rs desktop/src-tauri/src/lib.rs
git commit -F - <<'EOF'
feat(desktop): factor command (cross-sectional IC analysis via run_factor)
EOF
```

---

## Task 7: 分析器命令(读 screen run + 数据 → analyze.rs)

**Files:** Create `analyze_cmds.rs`;Modify `lib.rs`

**Interfaces — Consumes:** `analyze::{sector_attribution, two_leg, deploy}`、`screen_runs::read_report`、`index_relative::load_index`、`paths`。**Produces:** 命令 `analyze_sector(run_id)`、`analyze_twoleg(value_run_id, growth_run_id, w)`、`analyze_deploy(run_id)`。

- [ ] **Step 1: 实现**(`analyze_cmds.rs`;helpers 读 holdings 的 rebals、逐股 kday 价/amount、sector 面板):
```rust
use crate::commands::AppState;
use crate::dto_analyze::*;
use std::collections::{BTreeMap, HashMap};

// 从 screen net.json 取每调仓 (day, 选中symbols)
fn rebals_of(net: &serde_json::Value) -> Vec<(String, Vec<String>)> {
    net.get("holdings").and_then(|h| h.as_array()).map(|a| a.iter().map(|h| {
        let t: String = h.get("t").and_then(|x| x.as_str()).unwrap_or("").chars().take(10).collect();
        let syms = h.get("selected").and_then(|s| s.as_array()).map(|a| a.iter()
            .filter_map(|p| p.as_array().and_then(|pr| pr.first()).and_then(|x| x.as_str()).map(String::from)).collect()).unwrap_or_default();
        (t, syms)
    }).collect()).unwrap_or_default()
}
// 读单股 kday 的 day→(close,amount)
fn load_kday(ws: &crate::paths::Workspace, sym: &str) -> HashMap<String, (f64, f64)> {
    let mut m = HashMap::new();
    if let Ok(txt) = std::fs::read_to_string(ws.kday_dir().join(format!("{sym}.csv"))) {
        for line in txt.lines().skip(1) { let c: Vec<&str> = line.split(',').collect();
            if c.len() >= 7 { if let (Ok(close), Ok(amt)) = (c[4].parse::<f64>(), c[6].parse::<f64>()) {
                m.insert(c[0].get(..10).unwrap_or(c[0]).to_string(), (close, amt)); } } }
    }
    m
}

#[tauri::command]
pub fn analyze_sector(state: tauri::State<AppState>, run_id: String) -> Result<SectorAttribDto, String> {
    let net = crate::screen_runs::read_report(&state.ws, &run_id, "net")?;
    let rebals = rebals_of(&net);
    let meta = crate::screen_runs::read_meta(&state.ws, &run_id)?;
    // 价:逐股 kday close
    let syms: std::collections::BTreeSet<String> = rebals.iter().flat_map(|(_, s)| s.clone()).collect();
    let mut px: HashMap<String, HashMap<String,(f64,f64)>> = HashMap::new();
    for s in &syms { px.insert(s.clone(), load_kday(&state.ws, s)); }
    let price = |s: &str, d: &str| px.get(s).and_then(|m| m.get(d)).map(|(c,_)| *c);
    // 行业 membership
    let mut sector_of = HashMap::new();
    if let Ok(txt) = std::fs::read_to_string(state.ws.sector_membership_path()) {
        for line in txt.lines().skip(1) { let c: Vec<&str> = line.split(',').collect();
            if c.len() >= 2 { sector_of.insert(c[0].to_string(), c[1].to_string()); } } }
    // 板块 index 面板(按需载入)
    let mut sec_panel: HashMap<String, HashMap<String,f64>> = HashMap::new();
    for ind in sector_of.values().cloned().collect::<std::collections::BTreeSet<_>>() {
        let mut m = HashMap::new();
        if let Ok(txt) = std::fs::read_to_string(state.ws.sector_dir().join(format!("{ind}.csv"))) {
            for line in txt.lines().skip(1) { let c: Vec<&str> = line.split(',').collect();
                if c.len() >= 3 { if let Ok(idx) = c[2].parse::<f64>() { m.insert(c[0].get(..10).unwrap_or(c[0]).to_string(), idx); } } } }
        sec_panel.insert(ind, m);
    }
    let sector_lvl = |ind: &str, d: &str| sec_panel.get(ind).and_then(|m| m.get(d)).copied();
    let idx = crate::index_relative::load_index(&state.ws.index_dir().join("csi300.csv"))?;
    let bench = |d: &str| crate::index_relative::idx_at(&idx, d);
    let r = crate::analyze::sector_attribution(&rebals, &price, &sector_of, &sector_lvl, &bench);
    let _ = meta;
    Ok(SectorAttribDto { excess_total: r.excess_total, alloc_pct: r.alloc_pct, select_pct: r.select_pct,
        cum: r.cum.into_iter().map(|(t, rp, ra, rb)| SectorCumDto { t, r_p: rp, r_alloc: ra, r_bench: rb }).collect() })
}

#[tauri::command]
pub fn analyze_twoleg(state: tauri::State<AppState>, value_run_id: String, growth_run_id: String, _w: f64) -> Result<TwoLegDto, String> {
    let vn = crate::screen_runs::read_report(&state.ws, &value_run_id, "net")?;
    let gn = crate::screen_runs::read_report(&state.ws, &growth_run_id, "net")?;
    let nav_of = |net: &serde_json::Value| -> Vec<(String,f64)> {
        net.get("holdings").and_then(|h| h.as_array()).map(|a| a.iter().filter_map(|h| {
            Some((h.get("t")?.as_str()?.chars().take(10).collect(), h.get("nav")?.as_f64()?)) }).collect()).unwrap_or_default() };
    let regimes: Vec<(String,String,String)> = vn.get("regime_slices").and_then(|s| s.as_array()).map(|a| a.iter().filter_map(|s|
        Some((s.get("label")?.as_str()?.to_string(), s.get("from")?.as_str()?.to_string(), s.get("to")?.as_str()?.to_string()))).collect()).unwrap_or_default();
    let idx = crate::index_relative::load_index(&state.ws.index_dir().join("csi300.csv"))?;
    let r = crate::analyze::two_leg(&nav_of(&vn), &nav_of(&gn), &idx, &regimes);
    if r.rows.is_empty() { return Err("两腿对齐点太少——需同 universe/区间/调仓".into()); }
    Ok(TwoLegDto { rows: r.rows.into_iter().map(|c| TwoLegCellDto { w: c.w, net_total: c.net_total, excess: c.excess, oos_excess: c.oos_excess, sharpe: c.sharpe, max_dd: c.max_dd }).collect(), best_w: r.best_w })
}

#[tauri::command]
pub fn analyze_deploy(state: tauri::State<AppState>, run_id: String) -> Result<DeployDto, String> {
    let net = crate::screen_runs::read_report(&state.ws, &run_id, "net")?;
    let rebals = rebals_of(&net);
    let syms: std::collections::BTreeSet<String> = rebals.iter().flat_map(|(_, s)| s.clone()).collect();
    let mut px: HashMap<String, HashMap<String,(f64,f64)>> = HashMap::new();
    for s in &syms { px.insert(s.clone(), load_kday(&state.ws, s)); }
    let price = |s: &str, d: &str| px.get(s).and_then(|m| m.get(d)).map(|(c,_)| *c);
    // adv:20 日均 amount(≤d)
    let adv = |s: &str, d: &str| -> Option<f64> {
        let m = px.get(s)?; let mut vals: Vec<f64> = m.iter().filter(|(k,_)| k.as_str() <= d).map(|(_,(_,a))| *a).collect();
        if vals.is_empty() { return None; } vals.sort_by(|a,b| a.partial_cmp(b).unwrap());
        let tail = &vals[vals.len().saturating_sub(20)..]; Some(tail.iter().sum::<f64>() / tail.len() as f64) };
    let idx = crate::index_relative::load_index(&state.ws.index_dir().join("csi300.csv"))?;
    let bench = |d: &str| crate::index_relative::idx_at(&idx, d);
    let r = crate::analyze::deploy(&rebals, &price, &adv, &bench, 1.0);
    Ok(DeployDto { lag0_excess: r.lag0_excess, lag1_excess: r.lag1_excess, drag: r.drag, adv_median: r.adv_median,
        capacity: r.capacity.into_iter().map(|(p, a)| CapacityRowDto { adv_pct: p, max_aum: a }).collect() })
}
```
*(基准固定 CSI300——与 sub-1 分析口径一致;`idx_at` 须在 index_relative 中为 `pub`,sub-1 已是。`load_kday` 的 amount 取第 7 列 `amount`。)*

- [ ] **Step 2: 注册 + 编译** lib.rs 加 `mod analyze_cmds;` + 3 handler。`cargo build -p rquant-desktop && cargo test -p rquant-desktop 2>&1 | tail -3` 全绿。

- [ ] **Step 3: Commit**
```bash
git add desktop/src-tauri/src/analyze_cmds.rs desktop/src-tauri/src/lib.rs
git commit -F - <<'EOF'
feat(desktop): analyzer commands (sector/twoleg/deploy on screen runs)
EOF
```

---

## Task 8: 前端 IPC + 路由 + labels + 空壳

**Files:** Modify `api/ipc.ts`、`App.tsx`、`labels.ts`;Create 空 `pages/{Factor,Verdict}.tsx`

- [ ] **Step 1: api/ipc.ts 追加**(camelCase 自动映射):
```typescript
  // 因子
  factorRun: (factors: [string,string][], horizon: number, layers: number, sample: number) =>
    invoke<string>("factor_run", { factors, horizon, layers, sample }),
  // 认证
  evalListReports: () => invoke<import("@bindings/OptimizeReportInfoDto").OptimizeReportInfoDto[]>("eval_list_reports"),
  evalCertify: (paths: string[], name: string) => invoke<import("@bindings/VerdictDto").VerdictDto>("eval_certify", { paths, name }),
  // 分析器
  analyzeSector: (runId: string) => invoke<import("@bindings/SectorAttribDto").SectorAttribDto>("analyze_sector", { runId }),
  analyzeTwoleg: (valueRunId: string, growthRunId: string, w: number) => invoke<import("@bindings/TwoLegDto").TwoLegDto>("analyze_twoleg", { valueRunId, growthRunId, w }),
  analyzeDeploy: (runId: string) => invoke<import("@bindings/DeployDto").DeployDto>("analyze_deploy", { runId }),
```

- [ ] **Step 2: 空壳页**`pages/Factor.tsx`/`pages/Verdict.tsx`:`export default function Factor(){return <div>因子工作台(开发中)</div>;}`(Verdict 同)。

- [ ] **Step 3: App.tsx 路由** MODULES 在 `research` 后加 `{ key: "verdict", label: "认证" }, { key: "factor", label: "因子工作台" }`;import 两页;`<Route path="/verdict" element={<Verdict/>}/>`、`<Route path="/factor" element={<Factor/>}/>`;占位过滤排除 `verdict`/`factor`(注:`factor` 原在占位列表里,需从占位逻辑移除改真路由)。

- [ ] **Step 4: labels.ts 追加**:
```typescript
export const GATE_STATUS_ZH: Record<string,string> = { pass: "通过", fail: "未过", indeterminate: "不定" };
export const FACTOR_TERM = { ic: "IC(信息系数)", icir: "ICIR", rankic: "RankIC", decay: "IC 衰减", layers: "分层收益", mono: "单调性", spread: "多空价差", cert: "认证", alloc: "配置效应", select: "选择效应", drag: "执行拖累", capacity: "容量" } as const;
```

- [ ] **Step 5: 验证** `cd desktop/ui && npx tsc --noEmit 2>&1 | tail -10` → 0 错;`npm --prefix . run test -- --run` 仍全过。

- [ ] **Step 6: Commit**
```bash
git add desktop/ui/src/api/ipc.ts desktop/ui/src/App.tsx desktop/ui/src/labels.ts desktop/ui/src/pages/Factor.tsx desktop/ui/src/pages/Verdict.tsx
git commit -F - <<'EOF'
feat(ui): wire cert/factor/analyzer IPC, routes, labels, page shells
EOF
```

---

## Task 9: stores(factor / verdict)+ 测试

**Files:** Create `stores/factor.ts`、`stores/verdict.ts`、`stores/verdict.test.ts`

- [ ] **Step 1: stores/factor.ts**(仿 sub-1 screen store):
```typescript
import { create } from "zustand";
import type { FactorReportDto } from "@bindings/FactorReportDto";
import { api as realApi, type Api } from "../api/ipc";
import { friendlyError } from "../errors";
interface FactorState { api: Api; report: FactorReportDto | null; error: string | null;
  setReport: (r: FactorReportDto | null) => void; setError: (e: string | null) => void; }
export const useFactor = create<FactorState>((set) => ({ api: realApi, report: null, error: null,
  setReport: (report) => set({ report }), setError: (error) => set({ error }) }));
```

- [ ] **Step 2: stores/verdict.ts**:
```typescript
import { create } from "zustand";
import type { OptimizeReportInfoDto } from "@bindings/OptimizeReportInfoDto";
import type { VerdictDto } from "@bindings/VerdictDto";
import { api as realApi, type Api } from "../api/ipc";
import { friendlyError } from "../errors";
interface VerdictState { api: Api; reports: OptimizeReportInfoDto[]; verdict: VerdictDto | null; error: string | null;
  loadReports: () => Promise<void>; certify: (paths: string[], name: string) => Promise<void>; }
export const useVerdict = create<VerdictState>((set, get) => ({ api: realApi, reports: [], verdict: null, error: null,
  loadReports: async () => { try { set({ reports: await get().api.evalListReports() }); } catch { /* 静默 */ } },
  certify: async (paths, name) => { set({ verdict: null, error: null });
    try { set({ verdict: await get().api.evalCertify(paths, name) }); }
    catch (e) { set({ error: friendlyError(String(e)).title }); } }));
```

- [ ] **Step 3: 测试**`stores/verdict.test.ts`:
```typescript
import { test, expect, afterEach } from "vitest";
import { useVerdict } from "./verdict";
const real = useVerdict.getState().api;
afterEach(() => useVerdict.setState({ api: real, reports: [], verdict: null, error: null }));
test("certify stores verdict", async () => {
  useVerdict.setState({ api: { ...real, evalCertify: async () => ({ strategy: "x", n_symbols: 3, certified: true, gates: [], failed_gates: [] }) } });
  await useVerdict.getState().certify(["a.json"], "x");
  expect(useVerdict.getState().verdict?.certified).toBe(true);
});
```

- [ ] **Step 4: 跑** `cd desktop/ui && npx vitest run src/stores/verdict.test.ts` → PASS;`npx tsc --noEmit` → 0 错。

- [ ] **Step 5: Commit**
```bash
git add desktop/ui/src/stores/factor.ts desktop/ui/src/stores/verdict.ts desktop/ui/src/stores/verdict.test.ts
git commit -F - <<'EOF'
feat(ui): factor & verdict stores + test
EOF
```

---

## Task 10: 认证页(Verdict + VerdictMatrix)

**Files:** Create `components/VerdictMatrix.tsx`、`components/VerdictMatrix.test.tsx`;替换 `pages/Verdict.tsx`

- [ ] **Step 1: VerdictMatrix.tsx**(门槛矩阵表):
```tsx
import { Card, Table, Tag } from "antd";
import type { VerdictDto } from "@bindings/VerdictDto";
import { GATE_STATUS_ZH } from "../labels";
export default function VerdictMatrix({ v }: { v: VerdictDto }) {
  const color = (s: string) => (s === "pass" ? "green" : s === "fail" ? "red" : "orange");
  return (
    <Card size="small" title={`认证:${v.strategy} · ${v.n_symbols} 标的`}
      extra={<Tag color={v.certified ? "green" : "red"}>{v.certified ? "已认证 ✓" : "未通过"}</Tag>}>
      <Table size="small" pagination={false} rowKey="gate" dataSource={v.gates}
        columns={[
          { title: "门槛", dataIndex: "gate" },
          { title: "状态", dataIndex: "status", render: (s: string) => <Tag color={color(s)}>{GATE_STATUS_ZH[s] ?? s}</Tag> },
          { title: "值", dataIndex: "value", render: (x: number) => x.toFixed(3) },
          { title: "阈值", dataIndex: "threshold", render: (x: number) => x.toFixed(3) },
          { title: "说明", dataIndex: "note", ellipsis: true },
        ]} />
    </Card>
  );
}
```

- [ ] **Step 2: 测试**`VerdictMatrix.test.tsx`:
```tsx
import { render, screen } from "@testing-library/react";
import "@testing-library/jest-dom";
import { test, expect } from "vitest";
import VerdictMatrix from "./VerdictMatrix";
import type { VerdictDto } from "@bindings/VerdictDto";
const V: VerdictDto = { strategy: "树4", n_symbols: 10, certified: false,
  gates: [{ gate: "T1_os_breadth", status: "fail", value: 0.4, threshold: 0.6, note: "样本外正占比不足" }], failed_gates: ["T1_os_breadth"] };
test("verdict matrix shows gate + status zh", () => {
  render(<VerdictMatrix v={V} />);
  expect(screen.getByText("未通过")).toBeInTheDocument();
  expect(screen.getByText("T1_os_breadth")).toBeInTheDocument();
  expect(screen.getByText("未过")).toBeInTheDocument();
});
```

- [ ] **Step 3: Verdict.tsx**(左:报告多选 + 策略名 + 运行;右:VerdictMatrix):
```tsx
import { useEffect, useState } from "react";
import { App as AntApp, Button, Card, Col, Input, Row, Table } from "antd";
import { useVerdict } from "../stores/verdict";
import VerdictMatrix from "../components/VerdictMatrix";
export default function Verdict() {
  const st = useVerdict(); const { message } = AntApp.useApp();
  const [sel, setSel] = useState<string[]>([]); const [name, setName] = useState("");
  useEffect(() => { void st.loadReports(); }, []);
  return (
    <Row gutter={12}>
      <Col span={9}>
        <Card size="small" title="选 optimize 报告(可多选)">
          <Table size="small" rowKey="path" pagination={false} dataSource={st.reports}
            rowSelection={{ selectedRowKeys: sel, onChange: (k) => setSel(k as string[]) }}
            columns={[{ title: "报告", dataIndex: "name", render: (n: string, r) => n ?? r.path },
                      { title: "组合", dataIndex: "n_combos" }, { title: "折", dataIndex: "folds" }]} />
          <Input style={{ marginTop: 8 }} placeholder="策略名(可空,默认首标的)" value={name} onChange={(e) => setName(e.target.value)} />
          <Button type="primary" block style={{ marginTop: 8 }} disabled={!sel.length}
            onClick={() => { void st.certify(sel, name); }}>运行认证</Button>
        </Card>
      </Col>
      <Col span={15}>{st.verdict ? <VerdictMatrix v={st.verdict} /> : <span style={{ opacity: .6 }}>选报告并运行认证</span>}</Col>
    </Row>
  );
}
```

- [ ] **Step 4: 跑** `npx vitest run src/components/VerdictMatrix.test.tsx` → PASS;`npx tsc --noEmit` → 0 错。

- [ ] **Step 5: Commit**
```bash
git add desktop/ui/src/components/VerdictMatrix.tsx desktop/ui/src/components/VerdictMatrix.test.tsx desktop/ui/src/pages/Verdict.tsx
git commit -F - <<'EOF'
feat(ui): 认证 page — verdict matrix (5-gate certification view)
EOF
```

---

## Task 11: 因子工作台(Factor + FactorReport)

**Files:** Create `components/FactorReport.tsx`、`components/FactorReport.test.tsx`;替换 `pages/Factor.tsx`

**Interfaces — Consumes:** `useFactor`、`FactorReportDto`、antd Table、ECharts(IC 衰减/分层)。

- [ ] **Step 1: FactorReport.tsx**(因子 IC 表 + 选中因子 IC 衰减/分层;表格全中文列):
```tsx
import { useState } from "react";
import { Card, Table } from "antd";
import type { FactorReportDto } from "@bindings/FactorReportDto";
import type { FactorStatsDto } from "@bindings/FactorStatsDto";
export default function FactorReport({ report }: { report: FactorReportDto }) {
  const [sel, setSel] = useState(0);
  const f = report.factors[sel];
  const fx = (v?: number | null) => (v == null ? "—" : v.toFixed(3));
  return (
    <div>
      <Card size="small" title={`因子 IC(${report.n_symbols} 标的 · horizon ${report.horizon} · ${report.layers_q} 层)`}>
        <Table<FactorStatsDto> size="small" rowKey="name" pagination={false} dataSource={report.factors}
          onRow={(_, i) => ({ onClick: () => setSel(i ?? 0), style: { cursor: "pointer" } })}
          columns={[{ title: "因子", dataIndex: "name" }, { title: "表达式", dataIndex: "expr", ellipsis: true },
            { title: "IC 均值", dataIndex: "ic_mean", render: fx }, { title: "ICIR", dataIndex: "icir", render: fx },
            { title: "RankIC", dataIndex: "rank_ic_mean", render: fx }, { title: "RankICIR", dataIndex: "rank_icir", render: fx },
            { title: "IC t 值", dataIndex: "ic_t", render: fx }]} />
      </Card>
      {f && (<Card size="small" title={`${f.name} · IC 衰减 / 分层收益`} style={{ marginTop: 8 }}>
        <div style={{ fontSize: 12 }}>IC 衰减:{f.ic_decay.map((d) => `${d.horizon}→${fx(d.rank_ic)}`).join("  ")}</div>
        {f.layers && <div style={{ fontSize: 12, marginTop: 6 }}>分层年化:{f.layers.ann_returns.map((r, i) => `Q${i+1} ${r == null ? "—" : (r*100).toFixed(1)+"%"}`).join("  ")} · 单调性 {fx(f.layers.monotonicity)} · 多空价差 {(f.layers.spread_total*100).toFixed(1)}%</div>}
      </Card>)}
    </div>
  );
}
```
*(IC 衰减/分层先以紧凑文本呈现;实现期可用 ECharts 折线/柱替换提质——复用 sub-1 的 ECharts 范式,非阻塞。)*

- [ ] **Step 2: 测试**`FactorReport.test.tsx`:
```tsx
import { render, screen } from "@testing-library/react";
import "@testing-library/jest-dom";
import { test, expect } from "vitest";
import FactorReport from "./FactorReport";
import type { FactorReportDto } from "@bindings/FactorReportDto";
const R: FactorReportDto = { n_symbols: 1073, sample: 16, horizon: 16, layers_q: 5, corr: null,
  factors: [{ name: "value_pb", expr: "1/(1+fund.pb)", n_periods: 100, ic_mean: 0.04, icir: 0.5, ic_t: 3.1, ic_pos_share: 0.6, rank_ic_mean: 0.05, rank_icir: 0.6, ic_decay: [{horizon:8, rank_ic:0.05}], layers: { q:5, ann_returns:[0.2,0.1,0.05,0.0,-0.1], spread_total:0.3, spread_sharpe:1.0, monotonicity:0.9 } }] };
test("factor report shows IC table", () => {
  render(<FactorReport report={R} />);
  expect(screen.getByText("value_pb")).toBeInTheDocument();
  expect(screen.getByText("0.040")).toBeInTheDocument();
});
```

- [ ] **Step 3: Factor.tsx**(左:universe 固定 + 因子表达式增删 + horizon/层/采样 + 运行;右:FactorReport;运行经 task 事件取结果):
```tsx
import { useState } from "react";
import { App as AntApp, Button, Card, Col, Input, InputNumber, Row, Space } from "antd";
import { listen } from "@tauri-apps/api/event";
import { useFactor } from "../stores/factor";
import type { FactorReportDto } from "@bindings/FactorReportDto";
import FactorReport from "../components/FactorReport";
export default function Factor() {
  const st = useFactor(); const { message } = AntApp.useApp();
  const [exprs, setExprs] = useState<[string,string][]>([["value_pb", "1/(1+fund.pb)"]]);
  const [horizon, setH] = useState(16); const [layers, setL] = useState(5); const [sample, setS] = useState(16);
  const [running, setRunning] = useState(false);
  async function run() {
    const valid = exprs.filter(([n, e]) => n && e);
    if (!valid.length) { message.warning("请添加因子表达式"); return; }
    setRunning(true);
    try {
      const taskId = await st.api.factorRun(valid, horizon, layers, sample);
      const un = await listen<{ id: string; status: string; result: FactorReportDto | null }>("task://progress", (ev) => {
        if (ev.payload.id !== taskId) return;
        if (ev.payload.status === "done") { st.setReport(ev.payload.result); setRunning(false); void un(); }
        else if (ev.payload.status === "failed") { message.error("因子分析失败"); setRunning(false); void un(); } });
    } catch (e) { message.error(String(e)); setRunning(false); }
  }
  return (
    <Row gutter={12}>
      <Col span={8}><Card size="small" title="因子工作台">
        <Space direction="vertical" style={{ width: "100%" }}>
          {exprs.map((e, i) => (<Space key={i}>
            <Input placeholder="名" value={e[0]} style={{ width: 90 }} onChange={(ev) => setExprs(x => x.map((y, j) => j === i ? [ev.target.value, y[1]] : y))} />
            <Input placeholder="DSL 表达式" value={e[1]} onChange={(ev) => setExprs(x => x.map((y, j) => j === i ? [y[0], ev.target.value] : y))} /></Space>))}
          <Button size="small" onClick={() => setExprs(x => [...x, ["", ""]])}>+ 因子</Button>
          <Space><InputNumber addonBefore="horizon" value={horizon} onChange={v => setH(v ?? 16)} />
            <InputNumber addonBefore="层" value={layers} onChange={v => setL(v ?? 5)} /></Space>
          <InputNumber addonBefore="采样间隔" value={sample} onChange={v => setS(v ?? 16)} />
          <Button type="primary" block loading={running} onClick={run}>运行分析</Button>
        </Space>
      </Card></Col>
      <Col span={16}>{st.report ? <FactorReport report={st.report} /> : <span style={{ opacity: .6 }}>添加因子并运行</span>}</Col>
    </Row>
  );
}
```

- [ ] **Step 4: 跑** `npx vitest run src/components/FactorReport.test.tsx` → PASS;`npx tsc --noEmit` → 0 错。

- [ ] **Step 5: Commit**
```bash
git add desktop/ui/src/components/FactorReport.tsx desktop/ui/src/components/FactorReport.test.tsx desktop/ui/src/pages/Factor.tsx
git commit -F - <<'EOF'
feat(ui): 因子工作台 page — factor IC report (fills /factor placeholder)
EOF
```

---

## Task 12: 选股回测结果 → 「分析」tab 组

**Files:** Create `components/{SectorAttrib,TwoLegBlend,DeployHardening}.tsx` + `DeployHardening.test.tsx`;Modify `components/ScreenBacktestResult.tsx`

**Interfaces — Consumes:** `api.analyze{Sector,Twoleg,Deploy}`、对应 DTO、`useScreen`(取 runs 供两腿选第二个 run + 当前 run_id)。

- [ ] **Step 1: SectorAttrib.tsx**(选中 run → analyzeSector → 配置/选择% + 累计):
```tsx
import { useEffect, useState } from "react";
import { App as AntApp, Card, Statistic, Row, Col } from "antd";
import { useScreen } from "../stores/screen";
import type { SectorAttribDto } from "@bindings/SectorAttribDto";
export default function SectorAttrib({ runId }: { runId: string }) {
  const st = useScreen(); const { message } = AntApp.useApp();
  const [d, setD] = useState<SectorAttribDto | null>(null);
  useEffect(() => { (async () => { try { setD(await st.api.analyzeSector(runId)); } catch (e) { message.error(String(e)); } })(); }, [runId]);
  if (!d) return <span style={{ opacity: .6 }}>计算中…</span>;
  const pct = (v: number) => `${(v*100).toFixed(1)}%`;
  return <Row gutter={16}>
    <Col><Statistic title="总超额" value={pct(d.excess_total)} /></Col>
    <Col><Statistic title="配置效应占比" value={pct(d.alloc_pct)} /></Col>
    <Col><Statistic title="选择效应占比" value={pct(d.select_pct)} /></Col>
  </Row>;
}
```

- [ ] **Step 2: TwoLegBlend.tsx**(选第二个(成长)run + w 滑杆 → analyzeTwoleg 表 + 最优 w):
```tsx
import { useEffect, useState } from "react";
import { App as AntApp, Card, Select, Slider, Table } from "antd";
import { useScreen } from "../stores/screen";
import type { TwoLegDto } from "@bindings/TwoLegDto";
export default function TwoLegBlend({ runId }: { runId: string }) {
  const st = useScreen(); const { message } = AntApp.useApp();
  const [growth, setGrowth] = useState<string>(""); const [w, setW] = useState(0.8);
  const [d, setD] = useState<TwoLegDto | null>(null);
  useEffect(() => { void st.loadRuns(); }, []);
  useEffect(() => { if (!growth) return; (async () => { try { setD(await st.api.analyzeTwoleg(runId, growth, w)); } catch (e) { message.error(String(e)); } })(); }, [growth]);
  const pct = (v?: number | null) => (v == null ? "—" : `${(v*100).toFixed(0)}%`);
  return <div>
    <Select style={{ width: 320 }} placeholder="选成长腿 run" value={growth || undefined} onChange={setGrowth}
      options={st.runs.filter(r => r.id !== runId).map(r => ({ value: r.id, label: `${r.config} · ${r.created}` }))} />
    {d && <>
      <div style={{ margin: "8px 0" }}>价值腿权重 w={w.toFixed(1)}（最优 {d.best_w.toFixed(1)}）<Slider min={0} max={1} step={0.1} value={w} onChange={setW} /></div>
      <Table size="small" pagination={false} rowKey="w" dataSource={d.rows}
        columns={[{ title: "w(价值)", dataIndex: "w", render: (x: number) => x.toFixed(1) },
          { title: "净总", dataIndex: "net_total", render: pct }, { title: "超额", dataIndex: "excess", render: pct },
          { title: "样本外超额", dataIndex: "oos_excess", render: pct }, { title: "夏普", dataIndex: "sharpe", render: (x: number) => x.toFixed(2) },
          { title: "最大回撤", dataIndex: "max_dd", render: pct }]} />
    </>}
  </div>;
}
```

- [ ] **Step 3: DeployHardening.tsx** + 测试(T+1 拖累 + 容量表):
```tsx
import { useEffect, useState } from "react";
import { App as AntApp, Statistic, Row, Col, Table } from "antd";
import { useScreen } from "../stores/screen";
import type { DeployDto } from "@bindings/DeployDto";
export default function DeployHardening({ runId }: { runId: string }) {
  const st = useScreen(); const { message } = AntApp.useApp();
  const [d, setD] = useState<DeployDto | null>(null);
  useEffect(() => { (async () => { try { setD(await st.api.analyzeDeploy(runId)); } catch (e) { message.error(String(e)); } })(); }, [runId]);
  if (!d) return <span style={{ opacity: .6 }}>计算中…</span>;
  const pct = (v: number) => `${(v*100).toFixed(1)}%`;
  const yi = (v: number) => `${(v/1e8).toFixed(2)} 亿`;
  return <div>
    <Row gutter={16}>
      <Col><Statistic title="即时执行超额" value={pct(d.lag0_excess)} /></Col>
      <Col><Statistic title="T+1 执行超额" value={pct(d.lag1_excess)} /></Col>
      <Col><Statistic title="执行拖累" value={pct(d.drag)} /></Col>
      <Col><Statistic title="持仓中位 ADV" value={yi(d.adv_median)} /></Col>
    </Row>
    <Table size="small" pagination={false} rowKey="adv_pct" style={{ marginTop: 8 }} dataSource={d.capacity}
      columns={[{ title: "%ADV", dataIndex: "adv_pct", render: (x: number) => `${(x*100).toFixed(0)}%` },
        { title: "最大容量(AUM)", dataIndex: "max_aum", render: yi }]} />
  </div>;
}
```
```tsx
// DeployHardening.test.tsx
import { render, screen } from "@testing-library/react";
import "@testing-library/jest-dom";
import { test, expect, afterEach } from "vitest";
import { App as AntApp } from "antd";
import DeployHardening from "./DeployHardening";
import { useScreen } from "../stores/screen";
const real = useScreen.getState().api;
afterEach(() => useScreen.setState({ api: real }));
test("deploy hardening shows drag + capacity", async () => {
  useScreen.setState({ api: { ...real, analyzeDeploy: async () => ({ lag0_excess: 3.0, lag1_excess: 3.05, drag: 0.05, adv_median: 3.18e8, capacity: [{ adv_pct: 0.1, max_aum: 2.5e8 }] }) } });
  render(<AntApp><DeployHardening runId="scr-1" /></AntApp>);
  expect(await screen.findByText("执行拖累")).toBeInTheDocument();
});
```

- [ ] **Step 4: 扩 ScreenBacktestResult.tsx** 在结果区底部加一个「分析」Tabs(仅当选中某 run 即 `selId` 有值时显示),三 tab 分别渲染 `<SectorAttrib runId={selId}/>` / `<TwoLegBlend runId={selId}/>` / `<DeployHardening runId={selId}/>`。读取 ScreenBacktestResult 现有结构,在三联面板之后插入:
```tsx
{selId && (<Card size="small" title="分析" style={{ marginTop: 8 }}>
  <Tabs items={[
    { key: "sector", label: "行业归因", children: <SectorAttrib runId={selId} /> },
    { key: "twoleg", label: "两腿组合", children: <TwoLegBlend runId={selId} /> },
    { key: "deploy", label: "部署加固", children: <DeployHardening runId={selId} /> },
  ]} />
</Card>)}
```
(import 三组件 + 确保 `selId` 即当前选中 run 的 id;若 ScreenBacktestResult 用本地 `selId` state,直接复用。)

- [ ] **Step 5: 跑** `npx vitest run src/components/DeployHardening.test.tsx` → PASS;`npx tsc --noEmit` → 0 错;`npm --prefix . run test -- --run` 全过。

- [ ] **Step 6: Commit**
```bash
git add desktop/ui/src/components/SectorAttrib.tsx desktop/ui/src/components/TwoLegBlend.tsx desktop/ui/src/components/DeployHardening.tsx desktop/ui/src/components/DeployHardening.test.tsx desktop/ui/src/components/ScreenBacktestResult.tsx
git commit -F - <<'EOF'
feat(ui): screen backtest 分析 tabs — sector / two-leg / deploy analyzers
EOF
```

---

## Task 13: 收尾闸 + 文档 + 记忆 + finishing

- [ ] **Step 1: 全量后端闸** `cargo test --workspace 2>&1 | grep "test result"` → 全 ok 0 failed。
- [ ] **Step 2: 前端闸** `npm --prefix desktop/ui run build` 成功 + `npm --prefix desktop/ui run test -- --run` 全过。
- [ ] **Step 3: 真数据冒烟** 启 `npm run tauri dev`:① 因子工作台跑 `1/(1+fund.pb)` → IC 表非空;② 认证页选一个真 optimize JSON(如 `.daily_runs/` 下)→ Verdict 矩阵;③ 选股回测选一个真 run → 三分析 tab 出数,**与 `python scripts/analyze_{sector,twoleg,deploy}.py <run.json>` 数值对账一致**(诚实对拍)。
- [ ] **Step 4: 文档 + 记忆** 更新 `docs/desktop-screen-research.md`(加认证/因子/分析器一节);更新记忆 `rquant-project.md`(sub-2a 落地)。
- [ ] **Step 5: Commit**
```bash
git add docs/ && git commit -F - <<'EOF'
docs(desktop): cert & analysis (sub-2a) usage; finalize
EOF
```
- [ ] **Step 6: finishing** 调用 superpowers:finishing-a-development-branch 收口。

---

## 自审备忘(写计划时已校)

- **类型一致**:DTO 字段镜像真实库结构体(FactorStats/LayerStats/CorrMatrix/Verdict/GateOutcome/OptimizeReport);命令名 snake↔camel 一致;新 DTO 名全局唯一(无与 sub-1/dto.rs 冲突)。
- **诚实纪律**:eval 用 `certify`+默认阈值不重判;分析器纯算术、数值对拍 Python;`GateStatus` 枚举→lowercase 字符串入 DTO。
- **复用**:`index_relative::{load_index,idx_at}`(两腿/分析器)、`screen_runs::read_report`(读 run)、`TaskRegistry`、sub-1 全部前端范式。
- **范围**:optimize/portfolio 占位留 2b;eval 只消费已有 optimize JSON;基准固定 CSI300(与 sub-1 分析口径一致)。
- **已知取舍**:factor IC 衰减/分层先文本呈现(ECharts 提质留实现期,非阻塞);分析器基准 CSI300 硬编码(与 analyze_*.py 默认一致)。
