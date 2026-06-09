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

/// 构造新浪 getKLineData 请求 URL（确定性，便于单测）。
pub fn sina_kline_url(base_url: &str, symbol: &str, scale: u32, datalen: u32) -> String {
    format!(
        "{}/CN_MarketDataService.getKLineData?symbol={}&scale={}&ma=no&datalen={}",
        base_url.trim_end_matches('/'),
        symbol,
        scale,
        datalen
    )
}

async fn fetch_once(http: &reqwest::Client, url: &str) -> Result<String> {
    let resp = http
        .get(url)
        .send()
        .await
        .map_err(|e| Error::Data(format!("sina request error: {e}")))?;
    if !resp.status().is_success() {
        return Err(Error::Data(format!("sina http status {}", resp.status())));
    }
    resp.text()
        .await
        .map_err(|e| Error::Data(format!("sina read body: {e}")))
}

/// 从新浪拉最近 datalen 根 K 线（带重试）。空结果（错误 symbol/无数据）报错。
pub async fn fetch_sina_klines(
    http: &reqwest::Client,
    base_url: &str,
    symbol: &str,
    scale: u32,
    datalen: u32,
    max_retries: u32,
) -> Result<Vec<Bar>> {
    let url = sina_kline_url(base_url, symbol, scale, datalen);
    let mut last = String::from("no attempt");
    for _ in 0..=max_retries {
        match fetch_once(http, &url).await {
            Ok(body) => {
                let bars = parse_sina_klines(&body)?;
                if bars.is_empty() {
                    return Err(Error::Data(format!("sina returned no bars for {symbol}")));
                }
                return Ok(bars);
            }
            Err(e) => last = e.to_string(),
        }
    }
    Err(Error::Data(format!("sina fetch failed after retries: {last}")))
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

    #[test]
    fn rejects_bad_float_field() {
        let json = r#"[{"day":"2024-01-02 15:00:00","open":"not-a-number","high":"10.5","low":"9.8","close":"10.2","volume":"1000"}]"#;
        assert!(parse_sina_klines(json).is_err());
    }

    #[test]
    fn builds_kline_url_trimming_trailing_slash() {
        let u = sina_kline_url("https://x/api/", "sh600000", 15, 1023);
        assert_eq!(
            u,
            "https://x/api/CN_MarketDataService.getKLineData?symbol=sh600000&scale=15&ma=no&datalen=1023"
        );
    }
}
