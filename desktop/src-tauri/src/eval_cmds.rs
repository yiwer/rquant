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
            if let Ok(r) = serde_json::from_str::<rquant::optimize::OptimizeReport>(&txt) {
                let rel = p.strip_prefix(state.ws.root()).unwrap_or(&p).to_string_lossy().replace('\\', "/");
                out.push(OptimizeReportInfoDto {
                    path: rel,
                    name: if r.primary.is_empty() { p.file_stem().and_then(|s| s.to_str()).map(String::from) } else { Some(r.primary.clone()) },
                    mode: Some(r.mode.clone()), n_combos: Some(r.n_combos as u32), folds: Some(r.folds as u32), error: None });
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
            status: serde_json::to_value(&g.status).ok().and_then(|x| x.as_str().map(String::from)).unwrap_or_default(),
            value: g.value, threshold: g.threshold, note: g.note.clone() }).collect(),
        failed_gates: v.failed_gates.clone(),
    })
}
