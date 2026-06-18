use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 镜像 .iter/ledger.jsonl 的一行;数值键缺省为 None(老轮次可能缺)。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LedgerRoundDto {
    pub round: i64, pub label: String,
    #[serde(default)] pub axis: String,
    #[serde(default)] pub note: String,
    #[serde(default)] pub benchmark: String,
    #[serde(default = "one")] pub rebalance: i64,
    pub verdict: String,
    #[serde(default)] pub flags: Vec<String>,
    #[serde(default)] pub gross_ex: Option<f64>,
    #[serde(default)] pub net_ex: Option<f64>,
    #[serde(default)] pub net_oos_ex: Option<f64>,
    #[serde(default)] pub net_train_ex: Option<f64>,
    #[serde(default)] pub net_sharpe: Option<f64>,
    #[serde(default)] pub break_even: Option<f64>,
}
fn one() -> i64 { 1 }

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct GateDto { pub name: String, pub pass: bool, pub value: Option<f64>, pub threshold: Option<f64>, pub note: String }

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Tier2CellDto { pub top: i64, pub rebalance: i64, pub net_excess: f64 }

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RoundCardDto {
    pub round: i64, pub label: String, pub benchmark: String, pub rebalance: i64,
    pub verdict: String, pub gates: Vec<GateDto>, pub tier2: Vec<Tier2CellDto>,
    pub flags: Vec<String>, pub config_path: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct IterQueueDto { pub queue: Vec<String>, pub falsified: Vec<String> }
