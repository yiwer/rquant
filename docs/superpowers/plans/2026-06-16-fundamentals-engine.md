# 基本面进引擎（子项①）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring A-share fundamentals (point-in-time, via akshare) into the engine as a per-stock `fund.<col>` DSL namespace, and validate on the existing 20 that fundamental factors predict forward returns.

**Architecture:** Python akshare pipeline (`stock_yjbb_em` → per-stock CSVs keyed by announcement date 公告日) + a new `src/data/fundamentals.rs` (`FundamentalSeries` with as-of-t lookup) + `UniverseEntry` 4th column + `build_context` resolving an as-of-t snapshot into `Context.fundamentals` + an `eval::resolve_series` `fund.` branch (NaN before first filing = abstain). Validation reuses `rquant factor`.

**Tech Stack:** Rust 2024 (data/dsl/features), Python 3.13 + akshare (data pipeline only; engine stays Rust). Spec: `docs/superpowers/specs/2026-06-15-fundamentals-engine-design.md`.

---

## Reuse reference (exact, current code)

`src/data/universe.rs`: `pub struct UniverseEntry { pub symbol: String, pub primary: PathBuf, pub context: PathBuf }`; `read_universe_csv` checks header `symbol,primary[,context]`, `has_ctx = headers.len()>=3 && headers[2]=="context"`.

`src/features/context.rs`: `Context { t, primary: Window, context: Window, news: Option<NewsView>, aux: BTreeMap<String,AuxView>, sim: SimState, eval_cache: RefCell<...> }`. `build_context(primary:&[Bar], context:&[Bar], news:&[NewsRecord], aux:&BTreeMap<String,AuxTable>, t:NaiveDateTime, window:usize) -> Context`. aux gated via `table.times.partition_point(|x| *x <= t)`.

`src/dsl/eval.rs:186` `fn resolve_series(name:&str, ctx:&Context) -> Result<Vec<f64>>`: first branch `if let Some(rest)=name.strip_prefix("aux.") { … ctx.aux.get(table)… }` then `ctx.`/bare. Scalar context takes the series' last element; NaN compares false (abstain).

**`build_context` call sites (grep) — ALL must get the new `fundamentals` arg:**
`src/backtest/runner.rs:66`, `src/backtest/soft.rs:133`, `src/backtest/sim.rs:419`, `src/backtest/portfolio.rs:96`, `src/optimize/mod.rs:78/118/162`, `src/factor/mod.rs:204`, `src/signal/mod.rs:286/338`, `src/screen/mod.rs:84`, `src/tree/loader.rs:1217`(test), `src/features/context.rs` tests (124/135/153/175/178), **`desktop/src-tauri/src/data_bench.rs:96`**, **`desktop/src-tauri/src/replay.rs:102`**. Plus `Context { … }` struct literals in `src/dsl/eval.rs` (~569/928/955) need the new field.

---

## Task FE-1: `FundamentalSeries` loader + as-of-t lookup

**Files:**
- Create: `src/data/fundamentals.rs`
- Modify: `src/data/mod.rs` (add `pub mod fundamentals;`)

- [ ] **Step 1: Add module declaration**

In `src/data/mod.rs`, add (with the other `pub mod` lines): `pub mod fundamentals;`

- [ ] **Step 2: Write `src/data/fundamentals.rs`**

