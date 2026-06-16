//! 时变 universe 成员（survivorship-free top-N membership）：按再平衡日升序的成员快照，
//! `effective_at(t)` 取 ≤t 的最近一期——point-in-time，t 时刻只见已生效名单（不累积）。
use crate::{Error, Result};
use chrono::{NaiveDate, NaiveDateTime};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct Membership {
    snapshots: Vec<(NaiveDate, BTreeSet<String>)>,
}

impl Membership {
    /// 从 long 格式 CSV 加载（表头 `date,symbol`；date=`%Y-%m-%d`）。
    /// 同 date 多行聚为一期成员集；快照按日期升序。
    pub fn load_csv(path: &Path) -> Result<Self> {
        let mut rdr = csv::Reader::from_path(path)?;
        let headers = rdr.headers()?.clone();
        if headers.len() < 2 || &headers[0] != "date" || &headers[1] != "symbol" {
            return Err(Error::Data("membership csv must have columns: date,symbol".into()));
        }
        let mut by_date: BTreeMap<NaiveDate, BTreeSet<String>> = BTreeMap::new();
        for rec in rdr.records() {
            let rec = rec?;
            let d = NaiveDate::parse_from_str(rec[0].trim(), "%Y-%m-%d")
                .map_err(|e| Error::Data(format!("membership: bad date '{}': {e}", &rec[0])))?;
            let sym = rec[1].trim().to_string();
            if sym.is_empty() {
                return Err(Error::Data("membership: empty symbol".into()));
            }
            by_date.entry(d).or_default().insert(sym);
        }
        Ok(Membership { snapshots: by_date.into_iter().collect() })
    }

    /// 生效成员集 = 再平衡日 ≤ t.date() 的最近一期；t 早于首期 → None。
    pub fn effective_at(&self, t: NaiveDateTime) -> Option<&BTreeSet<String>> {
        let d = t.date();
        let i = self.snapshots.partition_point(|(snap_d, _)| *snap_d <= d);
        if i == 0 { None } else { Some(&self.snapshots[i - 1].1) }
    }

    /// 无任何快照。
    pub fn is_empty(&self) -> bool { self.snapshots.is_empty() }
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
    fn dt(y: i32, m: u32, d: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(15, 0, 0).unwrap()
    }

    #[test]
    fn load_groups_by_date() {
        let f = write_tmp("date,symbol\n2018-02-28,sh600000\n2018-01-31,sz000001\n2018-01-31,sh600000\n");
        let m = Membership::load_csv(f.path()).unwrap();
        let jan = m.effective_at(dt(2018, 1, 31)).unwrap();
        assert_eq!(jan.len(), 2);
        assert!(jan.contains("sh600000") && jan.contains("sz000001"));
    }

    #[test]
    fn effective_at_is_point_in_time_non_cumulative() {
        let f = write_tmp("date,symbol\n2018-01-31,A\n2018-02-28,B\n");
        let m = Membership::load_csv(f.path()).unwrap();
        assert!(m.effective_at(dt(2018, 1, 1)).is_none()); // t 早于首期
        let s = m.effective_at(dt(2018, 2, 10)).unwrap();   // [1-31,2-28)
        assert!(s.contains("A") && !s.contains("B"));
        let s2 = m.effective_at(dt(2018, 3, 1)).unwrap();    // ≥2-28：最近一期，不累积
        assert!(s2.contains("B") && !s2.contains("A"));
    }

    #[test]
    fn rejects_bad_header() {
        let f = write_tmp("d,s\n2018-01-31,A\n");
        assert!(Membership::load_csv(f.path()).is_err());
    }
}
