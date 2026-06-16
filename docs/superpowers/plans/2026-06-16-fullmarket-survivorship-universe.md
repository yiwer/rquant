# 全市场扩展 + survivorship-free universe + 宽截面基本面 IC 验证（子项②）实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把标的范围从 deep-20 扩到全市场（~5000+退市），以 survivorship-free 的 top-2000-按成交额-at-t 构造 universe，并在宽截面上诚实检验基本面因子 IC。

**Architecture:** Rust 侧新增 `data::membership`（点时成员快照 + `effective_at`）并让 `factor` 工具支持 `--membership` mask（None=行为冻结）；Python 侧新增 akshare 全市场 OHLCV 抓取 + 月末 top-2000 成交额 membership builder（≤t 排名，退市股活跃期自动入/退市后自动出）。引擎 mechanism 先离线 TDD 证完，再跑 akshare 数据 + 宽截面 IC 验证。

**Tech Stack:** Rust（csv/chrono/serde，现有 factor/data 模块）；Python 3.13 + akshare 1.18.64 + pandas（数据管线，与 `fetch_fundamentals.py` 同模式）。

**Spec:** `docs/superpowers/specs/2026-06-16-fullmarket-survivorship-universe-design.md`

**Commit footer（所有 commit 附）：**
```
Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
```

**Worktree：** 执行起手用 `superpowers:using-git-worktrees` 从**本地 HEAD**（master，含未推送提交）建 `worktree-fullmarket` 分支——**不要**用默认 `fresh`/origin baseRef（会丢未推送工作）。命令：`git worktree add .claude/worktrees/worktree-fullmarket -b worktree-fullmarket HEAD` 然后 `EnterWorktree(path=.claude/worktrees/worktree-fullmarket)`。

**执行序说明：** Task 1-3（Rust 引擎，离线 TDD，子代理实现）→ Task 4（联网 spike，控制器跑）→ Task 5-7（Python 脚本，子代理实现）→ Task 8-10（联网/计算执行，控制器跑）→ Task 11（收尾，控制器）。

---

## 文件结构

| 文件 | 职责 |
|---|---|
| `src/data/membership.rs`（新）| `Membership` 点时成员快照：`load_csv` + `effective_at`（partition_point）|
| `src/data/mod.rs`（改）| 挂 `pub mod membership;` |
| `src/factor/mod.rs`（改）| `FactorConfig.membership_path` + `collect_periods` 加载 + per-symbol mask 闸 |
| `src/cli/mod.rs`（改）| `Cmd::Factor` 加 `--membership` 可选参，透传 |
| `docs/cli-reference.md`（改）| factor `--membership` + universe_full/membership 文件格式 |
| `scripts/build_roster.py`（新）| 在市+退市清单合并 → `data/universe_full.csv` |
| `scripts/fetch_ohlcv.py`（新）| 逐股 `stock_zh_a_hist` qfq → `data/<sym>.csv`（resume=陈旧则整重拉，qfq 防接缝）|
| `scripts/build_membership.py`（新）| 月末 top-N 成交额（≤d 排名 + 在市窗）→ `membership_top2000.csv` + 成员并集 roster |
| `scripts/test_build_membership.py`（新）| build_membership 纯逻辑自测（无 pytest 依赖）|
| `docs/superpowers/2026-06-16-fullmarket-akshare-spike.md`（新）| spike 结论（退市数据可得性 + 降级声明）|
| `docs/superpowers/2026-06-16-fullmarket-fundamental-ic-findings.md`（新）| 宽截面 IC findings（诚实判定）|

---

## Task 1: `data::membership` — 点时成员快照（Rust，TDD）

**Files:**
- Create: `src/data/membership.rs`
- Modify: `src/data/mod.rs:13`（在 `pub mod fundamentals;` 后加 `pub mod membership;`）
- Test: `src/data/membership.rs`（内联 `#[cfg(test)]`）

- [ ] **Step 1: 挂模块**

`src/data/mod.rs` 第 13 行 `pub mod fundamentals;` 之后加：
```rust
pub mod membership;
```

- [ ] **Step 2: 写失败测试**

新建 `src/data/membership.rs`，先只放测试 + 空类型占位（让它编译失败/断言失败）：
```rust
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
```

- [ ] **Step 3: 跑测试确认失败**

Run: `cargo test membership`
Expected: 编译失败（`load_csv`/`effective_at` 未定义）。

- [ ] **Step 4: 实现**

在 `Membership` 定义之后、`#[cfg(test)]` 之前加：
```rust
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
```

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test membership`
Expected: 3 测试全 PASS。

- [ ] **Step 6: clippy**

Run: `cargo clippy --lib`
Expected: 无 warning（`is_empty` 可能 dead_code——保留，Task 2 不一定用；若 clippy 报 dead_code，加 `#[allow(dead_code)]` 于 `is_empty` 上方，注释「保留给消费方探查」）。

- [ ] **Step 7: 提交**

