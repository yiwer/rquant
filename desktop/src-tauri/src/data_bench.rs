//! 数据工作台:CSV 清单/新鲜度、K线读取(tail 上限)、因子叠加现算、universe 管理、批量拉取任务。
//! 一切路径经 resolve_under_root 越界守卫(spec §9 fs 收敛)。
use crate::dto::{BarDto, CsvInfoDto, FactorPointDto, UniverseEntryDto, UniverseInfoDto};
use crate::paths::{valid_symbol, Workspace};
use rquant::data::bar::Bar;

const MAX_TAIL: usize = 2000;

fn iso(t: &chrono::NaiveDateTime) -> String {
    t.format("%Y-%m-%dT%H:%M:%S").to_string()
}

/// 工作区相对路径 → 绝对路径,拒绝越界(canonicalize 前缀校验;文件须存在)。
fn resolve_under_root(ws: &Workspace, rel: &str) -> Result<std::path::PathBuf, String> {
    let joined = ws.root().join(rel);
    let canon = joined.canonicalize().map_err(|e| format!("{}: {}", rel, e))?;
    let root = ws.root().canonicalize().map_err(|e| e.to_string())?;
    if !canon.starts_with(&root) {
        return Err(format!("path escapes workspace: {}", rel));
    }
    Ok(canon)
}

fn rel_of(ws: &Workspace, abs: &std::path::Path) -> String {
    abs.strip_prefix(ws.root())
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| abs.to_string_lossy().to_string())
}

fn scan_dir(ws: &Workspace, dir: &std::path::Path, out: &mut Vec<CsvInfoDto>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.filter_map(|e| e.ok()) {
        let p = e.path();
        if p.extension().map(|x| x == "csv").unwrap_or(false) {
            let info = match rquant::data::reader::read_bars_csv(&p) {
                Ok(bars) if !bars.is_empty() => CsvInfoDto {
                    path: rel_of(ws, &p),
                    rows: Some(bars.len() as u32),
                    first_t: Some(iso(&bars[0].time)),
                    last_t: Some(iso(&bars[bars.len() - 1].time)),
                },
                _ => CsvInfoDto { path: rel_of(ws, &p), rows: None, first_t: None, last_t: None },
            };
            out.push(info);
        }
    }
}

pub fn csv_list(ws: &Workspace) -> Vec<CsvInfoDto> {
    let mut v = Vec::new();
    scan_dir(ws, &ws.paper_dir(), &mut v);
    scan_dir(ws, &ws.data_dir(), &mut v);
    v.sort_by(|a, b| a.path.cmp(&b.path));
    v
}

pub fn read_bars(ws: &Workspace, rel: &str, tail: usize) -> Result<Vec<BarDto>, String> {
    let abs = resolve_under_root(ws, rel)?;
    let bars = rquant::data::reader::read_bars_csv(&abs).map_err(|e| e.to_string())?;
    let take = tail.clamp(1, MAX_TAIL);
    let start = bars.len().saturating_sub(take);
    Ok(bars[start..]
        .iter()
        .map(|b: &Bar| BarDto {
            t: iso(&b.time),
            open: b.open,
            high: b.high,
            low: b.low,
            close: b.close,
            volume: b.volume,
        })
        .collect())
}

/// 因子叠加:对尾部 tail 根 bar 逐点 build_context+eval(标量取值/序列取末位)。
// TODO(M3): cache bars by abs path(同文件反复 read_bars_csv)
pub fn eval_factor(
    ws: &Workspace,
    rel: &str,
    expr_src: &str,
    window: usize,
    tail: usize,
) -> Result<Vec<FactorPointDto>, String> {
    let abs = resolve_under_root(ws, rel)?;
    let expr = rquant::dsl::parser::parse_str(expr_src).map_err(|e| e.to_string())?;
    let bars = rquant::data::reader::read_bars_csv(&abs).map_err(|e| e.to_string())?;
    let take = tail.clamp(1, MAX_TAIL).min(bars.len());
    let start = bars.len() - take;
    let aux = Default::default();
    Ok(bars[start..]
        .iter()
        .map(|b| {
            let ctx =
                rquant::features::context::build_context(&bars, &bars, &[], &aux, b.time, window);
            let v = rquant::dsl::eval::eval(&expr, &ctx).ok().and_then(|val| match val {
                rquant::dsl::eval::Value::Scalar(x) => Some(x),
                rquant::dsl::eval::Value::Series(s) => s.last().copied(),
                rquant::dsl::eval::Value::Bool(x) => Some(if x { 1.0 } else { 0.0 }),
            });
            FactorPointDto { t: iso(&b.time), value: v.filter(|x| x.is_finite()) }
        })
        .collect())
}

