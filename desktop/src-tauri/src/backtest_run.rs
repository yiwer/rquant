//! 回测执行任务体:可选 fetch → 构造 BacktestConfig(out/traces 指进 run 目录) →
//! 按 mode 调引擎 → 桥接写 config/meta。引擎语义零触碰。
//!
//! Step-1 调研结论(2026-06-13):
//!   run_sim   → std::fs::write(&cfg.out_path, json)   — 引擎自写
//!   run_soft  → write_soft_report(&report, &cfg.out_path) — 引擎自写
//!   run()     → write_report(&report, &cfg.out_path)   — 引擎自写
//!   三路均自写 out_path。因此 persist_if_needed 简化为存在性校验(幂等 no-op):
//!   不重复落盘，仅从返回值提取 tree_name 供 meta.json 使用。
use crate::dto::BacktestConfigDto;
use crate::paths::Workspace;
use crate::runs;

/// 进度抽象:TaskCtx 在 commands 侧适配;测试用 NoopProgress。
pub trait RunProgress {
    fn progress(&self, pct: f32, stage: &str, detail: &str);
    fn cancelled(&self) -> bool;
}

impl RunProgress for crate::tasks::TaskCtx {
    fn progress(&self, pct: f32, stage: &str, detail: &str) {
        crate::tasks::TaskCtx::progress(self, pct, stage, detail)
    }
    fn cancelled(&self) -> bool {
        crate::tasks::TaskCtx::cancelled(self)
    }
}

