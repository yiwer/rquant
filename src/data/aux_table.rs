use crate::{Error, Result};
use chrono::NaiveDateTime;
use std::collections::BTreeMap;
use std::path::Path;

/// 通用外部序列表：time + 任意数值列（列名即 DSL 字段名）。
pub struct AuxTable {
    pub times: Vec<NaiveDateTime>,
    pub cols: BTreeMap<String, Vec<f64>>,
}

fn parse_aux_time(s: &str) -> Result<NaiveDateTime> {
    if let Ok(t) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Ok(t);
    }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Ok(d.and_hms_opt(0, 0, 0).unwrap());
    }
    Err(Error::Data(format!("bad aux time '{s}'")))
}

/// 读通用 aux CSV：首列必须 time（带时分秒或日频）；时间严格递增；其余列 f64。
pub fn read_aux_csv(path: &Path) -> Result<AuxTable> {
    let mut rdr = csv::Reader::from_path(path)?;
    let headers = rdr.headers()?.clone();
    if headers.is_empty() || &headers[0] != "time" {
        return Err(Error::Data("aux csv first column must be 'time'".into()));
    }
    let names: Vec<String> = headers.iter().skip(1).map(|h| h.trim().to_string()).collect();
    for n in &names {
        if n.is_empty() || n.contains('.') || n.contains(char::is_whitespace) {
            return Err(Error::Data(format!("bad aux column name '{n}'")));
        }
    }
    let mut times = Vec::new();
    let mut cols: BTreeMap<String, Vec<f64>> = names.iter().map(|n| (n.clone(), Vec::new())).collect();
    for rec in rdr.records() {
        let rec = rec?;
        let t = parse_aux_time(&rec[0])?;
        if let Some(last) = times.last()
            && *last >= t
        {
            return Err(Error::Data(format!("aux non-increasing time at {t}")));
        }
        times.push(t);
        for (j, n) in names.iter().enumerate() {
            let raw = &rec[j + 1];
            let v: f64 = raw.trim().parse().map_err(|_| Error::Data(format!("bad aux value '{raw}' in column '{n}'")))?;
            cols.get_mut(n).unwrap().push(v);
        }
    }
    Ok(AuxTable { times, cols })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_tmp(content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn reads_multi_column_and_daily_format() {
        let f = write_tmp("time,netbuy,pe\n2024-01-02,1.5,12.0\n2024-01-03 10:00:00,-0.5,12.1\n");
        let t = read_aux_csv(f.path()).unwrap();
        assert_eq!(t.times.len(), 2);
        assert_eq!(t.times[0], chrono::NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(0, 0, 0).unwrap());
        assert_eq!(t.cols["netbuy"], vec![1.5, -0.5]);
        assert_eq!(t.cols["pe"], vec![12.0, 12.1]);
    }

    #[test]
    fn rejects_bad_inputs() {
        // 非递增
        assert!(read_aux_csv(write_tmp("time,v\n2024-01-03,1\n2024-01-02,2\n").path()).is_err());
        // 坏数值
        assert!(read_aux_csv(write_tmp("time,v\n2024-01-02,abc\n").path()).is_err());
        // 首列非 time
        assert!(read_aux_csv(write_tmp("t,v\n2024-01-02,1\n").path()).is_err());
        // 列名含点
        assert!(read_aux_csv(write_tmp("time,a.b\n2024-01-02,1\n").path()).is_err());
        // 坏时间
        assert!(read_aux_csv(write_tmp("time,v\nnot-a-date,1\n").path()).is_err());
    }
}