```rust
//! 逐股基本面时点序列：行=季报（按公告日升序），as_of(t) 取公告日≤t 的最近一行。
//! point-in-time 命根：首份财报公告前 → 空（DSL fund.* = NaN 弃权）。

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

/// 载财务 CSV：首列 time=`%Y-%m-%d`（公告日，须严格升序），其余为数值列；空单元 → NaN。
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
        let d = NaiveDate::parse_from_str(rec[0].trim(), "%Y-%m-%d")
            .map_err(|e| Error::Data(format!("fundamentals bad date '{}': {e}", &rec[0])))?;
        if let Some(last) = dates.last() {
            if d <= *last {
                return Err(Error::Data(format!("fundamentals time not strictly increasing at {d}")));
            }
        }
        dates.push(d);
        for (i, c) in col_names.iter().enumerate() {
            let cell = rec.get(i + 1).unwrap_or("").trim();
            let val = if cell.is_empty() { f64::NAN } else {
                cell.parse::<f64>().map_err(|e| Error::Data(format!("fundamentals bad number '{cell}': {e}")))?
            };
            cols.get_mut(c).unwrap().push(val);
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
        // 首报前 → 空（弃权）
        assert!(s.as_of(dt(2024, 4, 26)).is_empty());
        // 公告日当天/之后 → 该行
        assert_eq!(s.as_of(dt(2024, 4, 27)).get("roe").copied(), Some(34.1));
        assert_eq!(s.as_of(dt(2024, 8, 19)).get("roe").copied(), Some(34.1)); // 仍取 Q1（Q2 未公告）
        assert_eq!(s.as_of(dt(2024, 8, 20)).get("roe").copied(), Some(18.0)); // Q2 已公告
        assert_eq!(s.as_of(dt(2025, 1, 1)).get("eps").copied(), Some(4.2));
    }

    #[test]
    fn empty_cell_is_absent() {
        let f = wf("time,roe,eps\n2024-04-27,,8.05\n");
        let s = load_fundamentals_csv(f.path()).unwrap();
        let snap = s.as_of(dt(2024, 5, 1));
        assert!(!snap.contains_key("roe")); // 空 → 缺
        assert_eq!(snap.get("eps").copied(), Some(8.05));
    }

    #[test]
    fn rejects_non_increasing_time() {
        let f = wf("time,roe\n2024-08-20,1.0\n2024-04-27,2.0\n");
        assert!(load_fundamentals_csv(f.path()).is_err());
    }
}
```

- [ ] **Step 3: Run** `cargo test --lib data::fundamentals` → 3 pass. (`csv`/`tempfile` already deps.) Fix `Error::Data` variant name if different (grep `enum Error`).

- [ ] **Step 4: Commit** `git add src/data/mod.rs src/data/fundamentals.rs` → `git commit` (msg "feat(fundamentals): FundamentalSeries loader + as-of-t point-in-time lookup" + Co-Authored-By footer).

---

## Task FE-2: `UniverseEntry.fundamentals` (4th column)

**Files:** Modify `src/data/universe.rs`

- [ ] **Step 1: Add the field** to `UniverseEntry`:
```rust
pub struct UniverseEntry {
    pub symbol: String,
    pub primary: PathBuf,
    pub context: PathBuf,
    /// 可选逐股基本面 CSV（universe 第4列 `fundamentals`）；None = 无。
    pub fundamentals: Option<PathBuf>,
}
```

- [ ] **Step 2: Parse the optional 4th column** in `read_universe_csv`. After the `has_ctx` line add:
```rust
    let has_fund = headers.len() >= 4 && &headers[3] == "fundamentals";
```
Inside the record loop, after computing `context`, add:
```rust
        let fundamentals = if has_fund && !rec.get(3).unwrap_or("").trim().is_empty() {
            Some(PathBuf::from(rec[3].trim()))
        } else {
            None
        };
```
And change the push to: `out.push(UniverseEntry { symbol, primary, context, fundamentals });`

- [ ] **Step 3: Update the existing test** `reads_two_and_three_column_and_sorts` — the existing `UniverseEntry` reads still work (new field defaults via parse). Add a new test:
```rust
    #[test]
    fn reads_optional_fundamentals_column() {
        let f = write_tmp("symbol,primary,context,fundamentals\nsh600000,a.csv,ac.csv,af.csv\nsz000001,b.csv,,\n");
        let u = read_universe_csv(f.path()).unwrap();
        assert_eq!(u[0].fundamentals.as_ref().unwrap().to_str().unwrap(), "af.csv");
        assert!(u[1].fundamentals.is_none()); // 空 → None
        // 无 fundamentals 列 → None
        let f2 = write_tmp("symbol,primary\nsh600000,a.csv\n");
        assert!(read_universe_csv(f2.path()).unwrap()[0].fundamentals.is_none());
    }
```

- [ ] **Step 4: Run** `cargo test --lib data::universe` → pass.

- [ ] **Step 5: Commit** `git add src/data/universe.rs` → commit ("feat(fundamentals): UniverseEntry optional fundamentals column" + footer).

---

## Task FE-3: `Context.fundamentals` + `build_context` param + thread through ALL sites (atomic)

**Files:** Modify `src/features/context.rs` + every `build_context` call site + every `Context {}` literal (see Reuse reference list).

- [ ] **Step 1: Add the field + param + resolution** in `src/features/context.rs`.