```bash
git add src/data/membership.rs src/data/mod.rs
git commit -F - <<'EOF'
feat(data): point-in-time universe Membership (survivorship-free top-N snapshots)

Membership::load_csv reads long-format (date,symbol) artifact, groups by
rebalance date; effective_at(t) returns the latest snapshot with date<=t
(non-cumulative, point-in-time). Consumed by the factor tool as a mask.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task 2: factor 支持 membership mask（Rust，TDD）

**Files:**
- Modify: `src/factor/mod.rs`（`FactorConfig` 加字段 + `collect_periods` 加载与 mask）
- Test: `src/factor/mod.rs`（内联 `#[cfg(test)]`，新增 2 测试）

- [ ] **Step 1: 加配置字段**

`src/factor/mod.rs` 的 `FactorConfig`（约 `:28-38`），在 `html_path: Option<PathBuf>,` 后加：
```rust
    pub html_path: Option<PathBuf>,
    /// 可选点时 universe 成员 CSV（date,symbol）；每截面只取该 t 生效成员。None=不过滤（行为冻结）。
    pub membership_path: Option<PathBuf>,
```

- [ ] **Step 2: 写失败测试**

在 `src/factor/mod.rs` 的 `#[cfg(test)] mod tests` 内（紧邻现有 `run_factor_*` 测试）加。注意：测试自带写 bar CSV 的辅助（用 `crate::data::reader::write_bars_csv` + `Bar`）与内联 universe/membership CSV：
```rust
    #[test]
    fn membership_mask_excludes_nonmembers() {
        use crate::data::bar::Bar;
        use chrono::NaiveDate;
        let dir = tempfile::tempdir().unwrap();
        // 两股 A、B，各 8 个日 bar（2018-01-01..08，15:00）
        let mk = |base: f64| -> Vec<Bar> {
            (1..=8).map(|d| {
                let t = NaiveDate::from_ymd_opt(2018,1,d).unwrap().and_hms_opt(15,0,0).unwrap();
                Bar { time: t, open: base, high: base+1.0, low: base-1.0, close: base + d as f64, volume: 100.0 }
            }).collect()
        };
        let pa = dir.path().join("A.csv");
        let pb = dir.path().join("B.csv");
        crate::data::reader::write_bars_csv(&mk(10.0), &pa).unwrap();
        crate::data::reader::write_bars_csv(&mk(20.0), &pb).unwrap();
        // universe 两股
        let uni = dir.path().join("uni.csv");
        std::fs::write(&uni, format!("symbol,primary\nA,{}\nB,{}\n", pa.display(), pb.display())).unwrap();
        // membership：自首日起只含 A
        let mem = dir.path().join("mem.csv");
        std::fs::write(&mem, "date,symbol\n2018-01-01,A\n").unwrap();

        let cfg = FactorConfig {
            universe_path: uni.clone(),
            factors: vec![FactorSpecItem { name: "px".into(), expr: "close".into() }],
            sample: 1, horizon: 2, layers: 2, warmup: 2, window: 3,
            out_path: dir.path().join("out.json"), html_path: None,
            membership_path: Some(mem),
        };
        let (periods, _ladder, _n) = collect_periods(&cfg).unwrap();
        // 每期截面只含 A，绝不含 B
        for p in &periods {
            assert!(p.points.iter().all(|sp| sp.symbol == "A"),
                "period {:?} leaked a non-member", p.t);
        }
        assert!(periods.iter().any(|p| !p.points.is_empty()), "no periods produced");
    }

    #[test]
    fn no_membership_is_frozen_both_symbols() {
        use crate::data::bar::Bar;
        use chrono::NaiveDate;
        let dir = tempfile::tempdir().unwrap();
        let mk = |base: f64| -> Vec<Bar> {
            (1..=8).map(|d| {
                let t = NaiveDate::from_ymd_opt(2018,1,d).unwrap().and_hms_opt(15,0,0).unwrap();
                Bar { time: t, open: base, high: base+1.0, low: base-1.0, close: base + d as f64, volume: 100.0 }
            }).collect()
        };
        let pa = dir.path().join("A.csv");
        let pb = dir.path().join("B.csv");
        crate::data::reader::write_bars_csv(&mk(10.0), &pa).unwrap();
        crate::data::reader::write_bars_csv(&mk(20.0), &pb).unwrap();
        let uni = dir.path().join("uni.csv");
        std::fs::write(&uni, format!("symbol,primary\nA,{}\nB,{}\n", pa.display(), pb.display())).unwrap();
        let cfg = FactorConfig {
            universe_path: uni, factors: vec![FactorSpecItem { name: "px".into(), expr: "close".into() }],
            sample: 1, horizon: 2, layers: 2, warmup: 2, window: 3,
            out_path: dir.path().join("out.json"), html_path: None,
            membership_path: None, // 冻结
        };
        let (periods, _l, _n) = collect_periods(&cfg).unwrap();
        let any_b = periods.iter().any(|p| p.points.iter().any(|sp| sp.symbol == "B"));
        assert!(any_b, "frozen mode must keep B");
    }
```

- [ ] **Step 3: 跑测试确认失败**

Run: `cargo test --lib factor::tests::membership_mask_excludes_nonmembers factor::tests::no_membership_is_frozen_both_symbols`
Expected: 编译失败（`FactorConfig` 缺 `membership_path`——其它现有 `FactorConfig {…}` 构造处也会编译错，见 Step 4 一并补）。

