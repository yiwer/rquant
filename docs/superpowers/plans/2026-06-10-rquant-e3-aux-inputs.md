# rquant E3 广义数据输入（aux 外部序列）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `--aux name=path.csv`（可重复）挂载任意外部数值序列（通用列 CSV），DSL 经 `aux.<name>.<column>` 引用，与 primary 同走 time≤t 闸门；低频序列自然取最近已知值。

**Architecture:** 在 master(HEAD `c9840f3`)上扩展。新 `data/aux_table.rs` 通用读取器；`Context` 加 `aux: BTreeMap<String, AuxView>`（build_context 截断）；`resolve_series` 解析 `aux.` 三段名；loader 左移格式校验；CLI/Config/runner/run_soft 接线。

**Tech Stack:** Rust 2024 + 既有（csv/chrono/serde）。

> 设计依据：`docs/superpowers/specs/2026-06-10-rquant-e3-aux-inputs-design.md`。提交信息用英文。**机械细节以实际代码为准**（先读目标文件）。

---

## 文件结构
```
新增: src/data/aux_table.rs       # AuxTable + read_aux_csv + 测试
改动: src/data/mod.rs             # + pub mod aux_table;
改动: src/features/context.rs     # AuxView + Context.aux + build_context 加参 + 闸门测试
改动: （Context/build_context 涟漪）src/dsl/eval.rs, src/eval/quant.rs, src/engine/soft.rs,
       src/eval/llm/mod.rs, src/eval/llm/prompt.rs 的测试助手；src/backtest/runner.rs, src/backtest/soft.rs
改动: src/dsl/eval.rs             # resolve_series aux. 分支
改动: src/tree/loader.rs          # check_no_unknown_idents aux 三段校验
改动: src/cli/mod.rs              # --aux 解析
改动: tests/e2e.rs（Config 涟漪 + 新 e2e）、docs/{dsl-reference,cli-reference,tree-yaml-schema}.md、README.md
```

---

## Task 1: aux_table.rs 读取器

**Files:**
- Create: `src/data/aux_table.rs`；Modify: `src/data/mod.rs`（+ `pub mod aux_table;`）
- Test: 同文件

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
    fn reads_multi_column_and_daily_format() {
        let f = write_tmp("time,netbuy,pe\n2024-01-02,1.5,12.0\n2024-01-03 10:00:00,-0.5,12.1\n");
        let t = read_aux_csv(f.path()).unwrap();
        assert_eq!(t.times.len(), 2);
        assert_eq!(t.times[0], chrono::NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(0, 0, 0).unwrap());
        assert_eq!(t.cols["netbuy"], vec![1.5, -0.5]);
        assert_eq!(t.cols["pe"], vec![12.0, 12.1]);
    }

    #[test]
    fn rejects_bad_inputs() {
        // 非递增
        assert!(read_aux_csv(write_tmp("time,v\n2024-01-03,1\n2024-01-02,2\n").path()).is_err());
        // 坏数值
        assert!(read_aux_csv(write_tmp("time,v\n2024-01-02,abc\n").path()).is_err());
        // 首列非 time
        assert!(read_aux_csv(write_tmp("t,v\n2024-01-02,1\n").path()).is_err());
        // 列名含点
        assert!(read_aux_csv(write_tmp("time,a.b\n2024-01-02,1\n").path()).is_err());
        // 坏时间
        assert!(read_aux_csv(write_tmp("time,v\nnot-a-date,1\n").path()).is_err());
    }
}
```

- [ ] **Step 2: RED**

Run: `cargo test --lib data::aux_table` → 编译失败。

- [ ] **Step 3: 实现（mirror `data/reader.rs` 的 csv 读取风格；以实际为准）**

```rust
use crate::{Error, Result};
use chrono::NaiveDateTime;
use std::collections::BTreeMap;
use std::path::Path;

/// 通用外部序列表：time + 任意数值列（列名即 DSL 字段名）。
pub struct AuxTable {
    pub times: Vec<NaiveDateTime>,
    pub cols: BTreeMap<String, Vec<f64>>,
}

fn parse_aux_time(s: &str) -> Result<NaiveDateTime> {
    if let Ok(t) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Ok(t);
    }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Ok(d.and_hms_opt(0, 0, 0).unwrap());
    }
    Err(Error::Data(format!("bad aux time '{s}'")))
}

