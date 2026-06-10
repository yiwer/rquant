use crate::data::bar::Bar;
use crate::{Error, Result};
use chrono::NaiveDateTime;
use std::path::Path;

#[derive(serde::Deserialize)]
struct Row {
    time: String,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
}

/// 读取本地 CSV 为按时间升序的 Bar 列表，并做基本校验：
/// 时间严格递增、high >= low。
pub fn read_bars_csv(path: &Path) -> Result<Vec<Bar>> {
    let mut rdr = csv::Reader::from_path(path)?;
    let mut bars: Vec<Bar> = Vec::new();
    for rec in rdr.deserialize() {
        let row: Row = rec?;
        let time = NaiveDateTime::parse_from_str(&row.time, "%Y-%m-%d %H:%M:%S")
            .map_err(|e| Error::Data(format!("bad time '{}': {e}", row.time)))?;
        let bar = Bar {
            time,
            open: row.open,
            high: row.high,
            low: row.low,
            close: row.close,
            volume: row.volume,
        };
        if bar.high < bar.low {
            return Err(Error::Data(format!("high < low at {time}")));
        }
        if let Some(prev) = bars.last()
            && time <= prev.time
        {
            return Err(Error::Data(format!("non-increasing time at {time}")));
        }
        bars.push(bar);
    }
    Ok(bars)
}

/// 把 Bar 列表写成 read_bars_csv 可读的 CSV（time 用 %Y-%m-%d %H:%M:%S）。
/// f64 用 Display 输出（Rust 保证最短可往返表示），写出再读回得到相同 f64。
pub fn write_bars_csv(bars: &[Bar], path: &Path) -> Result<()> {
    use std::io::Write;
    let mut f = std::fs::File::create(path)?;
    writeln!(f, "time,open,high,low,close,volume")?;
    for b in bars {
        writeln!(
            f,
            "{},{},{},{},{},{}",
            b.time.format("%Y-%m-%d %H:%M:%S"),
            b.open,
            b.high,
            b.low,
            b.close,
            b.volume
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_csv(content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        write!(f, "{}", content).unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn reads_valid_csv() {
        let f = write_csv(
            "time,open,high,low,close,volume\n\
             2024-01-02 09:45:00,10.0,10.5,9.8,10.2,1000\n\
             2024-01-02 10:00:00,10.2,10.6,10.1,10.4,1200\n",
        );
        let bars = read_bars_csv(f.path()).unwrap();
        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0].close, 10.2);
        assert_eq!(bars[1].volume, 1200.0);
    }

    #[test]
    fn rejects_non_increasing_time() {
        let f = write_csv(
            "time,open,high,low,close,volume\n\
             2024-01-02 10:00:00,10.0,10.5,9.8,10.2,1000\n\
             2024-01-02 09:45:00,10.2,10.6,10.1,10.4,1200\n",
        );
        assert!(read_bars_csv(f.path()).is_err());
    }

    #[test]
    fn rejects_high_below_low() {
        let f = write_csv(
            "time,open,high,low,close,volume\n\
             2024-01-02 09:45:00,10.0,9.0,9.8,10.2,1000\n",
        );
        assert!(read_bars_csv(f.path()).is_err());
    }

    // M3 — bad time in bars CSV
    #[test]
    fn rejects_bad_time_in_bars_csv() {
        let f = write_csv(
            "time,open,high,low,close,volume\n\
             not-a-date,10.0,10.5,9.8,10.2,1000\n",
        );
        assert!(read_bars_csv(f.path()).is_err());
    }

    #[test]
    fn write_then_read_roundtrips() {
        use crate::data::bar::Bar;
        use chrono::NaiveDate;
        let mk = |h: u32, m: u32, o: f64, hi: f64, lo: f64, c: f64, v: f64| Bar {
            time: NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(h, m, 0).unwrap(),
            open: o, high: hi, low: lo, close: c, volume: v,
        };
        let bars = vec![
            mk(9, 45, 10.0, 10.5, 9.8, 10.2, 1000.0),
            mk(10, 0, 10.2, 10.6, 10.1, 10.4, 1200.0),
        ];
        let f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        write_bars_csv(&bars, f.path()).unwrap();
        let back = read_bars_csv(f.path()).unwrap();
        assert_eq!(back, bars);
    }
}
