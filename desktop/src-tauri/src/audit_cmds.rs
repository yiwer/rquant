use crate::commands::AppState;
use crate::dto_audit::AuditRecordDto;

#[tauri::command]
pub fn audit_list(
    state: tauri::State<AppState>,
    limit: u32,
    kind: Option<String>,
    status: Option<String>,
) -> Vec<AuditRecordDto> {
    crate::audit::read(&state.ws.audit_path(), limit as usize, kind.as_deref(), status.as_deref())
        .into_iter().map(AuditRecordDto::from).collect()
}

#[tauri::command]
pub fn audit_log_tail(state: tauri::State<AppState>, lines: u32) -> String {
    let dir = state.ws.log_dir();
    let latest = std::fs::read_dir(&dir).ok().into_iter().flatten().flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "log").unwrap_or(false))
        .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
    match latest.and_then(|p| std::fs::read_to_string(p).ok()) {
        Some(txt) => {
            let v: Vec<&str> = txt.lines().collect();
            v[v.len().saturating_sub(lines as usize)..].join("\n")
        }
        None => "(暂无日志文件)".into(),
    }
}