/// 读通用 aux CSV：首列必须 time（带时分秒或日频）；时间严格递增；其余列 f64。
pub fn read_aux_csv(path: &Path) -> Result<AuxTable> {
    let mut rdr = csv::Reader::from_path(path)?;
    let headers = rdr.headers()?.clone();
    if headers.is_empty() || &headers[0] != "time" {
        return Err(Error::Data("aux csv first column must be 'time'".into()));
    }
    let names: Vec<String> = headers.iter().skip(1).map(|h| h.trim().to_string()).collect();
    for n in &names {
        if n.is_empty() || n.contains('.') || n.contains(char::is_whitespace) {
            return Err(Error::Data(format!("bad aux column name '{n}'")));
        }
    }
    let mut times = Vec::new();
    let mut cols: BTreeMap<String, Vec<f64>> = names.iter().map(|n| (n.clone(), Vec::new())).collect();
    for rec in rdr.records() {
        let rec = rec?;
        let t = parse_aux_time(&rec[0])?;
        if let Some(last) = times.last()
            && *last >= t
        {
            return Err(Error::Data(format!("aux non-increasing time at {t}")));
        }
        times.push(t);
        for (j, n) in names.iter().enumerate() {
            let raw = &rec[j + 1];
            let v: f64 = raw.trim().parse().map_err(|_| Error::Data(format!("bad aux value '{raw}' in column '{n}'")))?;
            cols.get_mut(n).unwrap().push(v);
        }
    }
    Ok(AuxTable { times, cols })
}
```
（若 `Error` 无 `From<csv::Error>`，按 `reader.rs` 现行方式 map_err；以实际为准。）

- [ ] **Step 4: GREEN + Commit**

Run: `cargo test --lib data::aux_table` → 2 PASS。
```bash
git add src/data/aux_table.rs src/data/mod.rs
git commit -m "feat(data): generic aux series table reader (time + arbitrary numeric columns)" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: Context.aux + build_context（涟漪一次切）

**Files:**
- Modify: `src/features/context.rs` + 全部 `Context {` 字面量与 `build_context(` 调用点（**grep 找全**：src/dsl/eval.rs、src/eval/quant.rs、src/engine/soft.rs、src/eval/llm/mod.rs、src/eval/llm/prompt.rs 的测试助手；src/backtest/runner.rs、src/backtest/soft.rs 的 eval_point*）

- [ ] **Step 1: context.rs 失败测试**

```rust
    #[test]
    fn aux_tables_gated_by_time() {
        use crate::data::aux_table::AuxTable;
        let base = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        let bars: Vec<Bar> = (0..3).map(|i| Bar {
            time: base + chrono::Duration::minutes(i * 15),
            open: 1.0, high: 1.0, low: 1.0, close: 1.0, volume: 1.0,
        }).collect();
        let mut aux = std::collections::BTreeMap::new();
        aux.insert("idx".to_string(), AuxTable {
            times: vec![base, base + chrono::Duration::minutes(15), base + chrono::Duration::minutes(45)],
            cols: std::collections::BTreeMap::from([("v".to_string(), vec![1.0, 2.0, 3.0])]),
        });
        // t = 第 2 根 bar（base+15）：第三行（base+45）必须被闸门挡住
        let ctx = build_context(&bars, &bars, &[], &aux, bars[1].time, 100);
        assert_eq!(ctx.aux["idx"].cols["v"], vec![1.0, 2.0]);
        // t 早于首行 → 空
        let ctx2 = build_context(&bars, &bars, &[], &aux, base - chrono::Duration::minutes(1), 100);
        assert!(ctx2.aux["idx"].cols["v"].is_empty());
    }
```

- [ ] **Step 2: RED → 实现**

(a) `context.rs` 加：
```rust
use crate::data::aux_table::AuxTable;

/// 已按 time≤t 截断的 aux 列视图。
#[derive(Debug, Clone)]
pub struct AuxView {
    pub cols: std::collections::BTreeMap<String, Vec<f64>>,
}
```
`Context` 加字段 `pub aux: std::collections::BTreeMap<String, AuxView>,`。
(b) `build_context` 签名加参 `aux: &std::collections::BTreeMap<String, AuxTable>`（放 `news` 之后、`t` 之前），函数体加：
```rust
    let aux_views = aux
        .iter()
        .map(|(name, table)| {
            let cut = table.times.partition_point(|x| *x <= t);
            let cols = table.cols.iter().map(|(c, v)| (c.clone(), v[..cut].to_vec())).collect();
            (name.clone(), AuxView { cols })
        })
        .collect();
```
构造 `Context { ..., aux: aux_views }`。
(c) **涟漪**：grep `Context {` 与 `build_context(`——所有测试助手字面量补 `aux: std::collections::BTreeMap::new(),`；`runner.rs`/`soft.rs` 的 `eval_point`/`eval_point_soft` 加参 `aux: &BTreeMap<String, AuxTable>` 并传给 `build_context`（本任务先在 `run`/`run_soft` 处传 `&BTreeMap::new()` 占位，Task 4 接 CLI）；context.rs 既有测试的 build_context 调用补 `&BTreeMap::new()`。