Add to `Context`:
```rust
    /// as-of-t 基本面快照（公告日≤t 最近一行；空=首报前/无）。DSL fund.<col> 读此。
    pub fundamentals: BTreeMap<String, f64>,
```
Change `build_context` signature (add `fundamentals` BEFORE `t`):
```rust
pub fn build_context(
    primary: &[Bar],
    context: &[Bar],
    news: &[NewsRecord],
    aux: &BTreeMap<String, AuxTable>,
    fundamentals: Option<&crate::data::fundamentals::FundamentalSeries>,
    t: NaiveDateTime,
    window: usize,
) -> Context {
```
In the body, before constructing `Context`, add:
```rust
    let fund_snapshot = fundamentals.map(|fs| fs.as_of(t)).unwrap_or_default();
```
Add `fundamentals: fund_snapshot,` to the returned `Context { … }`.

- [ ] **Step 2: Thread the new arg through ALL call sites with `None`** (frozen behavior — none wire fundamentals yet; FE-6 wires factor). For each non-test site, insert `None,` as the 5th arg (after `aux`, before `t`):
  - `src/backtest/runner.rs:66`, `src/backtest/soft.rs:133`, `src/backtest/sim.rs:419`, `src/backtest/portfolio.rs:96`, `src/optimize/mod.rs` (3 sites), `src/factor/mod.rs:204`, `src/signal/mod.rs` (2 sites), `src/screen/mod.rs:84`, `desktop/src-tauri/src/data_bench.rs:96`, `desktop/src-tauri/src/replay.rs:102`.
  - For each, the call becomes `build_context(primary, context, news_or_&[], aux, None, t, window)`.
- [ ] **Step 3: Update test-helper call sites + Context literals** to add `None,` (build_context in tests: `src/features/context.rs` 124/135/153/175/178, `src/tree/loader.rs:1217`) and `fundamentals: std::collections::BTreeMap::new(),` to `Context { … }` literals in `src/dsl/eval.rs` (~569, 928, 955). Grep to be exhaustive: `rg "build_context\(" src desktop` and `rg "Context \{" src` — every hit updated.

- [ ] **Step 4: Add a build_context fundamentals test** in `src/features/context.rs` `mod tests`:
```rust
    #[test]
    fn build_context_resolves_fundamentals_as_of_t() {
        use crate::data::fundamentals::FundamentalSeries;
        let fs = FundamentalSeries {
            announce_dates: vec![NaiveDate::from_ymd_opt(2024, 4, 27).unwrap()],
            cols: std::collections::BTreeMap::from([("roe".to_string(), vec![34.1])]),
        };
        let primary = series(10);
        let t = primary[5].time; // 2024-01-02-ish < 2024-04-27 → 首报前
        let ctx = build_context(&primary, &[], &[], &BTreeMap::new(), Some(&fs), t, 3);
        assert!(ctx.fundamentals.is_empty()); // point-in-time: 公告前空
    }
```

- [ ] **Step 5: Run** `cargo test --workspace 2>&1 | grep -E "test result:|error|FAILED"; echo "EXIT=${PIPESTATUS[0]}"` → compiles (root + **desktop bridge**) + all pass. `--workspace` mandatory (build_context is engine public API used by the bridge). Then `cargo clippy --workspace --all-targets -- -D warnings`.

- [ ] **Step 6: Commit** `git add -p`-style: explicitly `git add src/features/context.rs src/backtest/runner.rs src/backtest/soft.rs src/backtest/sim.rs src/backtest/portfolio.rs src/optimize/mod.rs src/factor/mod.rs src/signal/mod.rs src/screen/mod.rs src/dsl/eval.rs src/tree/loader.rs desktop/src-tauri/src/data_bench.rs desktop/src-tauri/src/replay.rs` → commit ("feat(fundamentals): Context.fundamentals + build_context param threaded (None) through all sites" + footer).

---

## Task FE-4: DSL `fund.<col>` resolution

**Files:** Modify `src/dsl/eval.rs`

- [ ] **Step 1: Add the `fund.` branch** in `resolve_series` (after the `aux.` branch, before the `ctx.`/bare handling):
```rust
    if let Some(col) = name.strip_prefix("fund.") {
        // as-of-t 标量（公告日≤t 最近已公告值）；缺/首报前 → NaN（弃权）。1 元序列，标量上下文取末位。
        return Ok(vec![ctx.fundamentals.get(col).copied().unwrap_or(f64::NAN)]);
    }
```