fn read_universe_file(
    ws: &Workspace,
    abs: &std::path::Path,
    frozen: bool,
) -> Option<UniverseInfoDto> {
    let txt = std::fs::read_to_string(abs).ok()?;
    let mut lines = txt.lines();
    let header = lines.next()?;
    if !header.trim_start().starts_with("symbol,primary") {
        return None; // 非 universe 形状的 csv 不算
    }
    let entries = lines
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| {
            let mut it = l.split(',');
            Some(UniverseEntryDto {
                symbol: it.next()?.trim().to_string(),
                primary: it.next()?.trim().to_string(),
            })
        })
        .collect();
    Some(UniverseInfoDto {
        path: rel_of(ws, abs),
        name: abs.file_stem().and_then(|s| s.to_str()).unwrap_or("?").to_string(),
        frozen,
        entries,
    })
}

pub fn universe_list(ws: &Workspace) -> Vec<UniverseInfoDto> {
    let mut v = Vec::new();
    for (dir, frozen) in [(ws.deploy_dir(), true), (ws.universes_dir(), false)] {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.filter_map(|e| e.ok()) {
            let p = e.path();
            if p.extension().map(|x| x == "csv").unwrap_or(false)
                && let Some(u) = read_universe_file(ws, &p, frozen)
            {
                v.push(u);
            }
        }
    }
    v.sort_by(|a, b| a.path.cmp(&b.path));
    v
}

/// 自定义清单写入(.rquant-desktop/universes/<name>.csv,原子);name 白名单防穿越。
pub fn universe_write(
    ws: &Workspace,
    name: &str,
    entries: &[UniverseEntryDto],
) -> Result<(), String> {
    if name.is_empty()
        || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
    {
        return Err(format!("invalid universe name: {}", name));
    }
    for e in entries {
        if e.symbol.contains([',', '\n', '\r']) || e.primary.contains([',', '\n', '\r']) {
            return Err("invalid entry: symbol/primary 不得含逗号或换行".to_string());
        }
    }
    let dir = ws.universes_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.csv", name));
    let mut s = String::from("symbol,primary\n");
    for e in entries {
        s.push_str(&format!("{},{}\n", e.symbol, e.primary));
    }
    let tmp = path.with_extension("csv.tmp");
    std::fs::write(&tmp, &s).map_err(|e| e.to_string())?;
    let renamed = std::fs::rename(&tmp, &path);
    if renamed.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    renamed.map_err(|e| e.to_string())
}

