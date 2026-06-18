//! 选股命令层:薄壳——配置/as-of 选股/回测(gross+net)/报告/指数相对。
//! 零业务逻辑——一切计算调 `rquant::screen`;长任务经 TaskRegistry 重槽。
use crate::commands::AppState;
use crate::dto_screen::*;

#[tauri::command]
pub fn screen_configs_list(state: tauri::State<AppState>) -> Vec<ScreenConfigDto> {
    let mut out = Vec::new();
    for (dir, frozen) in [(state.ws.screen_iter_dir(), false), (state.ws.deploy_dir(), true)] {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let is_yaml = p.extension().and_then(|s| s.to_str()).map(|x| x == "yaml" || x == "yml").unwrap_or(false);
            if !is_yaml { continue }
            let rel = p.strip_prefix(state.ws.root()).unwrap_or(&p).to_string_lossy().replace('\\', "/");
            // 选股配置合法性:用引擎自身的 load_screen_config(非 serde_yaml,桌面 crate 未依赖)。
            let (name, error) = match rquant::screen::config::load_screen_config(&p) {
                Ok(_) => (p.file_stem().and_then(|s| s.to_str()).map(String::from), None),
                Err(e) => (None, Some(format!("配置解析失败: {e}"))),
            };
            out.push(ScreenConfigDto { path: rel, name, frozen, error });
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

#[tauri::command]
pub fn index_list(state: tauri::State<AppState>) -> Vec<String> {
    let dir = state.ws.index_dir();
    let Ok(rd) = std::fs::read_dir(&dir) else { return vec![] };
    let mut v: Vec<String> = rd.flatten()
        .filter_map(|e| { let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("csv") {
                p.file_stem().and_then(|s| s.to_str()).map(String::from)
            } else { None } })
        .collect();
    v.sort();
    v
}

#[tauri::command]
pub fn screen_asof(state: tauri::State<AppState>, config: String, as_of: String, top: u32) -> Result<String, String> {
    let ws = state.ws.clone();
    state.tasks.start("screen_asof", true, move |ctx| {
        ctx.progress(0.1, "load", &config);
        let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
        let llm = rquant::cli::build_llm(String::new(), String::new(), ws.root().join(".rquant-cache").join("llm")).map_err(|e| e.to_string())?;
        let cfg = rquant::screen::ScreenRunConfig {
            config_path: ws.root().join(&config),
            universe_path: ws.root().join("data/baostock/universe_baostock_day.csv"),
            as_of: chrono::NaiveDate::parse_from_str(&as_of, "%Y-%m-%d").ok(),
            top: Some(top as usize), window: 260, out_path: None,
            membership_path: None, sectors_path: None,
        };
        ctx.progress(0.4, "screen", "");
        let res = rt.block_on(rquant::screen::run_screen(&cfg, &llm)).map_err(|e| e.to_string())?;
        let rows = res.rows.iter().map(|r| ScreenPickDto {
            rank: r.rank, symbol: r.symbol.clone(),
            quality_score: r.quality_score, speculative_score: r.speculative_score, combined_score: r.combined_score,
            tags: r.tags.clone(), selected: r.selected,
            reasons: r.reasons.iter().map(|x| ScreenReasonDto { tree: x.tree.clone(), leaf: x.leaf.clone(), score: x.score }).collect(),
        }).collect();
        let dto = ScreenResultDto { config, as_of: res.as_of.format("%Y-%m-%d").to_string(),
            n_universe: res.n_universe, top: res.top, rows };
        serde_json::to_value(dto).map_err(|e| e.to_string())
    })
}

#[tauri::command]
pub fn screen_backtest_run(state: tauri::State<AppState>, config: String, from: String, to: String, top: u32, rebalance: u32, cost_bps: f64) -> Result<String, String> {
    let ws = state.ws.clone();
    state.tasks.start("screen_backtest", true, move |ctx| {
        let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
        let llm = rquant::cli::build_llm(String::new(), String::new(), ws.root().join(".rquant-cache").join("llm")).map_err(|e| e.to_string())?;
        let mk = |cost: f64| rquant::screen::backtest::ScreenBacktestConfig {
            config_path: ws.root().join(&config),
            universe_path: ws.root().join("data/baostock/universe_baostock_day.csv"),
            from: chrono::NaiveDate::parse_from_str(&from, "%Y-%m-%d").ok(),
            to: chrono::NaiveDate::parse_from_str(&to, "%Y-%m-%d").ok(),
            rebalance: rebalance as usize, top: Some(top as usize),
            warmup: 260, window: 260, cost_bps: cost, soft: false,
            out_path: None, membership_path: None, sectors_path: None,
        };
        let id = crate::screen_runs::new_id();
        ctx.progress(0.2, "gross", "cost=0");
        let gross = rt.block_on(rquant::screen::backtest::run_screen_backtest(&mk(0.0), &llm)).map_err(|e| e.to_string())?;
        if ctx.cancelled() { return Err("cancelled".into()); }
        ctx.progress(0.6, "net", &format!("cost={cost_bps}"));
        let net = rt.block_on(rquant::screen::backtest::run_screen_backtest(&mk(cost_bps), &llm)).map_err(|e| e.to_string())?;
        let created = chrono::Local::now().naive_local().format("%Y-%m-%dT%H:%M:%S").to_string();
        let meta = ScreenRunMetaDto { id: id.clone(), config: config.clone(), from: from.clone(), to: to.clone(),
            top, rebalance, created, ok: true, error: None };
        crate::screen_runs::write_meta(&ws, &meta)?;
        crate::screen_runs::write_report(&ws, &id, "gross", &serde_json::to_string(&gross).map_err(|e| e.to_string())?)?;
        crate::screen_runs::write_report(&ws, &id, "net", &serde_json::to_string(&net).map_err(|e| e.to_string())?)?;
        ctx.progress(0.95, "archive", &id);
        Ok(serde_json::json!({ "run_id": id }))
    })
}

#[tauri::command]
pub fn screen_runs_list(state: tauri::State<AppState>) -> Vec<ScreenRunMetaDto> { crate::screen_runs::list_meta(&state.ws) }

#[tauri::command]
pub fn screen_run_report(state: tauri::State<AppState>, id: String) -> Result<ScreenBacktestReportDto, String> {
    let meta = crate::screen_runs::read_meta(&state.ws, &id)?;
    let net = crate::screen_runs::read_report(&state.ws, &id, "net")?;
    let gross = crate::screen_runs::read_report(&state.ws, &id, "gross")?;
    let f = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_f64());
    let net_total = f(&net, "total_return").unwrap_or(0.0);
    let gross_total = f(&gross, "total_return").unwrap_or(0.0);
    let cost = 20.0;
    let break_even = if gross_total > 0.0 && gross_total > net_total {
        Some(cost * gross_total / (gross_total - net_total)) } else { None };
    let nav = net.get("holdings").and_then(|h| h.as_array()).map(|a| a.iter().map(|h| NavPointDto {
        t: h.get("t").and_then(|x| x.as_str()).unwrap_or("").chars().take(10).collect(),
        nav: h.get("nav").and_then(|x| x.as_f64()).unwrap_or(0.0),
        benchmark_nav: h.get("benchmark_nav").and_then(|x| x.as_f64()).unwrap_or(0.0),
    }).collect()).unwrap_or_default();
    // DTO 子结构仅 derive Serialize(绑定已提交,不可改)——故手工逐字段映射,字段名对齐 rquant 源。
    let arr = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_array()).cloned().unwrap_or_default();
    let s = |o: &serde_json::Value, k: &str| o.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let num = |o: &serde_json::Value, k: &str| o.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0);
    let usz = |o: &serde_json::Value, k: &str| o.get(k).and_then(|x| x.as_u64()).unwrap_or(0) as usize;
    let tag_attribution: Vec<TagAttribDto> = arr(&net, "tag_attribution").iter().map(|o| TagAttribDto {
        tag: s(o, "tag"), n_picks: usz(o, "n_picks"), hit_rate: num(o, "hit_rate"), mean_fwd_return: num(o, "mean_fwd_return"),
    }).collect();
    let regime_slices: Vec<RegimeSliceDto> = arr(&net, "regime_slices").iter().map(|o| RegimeSliceDto {
        label: s(o, "label"), from: s(o, "from"), to: s(o, "to"),
        picks_return: num(o, "picks_return"), benchmark_return: num(o, "benchmark_return"), excess: num(o, "excess"),
    }).collect();
    let quality_layers: Vec<QualityLayerDto> = arr(&net, "quality_layers").iter().map(|o| QualityLayerDto {
        layer: usz(o, "layer"), n: usz(o, "n"), mean_quality: num(o, "mean_quality"), mean_fwd_return: num(o, "mean_fwd_return"),
    }).collect();
    Ok(ScreenBacktestReportDto {
        meta, net_total_return: net_total, gross_total_return: gross_total,
        abs_sharpe: net.get("risk").and_then(|r| r.get("sharpe")).and_then(|x| x.as_f64()),
        max_drawdown: f(&net, "max_drawdown").unwrap_or(0.0), turnover: f(&net, "turnover").unwrap_or(0.0),
        break_even, nav, tag_attribution, regime_slices, quality_layers,
    })
}

