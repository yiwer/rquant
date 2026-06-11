# rquant F-7b 复权数据通路（fetch --adjust qfq）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `fetch --adjust qfq`：日线直取腾讯前复权；分钟线三源合成（新浪 raw 分钟 × 腾讯日线 qfq/raw 因子），除息假跳空消除。

**Architecture:** 在 master(HEAD `27591cb`)上新增 `data/tencent.rs`（fqkline URL/解析/重试，⚠️ 行格式 `[day, open, close, high, low, volume]` 与新浪 OHLC 顺序不同）与 `data/adjust.rs`（因子=qfq_close/raw_close、阶梯前值回退、起点前报错）；CLI fetch 臂按 `--adjust`×`scale` 分流。spec：`docs/superpowers/specs/2026-06-11-rquant-f7b-adjust-design.md`。

**Tech Stack:** Rust 2024 + 既有（reqwest/serde_json/chrono）。

---

## 文件结构
```
新增: src/data/tencent.rs   # tencent_fqkline_url / parse_tencent_klines / fetch_tencent_daily
新增: src/data/adjust.rs    # adjust_factors / apply_factors（纯函数+黄金测试）
改动: src/data/mod.rs       # + pub mod tencent; pub mod adjust;
改动: src/cli/mod.rs        # Fetch 臂 + --adjust 分流
改动: docs/cli-reference.md、README.md
```

---

## Task 1: tencent.rs（URL/解析）

**Files:**
- Create: `src/data/tencent.rs`；Modify: `src/data/mod.rs`

- [ ] **Step 1: RED 测试**

```rust
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
```

- [ ] **Step 2: 实现**

```rust
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
```
`src/data/mod.rs` 加 `pub mod tencent;`。

- [ ] **Step 3: GREEN + clippy + Commit**

```bash
git add src/data/tencent.rs src/data/mod.rs
git commit -m "feat(data): tencent fqkline parser (qfq daily source, field-order trap pinned)" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: adjust.rs（因子纯函数）

**Files:**
- Create: `src/data/adjust.rs`；Modify: `src/data/mod.rs`（+ `pub mod adjust;`）

- [ ] **Step 1: RED 黄金测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn bar(d: (i32, u32, u32), h: u32, mi: u32, px: f64) -> Bar {
        let time = NaiveDate::from_ymd_opt(d.0, d.1, d.2).unwrap().and_hms_opt(h, mi, 0).unwrap();
        Bar { time, open: px, high: px, low: px, close: px, volume: 100.0 }
    }

    #[test]
    fn golden_factor_step_and_propagation() {
        // 除息发生在 d2 开盘前：raw d1=10/d2=10；qfq d1=9.5/d2=10 → 因子 {d1:0.95, d2:1.0}
        let raw = vec![bar((2025, 7, 4), 15, 0, 10.0), bar((2025, 7, 7), 15, 0, 10.0)];
        let qfq = vec![bar((2025, 7, 4), 15, 0, 9.5), bar((2025, 7, 7), 15, 0, 10.0)];
        let f = adjust_factors(&raw, &qfq).unwrap();
        assert_eq!(f.len(), 2);
        assert!((f[&NaiveDate::from_ymd_opt(2025, 7, 4).unwrap()] - 0.95).abs() < 1e-12);
        assert!((f[&NaiveDate::from_ymd_opt(2025, 7, 7).unwrap()] - 1.0).abs() < 1e-12);
        // 分钟传播：d1 close 10.2 → ×0.95；d2 不变；周末日(7-5/7-6)回退 d1 因子
        let mins = vec![
            bar((2025, 7, 4), 14, 30, 10.2),
            bar((2025, 7, 5), 10, 0, 10.1), // 假想缺因子日 → 前值 0.95
            bar((2025, 7, 7), 10, 0, 9.5),
        ];
        let adj = apply_factors(&mins, &f).unwrap();
        assert!((adj[0].close - 10.2 * 0.95).abs() < 1e-12);
        assert!((adj[1].close - 10.1 * 0.95).abs() < 1e-12);
        assert!((adj[2].close - 9.5).abs() < 1e-12);
        assert_eq!(adj[0].volume, 100.0); // volume 不动
        // 隔夜跳空消除：raw 跳空 (9.5/10.0−1)=−5% → 调整后 d1 末 close 10.0×0.95=9.5 vs d2 9.5 → 0%
        let d1_last = 10.0 * 0.95;
        assert!((adj[2].open / d1_last - 1.0).abs() < 1e-12);
    }

    #[test]
    fn errors_on_no_overlap_and_pre_start_bar() {
        let raw = vec![bar((2025, 7, 4), 15, 0, 10.0)];
        let qfq = vec![bar((2025, 8, 4), 15, 0, 9.5)];
        assert!(adjust_factors(&raw, &qfq).is_err()); // 无交集
        let qfq_same = vec![bar((2025, 7, 4), 15, 0, 9.5)];
        let f = adjust_factors(&raw, &qfq_same).unwrap();
        assert!(apply_factors(&[bar((2025, 7, 1), 10, 0, 10.0)], &f).is_err()); // 早于起点
    }

    #[test]
    fn errors_on_nonpositive_factor() {
        let raw = vec![bar((2025, 7, 4), 15, 0, 0.0)];
        let qfq = vec![bar((2025, 7, 4), 15, 0, 9.5)];
        assert!(adjust_factors(&raw, &qfq).is_err());
    }
}
```