- [ ] **Step 3: GREEN + 全量 + Commit**

Run: `cargo test` → 全绿（既有行为零变化）。`cargo clippy --all-targets` 干净。
```bash
git add -A src
git commit -m "feat(features): Context.aux views with time<=t gating in build_context" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: DSL aux 解析 + loader 格式左移

**Files:**
- Modify: `src/dsl/eval.rs`（resolve_series）、`src/tree/loader.rs`（check_no_unknown_idents）
- Test: 两文件

- [ ] **Step 1: eval 失败测试**

（测试助手 `ctx_from_closes` 经 Task 2 已有 `aux` 空字段；本测试需带 aux 的 ctx——加一个局部构造：）
```rust
    #[test]
    fn aux_identifier_resolves_and_gates() {
        let mut ctx = ctx_from_closes(&[1.0, 2.0, 3.0]);
        ctx.aux.insert("idx".to_string(), crate::features::context::AuxView {
            cols: std::collections::BTreeMap::from([("v".to_string(), vec![10.0, 20.0])]),
        });
        let f = |src: &str, c: &Context| eval(&parse_str(src).unwrap(), c).unwrap();
        // 归约取 last
        assert_eq!(f("aux.idx.v == 20", &ctx), Value::Bool(true));
        assert_eq!(f("aux.idx.v[-1] == 10", &ctx), Value::Bool(true));
        // 缺列/缺表 → Err
        assert!(eval(&parse_str("aux.idx.nope > 0").unwrap(), &ctx).is_err());
        assert!(eval(&parse_str("aux.none.v > 0").unwrap(), &ctx).is_err());
        // 空截断 → NaN → 比较 false（弃权）
        ctx.aux.get_mut("idx").unwrap().cols.insert("v".to_string(), vec![]);
        assert_eq!(f("aux.idx.v > 0", &ctx), Value::Bool(false));
    }
```

- [ ] **Step 2: RED → 实现 resolve_series**

`resolve_series` 开头加：
```rust
    if let Some(rest) = name.strip_prefix("aux.") {
        let (table, column) = rest
            .split_once('.')
            .ok_or_else(|| Error::Eval(format!("aux identifier must be aux.<table>.<column>: '{name}'")))?;
        let view = ctx.aux.get(table).ok_or_else(|| {
            Error::Eval(format!("aux table '{table}' not mounted (use --aux {table}=path.csv)"))
        })?;
        return view
            .cols
            .get(column)
            .cloned()
            .ok_or_else(|| Error::Eval(format!("aux table '{table}' has no column '{column}'")));
    }
```
> 先验证词法：`parse_str("aux.idx.v")` 应产出含两个点的单 Ident（`ctx.close` 同机制）；若 lexer 对第二个点断开，需在 lexer 的 ident 累积处确认 `.` 已被包含（以实际代码为准，必要时修 lexer 并加 lexer 测试）。

- [ ] **Step 3: loader 左移**

`check_no_unknown_idents` 的 `aux.` 分支（现为 `name.starts_with("ctx.")` 旁）改为格式校验：
```rust
            if let Some(rest) = name.strip_prefix("aux.") {
                return match rest.split_once('.') {
                    Some((t, c)) if !t.is_empty() && !c.is_empty() && !c.contains('.') => Ok(()),
                    _ => Err(Error::Tree(format!("{where_}: aux identifier must be aux.<table>.<column>, got '{name}'"))),
                };
            }
