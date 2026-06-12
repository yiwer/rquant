//! run.log 解析: 段落以 "==== " 开头行分隔 (deploy/paper_run.cmd 的 echo 格式)。
use crate::dto::RunlogStatusDto;
use crate::paths::Workspace;

pub fn classify(log: &str) -> RunlogStatusDto {
    let mut sections: Vec<(String, Vec<&str>)> = Vec::new();
    for line in log.lines() {
        if line.starts_with("==== ") {
            sections.push((line.to_string(), Vec::new()));
        } else if let Some((_, body)) = sections.last_mut() {
            body.push(line);
        }
    }
    let Some((header, body)) = sections.last() else {
        return RunlogStatusDto { last_header: None, ok: None, summary: "run.log 为空或不存在".into() };
    };
    let text = body.join("\n");
    let lower = text.to_lowercase();
    let bad = lower.contains("error") || lower.contains("panic");
    let finished = text.contains("committed state") || text.contains("[DRY RUN]");
    let ok = !bad && finished;
    let summary = if bad {
        format!("最近一次 run 含错误行:{}", body.iter().find(|l| l.to_lowercase().contains("error")).unwrap_or(&""))
    } else if finished {
        "最近一次 run 正常收尾".to_string()
    } else {
        "最近一次 run 无收尾标记(可能中断)".to_string()
    };
    RunlogStatusDto { last_header: Some(header.clone()), ok: Some(ok), summary }
}

pub fn tail_lines(log: &str, n: usize) -> String {
    let lines: Vec<&str> = log.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

pub fn read_status(ws: &Workspace) -> RunlogStatusDto {
    let log = std::fs::read_to_string(ws.run_log_path()).unwrap_or_default();
    classify(&log)
}

pub fn read_tail(ws: &Workspace, n: usize) -> String {
    let log = std::fs::read_to_string(ws.run_log_path()).unwrap_or_default();
    tail_lines(&log, n)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "\
==== Thu 06/11/2026 15:35:00.10 ====
fetched 1023 bars for sh600030
=== rquant SIGNAL (single) @ 2026-06-11 15:00:00 ===
committed state to paper\\paper_sh600030.json
==== Fri 06/12/2026 14:14:34.12 ====
fetched 1023 bars for sh600030
=== rquant SIGNAL (single) @ 2026-06-12 15:00:00 ===
[DRY RUN] 未落盘 state；加 --commit 提交
";

    #[test]
    fn splits_sections_by_marker_and_takes_latest() {
        let st = classify(LOG);
        assert_eq!(st.last_header.as_deref(), Some("==== Fri 06/12/2026 14:14:34.12 ===="));
        assert_eq!(st.ok, Some(true)); // DRY 收尾也算正常
    }

    #[test]
    fn error_section_flags_not_ok() {
        let log = "==== Fri 06/12/2026 15:35:00.00 ====\nerror: data error: bad csv\n";
        let st = classify(log);
        assert_eq!(st.ok, Some(false));
        assert!(st.summary.contains("error"));
    }

    #[test]
    fn empty_log_is_none() {
        let st = classify("");
        assert_eq!(st.ok, None);
    }

    #[test]
    fn tail_returns_last_n_lines() {
        let t = tail_lines(LOG, 2);
        assert_eq!(t.lines().count(), 2);
        assert!(t.contains("DRY RUN"));
    }
}
