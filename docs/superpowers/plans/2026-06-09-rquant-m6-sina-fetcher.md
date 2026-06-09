# rquant M6（新浪 fetcher → 本地 CSV）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新增 `fetch` 子命令，从新浪财经拉 A股 K 线 → 写本地 CSV（`time,open,high,low,close,volume`），供现有 `backtest` 读取；抓取与回测解耦。

**Architecture:** 在 M1–M5（HEAD `28b98f2`）上扩展。`data/sina.rs` 提供纯函数解析 + URL 构造（可单测）与 async 拉取（复用 M5 的 reqwest）；`data/reader.rs` 加 `write_bars_csv`（与 `read_bars_csv` 配对）；`cli` 加 `Fetch` 子命令。零新依赖。

**Tech Stack:** Rust 2024 + 既有（reqwest async / serde_json / csv / chrono / clap / anyhow）。

> 设计依据：`docs/superpowers/specs/2026-06-09-rquant-m6-sina-fetcher-design.md`。
> 提交信息用英文（PowerShell 5.1 中文 git 参数会乱码）。单元测试用同文件 `#[cfg(test)] mod tests`。

---

## 文件结构

```
新增: src/data/sina.rs        # SinaRow / parse_sina_klines / sina_kline_url / fetch_sina_klines
改动: src/data/mod.rs         # + pub mod sina;
改动: src/data/reader.rs      # + write_bars_csv
改动: src/cli/mod.rs          # + Fetch 子命令
改动: README.md               # + 取数一节
```

---

## Task 1: data/sina.rs — 解析新浪 JSON（纯函数）

**Files:**
- Create: `src/data/sina.rs`
- Modify: `src/data/mod.rs`（+ `pub mod sina;`）
- Test: 同文件

- [ ] **Step 1: `src/data/mod.rs` 增加 `pub mod sina;`**

- [ ] **Step 2: 写失败测试（`src/data/sina.rs`）**

```rust
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
        // 升序：14:45 在前
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
```

- [ ] **Step 3: 运行验证失败**

Run: `cargo test --lib data::sina`
Expected: 编译失败（`parse_sina_klines` 未定义）。

- [ ] **Step 4: 写实现（测试上方）**

```rust
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
```

- [ ] **Step 5: 运行验证通过**

Run: `cargo test --lib data::sina`
Expected: 三个测试 PASS。

- [ ] **Step 6: Commit**

```bash
git add src/data/sina.rs src/data/mod.rs
git commit -m "feat(data): parse Sina getKLineData JSON into sorted bars" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: data/reader.rs — write_bars_csv（与 read 配对）

**Files:**
- Modify: `src/data/reader.rs`（加 `write_bars_csv` + 往返测试）
- Test: 同文件

- [ ] **Step 1: 在 `src/data/reader.rs` 的 `mod tests` 内追加失败测试**

```rust
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
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test --lib data::reader::tests::write_then_read_roundtrips`
Expected: 编译失败（`write_bars_csv` 未定义）。

- [ ] **Step 3: 写实现（在 `read_bars_csv` 函数之后、`#[cfg(test)]` 之前追加）**

```rust
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
```

> `Path` 已在 `reader.rs` 顶部 `use std::path::Path;`（read_bars_csv 用）。无需新 import。

- [ ] **Step 4: 运行验证通过**

Run: `cargo test --lib data::reader`
Expected: 既有 3 个 + 新增 1 个 = 4 个 PASS。

- [ ] **Step 5: Commit**

```bash
git add src/data/reader.rs
git commit -m "feat(data): write_bars_csv (round-trips with read_bars_csv)" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: data/sina.rs — URL 构造 + async 拉取

**Files:**
- Modify: `src/data/sina.rs`（加 `sina_kline_url` + `fetch_once` + `fetch_sina_klines`；URL 单测）
- Test: 同文件

- [ ] **Step 1: 在 `mod tests` 内追加失败测试**

```rust
    #[test]
    fn builds_kline_url_trimming_trailing_slash() {
        let u = sina_kline_url("https://x/api/", "sh600000", 15, 1023);
        assert_eq!(
            u,
            "https://x/api/CN_MarketDataService.getKLineData?symbol=sh600000&scale=15&ma=no&datalen=1023"
        );
    }
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test --lib data::sina::tests::builds_kline_url_trimming_trailing_slash`
Expected: 编译失败（`sina_kline_url` 未定义）。

- [ ] **Step 3: 写实现（追加到 `src/data/sina.rs`，`#[cfg(test)]` 之前）**

```rust
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
```

