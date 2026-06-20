//! 流程审计:每次操作的完整轨迹落盘 JSONL(旁路,失败不毁主流程)。
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditStage { pub stage: String, pub detail: String, pub at_ms: f64 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRecord {
    pub id: String,
    pub kind: String,
    pub params: serde_json::Value,
    pub started_at: String,
    pub ended_at: String,
    pub duration_ms: f64,
    pub stages: Vec<AuditStage>,
    pub files: Vec<String>,
    pub status: String,
    pub error: Option<String>,
    pub result_summary: Option<String>,
    pub artifact: Option<String>,
}

/// 追加一行 JSON(自动建父目录)。
pub fn append(path: &Path, rec: &AuditRecord) -> std::io::Result<()> {
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    let line = serde_json::to_string(rec).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    writeln!(f, "{line}")
}

/// 读全部 → 过滤(kind/status)→ 取尾 limit → 新到旧。坏行/缺文件容错。
pub fn read(path: &Path, limit: usize, kind: Option<&str>, status: Option<&str>) -> Vec<AuditRecord> {
    let Ok(txt) = std::fs::read_to_string(path) else { return Vec::new() };
    let mut v: Vec<AuditRecord> = txt.lines()
        .filter_map(|l| serde_json::from_str::<AuditRecord>(l).ok())
        .filter(|r| kind.is_none_or(|k| r.kind == k))
        .filter(|r| status.is_none_or(|s| r.status == s))
        .collect();
    v.reverse();
    v.truncate(limit);
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rec(id: &str, kind: &str, status: &str) -> AuditRecord {
        AuditRecord {
            id: id.into(), kind: kind.into(), params: serde_json::json!({"as_of":"2026-06-16"}),
            started_at: "2026-06-16T10:00:00".into(), ended_at: "2026-06-16T10:00:02".into(),
            duration_ms: 2000.0, stages: vec![AuditStage{stage:"选股".into(),detail:"".into(),at_ms:100.0}],
            files: vec!["data/baostock/universe_baostock_day.csv".into()],
            status: status.into(), error: None, result_summary: Some("top-50".into()), artifact: None,
        }
    }
    #[test]
    fn append_then_read_roundtrip_newest_first_and_filter() {
        let td = tempfile::tempdir().unwrap();
        let p = td.path().join("audit/audit.jsonl");
        append(&p, &rec("t1", "screen_asof", "done")).unwrap();
        append(&p, &rec("t2", "deploy_month", "failed")).unwrap();
        append(&p, &rec("t3", "screen_asof", "done")).unwrap();
        let all = read(&p, 10, None, None);
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].id, "t3"); // 新→旧
        let only_screen = read(&p, 10, Some("screen_asof"), None);
        assert_eq!(only_screen.len(), 2);
        let only_failed = read(&p, 10, None, Some("failed"));
        assert_eq!(only_failed.len(), 1);
        assert_eq!(only_failed[0].id, "t2");
        assert_eq!(read(&p, 1, None, None).len(), 1); // limit
    }
    #[test]
    fn read_missing_file_is_empty() {
        assert!(read(std::path::Path::new("E:/nonexistent/audit.jsonl"), 10, None, None).is_empty());
    }
}