- [ ] **Step 4: 实现 mask + 补齐所有 FactorConfig 构造处**

(a) `collect_periods`（`src/factor/mod.rs`），在 universe 加载块（约 `:158`，`let universe = read_universe_csv(...)?;` 之后）加载 membership：
```rust
    let universe = read_universe_csv(&cfg.universe_path)?;
    let n_symbols = universe.len();

    // 点时成员（None=不过滤=行为冻结）
    let membership = cfg
        .membership_path
        .as_ref()
        .map(|p| crate::data::membership::Membership::load_csv(p))
        .transpose()?;
```

(b) 采样循环内（约 `:198`，`let t = timeline[ti];` 之后、`for (sym_idx, entry) in ...` 之前）算一次当期生效集：
```rust
        let t = timeline[ti];
        // 当期生效成员：None=未配置(不过滤)；Some(None)=配置但 t 早于首期(空截面)；Some(Some(set))=限定
        let eff = membership.as_ref().map(|m| m.effective_at(t));
        let mut points: Vec<SymbolPoint> = Vec::with_capacity(n_symbols);
```

(c) per-symbol 循环内，bar 存在性 `binary_search_by_key` 块（约 `:207-210`）**之后**、`build_context` 之前，加 mask 闸：
```rust
            let bar_i = match bars.binary_search_by_key(&t, |b| b.time) {
                Ok(i) => i,
                Err(_) => continue,
            };

            // membership mask（point-in-time）
            match eff {
                None => {}                                              // 未配置 → 保留
                Some(Some(set)) if set.contains(&entry.symbol) => {}    // 成员 → 保留
                _ => continue,                                          // 配置但空/非成员 → 跳过
            }
```

(d) 补齐其它 `FactorConfig { … }` 构造处的新字段。已知一处在 `src/cli/mod.rs:510`（Task 3 处理）。用 grep 找全：
```
rg -n "FactorConfig \{" src crates
```
对每处（非本测试内）加 `membership_path: None,`（冻结）。CLI 那处在 Task 3 改为透传，本步先让它编译（可暂填 `None`，Task 3 覆盖）。

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test --lib factor`
Expected: 新 2 测试 PASS，且现有 `run_factor_*` 全部仍 PASS（冻结无回归）。

- [ ] **Step 6: 提交**

```bash
git add src/factor/mod.rs src/cli/mod.rs
git commit -F - <<'EOF'
feat(factor): optional --membership point-in-time universe mask

FactorConfig.membership_path restricts each cross-section to the symbols
effective at sample time t (latest membership snapshot <= t). None preserves
prior behavior exactly (frozen). Survivorship-free: delisted names are in their
active months and drop out once they stop trading.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task 3: CLI `--membership` 透传 + 文档（Rust）

**Files:**
- Modify: `src/cli/mod.rs`（`Cmd::Factor` 结构 + match 解构 + `FactorConfig` 构造）
- Modify: `docs/cli-reference.md`（factor 节）

- [ ] **Step 1: 加 CLI 参**

`src/cli/mod.rs` 的 `Cmd::Factor`（约 `:217-237`），在 `html: Option<PathBuf>,` 后加：
```rust
        #[arg(long)]
        html: Option<PathBuf>,
        /// Point-in-time universe membership CSV (date,symbol); restrict each cross-section to that date's members
        #[arg(long)]
        membership: Option<PathBuf>,
    },
```

- [ ] **Step 2: match 解构 + 透传**

(a) match 臂（约 `:488`）`Cmd::Factor { universe, factor, sample, horizon, layers, warmup, window, out, html } =>` 改为加 `membership`：
```rust
        Cmd::Factor { universe, factor, sample, horizon, layers, warmup, window, out, html, membership } => {
```
(b) `FactorConfig` 构造（约 `:510-520`），把 Task 2 Step 4(d) 暂填的 `membership_path: None,` 改为：
```rust
                html_path: html.clone(),
                membership_path: membership,
            };
```

- [ ] **Step 3: 编译 + 全量测试**

Run: `cargo test --workspace`
Expected: 全绿（CLI 改动不影响逻辑；workspace 含桥接 crate）。

- [ ] **Step 4: 文档**

`docs/cli-reference.md` 的 `factor` 小节加（找到现有 `--factor`/`--sample` 说明附近）：
```markdown
- `--membership <PATH>`：点时 universe 成员 CSV（`date,symbol` long 格式，每月末一组）。指定后每个横截面只纳入该 t 生效（最近 ≤t 再平衡日）的成员——survivorship-free 宽截面验证用。缺省=不过滤（用全 universe）。
  - 配套文件：`data/universe_full.csv`（全市场 roster，`symbol,primary,context,fundamentals`，context 空=回退 primary）、`data/membership_top2000.csv`（成员表）、`data/universe_membership.csv`（成员并集 roster，factor 加载用以控内存）。由 `scripts/build_roster.py` / `scripts/fetch_ohlcv.py` / `scripts/build_membership.py` 生成。
```

