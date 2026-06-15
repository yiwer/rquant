//! 手动触发当日 run——参数严格镜像 deploy/paper_run.cmd(事实源,books.rs 同注)。
//! 在任务线程内自建 tokio runtime 跑引擎 async 函数。
use crate::books::{Book, BookKind, BOOKS};
use crate::paths::Workspace;
use crate::tasks::TaskCtx;
use rquant::signal::{SignalPortfolioConfig, SignalSingleConfig};

pub fn single_cfg(ws: &Workspace, book: &Book) -> SignalSingleConfig {
    let primary = book.primary_csv(ws);
    SignalSingleConfig {
        tree_path: book.tree_path(ws),
        primary_path: primary.clone(),
        context_path: primary,
        news_path: None,
        aux_paths: Vec::new(),
        window: 100,
        warmup: 80,
        cost_bps: 10.0,
        soft: false,
        state_path: book.state_path(ws),
    }
}

pub fn portfolio_cfg(ws: &Workspace) -> SignalPortfolioConfig {
    let b3 = &BOOKS[2];
    SignalPortfolioConfig {
        tree_path: b3.tree_path(ws),
        universe_path: ws.deploy_dir().join("universe_10.csv"),
        top: 3,
        window: 100,
        warmup: 80,
        cost_bps: 10.0,
        soft: true,
        aux_paths: Vec::new(),
        state_path: b3.state_path(ws),
    }
}

pub fn universe_symbols(ws: &Workspace) -> anyhow::Result<Vec<String>> {
    let txt = std::fs::read_to_string(ws.deploy_dir().join("universe_10.csv"))?;
    Ok(txt
        .lines()
        .skip(1)
        .filter_map(|l| l.split(',').next())
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string())
        .collect())
}

/// 任务体:books 子集 + commit 旗标。返回 run 摘要 JSON。
pub fn run_books(ws: &Workspace, ctx: &TaskCtx, book_ids: &[String], commit: bool) -> Result<serde_json::Value, String> {
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let llm = rquant::cli::build_llm(String::new(), String::new(), ws.root().join(".rquant-cache").join("llm"))
        .map_err(|e| e.to_string())?;
    let mut summary = Vec::new();
    let total = book_ids.len() as f32;

    for (i, id) in book_ids.iter().enumerate() {
        if ctx.cancelled() {
            return Err("cancelled by user".into());
        }
        let base = i as f32 / total;
        let book = crate::books::find_book(id).ok_or_else(|| format!("unknown book {}", id))?;
        match book.kind {
            BookKind::Single => {
                ctx.progress(base + 0.1 / total, "fetch", book.symbol);
                rt.block_on(rquant::cli::run_fetch_to_csv(
                    book.symbol, book.scale, 1023, rquant::cli::SINA_BASE_URL, "qfq", &book.primary_csv(ws), None,
                ))
                .map_err(|e| e.to_string())?;
                ctx.progress(base + 0.5 / total, "replay", book.symbol);
                let cfg = single_cfg(ws, book);
                let (sig, new_state) =
                    rt.block_on(rquant::signal::run_signal_single(&cfg, &llm)).map_err(|e| e.to_string())?;
                std::fs::write(book.sig_path(ws), serde_json::to_string_pretty(&sig).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
                if commit {
                    rquant::signal::write_paper_state(&cfg.state_path, &new_state).map_err(|e| e.to_string())?;
                }
                summary.push(serde_json::json!({
                    "book": book.id, "t": sig.t.to_string(), "target": sig.target,
                    "bars_replayed": sig.paper.bars_replayed, "committed": commit
                }));
            }
            BookKind::Portfolio => {
                let syms = universe_symbols(ws).map_err(|e| e.to_string())?;
                for (j, s) in syms.iter().enumerate() {
                    if ctx.cancelled() {
                        return Err("cancelled by user".into());
                    }
                    ctx.progress(base + (0.6 * j as f32 / syms.len() as f32) / total, "fetch", s);
                    rt.block_on(rquant::cli::run_fetch_to_csv(
                        s, 240, 1023, rquant::cli::SINA_BASE_URL, "qfq",
                        &ws.paper_dir().join(format!("pd_{}.csv", s)),
                        None,
                    ))
                    .map_err(|e| e.to_string())?;
                    std::thread::sleep(std::time::Duration::from_millis(500)); // sina 节流
                }
                if ctx.cancelled() {
                    return Err("cancelled by user".into());
                }
                ctx.progress(base + 0.8 / total, "select", "top3");
                let cfg = portfolio_cfg(ws);
                let (sig, new_state) =
                    rt.block_on(rquant::signal::run_signal_portfolio(&cfg, &llm)).map_err(|e| e.to_string())?;
                std::fs::write(book.sig_path(ws), serde_json::to_string_pretty(&sig).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
                if commit {
                    rquant::signal::write_holdings_state(&cfg.state_path, &new_state).map_err(|e| e.to_string())?;
                }
                summary.push(serde_json::json!({
                    "book": "b3", "t": sig.t.to_string(), "n_fresh": sig.n_fresh,
                    "targets": sig.targets, "committed": commit
                }));
            }
        }
    }
    Ok(serde_json::Value::Array(summary))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::books::BOOKS;
    use crate::paths::Workspace;

    #[test]
    fn single_cfg_mirrors_paper_run_cmd() {
        let ws = Workspace::new(std::path::PathBuf::from("E:/x"));
        let cfg = single_cfg(&ws, &BOOKS[0]);
        assert_eq!(cfg.warmup, 80);
        assert_eq!(cfg.window, 100);
        assert!((cfg.cost_bps - 10.0).abs() < 1e-12);
        assert!(!cfg.soft);
        assert!(cfg.primary_path.ends_with("paper/p_sh600030.csv"));
        assert_eq!(cfg.context_path, cfg.primary_path); // cmd 未传 --context → primary
        assert!(cfg.news_path.is_none());
        assert!(cfg.aux_paths.is_empty());
    }

    #[test]
    fn portfolio_cfg_mirrors_paper_run_cmd() {
        let ws = Workspace::new(std::path::PathBuf::from("E:/x"));
        let cfg = portfolio_cfg(&ws);
        assert_eq!(cfg.top, 3);
        assert!(cfg.soft);
        assert_eq!(cfg.warmup, 80);
        assert!(cfg.universe_path.ends_with("deploy/universe_10.csv"));
    }

    #[test]
    fn universe_symbols_parse() {
        let repo = Workspace::detect(&std::env::current_dir().unwrap()).unwrap();
        let syms = universe_symbols(&repo).unwrap();
        assert_eq!(syms.len(), 10);
        assert!(syms.contains(&"sh600519".to_string()));
    }
}