```
loader 测试：
```rust
    #[test]
    fn aux_identifier_format_validated_at_load() {
        let yaml = |when: &str| format!(r#"
meta: {{ name: t, forward_window: 3, stances: [long, flat] }}
root: a
nodes:
  a:
    type: quant
    branches: [ {{ when: "{when}", goto: leaf_l, label: up }} ]
    default: {{ goto: leaf_f, label: flat }}
leaves:
  leaf_l: {{ stance: long }}
  leaf_f: {{ stance: flat }}
"#);
        assert!(load_tree_str(&yaml("aux.idx.close > 0")).is_ok());
        assert!(load_tree_str(&yaml("aux.idx > 0")).is_err());
    }
```

- [ ] **Step 4: GREEN + Commit**

Run: `cargo test` 全绿；clippy 干净。
```bash
git add src/dsl/eval.rs src/tree/loader.rs
git commit -m "feat(dsl,tree): aux.<table>.<column> identifiers with load-time format check" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: CLI `--aux` + 编排 + e2e + 文档

**Files:**
- Modify: `src/cli/mod.rs`、`src/backtest/runner.rs`、`src/backtest/soft.rs`、`tests/e2e.rs`、`docs/{dsl-reference,cli-reference,tree-yaml-schema}.md`、`README.md`

- [ ] **Step 1: Config + CLI**

(a) `BacktestConfig` 加 `pub aux_paths: Vec<(String, PathBuf)>,`（**涟漪**：grep e2e 全部 `BacktestConfig {` 补 `aux_paths: vec![],`；cli 构造点）。
(b) cli `Backtest` 变体加：
```rust
        /// Mount an external series table: --aux name=path.csv (repeatable); DSL: aux.<name>.<column>
        #[arg(long = "aux", value_name = "NAME=PATH")]
        aux: Vec<String>,
```
解构 `aux`，解析：
```rust
            let mut aux_paths = Vec::new();
            for spec in &aux {
                let (n, p) = spec.split_once('=').ok_or_else(|| anyhow::anyhow!("--aux expects NAME=PATH, got '{spec}'"))?;
                if aux_paths.iter().any(|(en, _): &(String, PathBuf)| en == n) {
                    return Err(anyhow::anyhow!("duplicate --aux name '{n}'"));
                }
                aux_paths.push((n.to_string(), PathBuf::from(p)));
            }
```
传入 `BacktestConfig { ..., aux_paths }`。
(c) `run`/`run_soft`：把 Task 2 的占位空表换成真加载：
```rust
    let mut aux_tables: std::collections::BTreeMap<String, crate::data::aux_table::AuxTable> = std::collections::BTreeMap::new();
    for (name, p) in &cfg.aux_paths {
        aux_tables.insert(name.clone(), crate::data::aux_table::read_aux_csv(p)?);
    }
```
并传 `&aux_tables` 给 `eval_point*`。

- [ ] **Step 2: e2e**

新增 `aux_relative_strength_full_chain`：复用 `gen_primary_csv`/`gen_context_csv` 上升趋势 fixture；写一个 aux CSV tempfile（time 与 primary 对齐或更稀疏，列 `v` 缓慢上升、慢于 primary 涨速）；行内树 `when: "close/close[-5] > aux.idx.v/aux.idx.v[-5]"`（forward_window 4、warmup 6）；`BacktestConfig.aux_paths = vec![("idx".into(), aux_f.path().to_path_buf())]`；`run` 硬模式断言 `m.scored > 0 && m.active.count > 0`（primary 相对强势 → 分支触发）。

- [ ] **Step 3: 验证**

Run: `cargo test` 全绿；clippy 干净；`cargo run -- backtest --help` 含 `--aux`。

- [ ] **Step 4: 文档**

- `docs/cli-reference.md`：backtest 表加 `--aux NAME=PATH（可重复）` + 一段格式说明（首列 time 两种格式/任意数值列/严格递增）。
- `docs/dsl-reference.md`：标识符表加 `aux.<表>.<列>` 行 + 小节（time≤t 闸门、低频取最近已知值、公告滞后由行时间表达、空截断弃权、缺表运行时报错文案）。
- `docs/tree-yaml-schema.md`：校验规则补 aux 三段格式左移一条。
- `README.md`：Quick Start 区一句示例 `--aux idx=index.csv`。

- [ ] **Step 5: Commit**

```bash
git add -A src tests docs README.md
git commit -m "feat(cli,backtest): --aux mounting wired through run/run_soft; e2e + docs" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## 附录 A：Spec 覆盖对照（自检）
| Spec § | 实现于 |
|---|---|
| §3.1 读取器（双时间格式/校验）| Task 1 |
| §3.2 Context/AuxView/闸门 + 涟漪 | Task 2 |
| §3.3 resolve_series aux 分支（含词法验证）| Task 3 |
| §3.4 loader 三段格式左移 | Task 3 |
| §3.5 CLI/Config/编排 | Task 4 |
| §4 测试 + 文档 | Task 1-4 |

## 附录 B：明确不在范围（YAGNI）
- LLM inputs 开放 aux；aux 抓取器；重采样/合并；树内 aux 依赖声明。
