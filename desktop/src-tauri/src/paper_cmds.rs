//! 「纸面盘」命令层:读 paper_ridge 产物算状态 DTO;写操作 shell Python(镜像 iter_cmds)。
use crate::commands::AppState;
use crate::dto_paper::*;
use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};

fn jstr(v: &serde_json::Value, k: &str) -> String { v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string() }
fn jf64(v: &serde_json::Value, k: &str) -> f64 { v.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0) }
fn ji64(v: &serde_json::Value, k: &str) -> i64 { v.get(k).and_then(|x| x.as_i64()).unwrap_or(0) }

/// 从 stock_names.csv 文本(header: symbol,name + optional extra cols)解析代码→名称映射。
/// 文件缺失/解析失败时调用方传空串 → 返回空 map,不 panic。
pub(crate) fn parse_names_csv(csv_text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in csv_text.lines().skip(1) {
        let mut it = line.splitn(3, ',');
        let sym = match it.next() { Some(s) if !s.is_empty() => s.to_string(), _ => continue };
        let name = match it.next() { Some(s) => s.to_string(), None => continue };
        map.insert(sym, name);
    }
    map
}

fn load_names(state: &AppState) -> HashMap<String, String> {
    let path = state.ws.root().join("data").join("baostock").join("stock_names.csv");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    parse_names_csv(&text)
}

/// 纯函数:从 factors.csv 文本中找 symbol 的最新一行,返回 (asof, Vec<FactorKVDto>)。
/// 忽略 date / symbol / fwd_ret_5d 列;NaN 或空 → None。
pub(crate) fn latest_factor_row(csv_text: &str, symbol: &str) -> (String, Vec<FactorKVDto>) {
    let mut lines = csv_text.lines();
    let header = match lines.next() { Some(h) => h, None => return (String::new(), vec![]) };
    let cols: Vec<&str> = header.split(',').collect();
    let skip_set = ["date", "symbol", "fwd_ret_5d"];

    // Find latest row for this symbol (keep last match = latest date assuming sorted asc)
    let mut best_date = String::new();
    let mut best_fields: Vec<String> = vec![];
    for line in lines {
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() < cols.len() { continue; }
        // symbol is col 1 (after date)
        let sym_idx = cols.iter().position(|c| *c == "symbol").unwrap_or(1);
        if fields.get(sym_idx).map(|s| s.trim()) != Some(symbol) { continue; }
        let date_idx = cols.iter().position(|c| *c == "date").unwrap_or(0);
        let d = fields.get(date_idx).unwrap_or(&"").trim().to_string();
        if d >= best_date {
            best_date = d;
            best_fields = fields.iter().map(|s| s.to_string()).collect();
        }
    }
    if best_date.is_empty() { return (String::new(), vec![]); }
    let kvs: Vec<FactorKVDto> = cols.iter().enumerate()
        .filter(|(_, c)| !skip_set.contains(c))
        .map(|(i, c)| {
            let raw = best_fields.get(i).map(|s| s.trim()).unwrap_or("");
            let value = if raw.is_empty() { None } else {
                raw.parse::<f64>().ok().filter(|f| f.is_finite())
            };
            FactorKVDto { key: c.to_string(), value }
        })
        .collect();
    (best_date, kvs)
}

/// 纯:解析三产物 → DTO。weights 空/解析失败 → initialized=false。
pub fn parse_status(weights_json: &str, journal_csv: &str, blend_json: Option<&str>,
                    idx: &BTreeMap<String, f64>) -> PaperStatusDto {
    parse_status_with_names(weights_json, journal_csv, blend_json, idx, &HashMap::new())
}

