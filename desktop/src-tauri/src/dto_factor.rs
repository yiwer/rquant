use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DecayPointDto { pub horizon: u32, pub rank_ic: Option<f64> }

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct LayerStatsDto { pub q: u32, pub ann_returns: Vec<Option<f64>>, pub spread_total: f64, pub spread_sharpe: Option<f64>, pub monotonicity: Option<f64> }

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct FactorStatsDto { pub name: String, pub expr: String, pub n_periods: u32, pub ic_mean: Option<f64>, pub icir: Option<f64>, pub ic_t: Option<f64>, pub ic_pos_share: Option<f64>, pub rank_ic_mean: Option<f64>, pub rank_icir: Option<f64>, pub ic_decay: Vec<DecayPointDto>, pub layers: Option<LayerStatsDto> }

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct CorrDto { pub names: Vec<String>, pub values: Vec<Vec<Option<f64>>> }

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct FactorReportDto { pub n_symbols: u32, pub sample: u32, pub horizon: u32, pub layers_q: u32, pub factors: Vec<FactorStatsDto>, pub corr: Option<CorrDto> }
