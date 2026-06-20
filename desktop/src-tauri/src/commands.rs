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
    let mut card = read_book_card(ws, book);
    // 13-field snapshot: single + state ok only.
    // If read_paper_state returns Err (corrupt state file), surface it on the card
    // rather than silently returning None — mirrors read_book_card's corrupt handling.
    let snapshot = if card.kind == "single" && card.status == "ok" {
        let name = rquant::tree::loader::load_tree_file(&book.tree_path(ws)).map_err(|e| e.to_string())?.meta.name;
        match rquant::signal::read_paper_state(&book.state_path(ws), &name) {
            Ok(Some(st)) => Some(snapshot_to_dto(&st.account)),
            Ok(None) => None,
            Err(e) => {
                let e_str = e.to_string();
                log::error!("assemble_book_detail: corrupt state for {book_id}: {e_str}");
                card.status = "corrupt".into();
                card.advice = Some(
                    crate::error::ErrorDto::from_anyhow(&anyhow::anyhow!(e_str))
                        .advice
                        .unwrap_or_else(|| {
                            "state 异常:查看消息并考虑删除重建(重放幂等)".into()
                        }),
                );
                None
            }
        }
    } else {
        None
    };
    let journal = read_points(ws, book_id).unwrap_or_default();
    Ok(BookDetailDto { card, snapshot, journal })
}

// ---- tauri 薄壳 ----

#[tauri::command]
pub fn cockpit_overview(state: tauri::State<AppState>) -> OverviewDto {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| assemble_overview(&state.ws)))
        .unwrap_or_else(|_| {
            log::error!("cockpit_overview panicked");
            OverviewDto {
                cards: Vec::new(),
                diff: Vec::new(),
                diff_t: None,
                runlog: crate::dto::RunlogStatusDto {
                    last_header: None,
                    ok: None,
                    summary: "cockpit assembly panicked — check logs".into(),
                },
                schtask: None,
            }
        })
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
                // examples/ 下解析失败=坏树,呈现;deploy/ 混有选股配置等非树 yaml
                // (如 value_pb_deploy_frozen.yaml,缺 meta)——解析失败静默跳过,不当坏树列出。
                Err(e) if !frozen => v.push(crate::dto::TreeInfoDto {
                    path: rel,
                    name: None,
                    frozen,
                    error: Some(e.to_string()),
                }),
                Err(_) => {}
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
            let tree_abs = ws.root().join(&config.tree_path);
            let primary_abs = ws.root().join(&config.primary_path);
            ctx.note_params(serde_json::json!({
                "tree_path": &config.tree_path,
                "primary_path": &config.primary_path,
                "mode": &config.mode,
                "window": config.window,
                "cost_bps": config.cost_bps,
            }));
            ctx.note_file(&tree_abs.to_string_lossy().into_owned());
            ctx.note_file(&primary_abs.to_string_lossy().into_owned());
            log::info!("backtest_run: tree={} primary={} mode={} window={} cost_bps={}",
                config.tree_path, config.primary_path, config.mode, config.window, config.cost_bps);
            let result = crate::backtest_run::execute_backtest(&ws, ctx, &config)?;
            if let Some(id) = result.get("run_id").and_then(|v| v.as_str()) {
                let run_dir = ws.runs_dir().join(id);
                ctx.note_file(&run_dir.to_string_lossy().into_owned());
                ctx.note_summary(&format!("run {id}"));
            }
            Ok(result)
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
            ctx.note_params(serde_json::json!({
                "symbols_n": symbols.len(),
                "scale": scale,
                "datalen": datalen,
                "adjust": &adjust,
            }));
            log::info!("fetch_batch: symbols_n={} scale={scale} datalen={datalen} adjust={adjust}", symbols.len());
            let result = crate::data_bench::fetch_batch(&ws, ctx, &symbols, scale, datalen, &adjust)?;
            let written_n = result.get("written").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
            ctx.note_summary(&format!("written {written_n}"));
            Ok(result)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Workspace;

    #[test]
    fn book_detail_corrupt_state_surfaces_corrupt_card() {
        // I2 regression: an empty/corrupt state file must set card.status="corrupt"
        // rather than silently returning snapshot=None with status="ok".
        let (_td, ws) = {
            let td = tempfile::tempdir().unwrap();
            let root = td.path().to_path_buf();
            std::fs::create_dir_all(root.join("paper")).unwrap();
            std::fs::create_dir_all(root.join("deploy")).unwrap();
            let repo = Workspace::detect(&std::env::current_dir().unwrap()).unwrap();
            for f in ["tree_v4_frozen.yaml", "strength_v1_frozen.yaml"] {
                std::fs::copy(repo.deploy_dir().join(f), root.join("deploy").join(f)).unwrap();
            }
            let ws = Workspace::new(root);
            (td, ws)
        };
        // Write a corrupt (empty) state file for b1 (single book).
        let book = &crate::books::BOOKS[0];
        std::fs::write(book.state_path(&ws), b"").unwrap();
        // assemble_book_detail must surface corrupt rather than blank.
        let dto = assemble_book_detail(&ws, "b1").unwrap();
        assert_eq!(dto.card.status, "corrupt", "I2: corrupt state must be surfaced on card");
        assert!(dto.card.advice.is_some(), "I2: advice must be populated for corrupt card");
        assert!(dto.snapshot.is_none(), "I2: snapshot must be None for corrupt state");
    }

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
    fn tree_list_shows_examples_failures_skips_deploy_nontrees() {
        let td = tempfile::tempdir().unwrap();
        let root = td.path().to_path_buf();
        std::fs::create_dir_all(root.join("examples")).unwrap();
        std::fs::create_dir_all(root.join("deploy")).unwrap();
        std::fs::write(
            root.join("examples/ok.yaml"),
            crate::backtest_run::test_fixtures::MINI_TREE,
        )
        .unwrap();
        // examples/ 下坏树 → 呈现为错误
        std::fs::write(root.join("examples/bad.yaml"), "not: a tree").unwrap();
        // deploy/ 下非树 yaml(选股配置形态,缺 meta)→ 静默跳过,不当坏树列出
        std::fs::write(root.join("deploy/value_pb_deploy_frozen.yaml"), "quality_trees: [x.yaml]").unwrap();
        let ws = Workspace::new(root);
        let list = assemble_tree_list(&ws);
        let ok = list.iter().find(|t| t.path.ends_with("ok.yaml")).unwrap();
        assert_eq!(ok.name.as_deref(), Some("m2-mini"));
        assert!(!ok.frozen);
        let bad = list.iter().find(|t| t.path.ends_with("examples/bad.yaml")).unwrap();
        assert!(bad.name.is_none() && bad.error.is_some() && !bad.frozen);
        // deploy/ 非树静默跳过:不出现在列表
        assert!(list.iter().all(|t| !t.path.ends_with("value_pb_deploy_frozen.yaml")));
        assert_eq!(list.len(), 2);
    }
}
