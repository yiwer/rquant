//! 任务注册表:长任务统一入口——std::thread + catch_unwind,进度经 ProgressSink 推送。
//! 重任务(网格/批量/manual run)独占一个槽位(spec §12.5);轻命令不经此处。
//! paper/ 写互斥说明:M1 唯一写者 manual_run 是重任务,独占槽位即满足 spec §7 的
//! "同一时刻至多一个 commit 型任务"——后续里程碑引入第二类写者时再升级为显式锁。
use crate::audit::{AuditRecord, AuditStage};
use crate::dto::{TaskInfoDto, TaskProgressDto};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub trait ProgressSink: Send + Sync + 'static {
    fn emit(&self, info: &TaskInfoDto);
}

pub struct TaskCtx {
    cancel: Arc<AtomicBool>,
    id: String,
    shared: Arc<Shared>,
}

impl TaskCtx {
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
    pub fn progress(&self, pct: f32, stage: &str, detail: &str) {
        // Push an AuditStage with elapsed time before updating info
        self.shared.with_accum(&self.id, |a| {
            let at_ms = a.start.elapsed().as_millis() as f64;
            a.stages.push(AuditStage {
                stage: stage.to_string(),
                detail: detail.to_string(),
                at_ms,
            });
        });
        self.shared.update(&self.id, |info| {
            info.progress = TaskProgressDto {
                pct,
                stage: stage.to_string(),
                detail: detail.to_string(),
            };
        });
    }
    pub fn note_params(&self, p: serde_json::Value) {
        self.shared.with_accum(&self.id, |a| a.params = p);
    }
    pub fn note_file(&self, path: &str) {
        self.shared.with_accum(&self.id, |a| {
            if !a.files.iter().any(|f| f == path) {
                a.files.push(path.to_string())
            }
        });
    }
    pub fn note_summary(&self, s: &str) {
        self.shared.with_accum(&self.id, |a| a.summary = Some(s.to_string()));
    }
}

/// Per-task audit accumulator: collects data during task execution.
struct AuditAccum {
    started_at: String,
    start: Instant,
    params: serde_json::Value,
    files: Vec<String>,
    summary: Option<String>,
    stages: Vec<AuditStage>,
}

fn now_iso() -> String {
    chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()
}

struct Shared {
    tasks: Mutex<HashMap<String, (TaskInfoDto, Arc<AtomicBool>, AuditAccum)>>,
    sink: Arc<dyn ProgressSink>,
    heavy_busy: AtomicBool,
    audit_path: PathBuf,
}

impl Shared {
    fn update(&self, id: &str, f: impl FnOnce(&mut TaskInfoDto)) {
        let mut g = self.tasks.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((info, _, _)) = g.get_mut(id) {
            f(info);
            // sink panic 不得毒化 tasks 锁(注册表全局瘫痪)——隔离之
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.sink.emit(info)));
        }
    }

    fn with_accum(&self, id: &str, f: impl FnOnce(&mut AuditAccum)) {
        let mut g = self.tasks.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((_, _, accum)) = g.get_mut(id) {
            f(accum);
        }
    }
}

pub struct TaskRegistry {
    shared: Arc<Shared>,
    seq: AtomicU64,
}

impl TaskRegistry {
    pub fn new(sink: Arc<dyn ProgressSink>, audit_path: PathBuf) -> Self {
        TaskRegistry {
            shared: Arc::new(Shared {
                tasks: Mutex::new(HashMap::new()),
                sink,
                heavy_busy: AtomicBool::new(false),
                audit_path,
            }),
            seq: AtomicU64::new(1),
        }
    }

