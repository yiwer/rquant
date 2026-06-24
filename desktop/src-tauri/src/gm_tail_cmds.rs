//! gm 尾盘定时任务 Tauri 命令(薄壳——仅提取 state 并转发 gm_tail 逻辑)。
use crate::commands::AppState;
use crate::dto_gm::{GmTailConfig, GmTailStatusDto};
use crate::gm_tail;

/// 当前状态(任务是否装/排程/配置/token/产物计数/日志尾)。失败降级,不 panic。
#[tauri::command]
pub fn gm_tail_status(state: tauri::State<AppState>) -> GmTailStatusDto {
    gm_tail::status_dto(&state.ws)
}

#[tauri::command]
pub fn gm_tail_get_config(state: tauri::State<AppState>) -> GmTailConfig {
    gm_tail::read_config(&state.ws)
}

/// 写配置(sanitize 后落盘);返回回读的配置。不自动重装任务——改时间需再 install。
#[tauri::command]
pub fn gm_tail_set_config(
    state: tauri::State<AppState>,
    config: GmTailConfig,
) -> Result<GmTailConfig, String> {
    gm_tail::write_config(&state.ws, &config)?;
    Ok(gm_tail::read_config(&state.ws))
}

/// 安装/更新计划任务(给 config 则先落盘并用其 time;否则用现有配置)。返回新状态。
#[tauri::command]
pub fn gm_tail_install(
    state: tauri::State<AppState>,
    config: Option<GmTailConfig>,
) -> Result<GmTailStatusDto, String> {
    let cfg = config.unwrap_or_else(|| gm_tail::read_config(&state.ws));
    gm_tail::install(&state.ws, &cfg)?;
    Ok(gm_tail::status_dto(&state.ws))
}

#[tauri::command]
pub fn gm_tail_remove(state: tauri::State<AppState>) -> Result<GmTailStatusDto, String> {
    gm_tail::remove()?;
    Ok(gm_tail::status_dto(&state.ws))
}

/// 立刻手动触发一次(任务须已安装)。
#[tauri::command]
pub fn gm_tail_run_now(_state: tauri::State<AppState>) -> Result<(), String> {
    gm_tail::run_now()
}