- [ ] **Step 2: 实现**

```rust
use crate::data::bar::Bar;
use crate::{Error, Result};
use chrono::NaiveDate;
use std::collections::BTreeMap;

/// 按日期交集对齐：factor(d) = qfq_close(d) / raw_close(d)。
/// 空交集 / raw close ≤ 0 / 因子非有限或 ≤ 0 → Error（拒绝静默错数据）。
pub fn adjust_factors(raw_daily: &[Bar], qfq_daily: &[Bar]) -> Result<BTreeMap<NaiveDate, f64>> {
    let raw: BTreeMap<NaiveDate, f64> = raw_daily.iter().map(|b| (b.time.date(), b.close)).collect();
    let mut out = BTreeMap::new();
    for b in qfq_daily {
        let d = b.time.date();
        if let Some(rc) = raw.get(&d) {
            if *rc <= 0.0 {
                return Err(Error::Data(format!("adjust: raw close <= 0 on {d}")));
            }
            let f = b.close / rc;
            if !f.is_finite() || f <= 0.0 {
                return Err(Error::Data(format!("adjust: bad factor {f} on {d}")));
            }
            out.insert(d, f);
        }
    }
    if out.is_empty() {
        return Err(Error::Data("adjust: no overlapping dates between raw and qfq daily".into()));
    }
    Ok(out)
}

/// 逐 bar 乘当日因子（OHLC；volume 不动）。缺因子日回退最近前值
/// （复权因子是阶梯函数，前值语义正确）；早于因子表起点 → Error。
pub fn apply_factors(bars: &[Bar], factors: &BTreeMap<NaiveDate, f64>) -> Result<Vec<Bar>> {
    let mut out = Vec::with_capacity(bars.len());
    for b in bars {
        let d = b.time.date();
        let f = factors
            .range(..=d)
            .next_back()
            .map(|(_, f)| *f)
            .ok_or_else(|| Error::Data(format!("adjust: bar date {d} earlier than factor table start")))?;
        out.push(Bar {
            time: b.time,
            open: b.open * f,
            high: b.high * f,
            low: b.low * f,
            close: b.close * f,
            volume: b.volume,
        });
    }
    Ok(out)
}
```

- [ ] **Step 3: GREEN + clippy + Commit**

```bash
git add src/data/adjust.rs src/data/mod.rs
git commit -m "feat(data): qfq adjustment factors (step function, forward-fill, golden gap-elimination test)" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: fetch_tencent_daily 重试 + CLI --adjust 编排

**Files:**
- Modify: `src/data/tencent.rs`（追加 fetch_tencent_daily）、`src/cli/mod.rs`（Fetch 臂）

- [ ] **Step 1: fetch_tencent_daily（READ `src/data/sina.rs` 的 fetch 重试函数先，mirror 其结构/重试次数/错误风格）**

```rust
/// 拉腾讯日线（带重试，mirror sina）。end=本地今日、start=end−ceil(datalen×1.7) 天（覆盖节假日）。
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
    // 重试循环 mirror sina（同次数；每次 fetch_once 文本 → parse_tencent_klines；
    // 解析失败视为可重试错误；全部失败 → Error::Data("tencent fetch failed after retries: ...")）
    ...
}
```

- [ ] **Step 2: CLI（READ 现 Fetch 臂先；保持既有路径逐字不动）**

`Cmd::Fetch` 加：
```rust
        /// Price adjustment: none (raw, default) or qfq (forward-adjusted via Tencent daily)
        #[arg(long, default_value = "none")]
        adjust: String,