- [ ] **Step 2: Add tests** in `src/dsl/eval.rs` `mod tests` (the existing `ctx_from_closes`/`ctx` helper builds a Context; set `.fundamentals`):
```rust
    #[test]
    fn fund_namespace_resolves_and_abstains() {
        let mut ctx = ctx_from_closes(&[1.0, 2.0, 3.0]);
        ctx.fundamentals = std::collections::BTreeMap::from([("roe".to_string(), 34.1)]);
        let f = |src: &str, c: &Context| eval(&parse_str(src).unwrap(), c).unwrap();
        assert_eq!(f("fund.roe > 15", &ctx), Value::Bool(true));   // 34.1 > 15
        assert_eq!(f("fund.roe < 15", &ctx), Value::Bool(false));
        // 缺列 → NaN → 比较 false（弃权，不报错）
        assert_eq!(f("fund.nope > 0", &ctx), Value::Bool(false));
        // 派生 PE：close/fund.eps（eps 缺 → NaN → 比较 false）
        ctx.fundamentals.insert("eps".to_string(), 0.5);
        assert_eq!(f("close / fund.eps > 1", &ctx), Value::Bool(true)); // 3.0/0.5=6 > 1
    }
```
NOTE: confirm the test-helper that builds `ctx` (e.g. `ctx_from_closes`) — after FE-3 its `Context {}` literal already has `fundamentals: BTreeMap::new()`; the test mutates `ctx.fundamentals`.

- [ ] **Step 3: Confirm lexer needs no change** — add a quick lexer test in `src/dsl/lexer.rs` `mod tests`:
```rust
    #[test]
    fn fund_dotted_ident_is_single_token() {
        let toks = tokenize("fund.roe > 15").unwrap();
        assert_eq!(toks[0], Token::Ident("fund.roe".to_string()));
    }
```

- [ ] **Step 4: Run** `cargo test --lib dsl::` → pass (incl. new eval + lexer tests).

- [ ] **Step 5: Commit** `git add src/dsl/eval.rs src/dsl/lexer.rs` → commit ("feat(fundamentals): DSL fund.<col> namespace (as-of-t scalar, NaN abstain)" + footer).

---

## Task FE-5: Python akshare pipeline → per-stock fundamental CSVs

**Files:** Create `scripts/fetch_fundamentals.py`, `scripts/README-fundamentals.md`. (akshare already pip-installed this session.)

- [ ] **Step 1: Write `scripts/fetch_fundamentals.py`**
```python
"""拉 A 股全市场季度基本面 (akshare stock_yjbb_em) → 逐股 point-in-time CSV (公告日锚)。
用法: python scripts/fetch_fundamentals.py [--out data/fundamentals] [--from-year 2018]
单位铁律: roe/np_yoy/rev_yoy/gross_margin = 百分数(原样), eps/bps = 元。time = 最新公告日。"""
import argparse, os, sys
import akshare as ak
import pandas as pd

# yjbb 中文列 -> 输出列
COLMAP = {
    "净资产收益率": "roe",
    "净利润-同比增长": "np_yoy",
    "营业总收入-同比增长": "rev_yoy",
    "销售毛利率": "gross_margin",
    "每股收益": "eps",
    "每股净资产": "bps",
}
OUT_COLS = ["roe", "np_yoy", "rev_yoy", "gross_margin", "eps", "bps"]

def to_symbol(code: str):
    """6位码 -> 交易所前缀; 非沪深主板/创业/科创 -> None(跳过)."""
    code = str(code).zfill(6)
    if code[:2] in ("60", "68") or code[0] == "9":
        return "sh" + code
    if code[:2] in ("00", "30") or code[0] == "2":
        return "sz" + code
    return None

def quarters(from_year: int):
    import datetime
    today = datetime.date.today()
    out = []
    for y in range(from_year, today.year + 1):
        for md in ("0331", "0630", "0930", "1231"):
            d = f"{y}{md}"
            # 仅取已可能披露的季度 (季末 < 今天)
            qend = datetime.date(int(d[:4]), int(d[4:6]), int(d[6:8]))
            if qend < today:
                out.append(d)
    return out

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default="data/fundamentals")
    ap.add_argument("--from-year", type=int, default=2018)
    args = ap.parse_args()
    os.makedirs(args.out, exist_ok=True)
    # symbol -> list[ (公告日, {col:val}) ]
    per_stock = {}
    for q in quarters(args.from_year):
        try:
            df = ak.stock_yjbb_em(date=q)
        except Exception as e:
            print(f"WARN quarter {q} failed: {e}", file=sys.stderr); continue
        for _, r in df.iterrows():
            sym = to_symbol(r.get("股票代码", ""))
            if sym is None:
                continue
            ann = r.get("最新公告日期")
            if pd.isna(ann):
                continue
            ann = str(ann)[:10]  # YYYY-MM-DD
            row = {}
            for zh, en in COLMAP.items():
                v = r.get(zh)
                row[en] = "" if pd.isna(v) else f"{float(v):.6g}"
            per_stock.setdefault(sym, {})[ann] = row  # dedup by 公告日 (latest wins)
        print(f"  quarter {q}: {df.shape[0]} rows", file=sys.stderr)
    n = 0
    for sym, byann in per_stock.items():
        rows = sorted(byann.items())  # by 公告日 asc
        path = os.path.join(args.out, f"{sym}.csv")
        with open(path, "w", encoding="utf-8", newline="") as f:
            f.write("time," + ",".join(OUT_COLS) + "\n")
            for ann, row in rows:
                f.write(ann + "," + ",".join(row[c] for c in OUT_COLS) + "\n")
        n += 1
    print(f"wrote {n} per-stock fundamental CSVs to {args.out}")

if __name__ == "__main__":
    main()
```

