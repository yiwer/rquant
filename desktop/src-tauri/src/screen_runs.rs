use crate::dto_screen::ScreenRunMetaDto;
use crate::paths::Workspace;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
static SEQ: AtomicU64 = AtomicU64::new(0);

pub fn new_id() -> String {
    let now = chrono::Local::now().naive_local();
    let seq = SEQ.fetch_add(1, Ordering::Relaxed) % 100;
    format!("scr-{}-{:02}", now.format("%Y%m%d-%H%M%S"), seq)
}
pub fn run_dir(ws: &Workspace, id: &str) -> PathBuf { ws.screen_runs_dir().join(id) }

fn write_atomic(path: &Path, s: &str) -> Result<(), String> {
    std::fs::create_dir_all(path.parent().expect("screen run file has parent")).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, s).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}
pub fn write_meta(ws: &Workspace, m: &ScreenRunMetaDto) -> Result<(), String> {
    write_atomic(&run_dir(ws, &m.id).join("meta.json"), &serde_json::to_string_pretty(m).map_err(|e| e.to_string())?)
}
pub fn write_report(ws: &Workspace, id: &str, kind: &str, json: &str) -> Result<(), String> {
    write_atomic(&run_dir(ws, id).join(format!("{kind}.json")), json)
}
pub fn read_meta(ws: &Workspace, id: &str) -> Result<ScreenRunMetaDto, String> {
    let s = std::fs::read_to_string(run_dir(ws, id).join("meta.json")).map_err(|e| e.to_string())?;
    serde_json::from_str(&s).map_err(|e| e.to_string())
}
pub fn read_report(ws: &Workspace, id: &str, kind: &str) -> Result<serde_json::Value, String> {
    let s = std::fs::read_to_string(run_dir(ws, id).join(format!("{kind}.json"))).map_err(|e| e.to_string())?;
    serde_json::from_str(&s).map_err(|e| e.to_string())
}
pub fn list_meta(ws: &Workspace) -> Vec<ScreenRunMetaDto> {
    let dir = ws.screen_runs_dir();
    let Ok(rd) = std::fs::read_dir(&dir) else { return vec![] };
    let mut out: Vec<ScreenRunMetaDto> = rd.filter_map(|e| e.ok())
        .filter_map(|e| read_meta(ws, e.file_name().to_str()?).ok()).collect();
    out.sort_by(|a, b| b.created.cmp(&a.created));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn new_id_has_scr_prefix() {
        assert!(new_id().starts_with("scr-"));
    }
}
