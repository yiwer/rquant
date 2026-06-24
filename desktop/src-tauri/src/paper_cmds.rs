//! 「纸面盘」命令层:读 paper_ridge 产物算状态 DTO;写操作 shell Python(镜像 iter_cmds)。
use crate::commands::AppState;
use crate::dto_paper::*;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};

fn jstr(v: &serde_json::Value, k: &str) -> String { v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string() }
fn jf64(v: &serde_json::Value, k: &str) -> f64 { v.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0) }
fn ji64(v: &serde_json::Value, k: &str) -> i64 { v.get(k).and_then(|x| x.as_i64()).unwrap_or(0) }

/// 纯:解析三产物 → DTO。weights 空/解析失败 → initialized=false。
pub fn parse_status(weights_json: &str, journal_csv: &str, blend_json: Option<&str>,
                    idx: &BTreeMap<String, f64>) -> PaperStatusDto {
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
    PaperStatusDto {
        initialized, strategy: jstr(&w,"strategy"),
        train_lo: jstr(&w,"train_lo"), train_hi: jstr(&w,"train_hi"), n_train_dates: ji64(&w,"n_train_dates"),
        delta: jf64(&w,"delta"), top_n: ji64(&w,"top_n"), cost_bps: jf64(&w,"cost_bps"),
        open_picks, closed, cum_net, cum_excess, blend,
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
    Ok(parse_status(&weights, &journal, blend.as_deref(), &idx))
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
}
