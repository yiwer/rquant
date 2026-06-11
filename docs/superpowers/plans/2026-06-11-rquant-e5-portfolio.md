# rquant E5 横截面组合层（portfolio 子命令）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `rquant portfolio`：universe 内逐标的跑同一棵树取横截面分数（硬=dir×weight、软=E），逐期 top-N 等权纯多头，组合记账（换手成本/停牌最后价计价/等权基准），输出 PortfolioReport+holdings traces。

**Architecture:** 在 master(HEAD `828f885`)上加组合编排层。新 `data/universe.rs` 读取器；新 `backtest/portfolio.rs`（时间线并集/新鲜度/打分/select_top/accrue+turnover 纯函数/循环/报告）；CLI 新 `Cmd::Portfolio`。复用 build_context/traverse/traverse_soft/aux。语义权威=spec `docs/superpowers/specs/2026-06-11-rquant-e5-portfolio-design.md`。

**Tech Stack:** Rust 2024 + 既有。

> 提交信息英文。黄金记账用表达式链断言。

---

## 文件结构
```
新增: src/data/universe.rs       # UniverseEntry + read_universe_csv + 测试
新增: src/backtest/portfolio.rs  # 纯函数 + PortfolioConfig/run_portfolio/PortfolioReport/HoldingsRecord/print
改动: src/data/mod.rs、src/backtest/mod.rs（+ pub mod）
改动: src/cli/mod.rs             # Cmd::Portfolio
改动: tests/e2e.rs、docs/cli-reference.md、docs/architecture.md、README.md
```

---

## Task 1: universe 读取器

**Files:**
- Create: `src/data/universe.rs`；Modify: `src/data/mod.rs`

- [ ] **Step 1: 失败测试**

```rust
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

    #[test]
    fn reads_two_and_three_column_and_sorts() {
        let f = write_tmp("symbol,primary,context\nsz000001,b.csv,bc.csv\nsh600000,a.csv,\n");
        let u = read_universe_csv(f.path()).unwrap();
        assert_eq!(u.len(), 2);
        assert_eq!(u[0].symbol, "sh600000"); // 字典序
        assert_eq!(u[0].context, u[0].primary); // 空 context 回退 primary
        assert_eq!(u[1].context.to_str().unwrap(), "bc.csv");
        // 两列表头也合法
        let f2 = write_tmp("symbol,primary\nsh600000,a.csv\n");
        assert_eq!(read_universe_csv(f2.path()).unwrap()[0].context.to_str().unwrap(), "a.csv");
    }

    #[test]
    fn rejects_duplicate_and_empty_symbol() {
        assert!(read_universe_csv(write_tmp("symbol,primary\ns1,a.csv\ns1,b.csv\n").path()).is_err());
        assert!(read_universe_csv(write_tmp("symbol,primary\n,a.csv\n").path()).is_err());
    }
}
```

- [ ] **Step 2: RED → 实现**

```rust
use crate::{Error, Result};
use std::path::{Path, PathBuf};

/// universe 一行：标的 + 其 primary/context CSV 路径（context 缺省=primary）。
pub struct UniverseEntry {
    pub symbol: String,
    pub primary: PathBuf,
    pub context: PathBuf,
}

/// 读 universe CSV（表头 symbol,primary[,context]）；symbol 非空且唯一；按 symbol 字典序返回。
pub fn read_universe_csv(path: &Path) -> Result<Vec<UniverseEntry>> {
    let mut rdr = csv::Reader::from_path(path)?;
    let headers = rdr.headers()?.clone();
    if headers.len() < 2 || &headers[0] != "symbol" || &headers[1] != "primary" {
        return Err(Error::Data("universe csv must start with columns: symbol,primary[,context]".into()));
    }
    let has_ctx = headers.len() >= 3 && &headers[2] == "context";
    let mut out: Vec<UniverseEntry> = Vec::new();
    for rec in rdr.records() {
        let rec = rec?;
        let symbol = rec[0].trim().to_string();
        if symbol.is_empty() {
            return Err(Error::Data("universe: empty symbol".into()));
        }
        if out.iter().any(|e| e.symbol == symbol) {
            return Err(Error::Data(format!("universe: duplicate symbol '{symbol}'")));
        }
        let primary = PathBuf::from(rec[1].trim());
        let context = if has_ctx && !rec[2].trim().is_empty() {
            PathBuf::from(rec[2].trim())
        } else {
            primary.clone()
        };
        out.push(UniverseEntry { symbol, primary, context });
    }
    out.sort_by(|a, b| a.symbol.cmp(&b.symbol));
    Ok(out)
}
```
`src/data/mod.rs` 加 `pub mod universe;`。

