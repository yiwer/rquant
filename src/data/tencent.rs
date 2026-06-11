use crate::data::bar::Bar;
use crate::{Error, Result};
use chrono::NaiveDateTime;

/// 腾讯 fqkline 端点（日线复权数据源；2026-06 验证可用）。
pub const TENCENT_FQKLINE_BASE: &str = "https://web.ifzq.gtimg.cn/appstock/app/fqkline/get";

/// 构造请求 URL（adjust: "qfq" 前复权 / "" 不复权）。
pub fn tencent_fqkline_url(
    base_url: &str,
    symbol: &str,
    start: &str,
    end: &str,
    count: u32,
    adjust: &str,
) -> String {
    format!("{base_url}?param={symbol},day,{start},{end},{count},{adjust}")
}

/// 解析腾讯 fqkline 响应 → 升序 Bar。
/// ⚠️ 行格式 [day, open, close, high, low, volume]（与新浪 open/high/low/close 顺序不同）。
/// date-only day → 15:00:00（收盘，与 sina 日线约定一致）。行尾多余元素容忍。
pub fn parse_tencent_klines(json: &str, symbol: &str, adjust: &str) -> Result<Vec<Bar>> {
    let v: serde_json::Value = serde_json::from_str(json.trim())
        .map_err(|e| Error::Data(format!("tencent bad json: {e}")))?;
    let data = v
        .get("data")
        .and_then(|d| d.get(symbol))
        .ok_or_else(|| Error::Data(format!("tencent: no data for '{symbol}'")))?;
    let key = if adjust == "qfq" { "qfqday" } else { "day" };
    let rows = data
        .get(key)
        .or_else(|| data.get("day"))
        .or_else(|| data.get("qfqday"))
        .and_then(|r| r.as_array())
        .ok_or_else(|| Error::Data(format!("tencent: no kline array under '{key}'")))?;
    let mut bars: Vec<Bar> = Vec::with_capacity(rows.len());
    for row in rows {
        let arr = row
            .as_array()
            .ok_or_else(|| Error::Data("tencent: row is not an array".into()))?;
        if arr.len() < 6 {
            return Err(Error::Data(format!("tencent: row too short ({} fields)", arr.len())));
        }
        let s = |i: usize, field: &str| -> Result<&str> {
            arr[i]
                .as_str()
                .ok_or_else(|| Error::Data(format!("tencent: {field} is not a string")))
        };
        let day = s(0, "day")?;
        let time = NaiveDateTime::parse_from_str(day, "%Y-%m-%d %H:%M:%S")
            .or_else(|_| {
                chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d")
                    .map(|d| d.and_hms_opt(15, 0, 0).expect("15:00:00 is valid"))
            })
            .map_err(|e| Error::Data(format!("tencent bad day '{day}': {e}")))?;
        let num = |i: usize, field: &str| -> Result<f64> {
            let raw = s(i, field)?;
            raw.parse::<f64>()
                .map_err(|e| Error::Data(format!("tencent bad {field} '{raw}': {e}")))
        };
        bars.push(Bar {
            time,
            open: num(1, "open")?,
            close: num(2, "close")?,
            high: num(3, "high")?,
            low: num(4, "low")?,
            volume: num(5, "volume")?,
        });
    }
    bars.sort_by_key(|b| b.time);
    Ok(bars)
}

/// HTTP text fetch (one attempt) — shared by retry loop.
async fn fetch_once_tencent(http: &reqwest::Client, url: &str) -> Result<String> {
    let resp = http
        .get(url)
        .send()
        .await
        .map_err(|e| Error::Data(format!("tencent request error: {e}")))?;
    if !resp.status().is_success() {
        return Err(Error::Data(format!("tencent http status {}", resp.status())));
    }
    resp.text()
        .await
        .map_err(|e| Error::Data(format!("tencent read body: {e}")))
}