    /// heavy=true 时独占重任务槽;占用中返回 Err。
    /// body 返回 Ok(result) → done;Err(含 "cancelled") → cancelled;其余 Err → failed。
    pub fn start<F>(&self, kind: &str, heavy: bool, body: F) -> Result<String, String>
    where
        F: FnOnce(&TaskCtx) -> Result<serde_json::Value, String> + Send + 'static,
    {
        if heavy
            && self
                .shared
                .heavy_busy
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
        {
            return Err("已有重任务运行中,请等待其完成或取消".to_string());
        }
        let id = format!("t{}", self.seq.fetch_add(1, Ordering::Relaxed));
        let cancel = Arc::new(AtomicBool::new(false));
        let info = TaskInfoDto {
            id: id.clone(),
            kind: kind.to_string(),
            status: "running".to_string(),
            progress: TaskProgressDto {
                pct: 0.0,
                stage: "start".into(),
                detail: String::new(),
            },
            error: None,
            result: None,
        };
        let accum = AuditAccum {
            started_at: now_iso(),
            start: Instant::now(),
            params: serde_json::Value::Null,
            files: Vec::new(),
            summary: None,
            stages: Vec::new(),
        };
        {
            let mut g = self.shared.tasks.lock().unwrap_or_else(|p| p.into_inner());
            g.insert(id.clone(), (info.clone(), cancel.clone(), accum));
        }
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.shared.sink.emit(&info)));

        let ctx = TaskCtx {
            cancel,
            id: id.clone(),
            shared: self.shared.clone(),
        };
        let shared = self.shared.clone();
        let tid = id.clone();
        let task_kind = kind.to_string();
        std::thread::spawn(move || {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| body(&ctx)));
            if heavy {
                // 槽位释放须在 body 返回之后(文件写已完成,无双写窗口),
                // 状态终写在释放之后也安全——update 只按本任务 id 定位自身条目。
                shared.heavy_busy.store(false, Ordering::SeqCst);
            }
            shared.update(&tid, |info| match &outcome {
                Ok(Ok(v)) => {
                    info.status = "done".into();
                    info.progress.pct = 1.0;
                    info.result = Some(v.clone());
                }
                Ok(Err(msg)) if msg.contains("cancelled") => {
                    info.status = "cancelled".into();
                    info.error = Some(msg.clone());
                }
                Ok(Err(msg)) => {
                    info.status = "failed".into();
                    info.error = Some(msg.clone());
                }
                Err(_) => {
                    info.status = "failed".into();
                    info.error =
                        Some("panic in task body (engine call guarded by catch_unwind)".into());
                }
            });

            // --- Audit side-path: assemble AuditRecord and append to disk ---
            // Extract the final info + accum under a single lock, then release before I/O.
            let audit_data = {
                let g = shared.tasks.lock().unwrap_or_else(|p| p.into_inner());
                g.get(&tid).map(|(info, _, accum)| {
                    let duration_ms = accum.start.elapsed().as_millis() as f64;
                    AuditRecord {
                        id: tid.clone(),
                        kind: task_kind.clone(),
                        params: accum.params.clone(),
                        started_at: accum.started_at.clone(),
                        ended_at: now_iso(),
                        duration_ms,
                        stages: accum.stages.clone(),
                        files: accum.files.clone(),
                        status: info.status.clone(),
                        error: info.error.clone(),
                        result_summary: accum.summary.clone(),
                        artifact: None,
                    }
                })
            };
            if let Some(rec) = audit_data {
                if let Err(e) = crate::audit::append(&shared.audit_path, &rec) {
                    eprintln!("[audit] append failed for task {}: {}", tid, e);
                }
            }
        });
        Ok(id)
    }

    pub fn cancel(&self, id: &str) {
        let g = self.shared.tasks.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((_, c, _)) = g.get(id) {
            c.store(true, Ordering::Relaxed);
        }
    }

    pub fn get(&self, id: &str) -> Option<TaskInfoDto> {
        self.shared
            .tasks
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(id)
            .map(|(i, _, _)| i.clone())
    }

    pub fn list(&self) -> Vec<TaskInfoDto> {
        let mut v: Vec<_> = self
            .shared
            .tasks
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .values()
            .map(|(i, _, _)| i.clone())
            .collect();
        v.sort_by(|a, b| a.id.cmp(&b.id));
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    struct NullSink;
    impl ProgressSink for NullSink {
        fn emit(&self, _info: &crate::dto::TaskInfoDto) {}
    }

    fn wait_status(reg: &TaskRegistry, id: &str, want: &str) -> crate::dto::TaskInfoDto {
        for _ in 0..200 {
            let info = reg.get(id).unwrap();
            if info.status == want {
                return info;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("task {} never reached {}; last: {:?}", id, want, reg.get(id));
    }

    fn reg() -> (TaskRegistry, tempfile::TempDir) {
        let td = tempfile::tempdir().unwrap();
        let ap = td.path().join("audit.jsonl");
        (TaskRegistry::new(Arc::new(NullSink), ap), td)
    }

    #[test]
    fn task_runs_to_done_with_result() {
        let (r, _td) = reg();
        let id = r
            .start("demo", false, |ctx| {
                ctx.progress(0.5, "half", "");
                Ok(serde_json::json!({"answer": 42}))
            })
            .unwrap();
        let info = wait_status(&r, &id, "done");
        assert_eq!(info.result.unwrap()["answer"], 42);
    }

    #[test]
    fn cancel_flag_reaches_task_body() {
        let (r, _td) = reg();
        let id = r
            .start("loop", false, |ctx| {
                for _ in 0..1000 {
                    if ctx.cancelled() {
                        return Err("cancelled by user".into());
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                Ok(serde_json::Value::Null)
            })
            .unwrap();
        std::thread::sleep(Duration::from_millis(30));
        r.cancel(&id);
        let info = wait_status(&r, &id, "cancelled");
        assert!(info.error.unwrap().contains("cancelled"));
    }

    #[test]
    fn panic_becomes_failed_not_process_death() {
        let (r, _td) = reg();
        let id = r.start("boom", false, |_ctx| panic!("kaboom")).unwrap();
        let info = wait_status(&r, &id, "failed");
        assert!(info.error.unwrap().contains("panic"));
    }

    #[test]
    fn heavy_slot_is_exclusive() {
        let (r, _td) = reg();
        let _id1 = r
            .start("heavy1", true, |_ctx| {
                std::thread::sleep(Duration::from_millis(300));
                Ok(serde_json::Value::Null)
            })
            .unwrap();
        std::thread::sleep(Duration::from_millis(30));
        let err = r.start("heavy2", true, |_ctx| Ok(serde_json::Value::Null));
        assert!(err.is_err(), "second heavy task must be rejected while first runs");
    }

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
}
