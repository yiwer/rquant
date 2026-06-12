//! schtasks /query 包装——任务缺失/解析失败一律 None(驾驶舱降级显示)。
use crate::dto::SchtaskDto;

pub fn parse_schtasks_csv(csv_text: &str) -> Option<SchtaskDto> {
    let mut rdr = csv::Reader::from_reader(csv_text.as_bytes());
    let headers = rdr.headers().ok()?.clone();
    let find = |name: &str| headers.iter().position(|h| h == name);
    let (i_next, i_status, i_last, i_res) =
        (find("Next Run Time")?, find("Status")?, find("Last Run Time")?, find("Last Result")?);
    let rec = rdr.records().next()?.ok()?;
    let get = |i: usize| rec.get(i).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    Some(SchtaskDto { next_run: get(i_next), last_run: get(i_last), last_result: get(i_res), status: get(i_status) })
}

/// 实时查询(测试不调用;commands 层用)。
pub fn query(task_name: &str) -> Option<SchtaskDto> {
    let out = std::process::Command::new("schtasks")
        .args(["/query", "/tn", task_name, "/fo", "csv", "/v"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_schtasks_csv(&String::from_utf8_lossy(&out.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CSV: &str = "\
\"HostName\",\"TaskName\",\"Next Run Time\",\"Status\",\"Last Run Time\",\"Last Result\"
\"HOST\",\"\\rquant-paper\",\"6/12/2026 3:35:00 PM\",\"Ready\",\"11/30/1999 12:00:00 AM\",\"267011\"
";

    #[test]
    fn parses_columns_by_header_name() {
        let dto = parse_schtasks_csv(CSV).unwrap();
        assert_eq!(dto.next_run.as_deref(), Some("6/12/2026 3:35:00 PM"));
        assert_eq!(dto.status.as_deref(), Some("Ready"));
        assert_eq!(dto.last_result.as_deref(), Some("267011"));
    }

    #[test]
    fn garbage_returns_none() {
        assert!(parse_schtasks_csv("not,a,real,header\n").is_none());
    }
}