- [ ] **Step 3: GREEN + Commit**

```bash
git add src/data/universe.rs src/data/mod.rs
git commit -m "feat(data): universe csv reader (symbol,primary[,context])" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: portfolio.rs 骨架（时间线/新鲜度/打分/select_top）

**Files:**
- Create: `src/backtest/portfolio.rs`；Modify: `src/backtest/mod.rs`（+ `pub mod portfolio;`）

- [ ] **Step 1: 纯函数 + 失败测试**

```rust
use crate::data::bar::Bar;
use chrono::NaiveDateTime;
use std::collections::BTreeSet;

/// 全标的 bar 时间有序并集。
pub fn build_timeline(all: &[Vec<Bar>]) -> Vec<NaiveDateTime> {
    let mut set = BTreeSet::new();
    for bars in all {
        for b in bars {
            set.insert(b.time);
        }
    }
    set.into_iter().collect()
}

/// t 时刻最后已知收盘价（time ≤ t）。
pub fn last_close_at(bars: &[Bar], t: NaiveDateTime) -> Option<f64> {
    let cut = bars.partition_point(|b| b.time <= t);
    if cut == 0 { None } else { Some(bars[cut - 1].close) }
}

/// 新鲜：恰有 bar 在 t（停牌标的当期出局）。
pub fn is_fresh(bars: &[Bar], t: NaiveDateTime) -> bool {
    bars.binary_search_by_key(&t, |b| b.time).is_ok()
}

/// score>0 取前 n：score 降序、并列 symbol 升序（确定性）。
pub fn select_top(scores: &[(String, f64)], n: usize) -> Vec<(String, f64)> {
    let mut pos: Vec<(String, f64)> = scores.iter().filter(|(_, s)| *s > 0.0).cloned().collect();
    pos.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)));
    pos.truncate(n);
    pos
}
```
测试（同文件）：`build_timeline` 错位并集排序去重；`last_close_at` 命中/早于首行 None/落在中间取前值；`is_fresh` 精确命中与缺失；`select_top` 过滤 0/负分、降序、并列字典序（`[("b",0.5),("a",0.5),("c",0.9)]`,n=2 → `[("c",0.9),("a",0.5)]`）、不足 n 取全部。

- [ ] **Step 2: 打分（依赖 tree/llm，async）**

```rust
/// 单标的在 t 的横截面分数：不新鲜 → None；硬=叶 dir×weight；软=E=Σp·w·dir。
#[allow(clippy::too_many_arguments)]
pub async fn score_symbol(
    primary: &[Bar],
    context: &[Bar],
    aux: &std::collections::BTreeMap<String, crate::data::aux_table::AuxTable>,
    tree: &crate::tree::loader::Tree,
    llm: &crate::eval::llm::LlmEvaluator,
    soft: bool,
    t: NaiveDateTime,
    window: usize,
) -> crate::Result<Option<f64>> {
    if !is_fresh(primary, t) {
        return Ok(None);
    }
    let ctx = crate::features::context::build_context(primary, context, &[], aux, t, window);
    let dir = |s: crate::tree::schema::Stance| match s {
        crate::tree::schema::Stance::Long => 1.0,
        crate::tree::schema::Stance::Short => -1.0,
        crate::tree::schema::Stance::Flat => 0.0,
    };
    let score = if soft {
        let st = crate::engine::soft::traverse_soft(tree, &ctx, llm).await?;
        st.leaf_probs.iter().map(|(id, p)| {
            tree.leaves.get(id).map_or(0.0, |l| p * l.weight * dir(l.stance))
        }).sum()
    } else {
        let tr = crate::engine::traversal::traverse(tree, &ctx, llm).await?;
        tree.leaves.get(&tr.leaf).map_or(0.0, |l| l.weight * dir(l.stance))
    };
    Ok(Some(score))
}
```
测试（tokio）：两标的合成 bars + 简单 long/flat 量化树——新鲜者得分（>0 或 0）、人为错开时间使另一标的不新鲜 → None。

- [ ] **Step 3: GREEN + Commit**

```bash
git add src/backtest/portfolio.rs src/backtest/mod.rs
git commit -m "feat(backtest): portfolio skeleton (timeline, freshness, scoring, top-N selection)" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: 记账纯函数 + 组合循环