- [ ] **Step 5: 提交**

```bash
git add src/cli/mod.rs docs/cli-reference.md
git commit -F - <<'EOF'
feat(cli): rquant factor --membership flag + docs

Wire the point-in-time universe membership mask through the CLI; document the
flag and the universe_full / membership_top2000 / universe_membership artifacts.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task 4: akshare 退市数据可行性 spike（联网，控制器跑）

**目的：** 全量抓取前确认 survivorship-free 的命根——akshare 能否给退市股数据。决定是否降级。

**Files:**
- Create: `docs/superpowers/2026-06-16-fullmarket-akshare-spike.md`（结论）

- [ ] **Step 1: 探退市清单接口**

跑（PowerShell；中文列名用 utf8 防 GBK）：
```powershell
python -X utf8 -c "import akshare as ak; d=ak.stock_info_sh_delist(); print(d.shape); print(list(d.columns)); print(d.head().to_string())"
python -X utf8 -c "import akshare as ak; d=ak.stock_info_sz_delist(); print(d.shape); print(list(d.columns)); print(d.head().to_string())"
```
记录：是否返回、代码列名（含「代码」的列）、是否含退市日字段。若接口名报错（akshare 版本差异），试 `[n for n in dir(ak) if 'delist' in n]` 找正确名。

- [ ] **Step 2: 探退市股 OHLCV**

取 Step 1 里一只退市代码 `CODE`，跑：
```powershell
python -X utf8 -c "import akshare as ak; d=ak.stock_zh_a_hist(symbol='CODE', period='daily', start_date='20180101', end_date='20240101', adjust='qfq'); print(d.shape); print(d.head().to_string()); print(d.tail().to_string())"
```
记录：能否返回历史、列名（应含 日期/开盘/最高/最低/收盘/成交量）。

- [ ] **Step 3: 写结论 + 降级判定**

写 `docs/superpowers/2026-06-16-fullmarket-akshare-spike.md`：
- 退市清单接口名 + 代码列名 + 是否含退市日；
- 退市股 OHLCV 是否可得（样例）；
- **降级判定**：若退市 OHLCV **可得** → 全 survivorship-free（Task 5 roster 纳入退市）；若**不可得** → 诚实声明残余偏差（退市股无 bar → 自动不入任何 top-2000 → 偏差 = 退市尾部，Task 10 findings 量化其占比）。**无需改脚本**（脚本对缺数据股天然跳过）。

- [ ] **Step 4: 提交**

```bash
git add docs/superpowers/2026-06-16-fullmarket-akshare-spike.md
git commit -F - <<'EOF'
docs(fullmarket): akshare delisted-data feasibility spike conclusion

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task 5: `build_roster.py` — 全市场 roster（Python）

**Files:**
- Create: `scripts/build_roster.py`

> 按 Task 4 spike 确认的退市接口名/代码列名落地（下方代码用通用「含『代码』列」探测，对多数 akshare 版本鲁棒）。

- [ ] **Step 1: 写脚本**

```python
"""构建全市场 roster（含退市股）→ data/universe_full.csv。
每行 symbol,primary,context,fundamentals；context 留空(=primary)；fundamentals 已存在则填。
用法: python scripts/build_roster.py [--out data/universe_full.csv] [--data-dir data] [--fund-dir data/fundamentals]"""
import argparse, os, sys
import akshare as ak

def to_symbol(code):
    code = str(code).zfill(6)
    if code[:2] in ("60", "68") or code[0] == "9":
        return "sh" + code
    if code[:2] in ("00", "30") or code[0] == "2":
        return "sz" + code
    return None

def collect_codes():
    syms = set()
    try:
        df = ak.stock_info_a_code_name()  # columns: code, name
        for c in df["code"]:
            s = to_symbol(c)
            if s: syms.add(s)
        print(f"  in-listed: {len(syms)}", file=sys.stderr)
    except Exception as e:
        print(f"WARN in-listed list failed: {e}", file=sys.stderr)
    for fn in ("stock_info_sh_delist", "stock_info_sz_delist"):
        try:
            d = getattr(ak, fn)()
            col = next((c for c in d.columns if "代码" in str(c)), None)
            if col is None:
                print(f"WARN {fn}: no code column in {list(d.columns)}", file=sys.stderr); continue
            cnt = 0
            for c in d[col]:
                s = to_symbol(c)
                if s: syms.add(s); cnt += 1
            print(f"  {fn}: +{cnt}", file=sys.stderr)
        except Exception as e:
            print(f"WARN {fn} failed: {e}", file=sys.stderr)
    return sorted(syms)

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default="data/universe_full.csv")
    ap.add_argument("--data-dir", default="data")
    ap.add_argument("--fund-dir", default="data/fundamentals")
    args = ap.parse_args()
    syms = collect_codes()
    with open(args.out, "w", encoding="utf-8", newline="") as f:
        f.write("symbol,primary,context,fundamentals\n")
        for s in syms:
            fund = f"{args.fund_dir}/{s}.csv"
            fund_col = fund if os.path.exists(fund) else ""
            f.write(f"{s},{args.data_dir}/{s}.csv,,{fund_col}\n")
    print(f"wrote {len(syms)} symbols to {args.out}")

if __name__ == "__main__":
    main()
```

