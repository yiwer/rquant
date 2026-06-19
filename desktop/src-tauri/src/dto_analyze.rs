use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SectorCumDto { pub t: String, pub r_p: f64, pub r_alloc: f64, pub r_bench: f64 }

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SectorAttribDto { pub excess_total: f64, pub alloc_pct: f64, pub select_pct: f64, pub cum: Vec<SectorCumDto> }

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TwoLegCellDto { pub w: f64, pub net_total: f64, pub excess: f64, pub oos_excess: Option<f64>, pub sharpe: f64, pub max_dd: f64 }

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TwoLegDto { pub rows: Vec<TwoLegCellDto>, pub best_w: f64 }

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct CapacityRowDto { pub adv_pct: f64, pub max_aum: f64 }

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DeployDto { pub lag0_excess: f64, pub lag1_excess: f64, pub drag: f64, pub adv_median: f64, pub capacity: Vec<CapacityRowDto> }