#[tauri::command]
pub fn screen_index_relative(state: tauri::State<AppState>, id: String, benchmark: String) -> Result<IndexRelativeDto, String> {
    let net = crate::screen_runs::read_report(&state.ws, &id, "net")?;
    let holdings: Vec<(String, f64)> = net.get("holdings").and_then(|h| h.as_array()).map(|a| a.iter().filter_map(|h| {
        Some((h.get("t")?.as_str()?.chars().take(10).collect(), h.get("nav")?.as_f64()?))
    }).collect()).unwrap_or_default();
    let regimes: Vec<(String, String, String)> = net.get("regime_slices").and_then(|s| s.as_array()).map(|a| a.iter().filter_map(|s| {
        Some((s.get("label")?.as_str()?.to_string(), s.get("from")?.as_str()?.to_string(), s.get("to")?.as_str()?.to_string()))
    }).collect()).unwrap_or_default();
    let idx = crate::index_relative::load_index(&state.ws.index_dir().join(format!("{benchmark}.csv")))?;
    let r = crate::index_relative::compute(&holdings, &regimes, &idx);
    Ok(IndexRelativeDto {
        benchmark, excess_cum: r.excess_cum,
        curve: r.curve.into_iter().map(|(t, excess)| ExcessPointDto { t, excess }).collect(),
        per_regime: r.per_regime.into_iter().map(|(label, excess)| RegimeExcessDto { label, excess }).collect(),
    })
}
