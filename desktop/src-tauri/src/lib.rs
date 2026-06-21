//! rquant 桌面端桥接层：DTO 转换 + 任务调度 + 工作区路径解析。
//! 零业务逻辑——一切计算调 `rquant` 库；spec: docs/superpowers/specs/2026-06-12-rquant-desktop-design.md

pub mod audit;
pub mod audit_cmds;
pub mod analyze_cmds;
pub mod backtest_run;
pub mod deploy_book;
pub mod deploy_cmds;
pub mod dto_audit;
pub mod eval_cmds;
pub mod factor_cmds;
pub mod books;
pub mod commands;
pub mod data_bench;
pub mod dto;
pub mod dto_iter;
pub mod dto_screen;
pub mod dto_factor;
pub mod dto_eval;
pub mod dto_analyze;
pub mod dto_deploy;
pub mod error;
pub mod gates;
pub mod analyze;
pub mod index_relative;
pub mod iter_cmds;
pub mod iter_read;
pub mod journal;
pub mod manual_run;
pub mod paths;
pub mod readers;
pub mod replay;
pub mod results;
pub mod runlog;
pub mod runs;
pub mod schtask;
pub mod screen_cmds;
pub mod screen_runs;
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
    // Resolve log dir before plugin registration (ws only available inside .setup).
    let log_dir = paths::Workspace::detect(&std::env::current_dir().unwrap_or_default())
        .map(|w| w.log_dir());
    let log_plugin = {
        let builder = tauri_plugin_log::Builder::new()
            .level(tauri_plugin_log::log::LevelFilter::Info);
        if let Some(dir) = log_dir {
            builder
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::Folder { path: dir, file_name: None },
                ))
                .build()
        } else {
            builder.build()
        }
    };
    tauri::Builder::default()
        .plugin(log_plugin)
        .setup(|app| {
            use tauri::Manager;
            let ws = paths::Workspace::detect(&std::env::current_dir()?)
                .ok_or("workspace not found: run from inside the rquant repo")?;
            // 进程 CWD 设为仓库根:screen 配置内部按相对路径引用树(如 quality_trees:
            // [deploy/xxx.yaml]),由 run_screen 按 CWD 解析。桌面 app 经 cargo tauri/打包
            // 启动时 CWD≠仓库根→相对树路径找不到(os error 3)。与 CLI(自仓库根运行)对齐。
            let _ = std::env::set_current_dir(ws.root());
            let sink = Arc::new(TauriSink(app.handle().clone()));
            let audit_path = ws.audit_path();
            app.manage(commands::AppState { ws, tasks: Arc::new(tasks::TaskRegistry::new(sink, audit_path)) });
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
            screen_cmds::screen_configs_list,
            screen_cmds::index_list,
            screen_cmds::screen_asof,
            screen_cmds::screen_15m_asof,
            screen_cmds::screen_15m_configs_list,
            screen_cmds::screen_backtest_run,
            screen_cmds::screen_runs_list,
            screen_cmds::screen_run_report,
            screen_cmds::screen_index_relative,
            iter_cmds::iter_ledger,
            iter_cmds::iter_queue,
            iter_cmds::iter_round_card,
            iter_cmds::iter_run_round,
            eval_cmds::eval_list_reports,
            eval_cmds::eval_certify,
            factor_cmds::factor_run,
            analyze_cmds::analyze_sector,
            analyze_cmds::analyze_twoleg,
            analyze_cmds::analyze_deploy,
            deploy_cmds::deploy_book_read,
            deploy_cmds::deploy_run_month,
            deploy_cmds::deploy_commit_month,
            audit_cmds::audit_list,
            audit_cmds::audit_log_tail,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
