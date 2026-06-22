//! 部署命令层:读账本 / 预览当月(run_month) / 落账(commit_month)。
//! 零业务逻辑——状态读写委托 deploy_book；选股委托 rquant::screen。
use crate::commands::AppState;
use crate::dto_deploy::*;
use std::collections::HashMap;

const DEPLOY_CONFIG: &str = "deploy/value_growth_quality_frozen.yaml";

fn load_close(ws: &crate::paths::Workspace, sym: &str) -> HashMap<String, f64> {
    let mut m = HashMap::new();
    let Ok(txt) = std::fs::read_to_string(ws.kday_dir().join(format!("{sym}.csv"))) else {
        return m;
    };
    let mut lines = txt.lines();
    // Parse header to find the `close` column index.
    let Some(header) = lines.next() else { return m };
    let headers: Vec<&str> = header.split(',').collect();
    let Some(close_idx) = headers.iter().position(|h| h.trim().eq_ignore_ascii_case("close")) else {
        log::warn!("load_close: {sym}.csv has no 'close' column in header");
        return m;
    };
    for line in lines {
        let c: Vec<&str> = line.split(',').collect();
        if c.len() > close_idx {
            if let Ok(close) = c[close_idx].parse::<f64>() {
                m.insert(c[0].get(..10).unwrap_or(c[0]).to_string(), close);
            }
        }
    }
    m
}

// 跑 as-of screen(冻结配置) → (top-3 选中 symbols, 实际交易日日期)；top-3=用户资金/手续约束的集中口径(2026-06-22)
fn screen_picks(ws: &crate::paths::Workspace, as_of: &str) -> Result<(Vec<String>, String), String> {
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let llm = rquant::cli::build_llm(
        String::new(),
        String::new(),
        ws.root().join(".rquant-cache").join("llm"),
    )
    .map_err(|e| e.to_string())?;
    // 纸面盘恒定剔除 ST/*ST 高风险股(用户口径:投资不选 ST)——引擎级,选股前剔除使 top-3 回补到非 ST。
    // 名单缺失(未跑 scripts/build_stock_names.py)时退化为不过滤并告警,避免部署硬失败。
    let st_path = ws.root().join("data/baostock/st_symbols.csv");
    let st_symbols_path = if st_path.exists() {
        Some(st_path)
    } else {
        log::warn!("deploy screen_picks: 缺 st_symbols.csv,本月未做 ST 过滤(请跑 scripts/build_stock_names.py): {}", st_path.display());
        None
    };
    let cfg = rquant::screen::ScreenRunConfig {
        config_path: ws.root().join(DEPLOY_CONFIG),
        universe_path: ws.root().join("data/baostock/universe_baostock_day.csv"),
        as_of: chrono::NaiveDate::parse_from_str(as_of, "%Y-%m-%d").ok(),
        top: Some(3),
        window: 260,
        out_path: None,
        membership_path: None,
        sectors_path: None,
        st_symbols_path,
    };
    let res = rt
        .block_on(rquant::screen::run_screen(&cfg, &llm))
        .map_err(|e| e.to_string())?;
    let actual_date = res.as_of.format("%Y-%m-%d").to_string();
    let picks = res
        .rows
        .iter()
        .filter(|r| r.selected)
        .map(|r| r.symbol.clone())
        .collect();
    Ok((picks, actual_date))
}

// 共享:算一个月的预览(选股 + diff + 滚动 NAV),不写
fn compute_month(
    ws: &crate::paths::Workspace,
    as_of: &str,
) -> Result<(DeployMonthDto, crate::deploy_book::DeployState, f64, f64), String> {
    let st = crate::deploy_book::read_state(&ws.deploy_book_path()).map_err(|e| format!("账本文件损坏,拒绝覆盖以免丢失历史(请检查 value.json):{e}"))?;
    let (picks, actual_date) = screen_picks(ws, as_of)?;
    if actual_date != as_of {
        return Err(format!("该日数据未刷新(实际可用最近交易日 {actual_date},去数据工作台抓取):{as_of}"));
    }
    if picks.is_empty() {
        return Err("该日无选股(数据未刷新或配置异常)".into());
    }
    let dlist = crate::deploy_book::diff(&st.holdings, &picks);
    // 实现收益:上月持仓 last_date→as_of 的 EW 收益(首月=0)
    let syms: std::collections::BTreeSet<String> = st.holdings.iter().cloned().collect();
    let mut px: HashMap<String, HashMap<String, f64>> = HashMap::new();
    for s in &syms {
        px.insert(s.clone(), load_close(ws, s));
    }
    let price = |s: &str, d: &str| px.get(s).and_then(|m| m.get(d)).copied();
    let realized = match &st.last_date {
        Some(d0) => {
            if !st.holdings.is_empty() {
                let covered = st.holdings.iter().filter(|s| price(s, d0).is_some() && price(s, as_of).is_some()).count();
                if covered == 0 {
                    return Err("持仓行情缺失,无法计算实现收益(数据未刷新?)".into());
                }
            }
            crate::deploy_book::ew_return(&st.holdings, &price, d0, as_of)
        }
        None => 0.0,
    };
    let prev_nav = if st.nav > 0.0 { st.nav } else { 1.0 };
    let proj_nav = prev_nav * (1.0 + realized);
    // 沪深300 归一 bench_nav
    let idx =
        crate::index_relative::load_index(&ws.index_dir().join("csi300.csv"))?;
    let bench_at = crate::index_relative::idx_at(&idx, as_of)
        .ok_or_else(|| "沪深300 指数不覆盖该日(数据未刷新?)".to_string())?;
    let bench_base = st.bench_base.unwrap_or(bench_at);
    let bench_nav = if bench_base > 0.0 { bench_at / bench_base } else { 1.0 };
    let proj_excess = (proj_nav - 1.0) - (bench_nav - 1.0);
    let picks_dto: Vec<DeployHoldingDto> = picks
        .iter()
        .map(|s| DeployHoldingDto {
            symbol: s.clone(),
            weight: 1.0 / picks.len() as f64,
            since: as_of.to_string(),
        })
        .collect();
    let dto = DeployMonthDto {
        as_of: as_of.to_string(),
        picks: picks_dto,
        diff: dlist,
        proj_nav,
        proj_excess,
        realized_ret: realized,
    };
    Ok((dto, st, proj_nav, bench_nav))
}