/// 拉腾讯日线（带重试，mirror sina）。
/// end = 本地今日；start = end − ceil(datalen×1.7) 自然日（覆盖节假日空隙）。
pub async fn fetch_tencent_daily(
    http: &reqwest::Client,
    base_url: &str,
    symbol: &str,
    datalen: u32,
    adjust: &str,
) -> Result<Vec<Bar>> {
    let end = chrono::Local::now().date_naive();
    let start = end - chrono::Days::new((datalen as f64 * 1.7).ceil() as u64);
    let url = tencent_fqkline_url(
        base_url,
        symbol,
        &start.format("%Y-%m-%d").to_string(),
        &end.format("%Y-%m-%d").to_string(),
        datalen,
        adjust,
    );
    // 重试循环 mirror sina（3 次；解析失败/网络错误均可重试；全部失败 → Error::Data）
    let max_retries = 2u32;
    let mut last = String::from("no attempt");
    for _ in 0..=max_retries {
        match fetch_once_tencent(http, &url).await {
            Ok(body) => match parse_tencent_klines(&body, symbol, adjust) {
                Ok(bars) => return Ok(bars),
                Err(e) => last = e.to_string(),
            },
            Err(e) => last = e.to_string(),
        }
    }
    Err(Error::Data(format!("tencent fetch failed after retries: {last}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    // 真实响应削减样本（2026-06-11 捕获，qfq）
    const SAMPLE_QFQ: &str = r#"{"code":0,"msg":"","data":{"sh601398":{"qfqday":[
      ["2025-06-09","6.625","6.605","6.635","6.565","2913685.000"],
      ["2025-06-10","6.605","6.625","6.715","6.595","4161322.000"]]}}}"#;

    #[test]
    fn parses_qfq_rows_with_tencent_field_order() {
        let bars = parse_tencent_klines(SAMPLE_QFQ, "sh601398", "qfq").unwrap();
        assert_eq!(bars.len(), 2);
        // ⚠️ 腾讯顺序 [day, open, close, high, low, volume]
        let b = &bars[0];
        assert_eq!(b.open, 6.625);
        assert_eq!(b.close, 6.605);
        assert_eq!(b.high, 6.635);
        assert_eq!(b.low, 6.565);
        assert_eq!(b.volume, 2913685.0);
        assert_eq!(
            b.time,
            chrono::NaiveDate::from_ymd_opt(2025, 6, 9).unwrap().and_hms_opt(15, 0, 0).unwrap()
        );
        assert!(bars[0].time < bars[1].time);
    }

    #[test]
    fn raw_key_day_and_extra_row_elements_tolerated() {
        let json = r#"{"code":0,"msg":"","data":{"sh601088":{"day":[
          ["2025-07-04","37.160","37.810","37.900","37.000","350000.000",{"nd":"1"}]]}}}"#;
        let bars = parse_tencent_klines(json, "sh601088", "").unwrap();
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].close, 37.81);
    }

    #[test]
    fn errors_on_missing_symbol_bad_price_short_row() {
        assert!(parse_tencent_klines(r#"{"data":{}}"#, "sh600000", "qfq").is_err());
        let bad = r#"{"data":{"sh600000":{"qfqday":[["2025-06-09","x","1","1","1","1"]]}}}"#;
        assert!(parse_tencent_klines(bad, "sh600000", "qfq").is_err());
        let short = r#"{"data":{"sh600000":{"qfqday":[["2025-06-09","1","1"]]}}}"#;
        assert!(parse_tencent_klines(short, "sh600000", "qfq").is_err());
    }

    #[test]
    fn url_shape() {
        let u = tencent_fqkline_url(TENCENT_FQKLINE_BASE, "sh601398", "2024-01-01", "2026-06-11", 500, "qfq");
        assert_eq!(u, "https://web.ifzq.gtimg.cn/appstock/app/fqkline/get?param=sh601398,day,2024-01-01,2026-06-11,500,qfq");
    }
}