pub fn execute_backtest(
    ws: &Workspace,
    p: &dyn RunProgress,
    cfg: &BacktestConfigDto,
) -> Result<serde_json::Value, String> {
    match cfg.mode.as_str() {
        "sim_hard" | "sim_soft" | "score_hard" | "score_soft" => {}
        m => return Err(format!("unknown mode: {}", m)),
    }
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let llm =
        rquant::cli::build_llm(String::new(), String::new(), ws.root().join(".rquant-cache").join("llm"))
            .map_err(|e| e.to_string())?;

    let id = runs::new_run_id();
    let rp = runs::run_paths(ws, &id);
    std::fs::create_dir_all(&rp.dir).map_err(|e| e.to_string())?;

    // ── 可选 fetch ───────────────────────────────────────────────────────────
    let mut effective = cfg.clone();
    if let Some(f) = &cfg.fetch {
        if p.cancelled() {
            return Err("cancelled by user".into());
        }
        p.progress(0.05, "fetch", &f.symbol);
        let out_rel = format!(
            ".rquant-desktop/data/{}_{}_{}.csv",
            f.symbol, f.scale, f.adjust
        );
        let out_abs = ws.root().join(&out_rel);
        std::fs::create_dir_all(out_abs.parent().expect("data dir has parent"))
            .map_err(|e| e.to_string())?;
        rt.block_on(rquant::cli::run_fetch_to_csv(
            &f.symbol,
            f.scale,
            f.datalen,
            rquant::cli::SINA_BASE_URL,
            &f.adjust,
            &out_abs,
        ))
        .map_err(|e| e.to_string())?;
        effective.primary_path = out_rel;
    }

    // ── 构造引擎配置(out/traces 指进 run 目录) ────────────────────────────────
    let primary_abs = ws.root().join(&effective.primary_path);
    let engine_cfg = rquant::backtest::runner::BacktestConfig {
        tree_path: ws.root().join(&effective.tree_path),
        primary_path: primary_abs.clone(),
        context_path: primary_abs,
        news_path: None,
        out_path: rp.result_json.clone(),
        traces_path: Some(rp.traces_jsonl.clone()),
        cost_bps: effective.cost_bps,
        warmup: effective.warmup as usize,
        window: effective.window as usize,
        concurrency: 4,
        holidays_path: None,
        folds: 0,
        aux_paths: Vec::new(),
        decision_traces_path: if effective.mode == "sim_hard" {
            Some(rp.decision_jsonl.clone())
        } else {
            None
        },
    };

    if p.cancelled() {
        return Err("cancelled by user".into());
    }
    p.progress(0.3, "run", &effective.mode);

    // ── 调引擎;tree_name 从返回结构体取 ──────────────────────────────────────
    // 引擎三路均自写 out_path(Step-1 调研证实)。persist_if_needed 是存在性校验 no-op。
    let run_outcome: Result<String, String> = match effective.mode.as_str() {
        "sim_hard" => rt
            .block_on(rquant::backtest::sim::run_sim(&engine_cfg, &llm, false))
            .map(|r| r.tree_name)
            .map_err(|e| e.to_string()),
        "sim_soft" => rt
            .block_on(rquant::backtest::sim::run_sim(&engine_cfg, &llm, true))
            .map(|r| r.tree_name)
            .map_err(|e| e.to_string()),
        "score_hard" => rt
            .block_on(rquant::backtest::runner::run(&engine_cfg, &llm))
            .map(|r| r.tree_name)
            .map_err(|e| e.to_string()),
        "score_soft" => rt
            .block_on(rquant::backtest::soft::run_soft(&engine_cfg, &llm))
            .map(|r| r.tree_name)
            .map_err(|e| e.to_string()),
        _ => unreachable!("validated above"),
    };

    // ── 落 config + meta(成败都留痕) ──────────────────────────────────────────
    runs::write_config(ws, &id, &effective).map_err(|e| e.to_string())?;
    let primary_file = std::path::Path::new(&effective.primary_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("?")
        .to_string();
    let base_meta = crate::dto::RunMetaDto {
        id: id.clone(),
        kind: effective.mode.clone(),
        name: String::new(),
        tree_name: String::new(),
        created: chrono::Local::now()
            .naive_local()
            .format("%Y-%m-%dT%H:%M:%S")
            .to_string(),
        ok: run_outcome.is_ok(),
        error: run_outcome.as_ref().err().cloned(),
    };
    let meta = match &run_outcome {
        Ok(tree_name) => crate::dto::RunMetaDto {
            name: format!("{} × {}", tree_name, primary_file),
            tree_name: tree_name.clone(),
            ..base_meta
        },
        Err(_) => crate::dto::RunMetaDto {
            name: format!("(失败) × {}", primary_file),
            ..base_meta
        },
    };
    runs::write_meta(ws, &meta).map_err(|e| e.to_string())?;

    run_outcome?;
    p.progress(0.95, "archive", &id);
    Ok(serde_json::json!({ "run_id": id }))
}

/// 引擎自写 out_path 已证实:本函数为纯存在性校验——验证引擎确实写出了文件。
/// 若文件不存在说明引擎内部失败(error 已经被 ? 传播到上层)，不重复落盘。
#[allow(dead_code)]
fn persist_if_needed(path: &std::path::Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("engine did not write result to {}", path.display()));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod test_fixtures {
    use super::*;

    /// 合成 n 根上升 60m bar CSV(表头与 read_bars_csv 兼容:time,open,high,low,close,volume)。
    pub(crate) fn write_bars_csv(path: &std::path::Path, n: usize) {
        let mut s = String::from("time,open,high,low,close,volume\n");
        for i in 0..n {
            let day = 1 + i / 4;
            let hour = 10 + (i % 4);
            let px = 10.0 + i as f64 * 0.1;
            s.push_str(&format!(
                "2026-01-{:02} {:02}:00:00,{:.2},{:.2},{:.2},{:.2},1000\n",
                day,
                hour,
                px,
                px + 0.05,
                px - 0.05,
                px
            ));
        }
        std::fs::write(path, s).unwrap();
    }

    pub(crate) const MINI_TREE: &str = r#"
meta: { name: "m2-mini", forward_window: 4, stances: [long, flat] }
root: r
nodes:
  r:
    type: quant
    branches:
      - when: "close > sma(close, 5)"
        goto: l
        label: above_ma
    default: { goto: f, label: below_ma }
leaves:
  l: { stance: long, weight: 1.0 }
  f: { stance: flat }
"#;

    pub(crate) fn fixture_ws() -> (tempfile::TempDir, Workspace) {
        let td = tempfile::tempdir().unwrap();
        let root = td.path().to_path_buf();
        std::fs::create_dir_all(root.join("examples")).unwrap();
        std::fs::create_dir_all(root.join("data_in")).unwrap();
        std::fs::write(root.join("examples/mini.yaml"), MINI_TREE).unwrap();
        write_bars_csv(&root.join("data_in/bars.csv"), 40);
        (td, Workspace::new(root))
    }

    pub(crate) fn cfg(mode: &str) -> BacktestConfigDto {
        BacktestConfigDto {
            tree_path: "examples/mini.yaml".into(),
            primary_path: "data_in/bars.csv".into(),
            mode: mode.into(),
            cost_bps: 10.0,
            warmup: 10,
            window: 20,
            initial_capital: 100000.0,
            fetch: None,
        }
    }

    pub(crate) struct NoopProgress;
    impl RunProgress for NoopProgress {
        fn progress(&self, _pct: f32, _stage: &str, _detail: &str) {}
        fn cancelled(&self) -> bool {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_fixtures::{cfg, fixture_ws, NoopProgress};

    #[test]
    fn sim_hard_run_produces_full_archive() {
        let (_td, w) = fixture_ws();
        let out = execute_backtest(&w, &NoopProgress, &cfg("sim_hard")).unwrap();
        let id = out["run_id"].as_str().unwrap();
        let rp = crate::runs::run_paths(&w, id);
        assert!(rp.config_json.exists());
        assert!(rp.meta_json.exists());
        assert!(rp.result_json.exists());
        assert!(rp.traces_jsonl.exists());
        assert!(rp.decision_jsonl.exists(), "sim_hard must emit decision traces");
        let meta = crate::runs::read_meta(&w, id).unwrap();
        assert!(meta.ok);
        assert_eq!(meta.kind, "sim_hard");
        assert_eq!(meta.tree_name, "m2-mini");
    }

    #[test]
    fn score_hard_run_archives_without_decision_file() {
        let (_td, w) = fixture_ws();
        let out = execute_backtest(&w, &NoopProgress, &cfg("score_hard")).unwrap();
        let id = out["run_id"].as_str().unwrap();
        let rp = crate::runs::run_paths(&w, id);
        assert!(rp.result_json.exists());
        assert!(rp.traces_jsonl.exists(), "score traces are Trace jsonl");
        assert!(!rp.decision_jsonl.exists());
    }

    #[test]
    fn bad_mode_rejected() {
        let (_td, w) = fixture_ws();
        assert!(execute_backtest(&w, &NoopProgress, &cfg("nonsense")).is_err());
    }
}