pub fn parse_status_with_names(weights_json: &str, journal_csv: &str, blend_json: Option<&str>,
                    idx: &BTreeMap<String, f64>, name_map: &HashMap<String, String>) -> PaperStatusDto {
    let w: Option<serde_json::Value> = serde_json::from_str(weights_json).ok();
    let initialized = w.is_some();
    let w = w.unwrap_or(serde_json::Value::Null);

    let mut closed: Vec<PaperRowDto> = vec![];
    let mut open_picks: Vec<String> = vec![];
    let mut nav = 1.0;
    for line in journal_csv.lines().skip(1) {
        let f: Vec<&str> = line.split(',').collect();
        if f.len() < 7 { continue; }
        let picks: Vec<String> = f[2].split(';').filter(|s| !s.is_empty()).map(String::from).collect();
        let status = f[1].to_string();
        let parse = |s: &str| -> Option<f64> { let t = s.trim(); if t.is_empty() { None } else { t.parse().ok() } };
        if status == "closed" {
            let net = parse(f[6]).unwrap_or(0.0);
            nav *= 1.0 + net;
            closed.push(PaperRowDto { date: f[0].into(), status, picks,
                turnover: parse(f[4]), gross_ret: parse(f[5]), net_ret: Some(net), nav });
        } else {
            open_picks = picks;  // 最后一个 open 即当前持仓
        }
    }
    let cum_net = if closed.is_empty() { 0.0 } else { nav - 1.0 };
    let holdings: Vec<(String, f64)> = closed.iter().map(|r| (r.date.clone(), r.nav)).collect();
    // 无指数数据(如 csi300.csv 缺失,idx 空)→ 不把原始收益误当超额,返 None(与 paper_ridge.py 一致)
    let cum_excess = if idx.is_empty() {
        None
    } else {
        crate::index_relative::compute(&holdings, &[], idx).excess_cum
    };
    let blend = blend_json.and_then(|b| serde_json::from_str::<serde_json::Value>(b).ok()).map(|v| BlendDto {
        folds: v.get("folds").and_then(|x| x.as_array()).map(|a| a.iter().map(|f| BlendFoldDto {
            oos: jstr(f,"oos"), corr: jf64(f,"corr"),
            sh_ridge: jf64(f,"sh_ridge"), sh_val: jf64(f,"sh_val"), sh_blend: jf64(f,"sh_blend"),
            dd_ridge: jf64(f,"dd_ridge"), dd_val: jf64(f,"dd_val"), dd_blend: jf64(f,"dd_blend"),
            ex_ridge: jf64(f,"ex_ridge"), ex_val: jf64(f,"ex_val"), ex_blend: jf64(f,"ex_blend"),
        }).collect()).unwrap_or_default(),
        mean: { let m = v.get("mean").cloned().unwrap_or(serde_json::Value::Null); BlendFoldMeanDto {
            corr: jf64(&m,"corr"), sh_ridge: jf64(&m,"sh_ridge"), sh_val: jf64(&m,"sh_val"), sh_blend: jf64(&m,"sh_blend"),
            dd_ridge: jf64(&m,"dd_ridge"), dd_val: jf64(&m,"dd_val"), dd_blend: jf64(&m,"dd_blend"),
            ex_ridge: jf64(&m,"ex_ridge"), ex_val: jf64(&m,"ex_val"), ex_blend: jf64(&m,"ex_blend"),
        }},
    });
    // Collect all symbols visible in status, look them up in name_map
    let all_syms: std::collections::HashSet<&str> = open_picks.iter().map(String::as_str)
        .chain(closed.iter().flat_map(|r| r.picks.iter().map(String::as_str)))
        .collect();
    let names: HashMap<String, String> = all_syms.iter()
        .filter_map(|s| name_map.get(*s).map(|n| (s.to_string(), n.clone())))
        .collect();
    PaperStatusDto {
        initialized, strategy: jstr(&w,"strategy"),
        train_lo: jstr(&w,"train_lo"), train_hi: jstr(&w,"train_hi"), n_train_dates: ji64(&w,"n_train_dates"),
        delta: jf64(&w,"delta"), top_n: ji64(&w,"top_n"), cost_bps: jf64(&w,"cost_bps"),
        open_picks, closed, cum_net, cum_excess, blend, names,
    }
}

fn fp_dir(state: &AppState) -> std::path::PathBuf { state.ws.root().join("data").join("factor_panel") }

#[tauri::command]
pub fn paper_ridge_status(state: tauri::State<AppState>) -> Result<PaperStatusDto, String> {
    let d = fp_dir(&state);
    let weights = std::fs::read_to_string(d.join("paper_ridge_weights.json")).unwrap_or_default();
    let journal = std::fs::read_to_string(d.join("paper_ridge_journal.csv")).unwrap_or_default();
    let blend = std::fs::read_to_string(d.join("paper_blend.json")).ok();
    let idx = crate::index_relative::load_index(&state.ws.index_dir().join("csi300.csv")).unwrap_or_default();
    let name_map = load_names(&state);
    Ok(parse_status_with_names(&weights, &journal, blend.as_deref(), &idx, &name_map))
}

