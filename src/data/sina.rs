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
/// 新浪字段均为字符串；day 形如 "2024-01-02 15:00:00"（intraday）
/// 或 "2024-01-02"（scale=240 日线）——后者按当日 15:00:00（收盘）记时间，
/// 避免 00:00:00 让 time<=t 闸门在开盘前就"看到"当日收盘（未来函数）。
pub fn parse_sina_klines(json: &str) -> Result<Vec<Bar>> {
    let rows: Vec<SinaRow> = serde_json::from_str(json.trim())
        .map_err(|e| Error::Data(format!("sina bad json: {e}")))?;
    let mut bars: Vec<Bar> = Vec::with_capacity(rows.len());
    for r in rows {
        let time = NaiveDateTime::parse_from_str(&r.day, "%Y-%m-%d %H:%M:%S")
            .or_else(|_| {
                chrono::NaiveDate::parse_from_str(&r.day, "%Y-%m-%d")
                    .map(|d| d.and_hms_opt(15, 0, 0).expect("15:00:00 is valid"))
            })
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

/// 一次尝试的分类：成功非空=Done；合法但空=Empty（不重试，通常是错误 symbol）；
/// 网络错误或解析失败（截断/坏 body）=Retry（可重试）。
enum Attempt {
    Done(Vec<Bar>),
    Empty,
    Retry(String),
}

fn classify(attempt: Result<String>) -> Attempt {
    match attempt {
        Ok(body) => match parse_sina_klines(&body) {
            Ok(bars) if bars.is_empty() => Attempt::Empty,
            Ok(bars) => Attempt::Done(bars),
            Err(e) => Attempt::Retry(e.to_string()),
        },
        Err(e) => Attempt::Retry(e.to_string()),
    }
}

/// 从新浪拉最近 datalen 根 K 线（带重试：网络错误与截断/坏 body 都重试；
/// 合法但空的结果不重试，直接报错）。
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
        match classify(fetch_once(http, &url).await) {
            Attempt::Done(bars) => return Ok(bars),
            Attempt::Empty => {
                return Err(Error::Data(format!("sina returned no bars for {symbol}")));
            }
            Attempt::Retry(e) => last = e,
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
    fn parses_daily_date_only_rows_as_1500() {
        // scale=240（日线）的 day 无时间部分 → 解析为当日 15:00:00（收盘，避免 00:00 引入未来函数）
        let json = r#"[
          {"day":"2025-03-17","open":"6.750","high":"6.830","low":"6.740","close":"6.810","volume":"358619828"},
          {"day":"2025-03-18","open":"6.830","high":"6.840","low":"6.770","close":"6.800","volume":"236952810"}
        ]"#;
        let bars = parse_sina_klines(json).unwrap();
        assert_eq!(bars.len(), 2);
        assert_eq!(
            bars[0].time,
            chrono::NaiveDate::from_ymd_opt(2025, 3, 17)
                .unwrap()
                .and_hms_opt(15, 0, 0)
                .unwrap()
        );
        assert_eq!(bars[0].close, 6.81);
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
    fn classify_truncated_body_is_retryable() {
        // 截断/坏 body → 可重试（本次修复核心）
        assert!(matches!(classify(Ok("truncated not json".to_string())), Attempt::Retry(_)));
    }

    #[test]
    fn classify_network_error_is_retryable() {
        assert!(matches!(classify(Err(Error::Data("net".into()))), Attempt::Retry(_)));
    }

    #[test]
    fn classify_valid_empty_is_not_retried() {
        assert!(matches!(classify(Ok("[]".to_string())), Attempt::Empty));
    }

    #[test]
    fn classify_good_body_is_done() {
        let json = r#"[{"day":"2024-01-02 15:00:00","open":"1","high":"1","low":"1","close":"1","volume":"1"}]"#;
        match classify(Ok(json.to_string())) {
            Attempt::Done(bars) => assert_eq!(bars.len(), 1),
            _ => panic!("expected Done"),
        }
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
