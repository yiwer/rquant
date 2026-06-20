//! 迭代命令层:ledger/queue 读取 + 轮次卡片 + iterate.py 启动器。
//! 零业务逻辑——解析委托 iter_read;长任务经 TaskRegistry 重槽。
use crate::commands::AppState;
use crate::dto_iter::*;
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};

#[tauri::command]
pub fn iter_ledger(state: tauri::State<AppState>) -> Vec<LedgerRoundDto> {
    let txt = std::fs::read_to_string(state.ws.ledger_jsonl()).unwrap_or_default();
    let mut v = crate::iter_read::parse_ledger(&txt);
    v.sort_by(|a, b| b.round.cmp(&a.round));
    v
}

#[tauri::command]
pub fn iter_queue(state: tauri::State<AppState>) -> IterQueueDto {
    let md = std::fs::read_to_string(state.ws.ledger_md()).unwrap_or_default();
    crate::iter_read::parse_queue(&md)
}

#[tauri::command]
pub fn iter_round_card(state: tauri::State<AppState>, round: i32) -> Result<RoundCardDto, String> {
    let txt = std::fs::read_to_string(state.ws.ledger_jsonl()).map_err(|e| e.to_string())?;
    let r = crate::iter_read::parse_ledger(&txt)
        .into_iter()
        .find(|x| x.round == round)
        .ok_or_else(|| format!("ledger 无轮次 {round}"))?;
    let side = std::fs::read_to_string(state.ws.iter_dir().join(format!("round_{round}.json")))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
    let tier2: Vec<Tier2CellDto> = side
        .as_ref()
        .and_then(|v| v.get("tier2").cloned())
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let config_path = side
        .as_ref()
        .and_then(|v| v.get("config_path"))
        .and_then(|x| x.as_str())
        .map(String::from)
        .unwrap_or_else(|| format!("examples/screen/iter/{}.yaml", r.label));
    Ok(crate::iter_read::round_card(&r, tier2, config_path))
}

#[tauri::command]
pub fn iter_run_round(
    state: tauri::State<AppState>,
    config: String,
    note: String,
    axis: String,
    top: u32,
    benchmark: String,
    rebalance: u32,
) -> Result<String, String> {
    let ws = state.ws.clone();
    state.tasks.start("iter_round", true, move |ctx| {
        let py = crate::paths::python_exe();
        let cmdline = format!(
            "{py} scripts/iterate.py {config} --note {note} --axis {axis} --top {top} --benchmark {benchmark} --rebalance {rebalance}"
        );
        ctx.note_params(serde_json::json!({
            "config": &config,
            "note": &note,
            "axis": &axis,
            "top": top,
            "benchmark": &benchmark,
            "rebalance": rebalance,
            "cmdline": &cmdline,
        }));
        log::info!("iter_run_round: {cmdline}");

        let mut cmd = Command::new(&py);
        cmd.current_dir(ws.root())
            .arg("scripts/iterate.py")
            .arg(&config)
            .arg("--note")
            .arg(&note)
            .arg("--axis")
            .arg(&axis)
            .arg("--top")
            .arg(top.to_string())
            .arg("--benchmark")
            .arg(&benchmark)
            .arg("--rebalance")
            .arg(rebalance.to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        ctx.progress(0.05, "启动", "iterate.py");
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("启动 Python 失败(确认已装 Python 与依赖): {e}"))?;

        // Spawn a thread to drain stderr concurrently to avoid pipe-deadlock if
        // the child fills the pipe buffer before stdout is fully consumed.
        let stderr_handle = child.stderr.take().map(|stderr| {
            std::thread::spawn(move || {
                let mut s = String::new();
                let _ = BufReader::new(stderr).read_to_string(&mut s);
                s
            })
        });

        if let Some(out) = child.stdout.take() {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if ctx.cancelled() {
                    let _ = child.kill();
                    return Err("cancelled".into());
                }
                ctx.progress(0.5, "运行", &line);
            }
        }

        let status = child.wait().map_err(|e| e.to_string())?;

        // Collect full stderr now that the child has exited and the thread can finish.
        let stderr_output = stderr_handle
            .and_then(|h| h.join().ok())
            .unwrap_or_default();

        if !status.success() {
            return Err(format!(
                "iterate.py 退出码 {:?}: {}\n命令: {}",
                status.code(),
                stderr_output,
                cmdline,
            ));
        }

        let txt = std::fs::read_to_string(ws.ledger_jsonl()).unwrap_or_default();
        let last = crate::iter_read::parse_ledger(&txt)
            .into_iter()
            .max_by_key(|r| r.round);
        ctx.progress(0.98, "完成", "");
        if let Some(ref r) = last {
            ctx.note_summary(&format!("round {} {}", r.round, r.label));
        }
        serde_json::to_value(last).map_err(|e| e.to_string())
    })
}