- [ ] **Step 2: Run the pipeline (networked, all-market)**
```bash
python scripts/fetch_fundamentals.py --out data/fundamentals --from-year 2018 2>&1 | tail -8
```
Expected: per-quarter row counts (~5000 each) + "wrote ~5000 per-stock fundamental CSVs". Verify a known stock:
```bash
head -5 data/fundamentals/sh600519.csv
```
Expected: `time,roe,np_yoy,rev_yoy,gross_margin,eps,bps` + Moutai rows with 公告日 timestamps + ROE ~30+ (percent). If akshare endpoints fail/throttle, retry; if a quarter is missing it warns + continues.

- [ ] **Step 3: Write `scripts/README-fundamentals.md`** documenting: akshare dependency (`pip install akshare`), the command, the output format (per-stock CSV, 公告日 anchor, percent units for roe/growth/margin), and that `data/fundamentals/` is gitignored + reproducible.

- [ ] **Step 4: gitignore** — confirm `data/` (or add `data/fundamentals/`) is in `.gitignore` (it is: `data/*.csv`; add `data/fundamentals/` if needed). Do NOT commit the CSVs.

- [ ] **Step 5: Commit** `git add scripts/fetch_fundamentals.py scripts/README-fundamentals.md .gitignore` → commit ("feat(fundamentals): akshare pipeline → per-stock point-in-time CSVs" + footer).

---

## Task FE-6: Wire `factor` to load fundamentals + validate on the 20

**Files:** Modify `src/factor/mod.rs` (load per-symbol fundamentals + pass to build_context); create `data/universe_20_fund.csv`.

- [ ] **Step 1: In `src/factor/mod.rs`**, where the universe + bars are loaded (the `collect_periods` path), also load each entry's fundamentals and pass to `build_context`. Read the current loader; for each `UniverseEntry`, load `entry.fundamentals.as_ref().map(|p| load_fundamentals_csv(p)).transpose()?` into a `Vec<Option<FundamentalSeries>>` aligned with symbols; at the `build_context(...)` call (line ~204) pass `funds[i].as_ref()` instead of `None`.
  - exact: `use crate::data::fundamentals::load_fundamentals_csv;` at top; load `let fund = entry.fundamentals.as_ref().map(|p| load_fundamentals_csv(p)).transpose()?;` per symbol (store in a parallel Vec); pass `fund.as_ref()` to build_context.

