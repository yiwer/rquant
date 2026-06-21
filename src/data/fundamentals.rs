//! 逐股基本面时点序列：行=季报（按公告日升序），as_of(t) 取公告日≤t 的最近一行。
//! point-in-time 命根：首份财报公告前 → 空（DSL fund.* = NaN 弃权）。
//! 也接受 features_15m 格式（首列 datetime "YYYY-MM-DD HH:MM:SS"）：同日多行取最后一行（末柱快照）。

use crate::{Error, Result};
use chrono::{NaiveDate, NaiveDateTime};
use std::collections::BTreeMap;
use std::path::Path;

/// 一只股的基本面时点序列。announce_dates 升序；cols 各列与 announce_dates 等长对齐。
#[derive(Debug, Clone, Default)]
pub struct FundamentalSeries {
    pub announce_dates: Vec<NaiveDate>,
    pub cols: BTreeMap<String, Vec<f64>>,
}

impl FundamentalSeries {
    /// 公告日 ≤ t 的最近一行快照；无（首报前）→ 空 map。空单元 → 该列缺该值（不放入 map）。
    pub fn as_of(&self, t: NaiveDateTime) -> BTreeMap<String, f64> {
        let d = t.date();
        let cut = self.announce_dates.partition_point(|x| *x <= d);
        if cut == 0 {
            return BTreeMap::new();
        }
        let idx = cut - 1;
        let mut out = BTreeMap::new();
        for (c, v) in &self.cols {
            let val = v[idx];
            if val.is_finite() {
                out.insert(c.clone(), val);
            }
        }
        out
    }
}

/// 载基本面/因子 CSV：首列 time=`%Y-%m-%d` 或 `%Y-%m-%d %H:%M:%S`，其余为数值列；空单元 → NaN。
/// - 季报格式（date-only key，须严格升序）：标准点时序列。
/// - 日内因子格式（datetime key，每日多行）：同日多行取末行（末柱快照）；日间须升序。
pub fn load_fundamentals_csv(path: &Path) -> Result<FundamentalSeries> {
    let mut rdr = csv::Reader::from_path(path)?;
    let headers = rdr.headers()?.clone();
    if headers.is_empty() || &headers[0] != "time" {
        return Err(Error::Data("fundamentals csv must start with 'time' column".into()));
    }
    let col_names: Vec<String> = headers.iter().skip(1).map(|s| s.to_string()).collect();
    let mut dates: Vec<NaiveDate> = Vec::new();
    let mut cols: BTreeMap<String, Vec<f64>> = col_names.iter().map(|c| (c.clone(), Vec::new())).collect();
    for rec in rdr.records() {
        let rec = rec?;
        let raw = rec[0].trim();
        // Accept both date-only ("YYYY-MM-DD") and datetime ("YYYY-MM-DD HH:MM:SS") keys.
        let d = NaiveDate::parse_from_str(raw, "%Y-%m-%d")
            .or_else(|_| NaiveDate::parse_from_str(raw.get(..10).unwrap_or(raw), "%Y-%m-%d"))
            .map_err(|e| Error::Data(format!("fundamentals bad date '{}': {e}", raw)))?;
        // Parse values for this row.
        let mut row_vals: Vec<f64> = Vec::with_capacity(col_names.len());
        for (i, c) in col_names.iter().enumerate() {
            let cell = rec.get(i + 1).unwrap_or("").trim();
            let val = if cell.is_empty() { f64::NAN } else {
                cell.parse::<f64>().map_err(|e| Error::Data(format!("fundamentals bad number '{cell}': {e}")))?
            };
            let _ = c; // suppress unused warning; indexed below
            row_vals.push(val);
        }
        if dates.last() == Some(&d) {
            // Same date as previous row (intraday multi-bar): overwrite last row (last-bar-of-day wins).
            let last_idx = dates.len() - 1;
            for (i, c) in col_names.iter().enumerate() {
                cols.get_mut(c).unwrap()[last_idx] = row_vals[i];
            }
        } else {
            // New date: must be strictly after last date.
            if let Some(last) = dates.last()
                && d < *last
            {
                return Err(Error::Data(format!("fundamentals time not strictly increasing at {d}")));
            }
            dates.push(d);
            for (i, c) in col_names.iter().enumerate() {
                cols.get_mut(c).unwrap().push(row_vals[i]);
            }
        }
    }
    Ok(FundamentalSeries { announce_dates: dates, cols })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use std::io::Write;

    fn dt(y: i32, m: u32, d: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(0, 0, 0).unwrap()
    }
    fn wf(s: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(s.as_bytes()).unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn as_of_point_in_time_gate() {
        let f = wf("time,roe,eps\n2024-04-27,34.1,8.05\n2024-08-20,18.0,4.2\n");
        let s = load_fundamentals_csv(f.path()).unwrap();
        assert!(s.as_of(dt(2024, 4, 26)).is_empty());
        assert_eq!(s.as_of(dt(2024, 4, 27)).get("roe").copied(), Some(34.1));
        assert_eq!(s.as_of(dt(2024, 8, 19)).get("roe").copied(), Some(34.1));
        assert_eq!(s.as_of(dt(2024, 8, 20)).get("roe").copied(), Some(18.0));
        assert_eq!(s.as_of(dt(2025, 1, 1)).get("eps").copied(), Some(4.2));
    }

    #[test]
    fn empty_cell_is_absent() {
        let f = wf("time,roe,eps\n2024-04-27,,8.05\n");
        let s = load_fundamentals_csv(f.path()).unwrap();
        let snap = s.as_of(dt(2024, 5, 1));
        assert!(!snap.contains_key("roe"));
        assert_eq!(snap.get("eps").copied(), Some(8.05));
    }

    #[test]
    fn rejects_non_increasing_time() {
        let f = wf("time,roe\n2024-08-20,1.0\n2024-04-27,2.0\n");
        assert!(load_fundamentals_csv(f.path()).is_err());
    }
}
