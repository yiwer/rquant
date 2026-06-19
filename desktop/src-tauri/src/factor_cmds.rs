use crate::commands::AppState;
use crate::dto_factor::*;

#[tauri::command]
pub fn factor_run(state: tauri::State<AppState>, factors: Vec<(String, String)>, horizon: u32, layers: u32, sample: u32) -> Result<String, String> {
    let ws = state.ws.clone();
    if factors.is_empty() { return Err("请至少添加一个因子表达式".into()); }
    state.tasks.start("factor", true, move |ctx| {
        ctx.progress(0.2, "因子", "");
        let tmp = ws.root().join(".rquant-desktop").join("factor_report.json");
        std::fs::create_dir_all(tmp.parent().unwrap()).map_err(|e| e.to_string())?;
        let cfg = rquant::factor::FactorConfig {
            universe_path: ws.root().join("data/baostock/universe_baostock_day.csv"),
            factors: factors.into_iter().map(|(name, expr)| rquant::factor::FactorSpecItem { name, expr }).collect(),
            sample: sample as usize, horizon: horizon as usize, layers: layers as usize,
            warmup: 260, window: 260, out_path: tmp, html_path: None, membership_path: None,
        };
        let rep = rquant::factor::run_factor(&cfg).map_err(|e| e.to_string())?;
        let dto = FactorReportDto {
            n_symbols: rep.n_symbols as u32, sample: rep.sample as u32, horizon: rep.horizon as u32, layers_q: rep.layers_q as u32,
            factors: rep.factors.iter().map(|f| FactorStatsDto {
                name: f.name.clone(), expr: f.expr.clone(), n_periods: f.n_periods as u32,
                ic_mean: f.ic_mean, icir: f.icir, ic_t: f.ic_t, ic_pos_share: f.ic_pos_share,
                rank_ic_mean: f.rank_ic_mean, rank_icir: f.rank_icir,
                ic_decay: f.ic_decay.iter().map(|(h, v)| DecayPointDto { horizon: *h as u32, rank_ic: *v }).collect(),
                layers: f.layers.as_ref().map(|l| LayerStatsDto { q: l.q as u32, ann_returns: l.ann_returns.clone(), spread_total: l.spread_total, spread_sharpe: l.spread_sharpe, monotonicity: l.monotonicity }),
            }).collect(),
            corr: rep.corr.as_ref().map(|c| CorrDto { names: c.names.clone(), values: c.values.clone() }),
        };
        serde_json::to_value(dto).map_err(|e| e.to_string())
    })
}