**Files:**
- Modify: `src/backtest/portfolio.rs`

- [ ] **Step 1: accrue/turnover + 黄金测试（先测后实现）**

```rust
use std::collections::BTreeMap;

/// 区间收益：Σ w·(px_end/px_start − 1)；缺价成员贡献 0（防御；spec 保证持有成员价存在）。
pub fn accrue(weights: &BTreeMap<String, f64>, px_start: &BTreeMap<String, f64>, px_end: &BTreeMap<String, f64>) -> f64 {
    weights.iter().map(|(s, w)| {
        match (px_start.get(s), px_end.get(s)) {
            (Some(a), Some(b)) if *a > 0.0 => w * (b / a - 1.0),
            _ => 0.0,
        }
    }).sum()
}

/// 换手：Σ_union |w_new − w_old|。
pub fn turnover_between(old: &BTreeMap<String, f64>, new: &BTreeMap<String, f64>) -> f64 {
    let keys: BTreeSet<&String> = old.keys().chain(new.keys()).collect();
    keys.into_iter().map(|k| (new.get(k).copied().unwrap_or(0.0) - old.get(k).copied().unwrap_or(0.0)).abs()).sum()
}
```
黄金测试（表达式链，spec §3.2 口径，rate=0.001）：
```rust
    #[test]
    fn golden_two_period_walk() {
        let m = |pairs: &[(&str, f64)]| -> BTreeMap<String, f64> {
            pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
        };
        // t0：建仓 {A:0.5,B:0.5}，换手 1.0
        let w0 = m(&[("A", 0.5), ("B", 0.5)]);
        assert!((turnover_between(&BTreeMap::new(), &w0) - 1.0).abs() < 1e-12);
        let mut nav = 1.0 * (1.0 - 0.001 * 1.0);
        // 期1：A 10→11、B 20→19
        let r1 = accrue(&w0, &m(&[("A", 10.0), ("B", 20.0)]), &m(&[("A", 11.0), ("B", 19.0)]));
        assert!((r1 - (0.5 * 0.10 + 0.5 * (-0.05))).abs() < 1e-12);
        nav *= 1.0 + r1;
        // t1：换成 {A:0.5,C:0.5}，换手 = B 出 0.5 + C 进 0.5 = 1.0
        let w1 = m(&[("A", 0.5), ("C", 0.5)]);
        assert!((turnover_between(&w0, &w1) - 1.0).abs() < 1e-12);
        nav *= 1.0 - 0.001 * 1.0;
        // 期2：A 11→11、C 5→5.5
        let r2 = accrue(&w1, &m(&[("A", 11.0), ("C", 5.0)]), &m(&[("A", 11.0), ("C", 5.5)]));
        nav *= 1.0 + r2;
        assert!((nav - 0.999 * 1.025 * 0.999 * 1.05).abs() < 1e-12);
        // 停牌成员：px_end 缺失 → 贡献 0
        let r3 = accrue(&w1, &m(&[("A", 11.0), ("C", 5.0)]), &m(&[("A", 11.0)]));
        assert!((r3 - 0.0).abs() < 1e-12);
    }
```

