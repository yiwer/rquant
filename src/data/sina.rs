use crate::data::bar::Bar;
use crate::{Error, Result};
use chrono::NaiveDateTime;

#[derive(serde::Deserialize)]
struct SinaRow {
    day: String,
    open: String,
    high: String,
    low: String,
    close: String,
    volume: String,
}

/// 解析新浪 getKLineData 返回的 JSON 数组 → 按 time 升序的 Bar 列表。
/// 新浪字段均为字符串；day 形如 "2024-01-02 15:00:00"（intraday）。
pub fn parse_sina_klines(json: &str) -> Result<Vec<Bar>> {
    let rows: Vec<SinaRow> = serde_json::from_str(json.trim())
        .map_err(|e| Error::Data(format!("sina bad json: {e}")))?;
    let mut bars: Vec<Bar> = Vec::with_capacity(rows.len());
    for r in rows {
        let time = NaiveDateTime::parse_from_str(&r.day, "%Y-%m-%d %H:%M:%S")
            .map_err(|e| Error::Data(format!("sina bad day '{}': {e}", r.day)))?;
        let num = |s: &str, field: &str| -> Result<f64> {
            s.parse::<f64>()
                .map_err(|e| Error::Data(format!("sina bad {field} '{s}': {e}")))
        };
        bars.push(Bar {
            time,
            open: num(&r.open, "open")?,
            high: num(&r.high, "high")?,
            low: num(&r.low, "low")?,
            close: num(&r.close, "close")?,
            volume: num(&r.volume, "volume")?,
        });
    }
    bars.sort_by_key(|b| b.time);
    Ok(bars)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_sorts_ascending() {
        let json = r#"[
          {"day":"2024-01-02 15:00:00","open":"10.0","high":"10.5","low":"9.8","close":"10.2","volume":"1000"},
          {"day":"2024-01-02 14:45:00","open":"9.9","high":"10.1","low":"9.7","close":"10.0","volume":"900"}
        ]"#;
        let bars = parse_sina_klines(json).unwrap();
        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0].close, 10.0);
        assert_eq!(bars[0].volume, 900.0);
        assert_eq!(bars[1].close, 10.2);
        assert!(bars[0].time < bars[1].time);
    }

    #[test]
    fn empty_array_is_empty_vec() {
        assert!(parse_sina_klines("[]").unwrap().is_empty());
    }

    #[test]
    fn bad_json_errors() {
        assert!(parse_sina_klines("not json").is_err());
    }
}
