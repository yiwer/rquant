//! 部署命令层:读账本 / 预览当月(run_month) / 落账(commit_month)。
//! 零业务逻辑——状态读写委托 deploy_book；选股委托 rquant::screen。
use crate::commands::AppState;
use crate::dto_deploy::*;
use std::collections::HashMap;

const DEPLOY_CONFIG: &str = "deploy/value_pb_deploy_frozen.yaml";

fn load_close(ws: &crate::paths::Workspace, sym: &str) -> HashMap<String, f64> {
    let mut m = HashMap::new();
    if let Ok(txt) = std::fs::read_to_string(ws.kday_dir().join(format!("{sym}.csv"))) {
        for line in txt.lines().skip(1) {
            let c: Vec<&str> = line.split(',').collect();
            if c.len() >= 5 {
                if let Ok(close) = c[4].parse::<f64>() {
                    m.insert(c[0].get(..10).unwrap_or(c[0]).to_string(), close);
                }
            }
        }
    }
    m
}

// 跑 as-of screen(冻结配置) → top-50 选中 symbols(按 combined 降序)
fn screen_picks(ws: &crate::paths::Workspace, as_of: &str) -> Result<Vec<String>, String> {
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let llm = rquant::cli::build_llm(
        String::new(),
        String::new(),
        ws.root().join(".rquant-cache").join("llm"),
    )
    .map_err(|e| e.to_string())?;
    let cfg = rquant::screen::ScreenRunConfig {
        config_path: ws.root().join(DEPLOY_CONFIG),
        universe_path: ws.root().join("data/baostock/universe_baostock_day.csv"),
        as_of: chrono::NaiveDate::parse_from_str(as_of, "%Y-%m-%d").ok(),
        top: Some(50),
        window: 260,
        out_path: None,
        membership_path: None,
        sectors_path: None,
    };
    let res = rt
        .block_on(rquant::screen::run_screen(&cfg, &llm))
        .map_err(|e| e.to_string())?;
    Ok(res
        .rows
        .iter()
        .filter(|r| r.selected)
        .map(|r| r.symbol.clone())
        .collect())
}

// 共享:算一个月的预览(选股 + diff + 滚动 NAV),不写
fn compute_month(
    ws: &crate::paths::Workspace,
    as_of: &str,
) -> Result<(DeployMonthDto, crate::deploy_book::DeployState, f64, f64), String> {
    let st = crate::deploy_book::read_state(&ws.deploy_book_path());
    let picks = screen_picks(ws, as_of)?;
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
    let st = crate::deploy_book::read_state(&state.ws.deploy_book_path());
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
        ctx.progress(0.3f32, "选股", &as_of);
        let (dto, _st, _nav, _b) = compute_month(&ws, &as_of)?;
        serde_json::to_value(dto).map_err(|e| e.to_string())
    })
}

#[tauri::command]
pub fn deploy_commit_month(
    state: tauri::State<AppState>,
    as_of: String,
) -> Result<(), String> {
    let (dto, mut st, proj_nav, bench_nav) = compute_month(&state.ws, &as_of)?;
    let picks: Vec<String> = dto.picks.iter().map(|h| h.symbol.clone()).collect();
    let n_buy = dto.diff.iter().filter(|d| d.action == "Buy").count() as u32;
    let n_sell = dto.diff.iter().filter(|d| d.action == "Sell").count() as u32;
    if st.bench_base.is_none() {
        let idx = crate::index_relative::load_index(
            &state.ws.index_dir().join("csi300.csv"),
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
    st.last_date = Some(as_of);
    crate::deploy_book::write_state(&state.ws.deploy_book_path(), &st)
}
