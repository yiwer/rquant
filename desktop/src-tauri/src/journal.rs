//! 纸面盘净值 journal——桌面端自建历史(spec §5.1:state 只有最新快照)。
//! jsonl 一行一条;读全量→去重→temp+rename 整体重写(文件量级:年数百行,无性能问题)。
use crate::dto::JournalPointDto;
use crate::paths::Workspace;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    pub appended_at: String,
    /// "b1" | "b2" | "b3"
    pub book: String,
    /// 去重键的一半:已 commit state 的 last_time(ISO)。
    pub state_time: String,
    pub nav: Option<f64>,
    pub pos: Option<f64>,
    /// 账本3:持仓成员数。
    pub members: Option<u32>,
}

fn read_all(ws: &Workspace) -> Vec<JournalEntry> {
    let Ok(txt) = std::fs::read_to_string(ws.journal_path()) else {
        return Vec::new();
    };
    txt.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

/// 追加条目(按 (book, state_time) 去重,保持原有顺序,新条目排尾)。
pub fn append_entries(ws: &Workspace, new: &[JournalEntry]) -> anyhow::Result<()> {
    let mut all = read_all(ws);
    let mut seen: BTreeSet<(String, String)> =
        all.iter().map(|e| (e.book.clone(), e.state_time.clone())).collect();
    let mut changed = false;
    for e in new {
        if seen.insert((e.book.clone(), e.state_time.clone())) {
            all.push(e.clone());
            changed = true;
        }
    }
    if !changed {
        return Ok(());
    }
    let path = ws.journal_path();
    std::fs::create_dir_all(path.parent().expect("journal path has parent"))?;
    let mut buf = String::new();
    for e in &all {
        buf.push_str(&serde_json::to_string(e)?);
        buf.push('\n');
    }
    let tmp = path.with_extension("jsonl.tmp");
    std::fs::write(&tmp, &buf)?;
    std::fs::rename(&tmp, &path)?; // 原子替换(spec §7)
    Ok(())
}

/// 某账本的净值序列(按 state_time 升序)。
pub fn read_points(ws: &Workspace, book: &str) -> anyhow::Result<Vec<JournalPointDto>> {
    let mut pts: Vec<JournalPointDto> = read_all(ws)
        .into_iter()
        .filter(|e| e.book == book)
        .map(|e| JournalPointDto { state_time: e.state_time, nav: e.nav, pos: e.pos, members: e.members })
        .collect();
    pts.sort_by(|a, b| a.state_time.cmp(&b.state_time));
    Ok(pts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Workspace;

    fn ws() -> (tempfile::TempDir, Workspace) {
        let td = tempfile::tempdir().unwrap();
        let path = td.path().to_path_buf();
        (td, Workspace::new(path))
    }

    fn entry(book: &str, t: &str, nav: f64) -> JournalEntry {
        JournalEntry {
            appended_at: "2026-06-12T16:00:00".into(),
            book: book.into(),
            state_time: t.into(),
            nav: Some(nav),
            pos: Some(1.0),
            members: None,
        }
    }

    #[test]
    fn append_dedups_by_book_and_state_time() {
        let (_td, w) = ws();
        append_entries(&w, &[entry("b1", "2026-06-11T15:00:00", 1.01)]).unwrap();
        append_entries(&w, &[entry("b1", "2026-06-11T15:00:00", 1.01)]).unwrap(); // 重复
        append_entries(&w, &[entry("b1", "2026-06-12T15:00:00", 1.02)]).unwrap();
        let pts = read_points(&w, "b1").unwrap();
        assert_eq!(pts.len(), 2);
        assert_eq!(pts[0].state_time, "2026-06-11T15:00:00");
        assert_eq!(pts[1].state_time, "2026-06-12T15:00:00");
    }

    #[test]
    fn books_are_isolated() {
        let (_td, w) = ws();
        append_entries(&w, &[entry("b1", "2026-06-11T15:00:00", 1.01)]).unwrap();
        append_entries(&w, &[entry("b2", "2026-06-11T15:00:00", 0.99)]).unwrap();
        assert_eq!(read_points(&w, "b1").unwrap().len(), 1);
        assert_eq!(read_points(&w, "b2").unwrap().len(), 1);
    }

    #[test]
    fn atomic_rewrite_keeps_file_valid_jsonl() {
        let (_td, w) = ws();
        append_entries(&w, &[entry("b1", "2026-06-11T15:00:00", 1.01)]).unwrap();
        let txt = std::fs::read_to_string(w.journal_path()).unwrap();
        for line in txt.lines() {
            serde_json::from_str::<JournalEntry>(line).expect("every line valid json");
        }
    }
}
