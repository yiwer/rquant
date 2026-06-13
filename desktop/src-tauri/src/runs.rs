//! 回测留档:每次运行一个 runs/<id>/ 目录。result/traces 由引擎自写,
//! 桥接只写 config.json + meta.json(原子)。id 经正则校验防路径穿越。
use crate::dto::{BacktestConfigDto, RunMetaDto};
use crate::paths::Workspace;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

static SEQ: AtomicU32 = AtomicU32::new(1);

pub fn new_run_id() -> String {
    let now = chrono::Local::now().naive_local();
    let seq = SEQ.fetch_add(1, Ordering::Relaxed) % 100;
    format!(
        "{}-{:04x}-{:02}",
        now.format("%Y%m%d-%H%M%S"),
        std::process::id() % 0x10000,
        seq
    )
}

pub fn is_valid_run_id(id: &str) -> bool {
    let parts: Vec<&str> = id.split('-').collect();
    if parts.len() != 4 {
        return false;
    }
    let (d, t, pid, seq) = (parts[0], parts[1], parts[2], parts[3]);
    d.len() == 8 && d.bytes().all(|c| c.is_ascii_digit())
        && t.len() == 6 && t.bytes().all(|c| c.is_ascii_digit())
        && pid.len() == 4 && pid.bytes().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        && seq.len() == 2 && seq.bytes().all(|c| c.is_ascii_digit())
}

pub struct RunPaths {
    pub dir: PathBuf,
    pub config_json: PathBuf,
    pub meta_json: PathBuf,
    pub result_json: PathBuf,
    pub traces_jsonl: PathBuf,
    pub decision_jsonl: PathBuf,
}

pub fn run_paths(ws: &Workspace, id: &str) -> RunPaths {
    let dir = ws.runs_dir().join(id);
    RunPaths {
        config_json: dir.join("config.json"),
        meta_json: dir.join("meta.json"),
        result_json: dir.join("result.json"),
        traces_jsonl: dir.join("traces.jsonl"),
        decision_jsonl: dir.join("decision_traces.jsonl"),
        dir,
    }
}

// TODO(M-later): 与 journal.rs 的内联原子写合并为 crate 级 util(第三个使用者出现时)。
fn write_json_atomic(path: &Path, json: &str) -> anyhow::Result<()> {
    std::fs::create_dir_all(path.parent().expect("run file has parent"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn write_meta(ws: &Workspace, meta: &RunMetaDto) -> anyhow::Result<()> {
    let rp = run_paths(ws, &meta.id);
    // 同秒重启 + pid 复用可撞 id(概率极低);静默覆盖前留一条诊断
    if rp.meta_json.exists() {
        eprintln!("[rquant-desktop] run id collision detected, overwriting: {}", meta.id);
    }
    write_json_atomic(&rp.meta_json, &serde_json::to_string_pretty(meta)?)
}

pub fn read_meta(ws: &Workspace, id: &str) -> Option<RunMetaDto> {
    let rp = run_paths(ws, id);
    let txt = std::fs::read_to_string(rp.meta_json).ok()?;
    serde_json::from_str(&txt).ok()
}

/// 列出全部留档(按 id 降序=时间降序);meta 损坏的目录跳过。
pub fn list_runs(ws: &Workspace) -> Vec<RunMetaDto> {
    let Ok(rd) = std::fs::read_dir(ws.runs_dir()) else {
        return Vec::new();
    };
    let mut v: Vec<RunMetaDto> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
        .filter(|id| is_valid_run_id(id))
        .filter_map(|id| read_meta(ws, &id))
        .collect();
    v.sort_by(|a, b| b.id.cmp(&a.id));
    v
}

/// 注:id 合法但目录不存在时 remove_dir_all 返回 NotFound 错误(如双击删除竞态)——
/// 如实上抛,UI 端按需吞掉。
pub fn delete_run(ws: &Workspace, id: &str) -> anyhow::Result<()> {
    if !is_valid_run_id(id) {
        anyhow::bail!("invalid run id: {}", id);
    }
    std::fs::remove_dir_all(run_paths(ws, id).dir)?;
    Ok(())
}

pub fn write_config(ws: &Workspace, id: &str, cfg: &BacktestConfigDto) -> anyhow::Result<()> {
    write_json_atomic(&run_paths(ws, id).config_json, &serde_json::to_string_pretty(cfg)?)
}

pub fn read_config(ws: &Workspace, id: &str) -> anyhow::Result<BacktestConfigDto> {
    let txt = std::fs::read_to_string(run_paths(ws, id).config_json)?;
    Ok(serde_json::from_str(&txt)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws() -> (tempfile::TempDir, Workspace) {
        let td = tempfile::tempdir().unwrap();
        let path = td.path().to_path_buf();
        (td, Workspace::new(path))
    }

    fn meta(id: &str) -> RunMetaDto {
        RunMetaDto {
            id: id.into(),
            kind: "sim_hard".into(),
            name: "n".into(),
            tree_name: "t".into(),
            created: "2026-06-12T21:00:00".into(),
            ok: true,
            error: None,
        }
    }

    #[test]
    fn run_id_format_and_uniqueness() {
        let a = new_run_id();
        let b = new_run_id();
        assert!(is_valid_run_id(&a), "{}", a);
        assert_ne!(a, b, "seq must disambiguate same-second ids");
    }

    #[test]
    fn id_validation_rejects_traversal() {
        assert!(!is_valid_run_id("../../etc"));
        assert!(!is_valid_run_id("20260612-210000-abcd-01/.."));
        assert!(!is_valid_run_id(""));
        assert!(is_valid_run_id("20260612-210000-0a1b-07"));
        assert!(!is_valid_run_id("20260612-210000-ABCD-01")); // 全大写 hex 拒绝
        assert!(!is_valid_run_id("20260612-210000-0A1B-07")); // 混合大小写拒绝
    }

    #[test]
    fn meta_roundtrip_and_listing_desc() {
        let (_td, w) = ws();
        let m1 = meta("20260612-210000-0a1b-01");
        let m2 = meta("20260612-210001-0a1b-02");
        write_meta(&w, &m1).unwrap();
        write_meta(&w, &m2).unwrap();
        let all = list_runs(&w);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].id, m2.id, "desc by id");
    }

    #[test]
    fn delete_refuses_bad_id_and_removes_good() {
        let (_td, w) = ws();
        let m = meta("20260612-210000-0a1b-03");
        write_meta(&w, &m).unwrap();
        assert!(delete_run(&w, "../x").is_err());
        delete_run(&w, &m.id).unwrap();
        assert!(list_runs(&w).is_empty());
    }
}
