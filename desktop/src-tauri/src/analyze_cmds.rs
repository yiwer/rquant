use crate::commands::AppState;
use crate::dto_analyze::*;
use std::collections::HashMap;

fn rebals_of(net: &serde_json::Value) -> Vec<(String, Vec<String>)> {
    net.get("holdings").and_then(|h| h.as_array()).map(|a| a.iter().map(|h| {
        let t: String = h.get("t").and_then(|x| x.as_str()).unwrap_or("").chars().take(10).collect();
        let syms = h.get("selected").and_then(|s| s.as_array()).map(|a| a.iter()
            .filter_map(|p| p.as_array().and_then(|pr| pr.first()).and_then(|x| x.as_str()).map(String::from)).collect()).unwrap_or_default();
        (t, syms)
    }).collect()).unwrap_or_default()
}
fn load_kday(ws: &crate::paths::Workspace, sym: &str) -> HashMap<String, (f64, f64)> {
    let mut m = HashMap::new();
    if let Ok(txt) = std::fs::read_to_string(ws.kday_dir().join(format!("{sym}.csv"))) {
        for line in txt.lines().skip(1) { let c: Vec<&str> = line.split(',').collect();
            if c.len() >= 7 { if let (Ok(close), Ok(amt)) = (c[4].parse::<f64>(), c[6].parse::<f64>()) {
                m.insert(c[0].get(..10).unwrap_or(c[0]).to_string(), (close, amt)); } } }
    }
    m
}
fn nav_of(net: &serde_json::Value) -> Vec<(String, f64)> {
    net.get("holdings").and_then(|h| h.as_array()).map(|a| a.iter().filter_map(|h|
        Some((h.get("t")?.as_str()?.chars().take(10).collect(), h.get("nav")?.as_f64()?))).collect()).unwrap_or_default()
}

#[tauri::command]
pub fn analyze_sector(state: tauri::State<AppState>, run_id: String) -> Result<SectorAttribDto, String> {
    let net = crate::screen_runs::read_report(&state.ws, &run_id, "net")?;
    let rebals = rebals_of(&net);
    let syms: std::collections::BTreeSet<String> = rebals.iter().flat_map(|(_, s)| s.clone()).collect();
    let mut px: HashMap<String, HashMap<String, (f64, f64)>> = HashMap::new();
    for s in &syms { px.insert(s.clone(), load_kday(&state.ws, s)); }
    let price = |s: &str, d: &str| px.get(s).and_then(|m| m.get(d)).map(|(c, _)| *c);
    let mut sector_of = HashMap::new();
    if let Ok(txt) = std::fs::read_to_string(state.ws.sector_membership_path()) {
        for line in txt.lines().skip(1) { let c: Vec<&str> = line.split(',').collect();
            if c.len() >= 2 { sector_of.insert(c[0].to_string(), c[1].to_string()); } } }
    let inds: std::collections::BTreeSet<String> = sector_of.values().cloned().collect();
    let mut sec_panel: HashMap<String, HashMap<String, f64>> = HashMap::new();
    for ind in inds {
        let mut m = HashMap::new();
        if let Ok(txt) = std::fs::read_to_string(state.ws.sector_dir().join(format!("{ind}.csv"))) {
            for line in txt.lines().skip(1) { let c: Vec<&str> = line.split(',').collect();
                if c.len() >= 3 { if let Ok(idx) = c[2].parse::<f64>() { m.insert(c[0].get(..10).unwrap_or(c[0]).to_string(), idx); } } } }
        sec_panel.insert(ind, m);
    }
    let sector_lvl = |ind: &str, d: &str| sec_panel.get(ind).and_then(|m| m.get(d)).copied();
    let idx = crate::index_relative::load_index(&state.ws.index_dir().join("csi300.csv"))?;
    let bench = |d: &str| crate::index_relative::idx_at(&idx, d);
    let r = crate::analyze::sector_attribution(&rebals, &price, &sector_of, &sector_lvl, &bench);
    Ok(SectorAttribDto { excess_total: r.excess_total, alloc_pct: r.alloc_pct, select_pct: r.select_pct,
        cum: r.cum.into_iter().map(|(t, rp, ra, rb)| SectorCumDto { t, r_p: rp, r_alloc: ra, r_bench: rb }).collect() })
}

#[tauri::command]
pub fn analyze_twoleg(state: tauri::State<AppState>, value_run_id: String, growth_run_id: String, _w: f64) -> Result<TwoLegDto, String> {
    let vn = crate::screen_runs::read_report(&state.ws, &value_run_id, "net")?;
    let gn = crate::screen_runs::read_report(&state.ws, &growth_run_id, "net")?;
    let regimes: Vec<(String, String, String)> = vn.get("regime_slices").and_then(|s| s.as_array()).map(|a| a.iter().filter_map(|s|
        Some((s.get("label")?.as_str()?.to_string(), s.get("from")?.as_str()?.to_string(), s.get("to")?.as_str()?.to_string()))).collect()).unwrap_or_default();
    let idx = crate::index_relative::load_index(&state.ws.index_dir().join("csi300.csv"))?;
    let r = crate::analyze::two_leg(&nav_of(&vn), &nav_of(&gn), &idx, &regimes);
    if r.rows.is_empty() { return Err("两腿对齐点太少——需同 universe/区间/调仓".into()); }
    Ok(TwoLegDto { rows: r.rows.into_iter().map(|c| TwoLegCellDto { w: c.w, net_total: c.net_total, excess: c.excess, oos_excess: c.oos_excess, sharpe: c.sharpe, max_dd: c.max_dd }).collect(), best_w: r.best_w })
}

#[tauri::command]
pub fn analyze_deploy(state: tauri::State<AppState>, run_id: String) -> Result<DeployDto, String> {
    let net = crate::screen_runs::read_report(&state.ws, &run_id, "net")?;
    let rebals = rebals_of(&net);
    let syms: std::collections::BTreeSet<String> = rebals.iter().flat_map(|(_, s)| s.clone()).collect();
    let mut px: HashMap<String, HashMap<String, (f64, f64)>> = HashMap::new();
    for s in &syms { px.insert(s.clone(), load_kday(&state.ws, s)); }
    let price = |s: &str, d: &str| px.get(s).and_then(|m| m.get(d)).map(|(c, _)| *c);
    let adv = |s: &str, d: &str| -> Option<f64> {
        let m = px.get(s)?; let mut vals: Vec<(String, f64)> = m.iter().filter(|(k, _)| k.as_str() <= d).map(|(k, (_, a))| (k.clone(), *a)).collect();
        if vals.is_empty() { return None; } vals.sort_by(|a, b| a.0.cmp(&b.0));
        let tail = &vals[vals.len().saturating_sub(20)..]; Some(tail.iter().map(|(_, a)| *a).sum::<f64>() / tail.len() as f64) };
    let idx = crate::index_relative::load_index(&state.ws.index_dir().join("csi300.csv"))?;
    let bench = |d: &str| crate::index_relative::idx_at(&idx, d);
    let r = crate::analyze::deploy(&rebals, &price, &adv, &bench, 1.0);
    Ok(DeployDto { lag0_excess: r.lag0_excess, lag1_excess: r.lag1_excess, drag: r.drag, adv_median: r.adv_median,
        capacity: r.capacity.into_iter().map(|(p, a)| CapacityRowDto { adv_pct: p, max_aum: a }).collect() })
}