```
分流（解构加 `adjust`；伪码中"现状新浪路径"指当前已有代码）：
```rust
            if adjust != "none" && adjust != "qfq" {
                return Err(anyhow::anyhow!("--adjust must be 'none' or 'qfq'"));
            }
            let bars = if adjust == "qfq" {
                use crate::data::tencent::{fetch_tencent_daily, TENCENT_FQKLINE_BASE};
                if scale == 240 {
                    fetch_tencent_daily(&http, TENCENT_FQKLINE_BASE, &symbol, datalen, "qfq").await?
                } else {
                    // 三源合成：因子表天数 = 分钟 bar 覆盖天数 + 30 裕量（240/scale = bars/日）
                    let daily_len = (datalen * scale / 240 + 30).min(1023);
                    let raw_min = /* 现状新浪路径取分钟 bars */;
                    let raw_d = fetch_tencent_daily(&http, TENCENT_FQKLINE_BASE, &symbol, daily_len, "").await?;
                    let qfq_d = fetch_tencent_daily(&http, TENCENT_FQKLINE_BASE, &symbol, daily_len, "qfq").await?;
                    let factors = crate::data::adjust::adjust_factors(&raw_d, &qfq_d)?;
                    eprintln!("[rquant] qfq synthesis: {} factor days x {} intraday bars", factors.len(), raw_min.len());
                    crate::data::adjust::apply_factors(&raw_min, &factors)?
                }
            } else {
                /* 现状新浪路径，逐字不动 */
            };
            /* 现状 CSV 写出 */
```

- [ ] **Step 3: `cargo test` 全绿 + clippy + `fetch --help` 含 --adjust + Commit**

```bash
git add src/data/tencent.rs src/cli/mod.rs
git commit -m "feat(cli,data): fetch --adjust qfq (Tencent daily direct; intraday three-source synthesis)" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: 文档 + 真数据 smoke

**Files:**
- Modify: `docs/cli-reference.md`、`README.md`

- [ ] **Step 1: 文档**

- cli-reference fetch 节：`--adjust` 行 + 三源合成原理两句 + 数据源表（新浪=分钟 raw / 腾讯=日线 qfq）。
- README 数据获取节 + **诚实边界**：前复权锚定最新（重新拉取后历史整体重标，新旧 CSV 不可混用）；腾讯日线 volume 单位（手）与新浪不同（引擎内 volume 仅作相对量）；建议回测一律 `--adjust qfq`。

- [ ] **Step 2: 真数据 smoke（手动，产物不入库）**

```powershell
mkdir tmpfq
cargo run -q -- fetch --symbol sh601088 --scale 240 --adjust qfq --out tmpfq/d.csv
# 断言 2025-07-07 隔夜跳空消失（raw 为 −5.51%）：
# awk 扫 open/prev_close（同 F-7a 方法）→ 该日 |gap| < 1%
cargo run -q -- fetch --symbol sh601088 --scale 60 --adjust qfq --out tmpfq/m60.csv
# 60m：除权日（如 2025-07-07）首 bar open vs 前日末 bar close 平滑
Remove-Item -Recurse -Force tmpfq
```
把扫出的数字记入完成报告。

- [ ] **Step 3: 全绿 + Commit**

```bash
git add docs/cli-reference.md README.md
git commit -m "docs: --adjust qfq reference, data-source table, honesty notes" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## 附录 A：Spec 覆盖对照（自检）
| Spec § | 实现于 |
|---|---|
| §3.1 tencent.rs（含字段序陷阱/多余元素/15:00 约定）| Task 1 |
| §3.2 adjust.rs（因子/前值回退/起点报错/校验）| Task 2 |
| §3.3 CLI 分流 + fetch_tencent_daily 重试 | Task 3 |
| §4 诚实边界文档 | Task 4 |
| §5 测试（解析/黄金/跳空消除/非法 --adjust/真数据）| Task 1-4 |

## 附录 B：明确不在范围（YAGNI）
- hfq；分红明细自算；指数特判；本地缓存/增量；腾讯 base_url CLI 暴露。
