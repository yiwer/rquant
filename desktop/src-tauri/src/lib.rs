//! rquant 桌面端桥接层：DTO 转换 + 任务调度 + 工作区路径解析。
//! 零业务逻辑——一切计算调 `rquant` 库；spec: docs/superpowers/specs/2026-06-12-rquant-desktop-design.md

pub mod dto;
pub mod error;
pub mod paths;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::new().build())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
