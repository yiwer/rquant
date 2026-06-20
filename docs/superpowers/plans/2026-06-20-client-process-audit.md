# 流程审计 + 健壮性加固 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 后端把每次操作完整轨迹落盘 JSONL,客户端新增「审计」页浏览/筛选/详情,并修掉 Critical+Important 健壮性硬伤。

**Architecture:** 审计在 `TaskRegistry` 单一卡点捕获——任务体经 `ctx.note_params/note_file/note_summary` 声明数据,registry 在终态组装 `AuditRecord` 追加 `.rquant-desktop/audit/audit.jsonl`(旁路,失败不影响主流程);不改 `tasks.start` 签名。前端「审计」页读 `audit_list`。健壮性按文件就地修。

**Tech Stack:** Rust(serde/std::time/std::fs append)+ ts-rs DTO + React/Zustand/antd + Vitest;复用 `friendlyError`、`TaskRegistry`、`paths::Workspace`、`tauri_plugin_log`。

## Global Constraints

- 审计为**旁路**:`audit::append` 失败仅 `log::warn`,绝不 `?` 冒泡毁任务结果。
- 数字字段用 `f64`(ts-rs→TS `number`,避开 i64→bigint);新 DTO 名全局唯一。
- 审计落盘 `.rquant-desktop/audit/audit.jsonl`、日志 `.rquant-desktop/logs/`(均 gitignored,`.rquant-desktop` 已在 .gitignore)。
- "触及文件"=桥层可知输入/产物(config/universe/index/kday_dir/输出路径),**非**逐股 CSV;详情页注明。
- Tauri invoke 参数 camelCase↔snake_case 自动映射;新命令在 `generate_handler!` 注册。
- 验证三件套:`cargo test --workspace`;`node desktop/ui/node_modules/typescript/bin/tsc --noEmit -p desktop/ui/tsconfig.app.json`;`npm --prefix desktop/ui run test -- --run` + `npm --prefix desktop/ui run build`。英文 commit(`git commit -F -` heredoc);只 add 本任务文件;不 push。

---

### Task 1: 审计模型 + JSONL 落盘/读取(纯逻辑 TDD)

**Files:** Create `desktop/src-tauri/src/audit.rs`;Modify `desktop/src-tauri/src/paths.rs`(加路径)、`desktop/src-tauri/src/lib.rs`(`pub mod audit;`)

**Interfaces — Produces:**
- `audit::AuditStage { stage:String, detail:String, at_ms:f64 }`、`audit::AuditRecord { id:String, kind:String, params:serde_json::Value, started_at:String, ended_at:String, duration_ms:f64, stages:Vec<AuditStage>, files:Vec<String>, status:String, error:Option<String>, result_summary:Option<String>, artifact:Option<String> }`(均 `#[derive(Debug,Clone,Serialize,Deserialize)]`)
- `audit::append(path:&Path, rec:&AuditRecord) -> std::io::Result<()>`(创建父目录 + 追加一行 JSON)
- `audit::read(path:&Path, limit:usize, kind:Option<&str>, status:Option<&str>) -> Vec<AuditRecord>`(读全部、过滤、取尾 `limit`、新→旧)
- `paths::Workspace::audit_path()` = `.rquant-desktop/audit/audit.jsonl`;`log_dir()` = `.rquant-desktop/logs`

- [ ] **Step 1: paths.rs 加路径**(impl Workspace,近 deploy_book_path):

```rust
    pub fn audit_path(&self) -> PathBuf { self.desktop_data_dir().join("audit").join("audit.jsonl") }
    pub fn log_dir(&self) -> PathBuf { self.desktop_data_dir().join("logs") }
```

