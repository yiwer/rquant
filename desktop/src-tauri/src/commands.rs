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

// ───────────────────────── M2: 回测中心 / 数据工作台 ─────────────────────────

// TODO(M3 cache): 每次挂载对 examples/+deploy/ 逐文件 load_tree_file(<10 文件,~ms 级可接受);
// 与 readers.rs 的 TODO(M2) 树缓存属同一专项。
pub fn assemble_tree_list(ws: &Workspace) -> Vec<crate::dto::TreeInfoDto> {
    let mut v = Vec::new();
    for (dir, frozen) in [(ws.root().join("examples"), false), (ws.deploy_dir(), true)] {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.filter_map(|e| e.ok()) {
            let p = e.path();
            let is_yaml = p.extension().map(|x| x == "yaml" || x == "yml").unwrap_or(false);
            if !is_yaml {
                continue;
            }
            let rel = p
                .strip_prefix(ws.root())
                .map(|x| x.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| p.to_string_lossy().to_string());
            match rquant::tree::loader::load_tree_file(&p) {
                Ok(t) => v.push(crate::dto::TreeInfoDto {
                    path: rel,
                    name: Some(t.meta.name),
                    frozen,
                    error: None,
                }),
                Err(e) => v.push(crate::dto::TreeInfoDto {
                    path: rel,
                    name: None,
                    frozen,
                    error: Some(e.to_string()),
                }),
            }
        }
    }
    v.sort_by(|a, b| a.path.cmp(&b.path));
    v
}

// ---- M2 tauri 薄壳 ----

#[tauri::command]
pub fn tree_list(state: tauri::State<AppState>) -> Vec<crate::dto::TreeInfoDto> {
    assemble_tree_list(&state.ws)
}

#[tauri::command]
pub fn backtest_run(
    state: tauri::State<AppState>,
    config: crate::dto::BacktestConfigDto,
) -> Result<String, String> {
    // config 由 IPC 反序列化而来已是 owned,直接 move;ws 从 state 克隆(state 非 'static)。
    let ws = state.ws.clone();
    state
        .tasks
        .start("backtest", true, move |ctx| {
            crate::backtest_run::execute_backtest(&ws, ctx, &config)
        })
}

#[tauri::command]
pub fn runs_list(state: tauri::State<AppState>) -> Vec<crate::dto::RunMetaDto> {
    crate::runs::list_runs(&state.ws)
}

#[tauri::command]
pub fn run_delete(state: tauri::State<AppState>, id: String) -> Result<(), String> {
    crate::runs::delete_run(&state.ws, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn run_summary(state: tauri::State<AppState>, id: String) -> Result<crate::dto::RunSummaryDto, String> {
    crate::results::run_summary(&state.ws, &id)
}

#[tauri::command]
pub fn run_equity(state: tauri::State<AppState>, id: String) -> Result<Vec<crate::dto::EquityPointDto>, String> {
    crate::results::equity_series(&state.ws, &id)
}

#[tauri::command]
pub fn run_trades(state: tauri::State<AppState>, id: String) -> Result<Vec<crate::dto::TradeDto>, String> {
    crate::results::trades(&state.ws, &id)
}

#[tauri::command]
pub fn run_replay_frames(state: tauri::State<AppState>, id: String) -> Result<Vec<crate::dto::ReplayFrameDto>, String> {
    crate::replay::replay_frames(&state.ws, &id)
}

#[tauri::command]
pub fn run_replay_factors(
    state: tauri::State<AppState>,
    id: String,
    t: String,
) -> Result<Vec<crate::dto::FactorValueDto>, String> {
    crate::replay::replay_factors(&state.ws, &id, &t)
}

#[tauri::command]
pub fn data_csv_list(state: tauri::State<AppState>) -> Vec<crate::dto::CsvInfoDto> {
    crate::data_bench::csv_list(&state.ws)
}

#[tauri::command]
pub fn data_read_bars(state: tauri::State<AppState>, path: String, tail: u32) -> Result<Vec<crate::dto::BarDto>, String> {
    crate::data_bench::read_bars(&state.ws, &path, tail as usize)
}

#[tauri::command]
pub fn data_eval_factor(
    state: tauri::State<AppState>,
    path: String,
    expr: String,
    window: u32,
    tail: u32,
) -> Result<Vec<crate::dto::FactorPointDto>, String> {
    crate::data_bench::eval_factor(&state.ws, &path, &expr, window as usize, tail as usize)
}

#[tauri::command]
pub fn universe_list(state: tauri::State<AppState>) -> Vec<crate::dto::UniverseInfoDto> {
    crate::data_bench::universe_list(&state.ws)
}

#[tauri::command]
pub fn universe_write(
    state: tauri::State<AppState>,
    name: String,
    entries: Vec<crate::dto::UniverseEntryDto>,
) -> Result<(), String> {
    crate::data_bench::universe_write(&state.ws, &name, &entries)
}

#[tauri::command]
pub fn fetch_batch(
    state: tauri::State<AppState>,
    symbols: Vec<String>,
    scale: u32,
    datalen: u32,
    adjust: String,
) -> Result<String, String> {
    let ws = state.ws.clone();
    state
        .tasks
        .start("fetch_batch", true, move |ctx| {
            crate::data_bench::fetch_batch(&ws, ctx, &symbols, scale, datalen, &adjust)
        })
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

    #[test]
    fn tree_list_scans_examples_and_deploy() {
        let td = tempfile::tempdir().unwrap();
        let root = td.path().to_path_buf();
        std::fs::create_dir_all(root.join("examples")).unwrap();
        std::fs::create_dir_all(root.join("deploy")).unwrap();
        std::fs::write(
            root.join("examples/ok.yaml"),
            crate::backtest_run::test_fixtures::MINI_TREE,
        )
        .unwrap();
        std::fs::write(root.join("deploy/bad.yaml"), "not: a tree").unwrap();
        let ws = Workspace::new(root);
        let list = assemble_tree_list(&ws);
        assert_eq!(list.len(), 2);
        let ok = list.iter().find(|t| t.path.ends_with("ok.yaml")).unwrap();
        assert_eq!(ok.name.as_deref(), Some("m2-mini"));
        assert!(!ok.frozen);
        let bad = list.iter().find(|t| t.path.ends_with("bad.yaml")).unwrap();
        assert!(bad.name.is_none() && bad.error.is_some() && bad.frozen);
    }
}