- [ ] **Step 2: 报告类型 + 组合循环**

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct HoldingsRecord {
    pub t: NaiveDateTime,
    pub nav: f64,            // 该期调仓+扣成本后的净值
    pub benchmark_nav: f64,
    pub selected: Vec<(String, f64)>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PortfolioReport {
    pub tree_name: String,
    pub cost_bps: f64,
    pub top_n: usize,
    pub rebalance: usize,
    pub n_rebalances: usize,
    pub avg_members: f64,
    pub total_return: f64,
    pub max_drawdown: f64,
    pub turnover: f64,
    pub benchmark_return: f64,
    pub holdings: Vec<HoldingsRecord>,
}

pub struct PortfolioConfig {
    pub tree_path: PathBuf,
    pub universe_path: PathBuf,
    pub top: usize,
    pub rebalance: usize,
    pub warmup: usize,
    pub window: usize,
    pub cost_bps: f64,
    pub soft: bool,
    pub aux_paths: Vec<(String, PathBuf)>,
    pub out_path: PathBuf,
    pub traces_path: Option<PathBuf>,
}

pub async fn run_portfolio(cfg: &PortfolioConfig, llm: &LlmEvaluator) -> Result<PortfolioReport>
```
循环（spec §3.2）：
1. 读 universe/树/各标的 bars/aux；`timeline = build_timeline(...)`；调仓点 `idx = warmup, warmup+K, ...`；不足 2 个 → `Error::Data("universe timeline too short for warmup/rebalance")`。
2. 评估段序列 = 相邻调仓点对 + (末调仓点 → 末时间点（若不同))。
3. 每调仓点 t：逐标的 `score_symbol` → `select_top(scores, top)` → `w_new = 等权`；`tv = turnover_between(w_old, w_new)`；`nav *= 1 − rate·tv`；turnover 累加。基准 `bw = 全部"有 last_close_at(t)"标的等权`（无成本，每调仓点重置等权）。相邻调仓点同自然日且尚未警告 → eprintln T+1 提示一次。记录 `HoldingsRecord { t, nav, benchmark_nav, selected }`。
4. 段收益：`px_start/px_end = last_close_at` 各成员 → `nav *= 1 + accrue(...)`；基准同口径。峰值/回撤随每次 nav 更新维护（含段末）。
5. 汇总：`total_return = nav − 1`；`benchmark_return = bnav − 1`；`avg_members = selected 数均值`；写 out JSON + traces JSONL（HoldingsRecord 行）。
`print_portfolio_summary`：总收益/基准/超额(差值)/最大回撤/换手/调仓次数/平均成员数（风格同 print_sim_summary）。

- [ ] **Step 3: 集成测试（tokio，同文件）**

合成 3 标的（同一时间网格、跨多日）：A 每 bar +1%、B 横盘、C 每 bar −1%；树 `close > sma(close,3)` → leaf_long(weight 1) else flat；`top=1, rebalance=4, warmup=6, cost_bps=10`。断言：每期 selected==["A"]（B/C 分数 0 出局）、`total_return > benchmark_return`（A 跑赢等权基准）、`n_rebalances ≥ 2`、traces 行数==n_rebalances。

- [ ] **Step 4: GREEN + clippy + Commit**

```bash
git add src/backtest/portfolio.rs
git commit -m "feat(backtest): portfolio accounting loop with equal-weight benchmark and golden walk" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: CLI Cmd::Portfolio + e2e

**Files:**
- Modify: `src/cli/mod.rs`、`tests/e2e.rs`

- [ ] **Step 1: CLI**

`Cmd` 加变体（在 `Report` 之后；LLM 三件套与 `Backtest` 同名同默认）：
```rust
    /// Cross-sectional portfolio: run one tree across a universe, hold top-N equal-weight
    Portfolio {
        #[arg(long)]
        tree: PathBuf,
        #[arg(long)]
        universe: PathBuf,
        #[arg(long, default_value_t = 5)]
        top: usize,
        #[arg(long, default_value_t = 16)]
        rebalance: usize,
        #[arg(long, default_value_t = 100)]
        warmup: usize,
        #[arg(long, default_value_t = 100)]
        window: usize,
        #[arg(long, default_value_t = 10.0)]
        cost_bps: f64,
        #[arg(long, default_value_t = false)]
        soft: bool,
        #[arg(long = "aux", value_name = "NAME=PATH")]
        aux: Vec<String>,
        #[arg(long, default_value = "portfolio.json")]
        out: PathBuf,
        #[arg(long)]
        traces: Option<PathBuf>,
        #[arg(long, default_value = "")]
        llm_model: String,
        #[arg(long, default_value = "")]
        llm_base_url: String,
        #[arg(long, default_value = ".rquant-cache/llm")]
        llm_cache_dir: PathBuf,
    },
```
分流：复用 LLM 构造逻辑（与 Backtest 臂同式；用 `llm_enabled`）+ `--aux` 解析（同 Backtest 臂代码式样）→ `PortfolioConfig` → `run_portfolio` → `print_portfolio_summary`。

- [ ] **Step 2: e2e（`tests/e2e.rs`）**

`portfolio_full_chain`：3 个 tempfile CSV 合成标的（同 Task 3 集成测试形态：A 涨/B 平/C 跌，时间跨多日）+ universe tempfile + 简单动量树（行内 YAML）→ `run_portfolio(&cfg, &LlmEvaluator::Disabled)` → 断言 selected 全为 A、超额为正、out JSON 写出可反序列化为 `PortfolioReport`。

- [ ] **Step 3: 验证 + Commit**

`cargo test` 全绿；clippy 干净；`cargo run -- portfolio --help` 全旗标可见。
```bash
git add src/cli/mod.rs tests/e2e.rs
git commit -m "feat(cli): portfolio subcommand; e2e cross-sectional full chain" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 5: 文档 + 真数据 smoke

**Files:**
- Modify: `docs/cli-reference.md`、`docs/architecture.md`、`README.md`

- [ ] **Step 1: 文档**

- cli-reference：`portfolio` 子命令全旗标表 + universe CSV 格式 + 新鲜度/停牌语义 + 基准口径。
- architecture：组合层一段（第四种入口；单标的三模式之上的横截面编排）。
- README：portfolio 一节（命令示例 + 选股语义两行 + 诚实边界：伪概率只排序、等权、纯多头、T+1 不强制）。

- [ ] **Step 2: 真数据 smoke（手动，产物不入库）**

fetch 4 只真股票 60m（如 sh600000/sh600036/sz000001/sz000002）→ 写 universe.csv → `portfolio --top 2 --rebalance 8 --soft --tree examples/strength_tree.yaml --warmup 60` → 摘要数字记入报告（总收益/基准/超额/换手 sane）→ 清理。

- [ ] **Step 3: 全绿 + Commit**

```bash
git add docs README.md
git commit -m "docs: portfolio subcommand reference, architecture note, README section" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## 附录 A：Spec 覆盖对照（自检）
| Spec § | 实现于 |
|---|---|
| §3.1 universe 读取器 | Task 1 |
| §3.2 时间线/新鲜度/打分/select_top | Task 2 |
| §3.2 记账（accrue/turnover/基准/停牌计价/T+1 提示）| Task 3 |
| §3.3 报告/traces/print | Task 3/4 |
| §3.4 CLI | Task 4 |
| §4 测试 + 文档 + smoke | Task 1-5 |

## 附录 B：明确不在范围（YAGNI）
- 做空腿/分数加权/中性化/news 输入/打分并发/HTML/T+1 强制/期末清算。