- [ ] **Step 2: 语法冒烟**

Run: `python -c "import ast; ast.parse(open('scripts/build_roster.py',encoding='utf-8').read()); print('ok')"`
Expected: `ok`（实际联网跑在 Task 8）。

- [ ] **Step 3: 提交**

```bash
git add scripts/build_roster.py
git commit -F - <<'EOF'
feat(scripts): build_roster.py — full-market roster incl. delisted -> universe_full.csv

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task 6: `fetch_ohlcv.py` — 全市场 qfq 日线（Python）

**Files:**
- Create: `scripts/fetch_ohlcv.py`

> **qfq 防接缝（关键正确性）：** qfq 后复权按最新价归一，新除权会平移整段历史——**不能增量 append**（旧段用旧基准、新段用新基准 → 接缝）。故 resume = 「已是最新则跳过，否则整段重拉覆盖」。

- [ ] **Step 1: 写脚本**

```python
"""逐股拉日线 qfq OHLCV → data/<sym>.csv（引擎 primary 格式，time=收盘 15:00:00）。
resume: 已存在且最新(末日在 refresh-within 天内)则跳过；否则整段重拉覆盖(qfq 防接缝)。限速 + 失败续跑。
用法: python scripts/fetch_ohlcv.py [--universe data/universe_full.csv] [--data-dir data]
      [--start 20180101] [--sleep 0.3] [--refresh-within 5] [--limit N(0=all)]"""
import argparse, os, sys, time, datetime
import akshare as ak
import pandas as pd

def code6(sym):
    return sym[2:] if sym[:2] in ("sh", "sz") else sym

def last_date(path):
    if not os.path.exists(path): return None
    try:
        df = pd.read_csv(path)
        if df.empty: return None
        return pd.to_datetime(df["time"].iloc[-1]).date()
    except Exception:
        return None

def fetch_one(sym, start, end):
    df = ak.stock_zh_a_hist(symbol=code6(sym), period="daily",
                            start_date=start, end_date=end, adjust="qfq")
    if df is None or df.empty: return None
    return pd.DataFrame({
        "time": pd.to_datetime(df["日期"]).dt.strftime("%Y-%m-%d 15:00:00"),
        "open": df["开盘"], "high": df["最高"], "low": df["最低"],
        "close": df["收盘"], "volume": df["成交量"],
    })

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--universe", default="data/universe_full.csv")
    ap.add_argument("--data-dir", default="data")
    ap.add_argument("--start", default="20180101")
    ap.add_argument("--sleep", type=float, default=0.3)
    ap.add_argument("--refresh-within", type=int, default=5)
    ap.add_argument("--limit", type=int, default=0)
    args = ap.parse_args()
    os.makedirs(args.data_dir, exist_ok=True)
    syms = list(pd.read_csv(args.universe)["symbol"])
    if args.limit: syms = syms[:args.limit]
    today = datetime.date.today()
    today_s = today.strftime("%Y%m%d")
    ok = fail = skip = 0
    for i, sym in enumerate(syms):
        path = os.path.join(args.data_dir, f"{sym}.csv")
        ld = last_date(path)
        if ld is not None and (today - ld).days <= args.refresh_within:
            skip += 1; continue  # 已最新
        try:
            out = fetch_one(sym, args.start, today_s)  # 整段重拉(qfq 防接缝)
        except Exception as e:
            print(f"WARN {sym} fetch failed: {e}", file=sys.stderr); fail += 1; time.sleep(args.sleep); continue
        if out is None or out.empty:
            skip += 1; time.sleep(args.sleep); continue
        out.to_csv(path, index=False)  # 覆盖
        ok += 1
        if (i + 1) % 100 == 0:
            print(f"  {i+1}/{len(syms)} ok={ok} fail={fail} skip={skip}", file=sys.stderr)
        time.sleep(args.sleep)
    print(f"done: ok={ok} fail={fail} skip={skip} of {len(syms)}")

if __name__ == "__main__":
    main()
```

- [ ] **Step 2: 语法冒烟**

Run: `python -c "import ast; ast.parse(open('scripts/fetch_ohlcv.py',encoding='utf-8').read()); print('ok')"`
Expected: `ok`。

- [ ] **Step 3: 提交**

```bash
git add scripts/fetch_ohlcv.py
git commit -F - <<'EOF'
feat(scripts): fetch_ohlcv.py — per-symbol qfq daily OHLCV with seam-safe resume

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task 7: `build_membership.py` + 自测 — 月末 top-N 成交额（Python，TDD）

**Files:**
- Create: `scripts/build_membership.py`
- Create: `scripts/test_build_membership.py`（无 pytest 依赖）

> **survivorship 机制：** 「在市」= 在 `[d-active_days, d]`（日历）内有 bar；退市股在末交易日后无 bar → 自动出 universe。排名是 scale-invariant（成交额 ≈ close×volume），不复发迭代#2 量纲 bug。

- [ ] **Step 1: 先写自测（失败）**