- [ ] **Step 2: 写失败测试** `desktop/src-tauri/src/audit.rs`(文件内 `#[cfg(test)]`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    fn rec(id: &str, kind: &str, status: &str) -> AuditRecord {
        AuditRecord {
            id: id.into(), kind: kind.into(), params: serde_json::json!({"as_of":"2026-06-16"}),
            started_at: "2026-06-16T10:00:00".into(), ended_at: "2026-06-16T10:00:02".into(),
            duration_ms: 2000.0, stages: vec![AuditStage{stage:"选股".into(),detail:"".into(),at_ms:100.0}],
            files: vec!["data/baostock/universe_baostock_day.csv".into()],
            status: status.into(), error: None, result_summary: Some("top-50".into()), artifact: None,
        }
    }
    #[test]
    fn append_then_read_roundtrip_newest_first_and_filter() {
        let td = tempfile::tempdir().unwrap();
        let p = td.path().join("audit/audit.jsonl");
        append(&p, &rec("t1", "screen_asof", "done")).unwrap();
        append(&p, &rec("t2", "deploy_month", "failed")).unwrap();
        append(&p, &rec("t3", "screen_asof", "done")).unwrap();
        let all = read(&p, 10, None, None);
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].id, "t3"); // 新→旧
        let only_screen = read(&p, 10, Some("screen_asof"), None);
        assert_eq!(only_screen.len(), 2);
        let only_failed = read(&p, 10, None, Some("failed"));
        assert_eq!(only_failed.len(), 1);
        assert_eq!(only_failed[0].id, "t2");
        assert_eq!(read(&p, 1, None, None).len(), 1); // limit
    }
    #[test]
    fn read_missing_file_is_empty() {
        assert!(read(std::path::Path::new("E:/nonexistent/audit.jsonl"), 10, None, None).is_empty());
    }
}
```

- [ ] **Step 3: 跑确认失败** `cargo test -p rquant-desktop audit:: 2>&1 | tail -8` → FAIL(未实现)。

- [ ] **Step 4: 实现**(`audit.rs` 顶部):

```rust
//! 流程审计:每次操作的完整轨迹落盘 JSONL(旁路,失败不毁主流程)。
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditStage { pub stage: String, pub detail: String, pub at_ms: f64 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRecord {
    pub id: String,
    pub kind: String,
    pub params: serde_json::Value,
    pub started_at: String,
    pub ended_at: String,
    pub duration_ms: f64,
    pub stages: Vec<AuditStage>,
    pub files: Vec<String>,
    pub status: String,
    pub error: Option<String>,
    pub result_summary: Option<String>,
    pub artifact: Option<String>,
}

/// 追加一行 JSON(自动建父目录)。
pub fn append(path: &Path, rec: &AuditRecord) -> std::io::Result<()> {
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    let line = serde_json::to_string(rec).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    writeln!(f, "{line}")
}

/// 读全部 → 过滤(kind/status)→ 取尾 limit → 新到旧。坏行/缺文件容错。
pub fn read(path: &Path, limit: usize, kind: Option<&str>, status: Option<&str>) -> Vec<AuditRecord> {
    let Ok(txt) = std::fs::read_to_string(path) else { return Vec::new() };
    let mut v: Vec<AuditRecord> = txt.lines()
        .filter_map(|l| serde_json::from_str::<AuditRecord>(l).ok())
        .filter(|r| kind.is_none_or(|k| r.kind == k))
        .filter(|r| status.is_none_or(|s| r.status == s))
        .collect();
    v.reverse();
    v.truncate(limit);
    v
}
```

- [ ] **Step 5: 声明** `lib.rs` 加 `pub mod audit;`(与其他 `pub mod` 并列)。
- [ ] **Step 6: 跑确认通过** `cargo test -p rquant-desktop audit:: 2>&1 | tail -8` → PASS;`cargo build -p rquant-desktop 2>&1 | tail -3` 绿。(注:`is_none_or` 需 Rust 1.82+;若工具链旧,改 `kind.map_or(true, |k| r.kind==k)`。)

- [ ] **Step 7: Commit**

```bash
git add desktop/src-tauri/src/audit.rs desktop/src-tauri/src/paths.rs desktop/src-tauri/src/lib.rs
git commit -F - <<'EOF'
feat(desktop): audit record model + JSONL append/read (process-audit foundation)
EOF
```

---

### Task 2: TaskRegistry 审计卡点 + C2 mutex 防中毒(TDD)

**Files:** Modify `desktop/src-tauri/src/tasks.rs`

**Interfaces:**
- Consumes: `crate::audit::{AuditRecord, AuditStage, append}`(Task 1)。
- Produces: `TaskCtx::note_params(&self, p: serde_json::Value)`、`note_file(&self, path:&str)`、`note_summary(&self, s:&str)`;`TaskRegistry::new(sink, audit_path: PathBuf)`(签名加 audit_path)。registry 在任务终态把 `AuditRecord` append 到 `audit_path`。

- [ ] **Step 1: 写失败测试**(`tasks.rs` 的 `#[cfg(test)] mod tests` 内追加;复用其 `reg()` 需改造为带临时 audit 路径):

```rust
    #[test]
    fn task_writes_audit_record_on_done() {
        let td = tempfile::tempdir().unwrap();
        let ap = td.path().join("audit.jsonl");
        let r = TaskRegistry::new(std::sync::Arc::new(NullSink), ap.clone());
        let id = r.start("screen_asof", false, |ctx| {
            ctx.note_params(serde_json::json!({"as_of":"2026-06-16","top":50}));
            ctx.note_file("data/baostock/universe_baostock_day.csv");
            ctx.progress(0.4, "选股", "");
            ctx.note_summary("top-50");
            Ok(serde_json::json!({"n":50}))
        }).unwrap();
        wait_status(&r, &id, "done");
        // 给写盘一点时间(终态写在 spawn 线程)
        for _ in 0..200 { if ap.exists() && !crate::audit::read(&ap,10,None,None).is_empty() { break } std::thread::sleep(Duration::from_millis(10)); }
        let recs = crate::audit::read(&ap, 10, None, None);
        assert_eq!(recs.len(), 1);
        let a = &recs[0];
        assert_eq!(a.kind, "screen_asof");
        assert_eq!(a.status, "done");
        assert_eq!(a.params["top"], 50);
        assert!(a.files.iter().any(|f| f.contains("universe_baostock_day")));
        assert!(a.stages.iter().any(|s| s.stage == "选股"));
        assert_eq!(a.result_summary.as_deref(), Some("top-50"));
    }
    #[test]
    fn task_writes_audit_record_on_failure() {
        let td = tempfile::tempdir().unwrap();
        let ap = td.path().join("audit.jsonl");
        let r = TaskRegistry::new(std::sync::Arc::new(NullSink), ap.clone());
        let id = r.start("boom", false, |_ctx| Err("kaboom".to_string())).unwrap();
        wait_status(&r, &id, "failed");
        for _ in 0..200 { if crate::audit::read(&ap,10,None,None).len()==1 { break } std::thread::sleep(Duration::from_millis(10)); }
        let recs = crate::audit::read(&ap, 10, None, None);
        assert_eq!(recs[0].status, "failed");
        assert_eq!(recs[0].error.as_deref(), Some("kaboom"));
    }
```

(更新现有测试 `reg()` 辅助为 `TaskRegistry::new(Arc::new(NullSink), std::env::temp_dir().join(format!("rq-audit-test-{}.jsonl", <唯一>)))`——用 `tempfile::tempdir` 更干净;把 `reg()` 改成返回 `(TaskRegistry, TempDir)` 或各测试就地建。)

- [ ] **Step 2: 跑确认失败** `cargo test -p rquant-desktop tasks:: 2>&1 | tail -10` → FAIL(签名/方法不存在)。

- [ ] **Step 3: 实现**(改 `tasks.rs`):
  1. `use std::time::Instant;` `use std::path::PathBuf;` `use crate::audit::{AuditRecord, AuditStage};`
  2. 新 `struct AuditAccum { started_at: String, start: Instant, params: serde_json::Value, files: Vec<String>, summary: Option<String>, stages: Vec<AuditStage> }`。
  3. `Shared.tasks` 值类型 `(TaskInfoDto, Arc<AtomicBool>)` → `(TaskInfoDto, Arc<AtomicBool>, AuditAccum)`;`Shared` 加 `audit_path: PathBuf`。所有解构处加第三元/忽略。
  4. **C2**:把全部 `.lock().expect("task map poisoned")` 改 `.lock().unwrap_or_else(|p| p.into_inner())`。
  5. `TaskCtx` 加方法(经 `shared` 改对应 id 的 AuditAccum):
```rust
    pub fn note_params(&self, p: serde_json::Value) { self.shared.with_accum(&self.id, |a| a.params = p); }
    pub fn note_file(&self, path: &str) { self.shared.with_accum(&self.id, |a| if !a.files.iter().any(|f| f==path) { a.files.push(path.to_string()) }); }
    pub fn note_summary(&self, s: &str) { self.shared.with_accum(&self.id, |a| a.summary = Some(s.to_string())); }
```
  6. `Shared` 加 `fn with_accum(&self, id:&str, f: impl FnOnce(&mut AuditAccum))`(锁 + 取第三元 + f)。
  7. `progress()` 现经 `shared.update`;改为同时 `with_accum` 追加 `AuditStage{stage,detail,at_ms: a.start.elapsed().as_millis() as f64}`。
  8. `start()`:建 AuditAccum{started_at: now_iso(), start: Instant::now(), params: Null, files: [], summary: None, stages: []};存入第三元。`now_iso()` 用 `chrono::Local::now().format("%Y-%m-%dT%H:%M:%S")`(chrono 已是依赖)。
  9. 终态 spawn 线程末尾(`shared.update` 写 status 之后):组装 `AuditRecord` 并 `let _ = crate::audit::append(&shared.audit_path, &rec);`(失败仅忽略——旁路;可 `eprintln`)。从 AuditAccum 取 params/files/summary/stages/start;status/error/result 从 TaskInfoDto 取;`ended_at=now_iso()`,`duration_ms=start.elapsed().as_millis() as f64`;`artifact=None`(命令可经 note_summary 暂代,artifact 后续按需);`result_summary=summary`。
  10. `TaskRegistry::new(sink, audit_path)` 存入 `Shared.audit_path`。

- [ ] **Step 4: 跑确认通过** `cargo test -p rquant-desktop tasks:: 2>&1 | tail -10` → 全 PASS(含既有 4 例 + 新 2 例)。

- [ ] **Step 5: Commit**

```bash
git add desktop/src-tauri/src/tasks.rs
git commit -F - <<'EOF'
feat(desktop): TaskRegistry audit chokepoint (note_params/file/summary, write AuditRecord on terminal) + mutex poison recovery
EOF
```

---

### Task 3: 审计 DTO + 命令 + 日志落盘接线

**Files:** Create `desktop/src-tauri/src/dto_audit.rs`、`desktop/src-tauri/src/audit_cmds.rs`;Modify `lib.rs`(mod + handler + log target + TaskRegistry::new 传 audit_path)

**Interfaces:**
- Consumes: `audit::{read, AuditRecord}`、`paths`。
- Produces: `AuditRecordDto`/`AuditStageDto`(bindings)、命令 `audit_list(state, limit:u32, kind:Option<String>, status:Option<String>) -> Vec<AuditRecordDto>`、`audit_log_tail(state, lines:u32) -> String`。

- [ ] **Step 1: dto_audit.rs**:

```rust
use serde::Serialize; use ts_rs::TS;
#[derive(Debug,Clone,Serialize,TS)] #[ts(export)]
pub struct AuditStageDto { pub stage: String, pub detail: String, pub at_ms: f64 }
#[derive(Debug,Clone,Serialize,TS)] #[ts(export)]
pub struct AuditRecordDto {
    pub id: String, pub kind: String, pub params: serde_json::Value,
    pub started_at: String, pub ended_at: String, pub duration_ms: f64,
    pub stages: Vec<AuditStageDto>, pub files: Vec<String>, pub status: String,
    pub error: Option<String>, pub result_summary: Option<String>, pub artifact: Option<String>,
}
impl From<crate::audit::AuditRecord> for AuditRecordDto {
    fn from(a: crate::audit::AuditRecord) -> Self {
        AuditRecordDto {
            id: a.id, kind: a.kind, params: a.params, started_at: a.started_at, ended_at: a.ended_at,
            duration_ms: a.duration_ms,
            stages: a.stages.into_iter().map(|s| AuditStageDto { stage: s.stage, detail: s.detail, at_ms: s.at_ms }).collect(),
            files: a.files, status: a.status, error: a.error, result_summary: a.result_summary, artifact: a.artifact,
        }
    }
}
```

- [ ] **Step 2: audit_cmds.rs**:

```rust
use crate::commands::AppState;
use crate::dto_audit::AuditRecordDto;
#[tauri::command]
pub fn audit_list(state: tauri::State<AppState>, limit: u32, kind: Option<String>, status: Option<String>) -> Vec<AuditRecordDto> {
    crate::audit::read(&state.ws.audit_path(), limit as usize, kind.as_deref(), status.as_deref())
        .into_iter().map(AuditRecordDto::from).collect()
}
#[tauri::command]
pub fn audit_log_tail(state: tauri::State<AppState>, lines: u32) -> String {
    // 取最新一个日志文件尾部 lines 行(tauri_plugin_log 按日期分文件)
    let dir = state.ws.log_dir();
    let latest = std::fs::read_dir(&dir).ok().into_iter().flatten().flatten()
        .map(|e| e.path()).filter(|p| p.extension().map(|x| x=="log").unwrap_or(false))
        .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
    match latest.and_then(|p| std::fs::read_to_string(p).ok()) {
        Some(txt) => { let v: Vec<&str> = txt.lines().collect(); v[v.len().saturating_sub(lines as usize)..].join("\n") }
        None => "(暂无日志文件)".into(),
    }
}
```

- [ ] **Step 3: lib.rs 接线**:
  1. `mod dto_audit; mod audit_cmds;`
  2. `tauri_plugin_log` 改:`tauri_plugin_log::Builder::new().target(tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Folder { path: <ws.log_dir()>, file_name: None })).level(log::LevelFilter::Info).build()` —— **注意**:`ws` 在 `.setup` 内才得;`tauri_plugin_log` 在 builder 阶段注册。解决:把 log_dir 解析提前(`Workspace::detect(current_dir)` 在 plugin 注册前调一次取 log_dir),或用 `tauri::path` 的 app_log_dir。**最简**:plugin 注册前 `let log_dir = paths::Workspace::detect(&std::env::current_dir()?).map(|w| w.log_dir());` 失败则退回默认 stdout-only。(实现时 READ 现 lib.rs `run()` 结构对齐;`?` 在 `run()` 返回 `()`,需在闭包外处理——用 `if let`。)
  3. `TaskRegistry::new(sink, ws.audit_path())`(改原 `TaskRegistry::new(sink)`)。
  4. `generate_handler!` 增 `audit_cmds::audit_list, audit_cmds::audit_log_tail`。
  5. 顶部加 `use log::info;`(或全限定)备后续命令用。

- [ ] **Step 4: 验证** `cargo test -p rquant-desktop 2>&1 | tail -3` 绿;确认 `desktop/src-tauri/bindings/AuditRecordDto.ts` `AuditStageDto.ts` 生成(`ls desktop/src-tauri/bindings/Audit*.ts`);`cargo build -p rquant-desktop` 绿。

- [ ] **Step 5: Commit**

```bash
git add desktop/src-tauri/src/dto_audit.rs desktop/src-tauri/src/audit_cmds.rs desktop/src-tauri/src/lib.rs desktop/src-tauri/bindings/
git commit -F - <<'EOF'
feat(desktop): audit DTOs + audit_list/audit_log_tail commands + log-to-file target
EOF
```

---

### Task 4: 任务类命令留痕 + 选股日期校验(I4)

**Files:** Modify `screen_cmds.rs`、`factor_cmds.rs`、`commands.rs`(backtest_run glue)、`manual_run.rs`、`data_bench.rs`(fetch_batch)、`deploy_cmds.rs`(deploy_run_month)

**Interfaces:** Consumes `TaskCtx::note_params/note_file/note_summary`(Task 2)。

每个任务体在 `tasks.start(...)` 闭包**开头**加 `ctx.note_params(json!({...}))` + `ctx.note_file(...)`(桥层已知输入/产物),并在产出后 `ctx.note_summary(...)`;命令函数加 `log::info!`。具体每命令(READ 各文件对齐现有变量名):

- [ ] **Step 1: screen_asof**(`screen_cmds.rs`):闭包头 `ctx.note_params(serde_json::json!({"config":&config,"as_of":&as_of,"top":top}));` + `ctx.note_file(&ws.root().join("data/baostock/universe_baostock_day.csv").to_string_lossy());` + `ctx.note_file(&ws.root().join(&config).to_string_lossy());`;`run_screen` 返回后 `ctx.note_summary(&format!("universe {} top {}", res.n_universe, res.top));`。**I4**:把 `as_of: chrono::NaiveDate::parse_from_str(&as_of, "%Y-%m-%d").ok()` 改为先校验:闭包头 `let _ = chrono::NaiveDate::parse_from_str(&as_of, "%Y-%m-%d").map_err(|_| format!("日期格式应为 YYYY-MM-DD: {as_of}"))?;` 再 `.ok()` 用于 config。

- [ ] **Step 2: screen_backtest_run**(`screen_cmds.rs`):note_params `{"config","from","to","top","rebalance","cost_bps"}`;note_file universe+config;归档后 `ctx.note_summary(&format!("run {id}"))`。**I4**:同样对 `from`/`to` 非空时校验格式,坏则 `?` 返错。

- [ ] **Step 3: factor_run**(`factor_cmds.rs`):note_params `{"factors":valid,"horizon","layers","sample"}`;产出后 `ctx.note_summary(&format!("symbols {}", report.n_symbols))`(按 FactorReport 实际字段名)。

- [ ] **Step 4: backtest_run**(`commands.rs` 调 `backtest_run::run`):在任务体加 note_params(config 关键字段:tree_path/primary/mode/window/cost_bps)+ note_file(tree_path、primary_path、输出 run 目录);归档后 note_summary(run id)。(READ backtest_run.rs 看 ctx 是否透传到体内;若体在 backtest_run.rs,改那里。)

- [ ] **Step 5: manual_run**(`manual_run.rs`):note_params `{"books":books,"commit":commit}`;每本处理可 note_file(state_path/tree_path);末尾 note_summary(committed 数/targets)。

- [ ] **Step 6: fetch_batch**(`data_bench.rs`):note_params `{"symbols_n":symbols.len(),"scale":scale,"datalen":datalen,"adjust":&adjust}`;note_summary(written 文件数)。

- [ ] **Step 7: deploy_run_month**(`deploy_cmds.rs`):note_params `{"as_of":&as_of,"config":DEPLOY_CONFIG}`;note_file(frozen config、universe、index csi300);产出 dto 后 note_summary(`format!("picks {} proj_nav {:.3}", dto.picks.len(), dto.proj_nav)`)。

- [ ] **Step 8: 验证 + Commit** 每步后 `cargo build -p rquant-desktop` 绿;全部完成 `cargo test -p rquant-desktop 2>&1 | tail -3` 绿。坏日期校验加一条单测(screen_cmds 若有测试模块;否则在 audit/集成层略过,靠 GUI 冒烟)。

```bash
git add desktop/src-tauri/src/screen_cmds.rs desktop/src-tauri/src/factor_cmds.rs desktop/src-tauri/src/commands.rs desktop/src-tauri/src/backtest_run.rs desktop/src-tauri/src/manual_run.rs desktop/src-tauri/src/data_bench.rs desktop/src-tauri/src/deploy_cmds.rs
git commit -F - <<'EOF'
feat(desktop): instrument task commands with audit params/files/summary + log::info; reject malformed screen dates
EOF
```

---

### Task 5: deploy_commit 改任务(C1)+ 前端落地 + kday CSV 列名(I6)

**Files:** Modify `deploy_cmds.rs`、`desktop/ui/src/stores/deploy.ts`、`desktop/ui/src/pages/Deploy.tsx`、`desktop/ui/src/api/ipc.ts`

**Interfaces:** `deploy_commit_month` 由同步 `Result<(),String>` 改为返回 `Result<String,String>`(task id),经 `task://progress` 落地;前端 `commit` 改 `trackTask`。

- [ ] **Step 1: 后端 C1**(`deploy_cmds.rs`):`deploy_commit_month` 改 `state.tasks.start("deploy_commit", true, move |ctx| { ctx.note_params(json!({"as_of":&as_of})); ctx.progress(0.3,"选股",&as_of); /* 原 compute_month + 写状态逻辑 */ ctx.note_summary(&format!("nav {:.3}", proj_nav)); Ok(serde_json::Value::Null) })`(返回 task id)。原同步体整体移入闭包;`?` 保留(闭包返 `Result<Value,String>`)。**I6**:`load_close` 改按表头列名解析 close 列(读首行定位 `close` 索引,而非硬编码 `c[4]`);找不到列名则返错。

- [ ] **Step 2: ipc.ts**:`deployCommitMonth: (asOf: string) => invoke<string>("deploy_commit_month", { asOf })`(返回类型 void→string)。

- [ ] **Step 3: deploy store**(`stores/deploy.ts`):`commit` 改:`const id = await get().api.deployCommitMonth(asOf); set({ commitTaskId: id }); trackTask(id, { done: () => { set({ preview:null, commitError:null }); void get().load(); }, failed: (info) => set({ commitError: friendlyError(info.error ?? "落账失败").title }) });`;加 `commitTaskId/commitError` 字段。返回值改 `Promise<void>`(发起即返;结果经 trackTask)。

- [ ] **Step 4: Deploy.tsx**:确认按钮 `loading`/`disabled` 改读 `useTaskInfo(st.commitTaskId)?.status==="running"`;运行中显 `<TaskRunning>`;`commitError` 红字显示。保留既有 corrupt/empty/preview UX。

- [ ] **Step 5: 验证 + Commit** `cargo test -p rquant-desktop` + `tsc` + `npm ... test --run` 全绿;更新 `stores/deploy.test.ts`(commit 现经 trackTask:发起置 commitTaskId、ingest done 清 preview)。

```bash
git add desktop/src-tauri/src/deploy_cmds.rs desktop/ui/src/stores/deploy.ts desktop/ui/src/pages/Deploy.tsx desktop/ui/src/api/ipc.ts desktop/ui/src/stores/deploy.test.ts
git commit -F - <<'EOF'
fix(desktop): deploy_commit_month runs as heavy task (no IPC block, audited); CSV close-by-header; UI commit via task
EOF
```

---

### Task 6: iter stderr 死锁(I5)+ iter 留痕 + eval_certify 直接审计

**Files:** Modify `iter_cmds.rs`、`eval_cmds.rs`

- [ ] **Step 1: iter_cmds I5 + cmdline**:重构 `iter_run_round` 子进程:组装命令后 `let cmdline = format!("python scripts/iterate.py {config} --note ...")`,`ctx.note_params(json!({"config":&config,"note":&note,"axis":&axis,"top":top,"benchmark":&benchmark,"rebalance":rebalance,"cmdline":&cmdline}));`。**并发读 stderr**:`spawn` 后开一个线程 `let h = std::thread::spawn(move || { let mut s=String::new(); stderr.read_to_string(&mut s).ok(); s });` 同时主线程逐行读 stdout 转 progress;`child.wait()` 后 `let err = h.join().unwrap_or_default();` —— 避免 wait-after-read 管道死锁。失败时 error 含**全量** stderr(非仅末行)+ exit code + cmdline。

- [ ] **Step 2: eval_certify 直接审计**(`eval_cmds.rs`):`eval_certify` 仍同步(快、无 IPC 阻塞),末尾追加一条审计:`let _ = crate::audit::append(&state.ws.audit_path(), &crate::audit::AuditRecord { id: format!("certify-{}", <时间戳>), kind:"eval_certify".into(), params: serde_json::json!({"reports": paths, "name": name}), started_at: now, ended_at: now, duration_ms: 0.0, stages: vec![], files: paths.clone(), status:"done".into(), error:None, result_summary: Some(format!("verdict {}", verdict.pass)), artifact:None });`(now 用 `chrono::Local::now().format(...)`;verdict 字段名按实际)。READ eval_cmds.rs 对齐变量。

- [ ] **Step 3: 验证 + Commit** `cargo test -p rquant-desktop` 绿;`cargo build` 绿。

```bash
git add desktop/src-tauri/src/iter_cmds.rs desktop/src-tauri/src/eval_cmds.rs
git commit -F - <<'EOF'
fix(desktop): iter_round concurrent stderr read (no pipe deadlock) + full capture + cmdline; audit eval_certify
EOF
```

---

### Task 7: commands/readers 健壮性(I1/I2/I3)+ analyze CSV 列名(I6)

**Files:** Modify `commands.rs`、`readers.rs`、`analyze_cmds.rs`

- [ ] **Step 1: I1 cockpit_overview catch_unwind**:`cockpit_overview` 改 `std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| assemble_overview(&state.ws))).unwrap_or_else(|_| <降级 OverviewDto>)`(降级 = 空/错误标注的 OverviewDto;READ OverviewDto 构造一个最小可序列化降级值,并 `log::error!` panic)。
- [ ] **Step 2: I2 book_detail 损坏透出**(`commands.rs:53` `assemble_book_detail`):`read_paper_state(...).ok().flatten()` 改为区分:读到 Err → 在 DTO 标 corrupt(参照 `read_book_card` 的 corrupt 处理:BookDetailDto 加/复用 status 字段,或 snapshot=None + 一个 error 字段)。READ BookDetailDto + read_book_card 对齐。
- [ ] **Step 3: I3 trip 序列化记日志**(`readers.rs:28`):`serde_json::to_value(t).unwrap_or_else(|e| { log::warn!("trip serialize failed: {e}"); serde_json::Value::Null })`。
- [ ] **Step 4: I6 analyze CSV 列名**(`analyze_cmds.rs:17,45`):close/amount/index 列改按表头名解析(同 Task5 load_close 手法),找不到则返错而非静默错列。
- [ ] **Step 5: 验证 + Commit** `cargo test -p rquant-desktop 2>&1 | tail -3` 绿(既有 error/readers 测试不破);加最小回归(book_detail 损坏态返 corrupt——若有 readers 测试模块)。

```bash
git add desktop/src-tauri/src/commands.rs desktop/src-tauri/src/readers.rs desktop/src-tauri/src/analyze_cmds.rs
git commit -F - <<'EOF'
fix(desktop): cockpit panic guard; book_detail surfaces corrupt state; trip serialize logs; analyze CSV by header
EOF
```

---

### Task 8: 客户端「审计」页

**Files:** Create `desktop/ui/src/stores/audit.ts`、`desktop/ui/src/stores/audit.test.ts`、`desktop/ui/src/pages/Audit.tsx`;Modify `api/ipc.ts`、`App.tsx`、`labels.ts`

**Interfaces:** Consumes `@bindings/AuditRecordDto`、`AuditStageDto`(Task 3)。

- [ ] **Step 1: ipc.ts 追加**:
```typescript
  auditList: (limit: number, kind?: string, status?: string) => invoke<import("@bindings/AuditRecordDto").AuditRecordDto[]>("audit_list", { limit, kind: kind ?? null, status: status ?? null }),
  auditLogTail: (lines: number) => invoke<string>("audit_log_tail", { lines }),
```

- [ ] **Step 2: store + 失败测试**(`stores/audit.ts` + `audit.test.ts`):

```typescript
// audit.ts
import { create } from "zustand";
import type { AuditRecordDto } from "@bindings/AuditRecordDto";
import { api as realApi, type Api } from "../api/ipc";
interface AuditState { api: Api; records: AuditRecordDto[]; error: string | null; load: (kind?: string, status?: string) => Promise<void>; }
export const useAudit = create<AuditState>((set, get) => ({
  api: realApi, records: [], error: null,
  load: async (kind, status) => { try { set({ records: await get().api.auditList(200, kind, status), error: null }); } catch (e) { set({ error: String(e) }); } },
}));
```
```typescript
// audit.test.ts
import { test, expect, afterEach } from "vitest";
import { useAudit } from "./audit";
const real = useAudit.getState().api;
afterEach(() => useAudit.setState({ api: real, records: [], error: null }));
test("load fills records from api", async () => {
  useAudit.setState({ api: { ...real, auditList: async () => ([{ id:"t1", kind:"screen_asof", params:{}, started_at:"x", ended_at:"y", duration_ms:1200, stages:[], files:[], status:"done", error:null, result_summary:"top-50", artifact:null }] as any) } });
  await useAudit.getState().load();
  expect(useAudit.getState().records[0].kind).toBe("screen_asof");
});
```

- [ ] **Step 3: labels.ts**:`export const AUDIT_KIND_ZH: Record<string,string> = { screen_asof:"指定日选股", screen_backtest:"选股回测", deploy_month:"部署预览", deploy_commit:"部署落账", factor:"因子分析", iter_round:"研究跑轮", backtest:"回测", manual_run:"手动跑单", fetch:"数据抓取", eval_certify:"认证" };` + `export const auditKindZh = (k:string) => AUDIT_KIND_ZH[k] ?? k;`(kind 串以各命令 `tasks.start` 传的为准——实现时核对 Task2/4/5/6 的 kind 字面量,保持一致)。

- [ ] **Step 4: Audit.tsx**(时间线表 + 筛选 + 详情抽屉):

```tsx
import { useEffect, useState } from "react";
import { Table, Select, Input, Drawer, Tag, Typography, Tabs } from "antd";
import { useAudit } from "../stores/audit";
import { auditKindZh } from "../labels";
import type { AuditRecordDto } from "@bindings/AuditRecordDto";
import { api } from "../api/ipc";
const STATUS_COLOR: Record<string,string> = { done:"green", failed:"red", cancelled:"default", running:"blue" };
export default function Audit() {
  const st = useAudit();
  const [kind, setKind] = useState<string|undefined>(); const [status, setStatus] = useState<string|undefined>();
  const [q, setQ] = useState(""); const [sel, setSel] = useState<AuditRecordDto|null>(null);
  const [rawLog, setRawLog] = useState("");
  useEffect(() => { void st.load(kind, status); }, [kind, status]);
  const rows = st.records.filter(r => !q || JSON.stringify(r.params).includes(q) || r.kind.includes(q) || (r.error??"").includes(q));
  return (
    <div>
      <div style={{ display:"flex", gap:8, marginBottom:8 }}>
        <Select allowClear placeholder="类型" style={{width:140}} value={kind} onChange={setKind}
          options={[...new Set(st.records.map(r=>r.kind))].map(k=>({value:k,label:auditKindZh(k)}))} />
        <Select allowClear placeholder="状态" style={{width:120}} value={status} onChange={setStatus}
          options={["done","failed","cancelled"].map(s=>({value:s,label:s}))} />
        <Input placeholder="检索参数/错误" value={q} onChange={e=>setQ(e.target.value)} style={{width:220}} allowClear />
        <Typography.Link onClick={() => { void api.auditLogTail(400).then(setRawLog); }}>原始日志</Typography.Link>
      </div>
      <Table size="small" rowKey="id" dataSource={rows} pagination={{pageSize:20}} onRow={(r)=>({onClick:()=>setSel(r)})}
        columns={[
          {title:"时间", dataIndex:"started_at", width:160},
          {title:"类型", dataIndex:"kind", render:auditKindZh, width:110},
          {title:"状态", dataIndex:"status", width:90, render:(s:string)=><Tag color={STATUS_COLOR[s]??"default"}>{s}</Tag>},
          {title:"耗时", dataIndex:"duration_ms", width:90, render:(m:number)=>`${(m/1000).toFixed(1)}s`},
          {title:"参数", dataIndex:"params", ellipsis:true, render:(p:unknown)=>JSON.stringify(p)},
          {title:"错误", dataIndex:"error", ellipsis:true, render:(e:string|null)=>e?<span style={{color:"#dc2626"}}>{e}</span>:""},
        ]} />
      <Drawer title={sel?`${auditKindZh(sel.kind)} · ${sel.id}`:""} open={!!sel} onClose={()=>setSel(null)} width={560}>
        {sel && <Tabs items={[
          {key:"detail", label:"详情", children:<>
            <p><b>参数</b></p><pre style={{whiteSpace:"pre-wrap"}}>{JSON.stringify(sel.params,null,2)}</pre>
            <p><b>阶段时序</b></p>{sel.stages.map((s,i)=><div key={i}>{(s.at_ms/1000).toFixed(1)}s · {s.stage} {s.detail}</div>)}
            <p><b>触及文件</b>(桥层输入/产物,非逐股)</p>{sel.files.map((f,i)=><div key={i} style={{fontSize:12,opacity:.8}}>{f}</div>)}
            {sel.result_summary && <p><b>结果</b>:{sel.result_summary}</p>}
            {sel.error && <><p><b>完整错误</b></p><pre style={{whiteSpace:"pre-wrap",color:"#dc2626"}}>{sel.error}</pre></>}
          </>},
        ]} />}
      </Drawer>
      {rawLog && <Drawer title="原始日志(尾部)" open={!!rawLog} onClose={()=>setRawLog("")} width={680}><pre style={{whiteSpace:"pre-wrap",fontSize:12}}>{rawLog}</pre></Drawer>}
    </div>
  );
}
```

- [ ] **Step 5: App.tsx**:`import Audit`;MODULES 加 `{ key:"audit", label:"审计" }`(放 research 后);`<Route path="/audit" element={<Audit/>}/>`;占位过滤排除 `audit`。

- [ ] **Step 6: 验证 + Commit** `npm --prefix desktop/ui run test -- --run src/stores/audit.test.ts` PASS;`tsc` 0;`npm ... test --run` 全绿。

```bash
git add desktop/ui/src/stores/audit.ts desktop/ui/src/stores/audit.test.ts desktop/ui/src/pages/Audit.tsx desktop/ui/src/api/ipc.ts desktop/ui/src/App.tsx desktop/ui/src/labels.ts
git commit -F - <<'EOF'
feat(ui): 审计 page — operation timeline, filter/search, detail drawer, raw log
EOF
```

---

### Task 9: 收尾闸 + 文档 + 记忆

- [ ] **Step 1: 全量后端闸** `cargo test --workspace 2>&1 | grep "test result"` 全 ok。
- [ ] **Step 2: 前端闸** `node desktop/ui/node_modules/typescript/bin/tsc --noEmit -p desktop/ui/tsconfig.app.json` 0;`npm --prefix desktop/ui run test -- --run` 全过;`npm --prefix desktop/ui run build` 成功。
- [ ] **Step 3: 审计旁路核验** `grep -rn "audit::append" desktop/src-tauri/src` 确认终态/认证调用;确认 `.rquant-desktop/audit/` `.rquant-desktop/logs/` 在 .gitignore 覆盖内(`.rquant-desktop` 已忽略)。
- [ ] **Step 4: GUI 冒烟**(release `cargo tauri dev --release --no-watch`,CWD 修复已合):跑一次 选股(指定日) → 审计页出现该记录(参数/阶段时序/耗时/触及文件齐全);故意输坏日期 → 任务失败 + 审计详情见完整 Err;点「原始日志」有内容;部署落账走任务且被审计。
- [ ] **Step 5: 文档 + 记忆** `docs/desktop-screen-research.md` 加「流程审计」一节;更新记忆 `rquant-project.md`(process-audit 落地 + 工程教训:审计旁路落盘、TaskRegistry 单卡点、Crit/Imp 健壮性修复清单)。
- [ ] **Step 6: Commit**

```bash
git add docs/ && git commit -F - <<'EOF'
docs(desktop): process-audit + robustness usage; finalize
EOF
```

- [ ] **Step 7: finishing** 调用 superpowers:finishing-a-development-branch 收口。

---

## 自审备忘(写计划时已校)

- **spec 覆盖**:审计模型/落盘→T1;卡点捕获+C2→T2;DTO/命令/日志落盘→T3;命令留痕+I4→T4;C1+前端+I6(kday)→T5;I5+iter留痕+certify审计→T6;I1/I2/I3+I6(analyze)→T7;审计页→T8;闸/文档/记忆/finishing→T9。Crit(C1=T5,C2=T2)、Imp(I1/I2/I3=T7,I4=T4,I5=T6,I6=T5+T7)全覆盖。
- **类型一致**:`AuditRecord`/`AuditStage`(后端)↔`AuditRecordDto`/`AuditStageDto`(前端)字段名/类型一致;`f64` 贯穿 duration_ms/at_ms;`note_params/note_file/note_summary` 命名贯穿 T2→T4/5/6;kind 字面量(screen_asof/screen_backtest/deploy_month/deploy_commit/factor/iter_round/backtest/manual_run/fetch/eval_certify)需 T4/5/6 与 T8 `AUDIT_KIND_ZH` 对齐(实现核对)。
- **旁路纪律**:`audit::append` 失败不冒泡(T2/T6),不毁主流程。
- **YAGNI**:不做日志检索引擎(前端筛选+tail);不做审计轮转 UI;Minor 健壮项不修。
- **已知依赖**:T4-T7 各命令需 READ 现有变量名对齐(param JSON 键引用真实变量);`tauri_plugin_log` 文件 target 的 API 形态实现时按当前 crate 版本核对(Target/TargetKind::Folder)。
