use serde::Serialize; use ts_rs::TS;
#[derive(Debug,Clone,Serialize,TS)] #[ts(export)]
pub struct AuditStageDto { pub stage: String, pub detail: String, pub at_ms: f64 }
#[derive(Debug,Clone,Serialize,TS)] #[ts(export)]
pub struct AuditRecordDto {
    pub id: String, pub kind: String,
    /// Run parameters (arbitrary JSON shape per kind).
    #[ts(type = "unknown")]
    pub params: serde_json::Value,
    pub started_at: String, pub ended_at: String, pub duration_ms: f64,
    pub stages: Vec<AuditStageDto>, pub files: Vec<String>, pub status: String,
    pub error: Option<String>, pub result_summary: Option<String>, pub artifact: Option<String>,
}
impl From<crate::audit::AuditRecord> for AuditRecordDto {
    fn from(a: crate::audit::AuditRecord) -> Self {
        AuditRecordDto {
            id: a.id, kind: a.kind, params: a.params, started_at: a.started_at, ended_at: a.ended_at,
            duration_ms: a.duration_ms,
            stages: a.stages.into_iter().map(|s| AuditStageDto { stage: s.stage, detail: s.detail, at_ms: s.at_ms }).collect(),
            files: a.files, status: a.status, error: a.error, result_summary: a.result_summary, artifact: a.artifact,
        }
    }
}
