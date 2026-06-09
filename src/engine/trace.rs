use crate::tree::schema::Stance;
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepRecord {
    pub node_id: String,
    pub label: String,
    pub confidence: f64,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trace {
    pub t: NaiveDateTime,
    pub path: Vec<StepRecord>,
    pub leaf: String,
    pub stance: Stance,
}
