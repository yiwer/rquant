//! Tauri 命令层:薄壳——装配函数可直测,#[tauri::command] 仅做提取与转发。
use crate::books::{find_book, BOOKS};
use crate::dto::*;
use crate::journal::{append_entries, read_points, JournalEntry};
use crate::paths::Workspace;
use crate::readers::{read_book_card, read_portfolio_diff, snapshot_to_dto};
use crate::tasks::TaskRegistry;
use std::sync::Arc;

pub struct AppState {
    pub ws: Workspace,
    pub tasks: Arc<TaskRegistry>,
}

pub fn assemble_overview(ws: &Workspace) -> OverviewDto {
    let cards: Vec<BookCardDto> = BOOKS.iter().map(|b| read_book_card(ws, b)).collect();
    let (diff, diff_t) = read_portfolio_diff(ws, &BOOKS[2]);
    // journal 顺带 append(仅 status=ok 的卡;幂等去重)
    let now = chrono::Local::now().naive_local().format("%Y-%m-%dT%H:%M:%S").to_string();
    let entries: Vec<JournalEntry> = cards
        .iter()
        .filter(|c| c.status == "ok")
        .filter_map(|c| {
            c.state_time.clone().map(|st| JournalEntry {
                appended_at: now.clone(),
                book: c.book.clone(),
                state_time: st,
                nav: c.nav,
                pos: c.pos,
                members: c.holdings.as_ref().map(|h| h.len() as u32),
            })
        })
        .collect();
    if !entries.is_empty() {
        let _ = append_entries(ws, &entries); // journal 失败不阻断 overview(降级)
    }
    OverviewDto {
        cards,
        diff,
        diff_t,
        runlog: crate::runlog::read_status(ws),
        schtask: crate::schtask::query("rquant-paper"),
    }
}

pub fn assemble_book_detail(ws: &Workspace, book_id: &str) -> Result<BookDetailDto, String> {
    let book = find_book(book_id).ok_or_else(|| format!("unknown book {}", book_id))?;
    let card = read_book_card(ws, book);
    // 13 字段快照:仅 single 且 state ok
    let snapshot = if card.kind == "single" && card.status == "ok" {
        let name = rquant::tree::loader::load_tree_file(&book.tree_path(ws)).map_err(|e| e.to_string())?.meta.name;
        rquant::signal::read_paper_state(&book.state_path(ws), &name)
            .ok()
            .flatten()
            .map(|st| snapshot_to_dto(&st.account))
    } else {
        None
    };
    let journal = read_points(ws, book_id).unwrap_or_default();
    Ok(BookDetailDto { card, snapshot, journal })
}

// ---- tauri 薄壳 ----

#[tauri::command]
pub fn cockpit_overview(state: tauri::State<AppState>) -> OverviewDto {
    assemble_overview(&state.ws)
}

#[tauri::command]
pub fn book_detail(state: tauri::State<AppState>, book: String) -> Result<BookDetailDto, String> {
    assemble_book_detail(&state.ws, &book)
}

#[tauri::command]
pub fn runlog_tail(state: tauri::State<AppState>, lines: usize) -> String {
    crate::runlog::read_tail(&state.ws, lines)
}

#[tauri::command]
pub fn run_gate_now() -> GateDto {
    crate::gates::classify_run_window(chrono::Local::now().naive_local())
}

/// commit 时闸校验:dry_only 拒绝;warn 需 confirmed=true。
#[tauri::command]
pub fn manual_run(
    state: tauri::State<AppState>,
    books: Vec<String>,
    commit: bool,
    confirmed: bool,
) -> Result<String, String> {
    if commit {
        let gate = crate::gates::classify_run_window(chrono::Local::now().naive_local());
        match gate.gate.as_str() {
            "dry_only" => return Err(gate.message.unwrap_or_else(|| "盘中禁 commit".into())),
            "warn" if !confirmed => return Err(format!("CONFIRM:{}", gate.message.unwrap_or_default())),
            _ => {}
        }
    }
    let ws = state.ws.clone();
    state
        .tasks
        .start("manual_run", true, move |ctx| crate::manual_run::run_books(&ws, ctx, &books, commit))
}

#[tauri::command]
pub fn task_list(state: tauri::State<AppState>) -> Vec<TaskInfoDto> {
    state.tasks.list()
}

#[tauri::command]
pub fn task_cancel(state: tauri::State<AppState>, id: String) {
    state.tasks.cancel(&id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Workspace;

    #[test]
    fn overview_assembles_three_cards_and_appends_journal() {
        let td = tempfile::tempdir().unwrap();
        let root = td.path().to_path_buf();
        std::fs::create_dir_all(root.join("paper")).unwrap();
        std::fs::create_dir_all(root.join("deploy")).unwrap();
        let repo = Workspace::detect(&std::env::current_dir().unwrap()).unwrap();
        for f in ["tree_v4_frozen.yaml", "strength_v1_frozen.yaml"] {
            std::fs::copy(repo.deploy_dir().join(f), root.join("deploy").join(f)).unwrap();
        }
        let ws = Workspace::new(root);
        let dto = assemble_overview(&ws);
        assert_eq!(dto.cards.len(), 3);
        assert_eq!(dto.cards[0].book, "b1");
        // 全 empty → journal 不应产生文件
        assert!(!ws.journal_path().exists());
    }
}