- [ ] **Step 2: Create `data/universe_20_fund.csv`** (4-column, points to fundamentals):
```
symbol,primary,context,fundamentals
sh600030,data/sh600030.csv,data/sh600030.csv,data/fundamentals/sh600030.csv
... (all 20, fundamentals = data/fundamentals/<sym>.csv)
```
Generate it from `data/universe_20.csv` (append the 4th column). This file is gitignored (data/*.csv) — it's a local validation artifact; OK.

- [ ] **Step 3: Run the factor IC validation (the §5 proof)** — needs FE-5's data + the deep-20 OHLCV present:
```bash
cargo run --release --bin rquant -- factor --universe data/universe_20_fund.csv \
  --factor "roe=fund.roe" --factor "npyoy=fund.np_yoy" --factor "grossm=fund.gross_margin" \
  --factor "pe=close/fund.eps" --factor "pb=close/fund.bps" \
  --sample 20 --horizon 20 --layers 5 --warmup 260 --window 260 --out tmps/fund_factor.json 2>&1 | grep -v "LLM not" | head -40
```
Read the RankIC/ICIR/layers honestly: does any fundamental factor clear F-1 (`|RankIC|>0.03 ∧ |ICIR|>0.3`)? Record the verdict (point-in-time, gross). **Honest §5.3**: don't tune; if fundamentals show no IC on the 20, that's a valid finding (note small-N caveat — 20 is thin cross-section).

- [ ] **Step 4: Commit** `git add src/factor/mod.rs` → commit ("feat(fundamentals): factor workbench reads per-stock fundamentals (point-in-time)" + footer). (universe_20_fund.csv is gitignored.)

---

## Task FE-7: Docs + full gate + finishing + memory

- [ ] **Step 1: Docs** — `docs/dsl-reference.md`: add a `fund.<col>` section (namespace, as-of-t scalar, 公告日 point-in-time gate, NaN-abstain before first filing, **percent units** for roe/np_yoy/rev_yoy/gross_margin & yuan for eps/bps, derived PE/PB via `close/fund.eps`). `docs/cli-reference.md`: note the universe CSV optional 4th column `fundamentals`.

- [ ] **Step 2: Full gate**
```bash
cargo test --workspace 2>&1 | grep -E "test result:|error|FAILED"; echo "EXIT=${PIPESTATUS[0]}"
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | grep -E "error|warning|Finished"; echo "EXIT=${PIPESTATUS[0]}"
```
Both EXIT=0, 0 failed. `--workspace` mandatory (build_context/UniverseEntry are engine public API used by the desktop bridge).

- [ ] **Step 3: Write a short findings note** `docs/superpowers/2026-06-16-fundamentals-engine-findings.md`: the factor IC verdict from FE-6 (do fundamentals predict on the 20, point-in-time) + honest small-N caveat + that the engine `fund.` channel + akshare pipeline are in place for sub-project ② (scale) / ③ (methodology).

- [ ] **Step 4: Commit docs + note** → commit ("docs(fundamentals): fund. DSL reference + sub-project-1 findings" + footer).

- [ ] **Step 5: Finish** — invoke **superpowers:finishing-a-development-branch** (verify → options → on choice merge `--no-ff` to master + cleanup). Do NOT push unless asked.

- [ ] **Step 6: Memory** — update `rquant-project.md` with sub-project ① outcome (fundamentals into engine via `fund.` + akshare pipeline; the IC verdict; next = ② scale to 2000 / ③ methodology).

---

## Self-Review (completed by plan author)

**Spec coverage:** §3 data pipeline → FE-5; §4 engine (fundamentals.rs/universe/context/dsl) → FE-1/FE-2/FE-3/FE-4; §5 validation (factor IC + point-in-time test) → FE-6 (IC) + FE-1/FE-3 (point-in-time as_of tests); §6 files → all tasks; §7 boundaries (point-in-time enforced, restatement=latest, survivorship→②, --workspace, behavior-frozen via None) → FE-3 (None threading freezes behavior) + FE-7 gate. All covered.

**Placeholder scan:** none — concrete code for fundamentals.rs / eval branch / Python script / factor wiring; exact call-site list for the FE-3 ripple; exact commands.

**Type consistency:** `FundamentalSeries { announce_dates, cols }` + `as_of -> BTreeMap<String,f64>` (FE-1) used by `build_context`'s `fundamentals: Option<&FundamentalSeries>` param → `Context.fundamentals: BTreeMap<String,f64>` (FE-3) → `eval` `fund.` reads `ctx.fundamentals` (FE-4) → `factor` passes `funds[i].as_ref()` (FE-6). `UniverseEntry.fundamentals: Option<PathBuf>` (FE-2) → loaded in FE-6. Consistent.

**Flagged check-points:** `Error::Data` variant (FE-1); exhaustive `build_context`/`Context {`-literal grep incl. desktop bridge (FE-3); akshare endpoint reliability + Python 3.13 (FE-5, already installed); factor loader exact shape — read before editing (FE-6).
```
