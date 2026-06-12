//! 任务注册表:长任务统一入口——std::thread + catch_unwind,进度经 ProgressSink 推送。
//! 重任务(网格/批量/manual run)独占一个槽位(spec §12.5);轻命令不经此处。
//! paper/ 写互斥说明:M1 唯一写者 manual_run 是重任务,独占槽位即满足 spec §7 的
//! "同一时刻至多一个 commit 型任务"——后续里程碑引入第二类写者时再升级为显式锁。
use crate::dto::{TaskInfoDto, TaskProgressDto};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

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
        self.shared.update(&self.id, |info| {
            info.progress = TaskProgressDto {
                pct,
                stage: stage.to_string(),
                detail: detail.to_string(),
            };
        });
    }
}

struct Shared {
    tasks: Mutex<HashMap<String, (TaskInfoDto, Arc<AtomicBool>)>>,
    sink: Arc<dyn ProgressSink>,
    heavy_busy: AtomicBool,
}

impl Shared {
    fn update(&self, id: &str, f: impl FnOnce(&mut TaskInfoDto)) {
        let mut g = self.tasks.lock().expect("task map poisoned");
        if let Some((info, _)) = g.get_mut(id) {
            f(info);
            // sink panic 不得毒化 tasks 锁(注册表全局瘫痪)——隔离之
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.sink.emit(info)));
        }
    }
}

pub struct TaskRegistry {
    shared: Arc<Shared>,
    seq: AtomicU64,
}

impl TaskRegistry {
    pub fn new(sink: Arc<dyn ProgressSink>) -> Self {
        TaskRegistry {
            shared: Arc::new(Shared {
                tasks: Mutex::new(HashMap::new()),
                sink,
                heavy_busy: AtomicBool::new(false),
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
        {
            let mut g = self.shared.tasks.lock().expect("task map poisoned");
            g.insert(id.clone(), (info.clone(), cancel.clone()));
        }
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.shared.sink.emit(&info)));

        let ctx = TaskCtx {
            cancel,
            id: id.clone(),
            shared: self.shared.clone(),
        };
        let shared = self.shared.clone();
        let tid = id.clone();
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
        });
        Ok(id)
    }

    pub fn cancel(&self, id: &str) {
        let g = self.shared.tasks.lock().expect("task map poisoned");
        if let Some((_, c)) = g.get(id) {
            c.store(true, Ordering::Relaxed);
        }
    }

    pub fn get(&self, id: &str) -> Option<TaskInfoDto> {
        self.shared
            .tasks
            .lock()
            .expect("task map poisoned")
            .get(id)
            .map(|(i, _)| i.clone())
    }

    pub fn list(&self) -> Vec<TaskInfoDto> {
        let mut v: Vec<_> = self
            .shared
            .tasks
            .lock()
            .expect("task map poisoned")
            .values()
            .map(|(i, _)| i.clone())
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

    fn reg() -> TaskRegistry {
        TaskRegistry::new(Arc::new(NullSink))
    }

    #[test]
    fn task_runs_to_done_with_result() {
        let r = reg();
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
        let r = reg();
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
        let r = reg();
        let id = r.start("boom", false, |_ctx| panic!("kaboom")).unwrap();
        let info = wait_status(&r, &id, "failed");
        assert!(info.error.unwrap().contains("panic"));
    }

    #[test]
    fn heavy_slot_is_exclusive() {
        let r = reg();
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
}