> `fetch_sina_klines`/`fetch_once` 是网络代码，不进 CI（仅编译验证 + 手动 smoke）。解析与 URL 构造已单测覆盖。

- [ ] **Step 4: 运行验证通过 + 构建**

Run: `cargo test --lib data::sina`
Expected: 四个测试 PASS（3 解析 + 1 URL）。

Run: `cargo build`
Expected: async 代码编译通过。

- [ ] **Step 5: Commit**

```bash
git add src/data/sina.rs
git commit -m "feat(data): Sina kline URL builder and async fetch with retries" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: cli `fetch` 子命令 + README

**Files:**
- Modify: `src/cli/mod.rs`（加 `Fetch` 变体 + 分发）
- Modify: `README.md`（加取数一节）

- [ ] **Step 1: 在 `src/cli/mod.rs` 的 `enum Cmd` 中追加 `Fetch` 变体**

在 `Backtest { ... }` 变体之后追加：
```rust
    /// Fetch K-line bars from Sina Finance into a local CSV
    Fetch {
        /// Symbol, e.g. sh600000 / sz000001
        #[arg(long)]
        symbol: String,
        /// K-line scale in minutes: 15, 60, 240 (daily)
        #[arg(long)]
        scale: u32,
        /// Output CSV path
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = 1023)]
        datalen: u32,
        #[arg(long, default_value = "https://money.finance.sina.com.cn/quotes_service/api/json_v2.php")]
        base_url: String,
    },
```

- [ ] **Step 2: 在 `main` 的 `match cli.cmd { ... }` 中追加 `Fetch` 分支**

在 `Cmd::Backtest { ... } => { ... }` 之后追加：
```rust
        Cmd::Fetch { symbol, scale, out, datalen, base_url } => {
            let http = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()?;
            let bars = crate::data::sina::fetch_sina_klines(&http, &base_url, &symbol, scale, datalen, 2).await?;
            crate::data::reader::write_bars_csv(&bars, &out)?;
            println!("wrote {} bars to {}", bars.len(), out.display());
        }
```

- [ ] **Step 3: 验证构建 + 帮助**

Run: `cargo build`
Expected: 编译通过。

Run: `cargo run -- fetch --help`
Expected: 打印 fetch 用法（含 `--symbol`、`--scale`、`--out`、`--datalen`、`--base-url`）。

Run: `cargo test`
Expected: 全量仍全绿（本任务不加测试，但不能弄坏既有）。

- [ ] **Step 4: 在 `README.md` 末尾追加一节**

```markdown
## 取数（新浪 fetcher）

从新浪财经拉 A股 K 线到本地 CSV（再喂给 backtest）：

    # 小周期 15min
    cargo run --release -- fetch --symbol sh600000 --scale 15 --out 15m.csv
    # 大周期 1h
    cargo run --release -- fetch --symbol sh600000 --scale 60 --out 1h.csv

`--symbol` 形如 `sh600000`(沪) / `sz000001`(深)；`--scale` 为分钟数（15/60/240=日线）；`--datalen` 最多 1023（新浪只给最近这么多根，浅历史）。端点可用 `--base-url` 覆盖。

抓取与回测解耦：fetch 出 CSV 后，照常 `cargo run -- backtest --primary 15m.csv --context 1h.csv ...`。
```

- [ ] **Step 5: Commit**

```bash
git add src/cli/mod.rs README.md
git commit -m "feat(cli): fetch subcommand pulling Sina klines to CSV; README" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## 附录 A：Spec 覆盖对照（自检）

| Spec 章节 | 实现于 |
|---|---|
| §4 组件（sina.rs parse+fetch / reader write / cli Fetch）| Task 1/3 / Task 2 / Task 4 |
| §5 新浪端点契约（URL/参数/字符串字段/升序）| Task 1（解析+升序）+ Task 3（URL）|
| §6 类型契约（parse/fetch/write 签名）| Task 1/2/3 |
| §7 错误处理（http/json/空/parse/io）| Task 1（json/parse）+ Task 3（http/空/retry）|
| §8 测试（解析+往返单测，网络手动）| Task 1/2/3 |
| §10 里程碑 M6.1–M6.4 | Task 1/2/3/4 |

## 附录 B：明确不在范围（YAGNI / 后置）
- Parquet/SQLite 缓存层；任意历史区间；多 symbol/scale 批量；回测自动抓取；增量合并/去重历史；复权。
- 真实网络 CI 测试（`fetch_sina_klines`/`fetch_once` 仅编译验证 + README 手动 smoke）。
- 日线 date-only `day` 兼容（M6 面向 intraday 含时分秒；如需后续在 parse 兼容）。