#[tauri::command]
pub fn paper_stock_detail(state: tauri::State<AppState>, symbol: String) -> Result<PaperStockDetailDto, String> {
    if !crate::paths::valid_symbol(&symbol) {
        return Err(format!("invalid symbol: {symbol}"));
    }
    let name_map = load_names(&state);
    let name = name_map.get(&symbol).cloned().unwrap_or_else(|| symbol.clone());
    let kday_path = format!("data/baostock/kday/{symbol}.csv");
    let factors_path = state.ws.root().join("data").join("factor_panel").join("factors.csv");
    let csv_text = std::fs::read_to_string(&factors_path).unwrap_or_default();
    let (asof, factors) = latest_factor_row(&csv_text, &symbol);
    Ok(PaperStockDetailDto { symbol, name, kday_path, asof, factors })
}

fn shell_python(state: &AppState, kind: &'static str, args: Vec<String>) -> Result<String, String> {
    let ws = state.ws.clone();
    state.tasks.start(kind, true, move |ctx| {
        let py = crate::paths::python_exe();
        let mut cmd = Command::new(&py);
        cmd.current_dir(ws.root());
        for a in &args { cmd.arg(a); }
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        ctx.progress(0.05, "启动", &args.join(" "));
        let mut child = cmd.spawn().map_err(|e| format!("启动 Python 失败: {e}"))?;
        let se = child.stderr.take().map(|s| std::thread::spawn(move || { let mut t=String::new(); let _=BufReader::new(s).read_to_string(&mut t); t }));
        if let Some(out) = child.stdout.take() {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if ctx.cancelled() { let _ = child.kill(); return Err("cancelled".into()); }
                ctx.progress(0.5, "运行", &line);
            }
        }
        let st = child.wait().map_err(|e| e.to_string())?;
        let err = se.and_then(|h| h.join().ok()).unwrap_or_default();
        if !st.success() { return Err(format!("Python 退出码 {:?}: {}", st.code(), err)); }
        ctx.progress(0.98, "完成", "");
        Ok(serde_json::json!({"ok": true}))
    })
}