/// 批量拉取任务体(重任务;串行+节流;落 .rquant-desktop/data/)。
pub fn fetch_batch(
    ws: &Workspace,
    p: &dyn crate::backtest_run::RunProgress,
    symbols: &[String],
    scale: u32,
    datalen: u32,
    adjust: &str,
) -> Result<serde_json::Value, String> {
    for sym in symbols {
        if !valid_symbol(sym) {
            return Err(format!("invalid symbol: {}", sym));
        }
    }
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(ws.data_dir()).map_err(|e| e.to_string())?;
    let mut written = Vec::new();
    for (i, sym) in symbols.iter().enumerate() {
        if p.cancelled() {
            return Err("cancelled by user".into());
        }
        p.progress(i as f32 / symbols.len() as f32, "fetch", sym);
        let out = ws.data_dir().join(format!("{}_{}_{}.csv", sym, scale, adjust));
        rt.block_on(rquant::cli::run_fetch_to_csv(
            sym,
            scale,
            datalen,
            rquant::cli::SINA_BASE_URL,
            adjust,
            &out,
            None,
        ))
        .map_err(|e| e.to_string())?;
        written.push(rel_of(ws, &out));
        std::thread::sleep(std::time::Duration::from_millis(500)); // sina 节流
    }
    Ok(serde_json::json!({ "written": written }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backtest_run::test_fixtures::write_bars_csv;
    use crate::paths::Workspace;

    fn ws() -> (tempfile::TempDir, Workspace) {
        let td = tempfile::tempdir().unwrap();
        let root = td.path().to_path_buf();
        std::fs::create_dir_all(root.join("paper")).unwrap();
        std::fs::create_dir_all(root.join(".rquant-desktop/data")).unwrap();
        std::fs::create_dir_all(root.join(".rquant-desktop/universes")).unwrap();
        std::fs::create_dir_all(root.join("deploy")).unwrap();
        (td, Workspace::new(root))
    }

    #[test]
    fn csv_list_scans_both_dirs_and_reports_freshness() {
        let (_td, w) = ws();
        write_bars_csv(&w.paper_dir().join("p_x.csv"), 10);
        write_bars_csv(&w.data_dir().join("sh600000_60_qfq.csv"), 20);
        std::fs::write(w.paper_dir().join("broken.csv"), "not,a,bar\n1,2,3\n").unwrap();
        let list = csv_list(&w);
        assert_eq!(list.len(), 3);
        let good = list.iter().find(|c| c.path.ends_with("p_x.csv")).unwrap();
        assert_eq!(good.rows, Some(10));
        assert!(good.last_t.as_deref().unwrap() > good.first_t.as_deref().unwrap());
        let bad = list.iter().find(|c| c.path.ends_with("broken.csv")).unwrap();
        assert!(bad.rows.is_none());
    }

    #[test]
    fn read_bars_rejects_path_escape() {
        let (_td, w) = ws();
        assert!(read_bars(&w, "../outside.csv", 100).is_err());
        assert!(read_bars(&w, "C:/Windows/system.ini", 100).is_err());
    }

    #[test]
    fn read_bars_tails_and_converts() {
        let (_td, w) = ws();
        write_bars_csv(&w.paper_dir().join("p_y.csv"), 30);
        let bars = read_bars(&w, "paper/p_y.csv", 10).unwrap();
        assert_eq!(bars.len(), 10);
        assert!(bars[0].t < bars[9].t);
    }

    #[test]
    fn eval_factor_over_tail() {
        let (_td, w) = ws();
        write_bars_csv(&w.paper_dir().join("p_z.csv"), 30);
        let pts = eval_factor(&w, "paper/p_z.csv", "sma(close, 5)", 20, 10).unwrap();
        assert_eq!(pts.len(), 10);
        assert!(pts.last().unwrap().value.unwrap() > 0.0);
        assert!(eval_factor(&w, "paper/p_z.csv", "not a (((expr", 20, 10).is_err());
    }

    #[test]
    fn universe_write_only_custom_dir_and_roundtrip() {
        let (_td, w) = ws();
        std::fs::write(
            w.deploy_dir().join("universe_10.csv"),
            "symbol,primary\nsh1,paper/a.csv\n",
        )
        .unwrap();
        let entries =
            vec![crate::dto::UniverseEntryDto { symbol: "sh600000".into(), primary: "paper/p.csv".into() }];
        universe_write(&w, "my_list", &entries).unwrap();
        let all = universe_list(&w);
        assert_eq!(all.len(), 2);
        let frozen = all.iter().find(|u| u.frozen).unwrap();
        assert!(frozen.path.starts_with("deploy"));
        let custom = all.iter().find(|u| !u.frozen).unwrap();
        assert_eq!(custom.entries.len(), 1);
        assert!(universe_write(&w, "../evil", &entries).is_err(), "name sanitized");
    }

    #[test]
    fn fetch_batch_rejects_bad_symbols() {
        let (_td, w) = ws();
        use crate::backtest_run::test_fixtures::NoopProgress;
        let bad = vec!["../../evil".to_string()];
        assert!(fetch_batch(&w, &NoopProgress, &bad, 60, 10, "qfq").is_err());
        let bad2 = vec!["sh12345".to_string()]; // 7 位
        assert!(fetch_batch(&w, &NoopProgress, &bad2, 60, 10, "qfq").is_err());
    }

    #[test]
    fn universe_write_rejects_injection() {
        let (_td, w) = ws();
        let evil = vec![crate::dto::UniverseEntryDto {
            symbol: "sh600000,extra".into(),
            primary: "paper/p.csv".into(),
        }];
        assert!(universe_write(&w, "ok_name", &evil).is_err());
    }

}
