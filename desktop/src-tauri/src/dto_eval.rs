use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct OptimizeReportInfoDto { pub path: String, pub name: Option<String>, pub mode: Option<String>, pub n_combos: Option<u32>, pub folds: Option<u32>, pub error: Option<String> }

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct GateOutcomeDto { pub gate: String, pub status: String, pub value: f64, pub threshold: f64, pub note: String }

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct VerdictDto { pub strategy: String, pub n_symbols: u32, pub certified: bool, pub gates: Vec<GateOutcomeDto>, pub failed_gates: Vec<String> }