#[tauri::command]
pub fn paper_ridge_advance(state: tauri::State<AppState>) -> Result<String, String> {
    shell_python(&state, "paper_advance", vec!["scripts/paper_ridge.py".into()])
}
#[tauri::command]
pub fn paper_ridge_retrain(state: tauri::State<AppState>) -> Result<String, String> {
    shell_python(&state, "paper_retrain", vec!["scripts/paper_ridge.py".into(), "--retrain".into()])
}
#[tauri::command]
pub fn paper_blend_recompute(state: tauri::State<AppState>) -> Result<String, String> {
    shell_python(&state, "paper_blend", vec!["scripts/eval_blend.py".into(), "--json".into(), "data/factor_panel/paper_blend.json".into()])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    fn idx() -> BTreeMap<String, f64> {
        BTreeMap::from([("2026-06-04".into(),100.0),("2026-06-11".into(),101.0),("2026-06-18".into(),99.0)])
    }
    const W: &str = r#"{"strategy":"ridge-on-gauss / 去相关岭组合","train_lo":"2018-02-06","train_hi":"2026-06-04","n_train_dates":404,"delta":0.05,"top_n":3,"cost_bps":20.0,"factor_cols":["f_bm"],"weights":[0.1]}"#;
    // 一平仓(net=0.02)+ 一开仓
    const J: &str = "date,status,picks,prev_picks,turnover,gross_ret,net_ret\n2026-06-11,closed,sh600208;sz000039;sz301316,,1.0,0.022,0.020\n2026-06-18,open,sh600000;sz000001;sz301316,sh600208;sz000039;sz301316,0.67,,\n";

    #[test]
    fn parses_meta_and_nav_and_open() {
        let s = parse_status(W, J, None, &idx());
        assert!(s.initialized);
        assert_eq!(s.n_train_dates, 404);
        assert_eq!(s.closed.len(), 1);
        assert!((s.closed[0].nav - 1.02).abs() < 1e-9);          // cumprod(1+0.02)
        assert!((s.cum_net - 0.02).abs() < 1e-9);
        assert_eq!(s.open_picks, vec!["sh600000","sz000001","sz301316"]);
    }
    #[test]
    fn uninitialized_when_weights_blank() {
        let s = parse_status("", "", None, &idx());
        assert!(!s.initialized);
    }
    #[test]
    fn excess_uses_index() {
        // closed 只有单行(J 中一条 closed),holdings 传给 compute 只有 1 个点 → nav.len() < 2 → excess_cum = None
        let s = parse_status(W, J, None, &idx());
        assert!(s.cum_excess.is_none());
    }
    // 2 平仓行:有指数→算出真超额(Some);空指数(csi300 缺失)→守卫返 None,不误把原始收益当超额
    const J2: &str = "date,status,picks,prev_picks,turnover,gross_ret,net_ret\n2026-06-11,closed,a;b;c,,1.0,0.022,0.020\n2026-06-18,closed,a;b;c,a;b;c,0.0,0.012,0.012\n";
    #[test]
    fn excess_some_when_index_present_and_two_closed() {
        let s = parse_status(W, J2, None, &idx());
        assert!(s.cum_excess.is_some());
    }
    #[test]
    fn excess_none_when_index_missing() {
        let s = parse_status(W, J2, None, &BTreeMap::new());
        assert!(s.cum_excess.is_none());
    }
    #[test]
    fn parses_blend() {
        let b = r#"{"folds":[{"oos":"2020","corr":0.28,"sh_ridge":1.06,"sh_val":0.43,"sh_blend":0.96,"dd_ridge":0.25,"dd_val":0.24,"dd_blend":0.11,"ex_ridge":0.095,"ex_val":-0.11,"ex_blend":0.01}],"mean":{"corr":0.36,"sh_ridge":0.68,"sh_val":0.43,"sh_blend":0.68,"dd_ridge":0.24,"dd_val":0.24,"dd_blend":0.17,"ex_ridge":0.186,"ex_val":0.08,"ex_blend":0.145}}"#;
        let s = parse_status(W, J, Some(b), &idx());
        let bl = s.blend.unwrap();
        assert_eq!(bl.folds.len(), 1);
        assert!((bl.mean.dd_blend - 0.17).abs() < 1e-9);
    }

    // ── latest_factor_row tests ──────────────────────────────────────────────
    const FACTORS_CSV: &str = "date,symbol,f_bm,f_mom,fwd_ret_5d\n\
2026-06-11,sh600000,0.5,1.2,0.01\n\
2026-06-18,sh600000,0.6,,0.02\n\
2026-06-11,sz000001,0.3,0.9,0.005\n";

    #[test]
    fn latest_factor_row_picks_latest_date() {
        let (asof, kvs) = latest_factor_row(FACTORS_CSV, "sh600000");
        assert_eq!(asof, "2026-06-18");
        // Should have f_bm and f_mom (not date/symbol/fwd_ret_5d)
        assert_eq!(kvs.len(), 2);
        let f_bm = kvs.iter().find(|k| k.key == "f_bm").unwrap();
        assert!((f_bm.value.unwrap() - 0.6).abs() < 1e-9);
        // f_mom is empty → None
        let f_mom = kvs.iter().find(|k| k.key == "f_mom").unwrap();
        assert!(f_mom.value.is_none());
    }

    #[test]
    fn latest_factor_row_excludes_skip_cols() {
        let (_, kvs) = latest_factor_row(FACTORS_CSV, "sh600000");
        assert!(!kvs.iter().any(|k| k.key == "date" || k.key == "symbol" || k.key == "fwd_ret_5d"));
    }

    #[test]
    fn latest_factor_row_returns_empty_for_unknown_symbol() {
        let (asof, kvs) = latest_factor_row(FACTORS_CSV, "sh999999");
        assert_eq!(asof, "");
        assert!(kvs.is_empty());
    }

    // ── parse_names_csv tests ────────────────────────────────────────────────
    const NAMES_CSV: &str = "symbol,name\nsh600000,浦发银行\nsz000001,平安银行\n";

    #[test]
    fn parse_names_csv_builds_map() {
        let m = parse_names_csv(NAMES_CSV);
        assert_eq!(m.get("sh600000").map(String::as_str), Some("浦发银行"));
        assert_eq!(m.get("sz000001").map(String::as_str), Some("平安银行"));
        assert!(m.get("sh999999").is_none());
    }

    #[test]
    fn parse_names_csv_empty_text_returns_empty_map() {
        assert!(parse_names_csv("").is_empty());
    }

    #[test]
    fn names_populated_in_parse_status_with_names() {
        let name_map: HashMap<String, String> = [
            ("sh600208".to_string(), "新湖中宝".to_string()),
            ("sh600000".to_string(), "浦发银行".to_string()),
        ].into_iter().collect();
        let s = parse_status_with_names(W, J, None, &idx(), &name_map);
        assert_eq!(s.names.get("sh600208").map(String::as_str), Some("新湖中宝"));
        assert_eq!(s.names.get("sh600000").map(String::as_str), Some("浦发银行"));
        // Symbol not in name_map should be absent
        assert!(s.names.get("sz000039").is_none());
    }
}
