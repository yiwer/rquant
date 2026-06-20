use serde::Serialize; use ts_rs::TS;
#[derive(Debug,Clone,Serialize,TS)] #[ts(export)]
pub struct DeployHoldingDto { pub symbol: String, pub weight: f64, pub since: String }
#[derive(Debug,Clone,Serialize,TS)] #[ts(export)]
pub struct DeployNavPointDto { pub t: String, pub nav: f64, pub bench_nav: f64 }
#[derive(Debug,Clone,Serialize,TS)] #[ts(export)]
pub struct DeployMonthRecDto { pub as_of: String, pub nav: f64, pub excess: f64, pub n_holdings: u32, pub n_buy: u32, pub n_sell: u32 }
#[derive(Debug,Clone,Serialize,TS)] #[ts(export)]
pub struct DeployBookDto { pub status: String, pub nav: Option<f64>, pub excess_total: Option<f64>, pub last_rebalance: Option<String>, pub holdings: Vec<DeployHoldingDto>, pub nav_history: Vec<DeployNavPointDto>, pub months: Vec<DeployMonthRecDto> }
#[derive(Debug,Clone,Serialize,TS)] #[ts(export)]
pub struct DeployMonthDto { pub as_of: String, pub picks: Vec<DeployHoldingDto>, pub diff: Vec<crate::dto::DiffRowDto>, pub proj_nav: f64, pub proj_excess: f64, pub realized_ret: f64 }
