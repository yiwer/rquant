use serde::Serialize;
use std::collections::HashMap;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, TS)] #[ts(export)]
pub struct PaperRowDto {
    pub date: String, pub status: String, pub picks: Vec<String>,
    pub turnover: Option<f64>, pub gross_ret: Option<f64>, pub net_ret: Option<f64>, pub nav: f64,
}
#[derive(Debug, Clone, Serialize, TS)] #[ts(export)]
pub struct FactorKVDto { pub key: String, pub value: Option<f64> }
#[derive(Debug, Clone, Serialize, TS)] #[ts(export)]
pub struct PaperStockDetailDto {
    pub symbol: String, pub name: String, pub kday_path: String,
    pub asof: String, pub factors: Vec<FactorKVDto>,
}
#[derive(Debug, Clone, Serialize, TS)] #[ts(export)]
pub struct BlendFoldDto {
    pub oos: String, pub corr: f64,
    pub sh_ridge: f64, pub sh_val: f64, pub sh_blend: f64,
    pub dd_ridge: f64, pub dd_val: f64, pub dd_blend: f64,
    pub ex_ridge: f64, pub ex_val: f64, pub ex_blend: f64,
}
#[derive(Debug, Clone, Serialize, TS)] #[ts(export)]
pub struct BlendDto { pub folds: Vec<BlendFoldDto>, pub mean: BlendFoldMeanDto }
#[derive(Debug, Clone, Serialize, TS)] #[ts(export)]
pub struct BlendFoldMeanDto {
    pub corr: f64, pub sh_ridge: f64, pub sh_val: f64, pub sh_blend: f64,
    pub dd_ridge: f64, pub dd_val: f64, pub dd_blend: f64,
    pub ex_ridge: f64, pub ex_val: f64, pub ex_blend: f64,
}
#[derive(Debug, Clone, Serialize, TS)] #[ts(export)]
pub struct PaperStatusDto {
    pub initialized: bool, pub strategy: String,
    pub train_lo: String, pub train_hi: String, pub n_train_dates: i64,
    pub delta: f64, pub top_n: i64, pub cost_bps: f64,
    pub open_picks: Vec<String>, pub closed: Vec<PaperRowDto>,
    pub cum_net: f64, pub cum_excess: Option<f64>, pub blend: Option<BlendDto>,
    pub names: HashMap<String, String>,
}