`scripts/test_build_membership.py`：
```python
"""build_membership 纯逻辑自测（无 pytest）：python scripts/test_build_membership.py → exit 0=pass。"""
import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import pandas as pd
import build_membership as bm

def test_rank_top_n():
    t = {"A": 100.0, "B": 300.0, "C": 200.0, "D": float("nan"), "E": -1.0}
    assert bm.rank_top_n(t, 2) == ["B", "C"]
    assert bm.rank_top_n(t, 10) == ["B", "C", "A"]   # NaN/负剔除
    print("ok rank_top_n")

def test_point_in_time_survivorship():
    # A 仅活到 2018-02-28("退市")，B 全程到 2018-03-31
    idx1 = pd.date_range("2018-01-01", "2018-02-28", freq="D")
    idx2 = pd.date_range("2018-01-01", "2018-03-31", freq="D")
    panel = {
        "A": pd.DataFrame({"close":[10.0]*len(idx1), "volume":[100.0]*len(idx1)}, index=idx1),
        "B": pd.DataFrame({"close":[10.0]*len(idx2), "volume":[ 50.0]*len(idx2)}, index=idx2),
    }
    mem = {d.strftime("%Y-%m"): set(s) for d, s in
           bm.compute_membership(panel, top=10, lookback=20, start="2018-01-01")}
    assert mem["2018-02"] == {"A", "B"}, mem   # A 活跃期入选(survivorship-free)
    assert mem["2018-03"] == {"B"}, mem        # A 退市后无 bar 自动出
    print("ok point_in_time_survivorship")

if __name__ == "__main__":
    test_rank_top_n(); test_point_in_time_survivorship(); print("ALL PASS")
```

- [ ] **Step 2: 跑自测确认失败**

Run: `python scripts/test_build_membership.py`
Expected: `ModuleNotFoundError: build_membership`（脚本未建）。

- [ ] **Step 3: 写脚本**

`scripts/build_membership.py`：
```python
"""构建 survivorship-free top-N membership（月末按近 lookback 日均成交额，≤d 排名 + 在市窗）。
读 data/<sym>.csv 面板 → data/membership_top2000.csv (date,symbol) + data/universe_membership.csv (成员并集 roster)。
point-in-time: 排名只用 ≤d 数据；在市=近 active_days 日内有 bar；退市股活跃期入、退市后出。
用法: python scripts/build_membership.py [--data-dir data] [--universe data/universe_full.csv]
      [--top 2000] [--lookback 20] [--start 2018-01-01] [--active-days 14]"""
import argparse, os, sys
import numpy as np
import pandas as pd

def rank_top_n(turnover, n):
    """turnover: sym->float；降序 top-n symbol；NaN/<=0 剔除。"""
    items = [(s, v) for s, v in turnover.items()
             if v is not None and np.isfinite(v) and v > 0]
    items.sort(key=lambda kv: kv[1], reverse=True)
    return [s for s, _ in items[:n]]

def month_end_dates(all_dates, start):
    s = pd.Timestamp(start)
    idx = all_dates[all_dates >= s]
    if len(idx) == 0: return []
    grp = pd.Series(idx, index=idx).groupby([idx.year, idx.month]).max()
    return list(grp.values)

def load_panel(data_dir, symbols):
    panel = {}
    for s in symbols:
        p = os.path.join(data_dir, f"{s}.csv")
        if not os.path.exists(p): continue
        try:
            df = pd.read_csv(p, usecols=["time", "close", "volume"])
        except Exception:
            continue
        if df.empty: continue
        df["date"] = pd.to_datetime(df["time"]).dt.normalize()
        panel[s] = df.set_index("date")[["close", "volume"]].sort_index()
    return panel

def compute_membership(panel, top, lookback, start, active_days=14):
    """→ list[(Timestamp, [symbols])]，每月末 top-N。"""
    if not panel: return []
    all_dates = pd.DatetimeIndex(sorted(set().union(*[df.index for df in panel.values()])))
    out = []
    for d in month_end_dates(all_dates, start):
        d = pd.Timestamp(d)
        lo = d - pd.Timedelta(days=active_days)
        turnover = {}
        for s, df in panel.items():
            if df.loc[lo:d].empty:            # 在市窗内无 bar → 不在市(退市/长停)
                continue
            win = df.loc[:d].tail(lookback)   # ≤d 近 lookback 交易日
            if win.empty: continue
            turnover[s] = float((win["close"] * win["volume"]).mean())  # 成交额近似；排名 scale-invariant
        members = rank_top_n(turnover, top)
        if members: out.append((d, members))
    return out

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--data-dir", default="data")
    ap.add_argument("--universe", default="data/universe_full.csv")
    ap.add_argument("--out-membership", default="data/membership_top2000.csv")
    ap.add_argument("--out-union", default="data/universe_membership.csv")
    ap.add_argument("--fund-dir", default="data/fundamentals")
    ap.add_argument("--top", type=int, default=2000)
    ap.add_argument("--lookback", type=int, default=20)
    ap.add_argument("--start", default="2018-01-01")
    ap.add_argument("--active-days", type=int, default=14)
    args = ap.parse_args()
    symbols = list(pd.read_csv(args.universe)["symbol"])
    panel = load_panel(args.data_dir, symbols)
    print(f"  loaded {len(panel)}/{len(symbols)} symbols with data", file=sys.stderr)
    mem = compute_membership(panel, args.top, args.lookback, args.start, args.active_days)
    union = set()
    with open(args.out_membership, "w", encoding="utf-8", newline="") as f:
        f.write("date,symbol\n")
        for d, members in mem:
            ds = d.strftime("%Y-%m-%d")
            for s in sorted(members):
                f.write(f"{ds},{s}\n"); union.add(s)
    with open(args.out_union, "w", encoding="utf-8", newline="") as f:
        f.write("symbol,primary,context,fundamentals\n")
        for s in sorted(union):
            fund = f"{args.fund_dir}/{s}.csv"
            fund_col = fund if os.path.exists(fund) else ""
            f.write(f"{s},{args.data_dir}/{s}.csv,,{fund_col}\n")
    print(f"wrote {len(mem)} rebalances, {len(union)} union symbols")

if __name__ == "__main__":
    main()
```