#[tauri::command]
pub fn deploy_book_read(state: tauri::State<AppState>) -> DeployBookDto {
    let st = match crate::deploy_book::read_state(&state.ws.deploy_book_path()) {
        Ok(st) => st,
        Err(_) => return DeployBookDto { status: "corrupt".into(), nav: None, excess_total: None, last_rebalance: None, holdings: vec![], nav_history: vec![], months: vec![] },
    };
    let status = if st.months.is_empty() { "empty" } else { "ok" }.to_string();
    let excess_total = st
        .nav_history
        .last()
        .map(|p| (p.nav - 1.0) - (p.bench_nav - 1.0));
    DeployBookDto {
        status,
        nav: if st.nav > 0.0 { Some(st.nav) } else { None },
        excess_total,
        last_rebalance: st.last_date.clone(),
        holdings: st
            .holdings
            .iter()
            .map(|s| DeployHoldingDto {
                symbol: s.clone(),
                weight: if st.holdings.is_empty() {
                    0.0
                } else {
                    1.0 / st.holdings.len() as f64
                },
                since: st.last_date.clone().unwrap_or_default(),
            })
            .collect(),
        nav_history: st
            .nav_history
            .iter()
            .map(|p| DeployNavPointDto {
                t: p.t.clone(),
                nav: p.nav,
                bench_nav: p.bench_nav,
            })
            .collect(),
        months: st
            .months
            .iter()
            .map(|m| DeployMonthRecDto {
                as_of: m.as_of.clone(),
                nav: m.nav,
                excess: (m.nav - 1.0) - (m.bench_nav - 1.0),
                n_holdings: m.picks.len() as u32,
                n_buy: m.n_buy,
                n_sell: m.n_sell,
            })
            .collect(),
    }
}

#[tauri::command]
pub fn deploy_run_month(
    state: tauri::State<AppState>,
    as_of: String,
) -> Result<String, String> {
    let ws = state.ws.clone();
    state.tasks.start("deploy_month", true, move |ctx| {
        ctx.note_params(serde_json::json!({"as_of": &as_of, "config": DEPLOY_CONFIG}));
        ctx.note_file(&ws.root().join(DEPLOY_CONFIG).to_string_lossy().into_owned());
        ctx.note_file(&ws.root().join("data/baostock/universe_baostock_day.csv").to_string_lossy().into_owned());
        ctx.note_file(&ws.index_dir().join("csi300.csv").to_string_lossy().into_owned());
        log::info!("deploy_run_month: as_of={as_of} config={DEPLOY_CONFIG}");
        ctx.progress(0.3f32, "选股", &as_of);
        let (dto, _st, _nav, _b) = compute_month(&ws, &as_of)?;
        ctx.note_summary(&format!("picks {} proj_nav {:.3}", dto.picks.len(), dto.proj_nav));
        serde_json::to_value(dto).map_err(|e| e.to_string())
    })
}

#[tauri::command]
pub fn deploy_commit_month(
    state: tauri::State<AppState>,
    as_of: String,
) -> Result<String, String> {
    let ws = state.ws.clone();
    state.tasks.start("deploy_commit", true, move |ctx| {
        ctx.note_params(serde_json::json!({"as_of": &as_of}));
        ctx.progress(0.3f32, "选股", &as_of);
        let (dto, mut st, proj_nav, bench_nav) = compute_month(&ws, &as_of)?;
        let picks: Vec<String> = dto.picks.iter().map(|h| h.symbol.clone()).collect();
        let n_buy = dto.diff.iter().filter(|d| d.action == "Buy").count() as u32;
        let n_sell = dto.diff.iter().filter(|d| d.action == "Sell").count() as u32;
        if st.bench_base.is_none() {
            let idx = crate::index_relative::load_index(
                &ws.index_dir().join("csi300.csv"),
            )?;
            st.bench_base = crate::index_relative::idx_at(&idx, &as_of);
        }
        st.nav = proj_nav;
        st.nav_history.push(crate::deploy_book::NavPoint {
            t: as_of.clone(),
            nav: proj_nav,
            bench_nav,
        });
        st.months.push(crate::deploy_book::MonthRec {
            as_of: as_of.clone(),
            picks: picks.clone(),
            nav: proj_nav,
            bench_nav,
            n_buy,
            n_sell,
        });
        st.holdings = picks;
        st.last_date = Some(as_of.clone());
        crate::deploy_book::write_state(&ws.deploy_book_path(), &st)?;
        ctx.note_summary(&format!("nav {:.3}", proj_nav));
        Ok(serde_json::Value::Null)
    })
}
