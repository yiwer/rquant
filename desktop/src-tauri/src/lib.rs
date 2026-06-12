//! rquant 桌面端桥接层：DTO 转换 + 任务调度 + 工作区路径解析。
//! 零业务逻辑——一切计算调 `rquant` 库；spec: docs/superpowers/specs/2026-06-12-rquant-desktop-design.md

pub mod books;
pub mod commands;
pub mod dto;
pub mod error;
pub mod gates;
pub mod journal;
pub mod manual_run;
pub mod paths;
pub mod readers;
pub mod runlog;
pub mod schtask;
pub mod tasks;

use std::sync::Arc;
use tauri::Emitter;

struct TauriSink(tauri::AppHandle);
impl tasks::ProgressSink for TauriSink {
    fn emit(&self, info: &dto::TaskInfoDto) {
        // 双发:精确通道(按 id 过滤用)+ 固定通道(前端统一订阅用,tauri 事件是精确匹配无前缀订阅)
        let _ = self.0.emit(&format!("task://progress/{}", info.id), info);
        let _ = self.0.emit("task://progress", info);
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::new().build())
        .setup(|app| {
            use tauri::Manager;
            let ws = paths::Workspace::detect(&std::env::current_dir()?)
                .ok_or("workspace not found: run from inside the rquant repo")?;
            let sink = Arc::new(TauriSink(app.handle().clone()));
            app.manage(commands::AppState { ws, tasks: Arc::new(tasks::TaskRegistry::new(sink)) });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::cockpit_overview,
            commands::book_detail,
            commands::runlog_tail,
            commands::run_gate_now,
            commands::manual_run,
            commands::task_list,
            commands::task_cancel,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