- [ ] **Step 4: 跑自测确认通过**

Run: `python scripts/test_build_membership.py`
Expected: `ok rank_top_n` / `ok point_in_time_survivorship` / `ALL PASS`。

- [ ] **Step 5: 提交**

```bash
git add scripts/build_membership.py scripts/test_build_membership.py
git commit -F - <<'EOF'
feat(scripts): build_membership.py — survivorship-free monthly top-N by turnover + self-test

Point-in-time: ranks on data<=d, "in market"=has a bar within active_days; a
delisted name is included in its active months and drops out once it stops
trading. rank_top_n is scale-invariant (no liquidity-units bug). Self-test
covers ranking + survivorship drop-out without a pytest dependency.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task 8: 跑全市场抓取（联网，控制器跑，可较长）

**前置：** Task 5/6 脚本就绪；Task 4 spike 已判定退市处理。

- [ ] **Step 1: 备份 deep-20 旧源（单源一致性）**

旧 deep-20 `data/<sym>.csv` 来自 Rust Tencent 源；akshare 重抓前备份，防丢 + 留对照：
```powershell
New-Item -ItemType Directory -Force data/_tencent_backup | Out-Null
Get-ChildItem data -Filter *.csv | Copy-Item -Destination data/_tencent_backup
```

- [ ] **Step 2: 建 roster**

Run: `python -X utf8 scripts/build_roster.py`
Expected: `wrote N symbols to data/universe_full.csv`（N 数千量级）。抽查文件头几行格式 `symbol,primary,context,fundamentals`。

- [ ] **Step 3: 冒烟抓 5 只（验证管线）**

Run: `python -X utf8 scripts/fetch_ohlcv.py --limit 5`
Expected: `done: ok=… ` 且 `data/<sym>.csv` 新生成、`time` 列为 `YYYY-MM-DD 15:00:00`。用引擎校验一只：
```powershell
cargo run --release -- validate-data --primary (Get-ChildItem data -Filter *.csv | Select-Object -First 1).FullName
```

- [ ] **Step 4: 全量抓取（长跑）**

Run: `python -X utf8 scripts/fetch_ohlcv.py`
Expected: 分批进度日志；`done: ok=… fail=… skip=…`。失败可重跑（resume 跳过已最新）。**记录** ok/fail/skip 计数（fail 多则查限流，加大 `--sleep` 重跑）。

- [ ] **Step 5: 抽样校验**

Run: `cargo run --release -- validate-data --primary data/sh600000.csv`（及另几只）
Expected: 覆盖/缺口报告合理（无大面积缺口）。

> 数据产物 `data/*.csv` 已被 `.gitignore` 忽略（DATA 子项既定）；本步无 commit。

---

## Task 9: 跑 membership builder（计算，控制器跑）

- [ ] **Step 1: 生成 membership + 并集 roster**

Run: `python -X utf8 scripts/build_membership.py --top 2000 --lookback 20 --start 2018-01-01`
Expected: `loaded M/N symbols with data` + `wrote R rebalances, U union symbols`（R≈月数~90，U 数千）。

- [ ] **Step 2: 健全性抽查**

```powershell
Get-Content data/membership_top2000.csv -TotalCount 5
(Get-Content data/membership_top2000.csv | Measure-Object -Line).Lines
Select-String -Path data/membership_top2000.csv -Pattern "sh600000" | Select-Object -First 2
```
Expected: 首行 `date,symbol`；总行数 ≈ R×2000；大盘股（如 sh600000）出现在多个月份。每月成员数应 ≈ min(2000, 当月在市股数)。

> 产物 gitignored；无 commit。

---

## Task 10: 宽截面基本面 IC 验证 + findings（计算，控制器跑）

**Files:**
- Create: `docs/superpowers/2026-06-16-fullmarket-fundamental-ic-findings.md`

- [ ] **Step 1: 跑 factor（宽截面 + membership mask）**

Run（用成员并集 roster 控内存 + membership mask）：
```powershell
cargo run --release -- factor `
  --universe data/universe_membership.csv `
  --membership data/membership_top2000.csv `
  --sample 20 --horizon 20 --layers 5 --warmup 60 --window 120 `
  --factor "roe=fund.roe" `
  --factor "npyoy=fund.np_yoy" `
  --factor "revyoy=fund.rev_yoy" `
  --factor "gm=fund.gross_margin" `
  --factor "pe=close/fund.eps" `
  --factor "pb=close/fund.bps" `
  --out fullmarket_factor_report.json
```
Expected: 打印每因子 IC/RankIC/ICIR/分层；JSON 落盘。**若 OOM**：universe_membership.csv 已是裁剪集；仍 OOM 则按年分段（`--start` 切片重建 membership 子集）跑多段，findings 注明。

- [ ] **Step 2: 写 findings（诚实判定）**

写 `docs/superpowers/2026-06-16-fullmarket-fundamental-ic-findings.md`，含：
- **口径**：universe=月末 top-2000 成交额（survivorship-free）；采样 sample=20/horizon=20（decay ladder 5/10/20/40/80）；样本期 2018–2026；有效期数/标的数。
- **每因子**：RankIC 均值、RankICIR、IC_t、pos_share、分层 spread（top−bottom）+ 单调性。
- **判定**（逐因子）：对照 F-1（|RankIC|>0.03 ∧ |ICIR|>0.3）→ **works / inconclusive / falsified**。
- **幸存者**：引用 Task 4 spike——若退市 OHLCV 不可得，量化残余偏差（缺数据退市股占比、对结论的方向性影响）。
- **对比 ①**：与 20-name 的 RankIC~0.02（inconclusive）对比，宽截面是否给出更确定结论。
- **诚实结语**：works/inconclusive/falsified 都如实写；**不调参凑数**（§5.3）。

- [ ] **Step 3: 提交 findings**

```bash
git add docs/superpowers/2026-06-16-fullmarket-fundamental-ic-findings.md
git commit -F - <<'EOF'
docs(fullmarket): wide-cross-section fundamental IC findings (honest verdict)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
```

---

## Task 11: 收尾闸 + finishing + 记忆（控制器）

- [ ] **Step 1: 全量闸**

Run: `cargo test --workspace`
Expected: 全绿（含 membership 单测 + factor 冻结回归）。

Run: `cargo clippy --workspace --all-targets`
Expected: 无 warning。

- [ ] **Step 2: finishing-a-development-branch**

调用 `superpowers:finishing-a-development-branch`：核验测试 → `ExitWorktree(keep)` → 切回 master → `git merge --no-ff worktree-fullmarket`（合并信息用临时文件 `-F`，英文）→ 清理 worktree（`git worktree remove --force` + `git worktree prune` + `git branch -d`）。**合并前** `git log --oneline master..worktree-fullmarket` 与 `git status` 核对，并查并行 session 是否在 master 落了新提交。

- [ ] **Step 3: 更新记忆**

更新 `C:\Users\Administrator\.claude\projects\E--rust-app-rquant\memory\rquant-project.md`：加子项② bullet（全市场 survivorship-free top-2000 membership 机制 + `--membership` + 宽截面 IC findings 结论 + akshare OHLCV 管线）。若需新文件则在 MEMORY.md 加指针。**仅记非显然、跨会话有用的**（机制位置、结论、降级声明）。

---

## 自审（writing-plans）

**1. Spec 覆盖：**
- §3.0 spike → Task 4 ✓；§3.1 roster → Task 5 ✓；§3.2 fetch（qfq 防接缝/resume）→ Task 6 ✓；§3.3 校验 → Task 8 Step 5 ✓
- §4 membership builder（≤d/在市/scale-invariant/并集 roster）→ Task 7 ✓；§4.3 消费 `Membership`/`effective_at` → Task 1 ✓
- §5 factor mask（membership_path/per-symbol 闸/冻结/内存并集）→ Task 2 + Task 10 Step 1 ✓
- §6 宽截面验证（命令/多前瞻/F-1/findings）→ Task 10 ✓
- §7 文件全覆盖 ✓；§8 诚实边界（works/inconclusive/falsified、单源备份、point-in-time 双闸、不调参）→ Task 4/8/10 各步嵌入 ✓
- 闸（--workspace + 单测）→ Task 11 ✓

**2. 占位符扫描：** 无 TBD/TODO；每代码步含完整代码；每命令含 expected。Task 4/8-10 为联网/计算执行步（非 TDD），已给确切命令 + 预期。✓

**3. 类型一致性：** `Membership::load_csv`/`effective_at`/`is_empty`（Task 1）与 Task 2 调用一致；`FactorConfig.membership_path: Option<PathBuf>`（Task 2 定义）与 Task 3 CLI 透传一致；Python `rank_top_n`/`compute_membership`/`load_panel`/`month_end_dates`（Task 7）与自测调用一致；CSV 列名 `date,symbol`（Task 7 写）与 `Membership::load_csv`（Task 1 读）一致；roster 列 `symbol,primary,context,fundamentals`（Task 5/7 写）与 `read_universe_csv`（既有）一致。✓
