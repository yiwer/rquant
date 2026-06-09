# rquant M5（接入 LLM 节点：OpenAI 标准 + 异步）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让决策树的 LLM 节点经 OpenAI 标准 `chat/completions`（可配 base_url → DashScope/DeepSeek）求值，强制 JSON 输出 `{label,confidence,reason}`、temp=0、内容寻址文件缓存；引擎异步化、有序并发；新增可选新闻输入；LLM 不可用即回退 default。

**Architecture:** 在已合并的 M1–M4（HEAD `0b51889`）上扩展。新增 `eval/llm/`（cache/prompt/client + LlmEvaluator 枚举）与 `data/news.rs`；把 `traverse`/`runner`/`cli` 改 async；`Context` 加 `news` 字段。LLM 评估器用枚举派发（OpenAi/Disabled/Stub），不引 async-trait、不用 dyn。

**Tech Stack:** Rust 2024 + 既有(chrono/serde/serde_yaml/serde_json/csv/clap/thiserror/anyhow) + 新增 **tokio**(rt-multi-thread,macros)、**reqwest**(json,rustls-tls,关默认特性)、**futures**(buffered)、**sha2**(缓存键)；dev: approx/tempfile。

> 设计依据：`docs/superpowers/specs/2026-06-09-rquant-m5-llm-nodes-design.md`。
> 提交信息用英文（PowerShell 5.1 中文 git 参数会乱码）。单元测试用同文件 `#[cfg(test)] mod tests`。异步测试用 `#[tokio::test]`。

---

## 文件结构

```
新增:
  src/data/news.rs                 # NewsRecord / NewsView / read_news_csv
  src/eval/llm/mod.rs              # LlmEvaluator 枚举 + LlmConfig + LlmNode + 决策助手 + StubLlm
  src/eval/llm/cache.rs            # 内容寻址文件缓存 (Cached / FileCache)
  src/eval/llm/prompt.rs           # render_user / SYSTEM_PROMPT / parse_answer
  src/eval/llm/client.rs           # OpenAiLlm (reqwest 调用/重试/回退/缓存接线)
改动:
  Cargo.toml / .gitignore
  src/data/mod.rs                  # + pub mod news;
  src/eval/mod.rs                  # + pub mod llm;
  src/features/context.rs          # Context 加 news 字段；build_context 加 news 参数
  src/engine/traversal.rs          # traverse 改 async + 接评估器
  src/backtest/runner.rs           # run 改 async + 并发 + 新闻/LLM 接线
  src/cli/mod.rs                   # #[tokio::main] + 新 flags + 启用判据
  src/dsl/eval.rs / src/eval/quant.rs  # 测试助手 Context 字面量补 news:None
  tests/e2e.rs                     # async 适配 + 新增 LLM(Stub) 端到端
```

---

## Task 1: 依赖 + .gitignore

**Files:**
- Modify: `Cargo.toml`
- Modify: `.gitignore`

- [ ] **Step 1: 加依赖到 Cargo.toml 的 `[dependencies]`**

在现有 `[dependencies]` 末尾追加：
```toml
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
futures = "0.3"
sha2 = "0.10"
```

- [ ] **Step 2: 确保 .gitignore 忽略 target/ 与 LLM 缓存**

Read `.gitignore`（仓库已有但未跟踪）。确保包含以下两行（缺则补）：
```
/target
/.rquant-cache
```

- [ ] **Step 3: 构建（首次会下载 tokio/reqwest 依赖树，较慢，属正常）**

Run: `cargo build`
Expected: 编译通过（新依赖下载并编译）。

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock .gitignore
git commit -m "build(m5): add tokio/reqwest/futures/sha2; ignore llm cache" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: data/news.rs — 新闻记录与加载器

**Files:**
- Create: `src/data/news.rs`
- Modify: `src/data/mod.rs`（+ `pub mod news;`）
- Test: 同文件

- [ ] **Step 1: 在 `src/data/mod.rs` 增加声明**

在 `src/data/mod.rs` 追加一行：
```rust
pub mod news;
```

- [ ] **Step 2: 写失败测试（`src/data/news.rs`）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp(content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        write!(f, "{content}").unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn reads_news_csv() {
        let f = tmp("time,score,headline\n2024-01-02 09:30:00,0.8,good A\n2024-01-02 10:00:00,-0.5,bad B\n");
        let n = read_news_csv(f.path()).unwrap();
        assert_eq!(n.len(), 2);
        assert_eq!(n[0].score, 0.8);
        assert_eq!(n[1].headline, "bad B");
    }

    #[test]
    fn rejects_out_of_order() {
        let f = tmp("time,score,headline\n2024-01-02 10:00:00,0.1,a\n2024-01-02 09:00:00,0.1,b\n");
        assert!(read_news_csv(f.path()).is_err());
    }
}
```

- [ ] **Step 3: 运行验证失败**

Run: `cargo test --lib data::news`
Expected: 编译失败（`read_news_csv` 未定义）。

- [ ] **Step 4: 写实现（测试上方）**

```rust
use crate::{Error, Result};
use chrono::NaiveDateTime;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct NewsRecord {
    pub time: NaiveDateTime,
    pub score: f64,
    pub headline: String,
}

/// 决策时点可见的最近若干条新闻（time <= t）。仅供 LLM 渲染读取。
#[derive(Debug, Clone)]
pub struct NewsView {
    pub recent: Vec<NewsRecord>,
}

#[derive(serde::Deserialize)]
struct Row {
    time: String,
    score: f64,
    headline: String,
}

/// 读取新闻 CSV（表头 time,score,headline）为按时间升序的记录。
/// 允许同一时间多条，但不允许时间回退。
pub fn read_news_csv(path: &Path) -> Result<Vec<NewsRecord>> {
    let mut rdr = csv::Reader::from_path(path)?;
    let mut out: Vec<NewsRecord> = Vec::new();
    for rec in rdr.deserialize() {
        let row: Row = rec?;
        let time = NaiveDateTime::parse_from_str(&row.time, "%Y-%m-%d %H:%M:%S")
            .map_err(|e| Error::Data(format!("bad news time '{}': {e}", row.time)))?;
        if let Some(prev) = out.last()
            && time < prev.time
        {
            return Err(Error::Data(format!("news time out of order at {time}")));
        }
        out.push(NewsRecord { time, score: row.score, headline: row.headline });
    }
    Ok(out)
}
```

- [ ] **Step 5: 运行验证通过**

Run: `cargo test --lib data::news`
Expected: 两个测试 PASS。

- [ ] **Step 6: Commit**

```bash
git add src/data/news.rs src/data/mod.rs
git commit -m "feat(data): news record type and CSV loader" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: Context.news + build_context（含 M1–M4 字段涟漪）

**Files:**
- Modify: `src/features/context.rs`（加 `news` 字段 + `build_context` 加参数 + 新闻防未来函数 + 适配 2 个既有测试）
- Modify: `src/dsl/eval.rs`（测试助手 `Context{}` 补 `news: None`）
- Modify: `src/eval/quant.rs`（同上）
- Modify: `src/engine/traversal.rs`（同上，测试助手）
- Modify: `src/backtest/runner.rs`（`build_context` 调用补 `&[]` 新闻参数，保持同步可编译）

> 说明：给 `Context` 加字段会让所有 `Context{}` 字面量与 `build_context` 调用点都要更新，本任务一次性改完，保证 `cargo test` 仍绿。

- [ ] **Step 1: 改 `src/features/context.rs`（实现 + 新增新闻防未来函数测试）**

把文件内容改为（在原有基础上加 `news`）：
```rust
use crate::data::bar::{Bar, Window};
use crate::data::news::{NewsRecord, NewsView};
use chrono::NaiveDateTime;

/// 决策时点上下文：节点能看到的全部信息（绝不含未来）。
#[derive(Debug, Clone)]
pub struct Context {
    pub t: NaiveDateTime,
    pub primary: Window,
    pub context: Window,
    pub news: Option<NewsView>,
}

fn trailing_visible(bars: &[Bar], t: NaiveDateTime, window: usize) -> Vec<Bar> {
    let visible_end = bars.partition_point(|b| b.time <= t);
    let start = visible_end.saturating_sub(window);
    bars[start..visible_end].to_vec()
}

/// 构建 t 时刻的 Context：小/大周期各取最近 window 根可见 bar；
/// news 非空时取 time<=t 的最近 5 条（同 partition_point 闸门），空切片则 news=None。
pub fn build_context(
    primary: &[Bar],
    context: &[Bar],
    news: &[NewsRecord],
    t: NaiveDateTime,
    window: usize,
) -> Context {
    let news_view = if news.is_empty() {
        None
    } else {
        let end = news.partition_point(|n| n.time <= t);
        let start = end.saturating_sub(5);
        Some(NewsView { recent: news[start..end].to_vec() })
    };
    Context {
        t,
        primary: Window { bars: trailing_visible(primary, t, window) },
        context: Window { bars: trailing_visible(context, t, window) },
        news: news_view,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::bar::Bar;
    use crate::data::news::NewsRecord;
    use chrono::NaiveDate;

    fn bar_at(min_from_open: i64, price: f64) -> Bar {
        let base = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        let time = base + chrono::Duration::minutes(min_from_open);
        Bar { time, open: price, high: price, low: price, close: price, volume: 1.0 }
    }
    fn series(n: usize) -> Vec<Bar> {
        (0..n).map(|i| bar_at(i as i64 * 15, i as f64)).collect()
    }

    #[test]
    fn window_takes_trailing_visible_bars() {
        let primary = series(10);
        let t = primary[5].time;
        let ctx = build_context(&primary, &[], &[], t, 3);
        assert_eq!(ctx.primary.bars.len(), 3);
        assert_eq!(ctx.primary.bars.last().unwrap().close, 5.0);
        assert!(ctx.news.is_none());
    }

    #[test]
    fn no_future_bar_leaks_property() {
        let primary = series(50);
        for i in 0..primary.len() {
            let t = primary[i].time;
            let ctx = build_context(&primary, &primary, &[], t, 100);
            for b in &ctx.primary.bars {
                assert!(b.time <= t, "future primary bar leaked at i={i}");
            }
            for b in &ctx.context.bars {
                assert!(b.time <= t, "future context bar leaked at i={i}");
            }
        }
    }

    #[test]
    fn news_respects_lookahead() {
        let news = vec![
            NewsRecord { time: bar_at(0, 0.0).time, score: 0.5, headline: "n0".into() },
            NewsRecord { time: bar_at(150, 0.0).time, score: -0.5, headline: "n1".into() },
        ];
        let primary = series(20);
        let t = primary[3].time; // 早于第二条新闻
        let ctx = build_context(&primary, &[], &news, t, 100);
        let v = ctx.news.unwrap();
        for r in &v.recent {
            assert!(r.time <= t, "future news leaked");
        }
        assert_eq!(v.recent.len(), 1);
    }
}
```

- [ ] **Step 2: 改 `src/backtest/runner.rs` 的 build_context 调用**

把 `let ctx = build_context(&primary, &context, t, cfg.window);` 改为：
```rust
let ctx = build_context(&primary, &context, &[], t, cfg.window);
```
（runner 仍同步；Task 9 再做 async + 真实新闻。）

- [ ] **Step 3: 给三个测试助手的 `Context{}` 字面量补 `news: None`**

在 `src/dsl/eval.rs`、`src/eval/quant.rs`、`src/engine/traversal.rs` 的测试 `ctx(...)` 助手里，把
```rust
Context { t, primary: Window { bars: bars.clone() }, context: Window { bars } }
```
改为
```rust
Context { t, primary: Window { bars: bars.clone() }, context: Window { bars }, news: None }
```

- [ ] **Step 4: 运行全量验证通过**

Run: `cargo test`
Expected: 全部 PASS（含新的 `news_respects_lookahead`；既有断言不变）。

- [ ] **Step 5: Commit**

```bash
git add src/features/context.rs src/backtest/runner.rs src/dsl/eval.rs src/eval/quant.rs src/engine/traversal.rs
git commit -m "feat(features): add news to Context with look-ahead-safe build_context" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: eval/llm 基础（mod foundation + 内容寻址缓存）

**Files:**
- Modify: `src/eval/mod.rs`（+ `pub mod llm;`）
- Create: `src/eval/llm/mod.rs`（LlmNode / LlmConfig / 决策助手 / StubLlm / `pub mod cache;`）
- Create: `src/eval/llm/cache.rs`
- Test: 两文件各自同文件

- [ ] **Step 1: `src/eval/mod.rs` 增加 `pub mod llm;`**

在 `src/eval/mod.rs` 顶部（`pub mod quant;` 旁）加：
```rust
pub mod llm;
```

- [ ] **Step 2: 写 `src/eval/llm/cache.rs`（含测试）**

```rust
use crate::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cached {
    pub label: String,
    pub confidence: f64,
    pub reason: String,
    pub model: String,
}

pub struct FileCache {
    dir: PathBuf,
}

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

impl FileCache {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// 缓存键 = sha256_hex(model \0 node_id \0 rendered)。必须随渲染确定。
    pub fn key(model: &str, node_id: &str, rendered: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(model.as_bytes());
        h.update([0u8]);
        h.update(node_id.as_bytes());
        h.update([0u8]);
        h.update(rendered.as_bytes());
        let d = h.finalize();
        let mut s = String::with_capacity(64);
        for b in d {
            s.push_str(&format!("{b:02x}"));
        }
        s
    }

    fn path_for(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.json"))
    }

    pub fn get(&self, key: &str) -> Option<Cached> {
        let s = std::fs::read_to_string(self.path_for(key)).ok()?;
        serde_json::from_str(&s).ok()
    }

    /// 原子写：写唯一临时文件再 rename（并发各写各的、崩溃不留半截）。
    pub fn put(&self, key: &str, c: &Cached) -> Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let uniq = format!(".{}.{}.tmp", std::process::id(), TMP_COUNTER.fetch_add(1, Ordering::Relaxed));
        let tmp = self.dir.join(uniq);
        std::fs::write(&tmp, serde_json::to_string(c)?)?;
        std::fs::rename(&tmp, self.path_for(key))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_is_stable_and_sensitive() {
        let a = FileCache::key("m", "n", "rendered");
        let b = FileCache::key("m", "n", "rendered");
        let c = FileCache::key("m", "n", "different");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn put_then_get_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let cache = FileCache::new(dir.path());
        let key = FileCache::key("m", "n", "r");
        assert!(cache.get(&key).is_none());
        let c = Cached { label: "go".into(), confidence: 0.7, reason: "ok".into(), model: "m".into() };
        cache.put(&key, &c).unwrap();
        assert_eq!(cache.get(&key).unwrap(), c);
    }
}
```

- [ ] **Step 3: 写 `src/eval/llm/mod.rs`（基础类型 + 助手 + StubLlm + 测试）**

```rust
pub mod cache;

use crate::eval::Decision;
use crate::features::context::Context;
use crate::Result;
use std::collections::HashMap;
use std::path::PathBuf;

/// traverse 传入的 LLM 节点借用视图。
pub struct LlmNode<'a> {
    pub inputs: &'a [String],
    pub prompt: &'a str,
    pub labels: &'a HashMap<String, String>,
    pub default: &'a str,
}

#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub timeout_secs: u64,
    pub max_retries: u32,
    pub cache_dir: PathBuf,
}

/// LLM 不可用/失败时的回退（走节点 default）。
pub fn default_decision(node: &LlmNode<'_>, why: &str) -> Decision {
    Decision {
        goto: node.default.to_string(),
        label: "default".to_string(),
        confidence: 0.0,
        rationale: format!("{why}: default branch"),
    }
}

/// 把 LLM 给的 label 映射成 Decision（goto = node.labels[label]，缺失则回退 default）。
pub fn decision_from_answer(node: &LlmNode<'_>, label: &str, confidence: f64, reason: &str, cached: bool) -> Decision {
    let goto = node.labels.get(label).cloned().unwrap_or_else(|| node.default.to_string());
    let tag = if cached { "LLM(cached)" } else { "LLM" };
    Decision { goto, label: label.to_string(), confidence, rationale: format!("{tag}: {reason}") }
}

/// 测试用 stub：node_id -> label（"ERROR" 模拟失败 → 回退 default）。
pub struct StubLlm {
    pub answers: HashMap<String, String>,
}
impl StubLlm {
    pub fn eval(&self, node_id: &str, node: &LlmNode<'_>, _ctx: &Context) -> Result<Decision> {
        match self.answers.get(node_id) {
            Some(l) if l == "ERROR" => Ok(default_decision(node, "LLM stub error")),
            Some(l) if node.labels.contains_key(l) => Ok(decision_from_answer(node, l, 0.9, "stub", false)),
            _ => Ok(default_decision(node, "LLM stub no-answer")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::bar::{Bar, Window};
    use chrono::NaiveDate;

    pub(super) fn ctx() -> Context {
        let t = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        Context { t, primary: Window { bars: vec![] }, context: Window { bars: vec![] }, news: None }
    }
    fn labels() -> HashMap<String, String> {
        HashMap::from([("go".to_string(), "leaf_l".to_string())])
    }

    #[test]
    fn stub_known_label_maps_goto() {
        let lbl = labels();
        let node = LlmNode { inputs: &[], prompt: "q", labels: &lbl, default: "leaf_f" };
        let stub = StubLlm { answers: HashMap::from([("n".to_string(), "go".to_string())]) };
        let d = stub.eval("n", &node, &ctx()).unwrap();
        assert_eq!(d.goto, "leaf_l");
        assert_eq!(d.label, "go");
    }

    #[test]
    fn stub_error_falls_back_to_default() {
        let lbl = labels();
        let node = LlmNode { inputs: &[], prompt: "q", labels: &lbl, default: "leaf_f" };
        let stub = StubLlm { answers: HashMap::from([("n".to_string(), "ERROR".to_string())]) };
        let d = stub.eval("n", &node, &ctx()).unwrap();
        assert_eq!(d.goto, "leaf_f");
        assert_eq!(d.label, "default");
    }
}
```

- [ ] **Step 4: 运行验证通过**

Run: `cargo test --lib eval::llm`
Expected: cache 2 + mod 2 = 4 个测试 PASS。

- [ ] **Step 5: Commit**

```bash
git add src/eval/mod.rs src/eval/llm/mod.rs src/eval/llm/cache.rs
git commit -m "feat(eval/llm): LlmConfig/LlmNode/decision helpers, StubLlm, content-addressed cache" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 5: eval/llm/prompt.rs — 渲染与响应解析

**Files:**
- Create: `src/eval/llm/prompt.rs`
- Modify: `src/eval/llm/mod.rs`（+ `pub mod prompt;`）
- Test: 同文件

- [ ] **Step 1: `src/eval/llm/mod.rs` 增加 `pub mod prompt;`**（放在 `pub mod cache;` 旁）

- [ ] **Step 2: 写失败测试（`src/eval/llm/prompt.rs`）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::bar::{Bar, Window};
    use crate::data::news::{NewsRecord, NewsView};
    use crate::features::context::Context;
    use chrono::NaiveDate;
    use std::collections::HashMap;

    fn ctx_with(closes: &[f64], news: Option<NewsView>) -> Context {
        let base = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        let bars: Vec<Bar> = closes.iter().enumerate().map(|(i, &c)| Bar {
            time: base + chrono::Duration::minutes(i as i64 * 15), open: c, high: c, low: c, close: c, volume: 1.0,
        }).collect();
        Context { t: base, primary: Window { bars: bars.clone() }, context: Window { bars }, news }
    }

    #[test]
    fn render_includes_prompt_sorted_labels_and_price() {
        let labels = HashMap::from([("b".to_string(), "x".to_string()), ("a".to_string(), "y".to_string())]);
        let node = LlmNode { inputs: &[], prompt: "trend?", labels: &labels, default: "d" };
        let s = render_user(&node, &ctx_with(&[1.0, 2.0, 3.0], None));
        assert!(s.contains("Question: trend?"));
        assert!(s.contains("Allowed labels: [a, b]"));
        assert!(s.contains("Latest close: 3.0000"));
    }

    #[test]
    fn render_news_inputs_present_and_absent() {
        let labels = HashMap::from([("go".to_string(), "x".to_string())]);
        let inputs = vec!["news_score".to_string(), "recent_headlines".to_string()];
        let node = LlmNode { inputs: &inputs, prompt: "q", labels: &labels, default: "d" };
        let base = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 30, 0).unwrap();
        let nv = NewsView { recent: vec![NewsRecord { time: base, score: 0.5, headline: "H".into() }] };
        let s = render_user(&node, &ctx_with(&[1.0], Some(nv)));
        assert!(s.contains("news_score: 0.5000"));
        assert!(s.contains("recent_headlines: H"));
        let s2 = render_user(&node, &ctx_with(&[1.0], None));
        assert!(s2.contains("news_score: none"));
        assert!(s2.contains("recent_headlines: none"));
    }

    #[test]
    fn parse_answer_valid_invalid_and_label_check() {
        let allowed = HashMap::from([("go".to_string(), "x".to_string())]);
        let ok = parse_answer("{\"label\":\"go\",\"confidence\":0.8,\"reason\":\"r\"}", &allowed).unwrap();
        assert_eq!(ok.label, "go");
        assert!(parse_answer("not json", &allowed).is_err());
        assert!(parse_answer("{\"label\":\"nope\"}", &allowed).is_err());
    }
}
```

- [ ] **Step 3: 运行验证失败**

Run: `cargo test --lib eval::llm::prompt`
Expected: 编译失败（`render_user`/`parse_answer` 未定义）。

- [ ] **Step 4: 写实现（测试上方）**

```rust
use crate::eval::llm::LlmNode;
use crate::features::context::Context;
use crate::{Error, Result};
use serde::Deserialize;

pub const SYSTEM_PROMPT: &str = "You are a financial-analysis classifier. Choose exactly one label from the allowed list. Respond ONLY with a JSON object: {\"label\": <one of the allowed labels>, \"confidence\": <number 0..1>, \"reason\": <short string>}.";

/// 渲染 user message。必须确定性（它是缓存键的一部分）：label 排序、价格定宽、inputs 按声明顺序。
pub fn render_user(node: &LlmNode<'_>, ctx: &Context) -> String {
    let mut s = String::new();
    s.push_str(&format!("Question: {}\n", node.prompt));

    let mut labels: Vec<&str> = node.labels.keys().map(|k| k.as_str()).collect();
    labels.sort_unstable();
    s.push_str(&format!("Allowed labels: [{}]\n", labels.join(", ")));

    let closes = ctx.primary.closes();
    let start = closes.len().saturating_sub(20);
    let recent: Vec<String> = closes[start..].iter().map(|c| format!("{c:.4}")).collect();
    s.push_str(&format!("Recent primary closes: [{}]\n", recent.join(", ")));
    if let Some(last) = closes.last() {
        s.push_str(&format!("Latest close: {last:.4}\n"));
    }

    for input in node.inputs {
        match input.as_str() {
            "news_score" => {
                let v = ctx.news.as_ref()
                    .and_then(|n| n.recent.last())
                    .map(|r| format!("{:.4}", r.score))
                    .unwrap_or_else(|| "none".to_string());
                s.push_str(&format!("news_score: {v}\n"));
            }
            "recent_headlines" => {
                let v = ctx.news.as_ref()
                    .filter(|n| !n.recent.is_empty())
                    .map(|n| n.recent.iter().map(|r| r.headline.clone()).collect::<Vec<_>>().join("; "))
                    .unwrap_or_else(|| "none".to_string());
                s.push_str(&format!("recent_headlines: {v}\n"));
            }
            other => s.push_str(&format!("{other}: unavailable\n")),
        }
    }
    s
}

#[derive(Debug, Deserialize)]
pub struct LlmAnswer {
    pub label: String,
    #[serde(default)]
    pub confidence: f64,
    #[serde(default)]
    pub reason: String,
}

/// 解析 LLM content（应为 JSON），并校验 label ∈ allowed。
pub fn parse_answer(content: &str, allowed: &std::collections::HashMap<String, String>) -> Result<LlmAnswer> {
    let ans: LlmAnswer = serde_json::from_str(content.trim())
        .map_err(|e| Error::Eval(format!("LLM output not valid JSON: {e}")))?;
    if !allowed.contains_key(&ans.label) {
        return Err(Error::Eval(format!("LLM label '{}' not in allowed labels", ans.label)));
    }
    Ok(ans)
}
```

- [ ] **Step 5: 运行验证通过**

Run: `cargo test --lib eval::llm::prompt`
Expected: 三个测试 PASS。

- [ ] **Step 6: Commit**

```bash
git add src/eval/llm/prompt.rs src/eval/llm/mod.rs
git commit -m "feat(eval/llm): deterministic prompt rendering and response parsing" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 6: eval/llm/client.rs — OpenAI 标准客户端

**Files:**
- Create: `src/eval/llm/client.rs`
- Modify: `src/eval/llm/mod.rs`（+ `pub mod client;`）
- Test: 同文件（仅"建请求/解析响应"边界，无真实网络）

- [ ] **Step 1: `src/eval/llm/mod.rs` 增加 `pub mod client;`**

- [ ] **Step 2: 写失败测试（`src/eval/llm/client.rs`）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn request_body_shape() {
        let b = build_request_body("deepseek-chat", "hello");
        assert_eq!(b["model"], "deepseek-chat");
        assert_eq!(b["temperature"], 0);
        assert_eq!(b["response_format"]["type"], "json_object");
        assert_eq!(b["messages"][1]["content"], "hello");
    }

    #[test]
    fn parses_openai_style_response() {
        let raw = r#"{"choices":[{"message":{"role":"assistant","content":"{\"label\":\"go\",\"confidence\":0.9,\"reason\":\"ok\"}"}}]}"#;
        let parsed: ChatResponse = serde_json::from_str(raw).unwrap();
        let content = parsed.choices.into_iter().next().unwrap().message.content;
        let allowed = HashMap::from([("go".to_string(), "leaf".to_string())]);
        let ans = crate::eval::llm::prompt::parse_answer(&content, &allowed).unwrap();
        assert_eq!(ans.label, "go");
        assert_eq!(ans.confidence, 0.9);
    }
}
```

- [ ] **Step 3: 运行验证失败**

Run: `cargo test --lib eval::llm::client`
Expected: 编译失败（`build_request_body`/`ChatResponse` 未定义）。

- [ ] **Step 4: 写实现（测试上方）**

```rust
use crate::eval::llm::cache::{Cached, FileCache};
use crate::eval::llm::prompt::{parse_answer, render_user, SYSTEM_PROMPT};
use crate::eval::llm::{decision_from_answer, default_decision, LlmConfig, LlmNode};
use crate::eval::Decision;
use crate::features::context::Context;
use crate::{Error, Result};
use serde::Deserialize;
use std::time::Duration;

pub struct OpenAiLlm {
    cfg: LlmConfig,
    cache: FileCache,
    http: reqwest::Client,
}

impl OpenAiLlm {
    pub fn new(cfg: LlmConfig) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(cfg.timeout_secs))
            .build()
            .map_err(|e| Error::Eval(format!("http client build: {e}")))?;
        let cache = FileCache::new(cfg.cache_dir.clone());
        Ok(Self { cfg, cache, http })
    }

    /// 缓存命中→直接还原；未命中→调用(带重试)→落缓存；失败→回退 default。
    pub async fn eval(&self, node_id: &str, node: &LlmNode<'_>, ctx: &Context) -> Result<Decision> {
        let rendered = render_user(node, ctx);
        let key = FileCache::key(&self.cfg.model, node_id, &rendered);
        if let Some(c) = self.cache.get(&key)
            && node.labels.contains_key(&c.label)
        {
            return Ok(decision_from_answer(node, &c.label, c.confidence, &c.reason, true));
        }
        match self.call_with_retries(&rendered, node).await {
            Ok((label, confidence, reason)) => {
                let _ = self.cache.put(&key, &Cached {
                    label: label.clone(), confidence, reason: reason.clone(), model: self.cfg.model.clone(),
                });
                Ok(decision_from_answer(node, &label, confidence, &reason, false))
            }
            Err(e) => Ok(default_decision(node, &format!("LLM fallback({e})"))),
        }
    }

    async fn call_with_retries(&self, rendered: &str, node: &LlmNode<'_>) -> Result<(String, f64, String)> {
        let mut last = String::from("no attempt");
        for _ in 0..=self.cfg.max_retries {
            match self.call_once(rendered, node).await {
                Ok(a) => return Ok(a),
                Err(e) => last = e.to_string(),
            }
        }
        Err(Error::Eval(last))
    }

    async fn call_once(&self, rendered: &str, node: &LlmNode<'_>) -> Result<(String, f64, String)> {
        let body = build_request_body(&self.cfg.model, rendered);
        let url = format!("{}/chat/completions", self.cfg.base_url.trim_end_matches('/'));
        let resp = self.http.post(&url).bearer_auth(&self.cfg.api_key).json(&body).send().await
            .map_err(|e| Error::Eval(format!("request error: {e}")))?;
        if !resp.status().is_success() {
            return Err(Error::Eval(format!("http status {}", resp.status())));
        }
        let parsed: ChatResponse = resp.json().await
            .map_err(|e| Error::Eval(format!("response decode: {e}")))?;
        let content = parsed.choices.into_iter().next().map(|c| c.message.content)
            .ok_or_else(|| Error::Eval("no choices in response".into()))?;
        let ans = parse_answer(&content, node.labels)?;
        Ok((ans.label, ans.confidence, ans.reason))
    }
}

fn build_request_body(model: &str, rendered: &str) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "temperature": 0,
        "response_format": {"type": "json_object"},
        "messages": [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": rendered}
        ]
    })
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}
#[derive(Deserialize)]
struct Choice {
    message: Msg,
}
#[derive(Deserialize)]
struct Msg {
    content: String,
}
```

- [ ] **Step 5: 运行验证通过**

Run: `cargo test --lib eval::llm::client`
Expected: 两个测试 PASS（async 调用路径仅编译、不连网）。

- [ ] **Step 6: Commit**

```bash
git add src/eval/llm/client.rs src/eval/llm/mod.rs
git commit -m "feat(eval/llm): OpenAI-standard async client with cache and retry/fallback" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 7: LlmEvaluator 枚举 + 异步派发

**Files:**
- Modify: `src/eval/llm/mod.rs`（加 `LlmEvaluator` 枚举 + `eval_llm` + 2 个 async 测试）
- Test: 同文件

- [ ] **Step 1: 在 `src/eval/llm/mod.rs` 加枚举与派发**

在 helpers 之后、`#[cfg(test)]` 之前插入：
```rust
pub enum LlmEvaluator {
    OpenAi(client::OpenAiLlm),
    Disabled,
    Stub(StubLlm),
}

impl LlmEvaluator {
    pub async fn eval_llm(&self, node_id: &str, node: &LlmNode<'_>, ctx: &Context) -> Result<Decision> {
        match self {
            LlmEvaluator::OpenAi(c) => c.eval(node_id, node, ctx).await,
            LlmEvaluator::Disabled => Ok(default_decision(node, "LLM disabled")),
            LlmEvaluator::Stub(s) => s.eval(node_id, node, ctx),
        }
    }
}
```

- [ ] **Step 2: 在 `mod tests` 内追加 2 个 async 测试**

```rust
    #[tokio::test]
    async fn disabled_returns_default() {
        let lbl = labels();
        let node = LlmNode { inputs: &[], prompt: "q", labels: &lbl, default: "leaf_f" };
        let d = LlmEvaluator::Disabled.eval_llm("n", &node, &ctx()).await.unwrap();
        assert_eq!(d.goto, "leaf_f");
        assert_eq!(d.label, "default");
    }

    #[tokio::test]
    async fn stub_via_enum_returns_label() {
        let lbl = labels();
        let node = LlmNode { inputs: &[], prompt: "q", labels: &lbl, default: "leaf_f" };
        let ev = LlmEvaluator::Stub(StubLlm { answers: HashMap::from([("n".to_string(), "go".to_string())]) });
        let d = ev.eval_llm("n", &node, &ctx()).await.unwrap();
        assert_eq!(d.goto, "leaf_l");
    }
```

- [ ] **Step 3: 运行验证通过**

Run: `cargo test --lib eval::llm`
Expected: 全部 PASS（cache 2 + mod 同步 2 + async 2 + prompt 3 + client 2）。

- [ ] **Step 4: Commit**

```bash
git add src/eval/llm/mod.rs
git commit -m "feat(eval/llm): LlmEvaluator enum dispatch (OpenAi/Disabled/Stub)" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 8: 异步切换（traverse + runner + cli + e2e 一起改）

> **为什么一个任务做 4 个文件**：`traverse` 改 async 会让 `runner` 不编译，`runner` 改签名会让 `cli`/`e2e` 不编译——它们是编译耦合的一条链，必须同任务切换才能保持 `cargo test` 全绿。

**Files:**
- Modify: `src/engine/traversal.rs`（async + 接评估器 + 测试转 async）
- Modify: `src/backtest/runner.rs`（async + 并发 + 新闻/LLM；保留既有 sync 测试）
- Modify: `src/cli/mod.rs`（`#[tokio::main]` + 新 flags + 启用判据）
- Modify: `tests/e2e.rs`（既有测试转 async + Disabled + 新字段）

- [ ] **Step 1: 重写 `src/engine/traversal.rs`（实现 + async 测试）为：**

```rust
use crate::engine::trace::{StepRecord, Trace};
use crate::eval::llm::{LlmEvaluator, LlmNode};
use crate::eval::quant::eval_quant;
use crate::features::context::Context;
use crate::tree::loader::{Node, Tree};
use crate::{Error, Result};

/// 从 root 走树到叶子。量化节点同步求值；LLM 节点 await 评估器（Disabled 时走 default）。
pub async fn traverse(tree: &Tree, ctx: &Context, llm: &LlmEvaluator) -> Result<Trace> {
    let mut path: Vec<StepRecord> = Vec::new();
    let mut current = tree.root.clone();
    let max_steps = tree.nodes.len() + 1;
    for _ in 0..=max_steps {
        if let Some(leaf) = tree.leaves.get(&current) {
            return Ok(Trace { t: ctx.t, path, leaf: current.clone(), stance: leaf.stance });
        }
        let node = tree
            .nodes
            .get(&current)
            .ok_or_else(|| Error::Engine(format!("dangling node '{current}'")))?;
        let decision = match node {
            Node::Quant { branches, default } => eval_quant(branches, default, ctx)?,
            Node::Llm { inputs, prompt, labels, default } => {
                let ln = LlmNode { inputs, prompt, labels, default };
                llm.eval_llm(&current, &ln, ctx).await?
            }
        };
        path.push(StepRecord {
            node_id: current.clone(),
            label: decision.label.clone(),
            confidence: decision.confidence,
            rationale: decision.rationale.clone(),
        });
        current = decision.goto;
    }
    Err(Error::Engine("traversal exceeded max steps (cycle?)".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::bar::{Bar, Window};
    use crate::eval::llm::{LlmEvaluator, StubLlm};
    use crate::features::context::Context;
    use crate::tree::loader::load_tree_str;
    use crate::tree::schema::Stance;
    use chrono::NaiveDate;
    use std::collections::HashMap;

    fn ctx(closes: &[f64]) -> Context {
        let base = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        let bars: Vec<Bar> = closes.iter().enumerate().map(|(i, &c)| Bar {
            time: base + chrono::Duration::minutes(i as i64 * 15), open: c, high: c, low: c, close: c, volume: 1.0,
        }).collect();
        let t = bars.last().unwrap().time;
        Context { t, primary: Window { bars: bars.clone() }, context: Window { bars }, news: None }
    }

    const QUANT_TREE: &str = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "close > sma(close,3)", goto: leaf_l, label: up } ]
    default: { goto: leaf_f, label: flat }
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;

    const LLM_TREE: &str = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
root: a
nodes:
  a:
    type: llm
    prompt: "x"
    labels: { yes: leaf_l }
    default: leaf_f
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;

    #[tokio::test]
    async fn quant_uptrend_reaches_long_leaf() {
        let tree = load_tree_str(QUANT_TREE).unwrap();
        let tr = traverse(&tree, &ctx(&[1.0, 2.0, 3.0, 4.0, 5.0]), &LlmEvaluator::Disabled).await.unwrap();
        assert_eq!(tr.leaf, "leaf_l");
        assert!(matches!(tr.stance, Stance::Long));
        assert_eq!(tr.path.len(), 1);
        assert_eq!(tr.path[0].node_id, "a");
    }

    #[tokio::test]
    async fn llm_node_disabled_takes_default() {
        let tree = load_tree_str(LLM_TREE).unwrap();
        let tr = traverse(&tree, &ctx(&[1.0, 2.0, 3.0]), &LlmEvaluator::Disabled).await.unwrap();
        assert_eq!(tr.leaf, "leaf_f");
        assert!(matches!(tr.stance, Stance::Flat));
        assert!(tr.path[0].rationale.contains("LLM disabled"));
    }

    #[tokio::test]
    async fn llm_node_stub_takes_label() {
        let tree = load_tree_str(LLM_TREE).unwrap();
        let ev = LlmEvaluator::Stub(StubLlm { answers: HashMap::from([("a".to_string(), "yes".to_string())]) });
        let tr = traverse(&tree, &ctx(&[1.0, 2.0, 3.0]), &ev).await.unwrap();
        assert_eq!(tr.leaf, "leaf_l");
        assert!(matches!(tr.stance, Stance::Long));
    }
}
```

- [ ] **Step 2: 重写 `src/backtest/runner.rs` 的实现部分（保留底部既有 `example_tree_loads_and_validates` 测试不动）为：**

```rust
use crate::backtest::costs::CostModel;
use crate::backtest::forward_return::{forward_return, ForwardResult};
use crate::backtest::metrics::compute_metrics;
use crate::data::bar::Bar;
use crate::data::news::NewsRecord;
use crate::engine::trace::Trace;
use crate::eval::llm::LlmEvaluator;
use crate::features::context::build_context;
use crate::report::Report;
use crate::tree::loader::Tree;
use crate::Result;
use futures::stream::{self, StreamExt};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct BacktestConfig {
    pub tree_path: PathBuf,
    pub primary_path: PathBuf,
    pub context_path: PathBuf,
    pub news_path: Option<PathBuf>,
    pub out_path: PathBuf,
    pub traces_path: Option<PathBuf>,
    pub cost_bps: f64,
    pub warmup: usize,
    pub window: usize,
    pub concurrency: usize,
}

#[allow(clippy::too_many_arguments)]
async fn eval_point(
    i: usize,
    primary: &[Bar],
    context: &[Bar],
    news: &[NewsRecord],
    tree: &Tree,
    costs: &CostModel,
    fw: usize,
    window: usize,
    llm: &LlmEvaluator,
) -> Result<(Trace, Option<ForwardResult>)> {
    let t = primary[i].time;
    let ctx = build_context(primary, context, news, t, window);
    let trace = crate::engine::traversal::traverse(tree, &ctx, llm).await?;
    let fr = forward_return(primary, i, fw, trace.stance, costs);
    Ok((trace, fr))
}

/// 端到端（异步、有序并发）：加载→逐点遍历→前瞻收益→度量→写报告。
pub async fn run(cfg: &BacktestConfig, llm: &LlmEvaluator) -> Result<Report> {
    let tree = crate::tree::loader::load_tree_file(&cfg.tree_path)?;
    let primary = crate::data::reader::read_bars_csv(&cfg.primary_path)?;
    let context = crate::data::reader::read_bars_csv(&cfg.context_path)?;
    let news: Vec<NewsRecord> = match &cfg.news_path {
        Some(p) => crate::data::news::read_news_csv(p)?,
        None => Vec::new(),
    };
    let costs = CostModel { round_trip_bps: cfg.cost_bps };
    let fw = tree.meta.forward_window;
    let start = cfg.warmup.min(primary.len());

    // 有序并发：buffered 保持产出顺序，复现性不破。
    let results: Vec<(Trace, Option<ForwardResult>)> = stream::iter(start..primary.len())
        .map(|i| eval_point(i, &primary, &context, &news, &tree, &costs, fw, cfg.window, llm))
        .buffered(cfg.concurrency.max(1))
        .collect::<Vec<Result<(Trace, Option<ForwardResult>)>>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>>>()?;

    let traces: Vec<Trace> = results.iter().map(|(t, _)| t.clone()).collect();
    let metrics = compute_metrics(&results, &primary);
    let report = Report {
        tree_name: tree.meta.name.clone(),
        forward_window: fw,
        cost_bps: cfg.cost_bps,
        metrics,
    };
    crate::report::write_report(&report, &cfg.out_path)?;
    if let Some(tp) = &cfg.traces_path {
        crate::report::write_traces_jsonl(&traces, tp)?;
    }
    Ok(report)
}
```

- [ ] **Step 3: 重写 `src/cli/mod.rs` 为：**

```rust
use crate::backtest::runner::{run, BacktestConfig};
use crate::eval::llm::client::OpenAiLlm;
use crate::eval::llm::{LlmConfig, LlmEvaluator};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "rquant", about = "Fuzzy decision-tree A-share backtester")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run a quant backtest over local CSV bars (LLM nodes via OpenAI-standard API if configured)
    Backtest {
        #[arg(long)]
        tree: PathBuf,
        #[arg(long)]
        primary: PathBuf,
        #[arg(long)]
        context: PathBuf,
        #[arg(long)]
        news: Option<PathBuf>,
        #[arg(long, default_value = "report.json")]
        out: PathBuf,
        #[arg(long)]
        traces: Option<PathBuf>,
        #[arg(long, default_value_t = 10.0)]
        cost_bps: f64,
        #[arg(long, default_value_t = 100)]
        warmup: usize,
        #[arg(long, default_value_t = 100)]
        window: usize,
        #[arg(long, default_value_t = 8)]
        concurrency: usize,
        #[arg(long, default_value = "")]
        llm_model: String,
        #[arg(long, default_value = "")]
        llm_base_url: String,
        #[arg(long, default_value = ".rquant-cache/llm")]
        llm_cache_dir: PathBuf,
    },
}

#[tokio::main]
pub async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Backtest {
            tree, primary, context, news, out, traces, cost_bps, warmup, window, concurrency,
            llm_model, llm_base_url, llm_cache_dir,
        } => {
            let api_key = std::env::var("RQUANT_LLM_API_KEY").unwrap_or_default();
            let llm = if !llm_model.is_empty() && !llm_base_url.is_empty() && !api_key.is_empty() {
                let cfg = LlmConfig {
                    base_url: llm_base_url,
                    api_key,
                    model: llm_model,
                    timeout_secs: 60,
                    max_retries: 2,
                    cache_dir: llm_cache_dir,
                };
                LlmEvaluator::OpenAi(OpenAiLlm::new(cfg)?)
            } else {
                eprintln!("[rquant] LLM not configured (need --llm-model, --llm-base-url, env RQUANT_LLM_API_KEY); LLM nodes will take their default branch.");
                LlmEvaluator::Disabled
            };
            let cfg = BacktestConfig {
                tree_path: tree, primary_path: primary, context_path: context, news_path: news,
                out_path: out, traces_path: traces, cost_bps, warmup, window, concurrency,
            };
            let report = run(&cfg, &llm).await?;
            crate::report::print_summary(&report);
        }
    }
    Ok(())
}
```

- [ ] **Step 4: 改 `tests/e2e.rs` 的既有测试为 async + Disabled + 新字段**

把 `#[test] fn end_to_end_uptrend_yields_positive_long_edge()` 改为 `#[tokio::test] async fn ...`，文件顶部加 `use rquant::eval::llm::LlmEvaluator;`，`BacktestConfig` 字面量加 `news_path: None,` 与 `concurrency: 4,`，并把 `let report = run(&cfg).unwrap();` 改为：
```rust
let report = run(&cfg, &LlmEvaluator::Disabled).await.unwrap();
```
（该树无 LLM 节点，Disabled 不改变行为，原 5 条断言不变。）

- [ ] **Step 5: 全量验证 + 构建**

Run: `cargo test`
Expected: 全部 PASS（lib 含 traversal 的 3 个 async 测试；e2e 既有 1 个仍绿）。

Run: `cargo build`
Expected: 通过。

Run: `cargo run -- backtest --help`
Expected: 打印含 `--news`、`--concurrency`、`--llm-model`、`--llm-base-url`、`--llm-cache-dir` 的用法。

- [ ] **Step 6: Commit**

```bash
git add src/engine/traversal.rs src/backtest/runner.rs src/cli/mod.rs tests/e2e.rs
git commit -m "feat(engine,cli): async traversal/runner with concurrent LLM eval and news input" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 9: LLM 端到端（Stub）证明测试 + README

**Files:**
- Modify: `tests/e2e.rs`（新增一个 LLM(Stub) 端到端测试）
- Create: `README.md`

- [ ] **Step 1: 在 `tests/e2e.rs` 追加 LLM 证明测试**

文件顶部确保有 `use rquant::eval::llm::{LlmEvaluator, StubLlm};` 与 `use std::collections::HashMap;`。追加：
```rust
fn llm_tree_yaml() -> String {
    r#"
meta: { name: e2e_llm, forward_window: 2, stances: [long, flat] }
root: gate
nodes:
  gate:
    type: quant
    branches: [ { when: "close > sma(close,5)", goto: judge, label: above } ]
    default: { goto: leaf_flat, label: below }
  judge:
    type: llm
    inputs: [news_score]
    prompt: "go or not"
    labels: { go: leaf_long }
    default: leaf_flat
leaves:
  leaf_long: { stance: long }
  leaf_flat: { stance: flat }
"#
    .to_string()
}

async fn run_llm_e2e(ev: &LlmEvaluator) -> rquant::report::Report {
    let tree_f = write_file(&llm_tree_yaml(), ".yaml");
    let primary_f = write_file(&gen_primary_csv(), ".csv");
    let context_f = write_file(&gen_context_csv(), ".csv");
    let out_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
    let cfg = BacktestConfig {
        tree_path: tree_f.path().to_path_buf(),
        primary_path: primary_f.path().to_path_buf(),
        context_path: context_f.path().to_path_buf(),
        news_path: None,
        out_path: out_f.path().to_path_buf(),
        traces_path: None,
        cost_bps: 10.0,
        warmup: 5,
        window: 100,
        concurrency: 4,
    };
    run(&cfg, ev).await.unwrap()
}

#[tokio::test]
async fn llm_node_changes_path_vs_disabled() {
    // Stub: judge -> "go" => 到达 leaf_long(看多) => 有 active 信号
    let stub = LlmEvaluator::Stub(StubLlm { answers: HashMap::from([("judge".to_string(), "go".to_string())]) });
    let with_llm = run_llm_e2e(&stub).await;
    assert!(with_llm.metrics.active.count > 0, "stub 'go' should produce long signals");

    // Disabled: judge 走 default(leaf_flat) => 全 flat => 无 active 信号
    let disabled = run_llm_e2e(&LlmEvaluator::Disabled).await;
    assert_eq!(disabled.metrics.active.count, 0, "disabled LLM should take default -> all flat");
}
```

> 复用 e2e 里已有的 `write_file` / `gen_primary_csv` / `gen_context_csv` 助手（同文件，M1–M4 已建）。

- [ ] **Step 2: 运行验证通过**

Run: `cargo test --test e2e`
Expected: 既有 1 个 + 新增 1 个，全 PASS。

- [ ] **Step 3: 写 `README.md`（项目根）**

```markdown
# rquant

基于模糊决策树的 A股离线回测引擎。决策树（YAML + 表达式 DSL）由用户提供，量化指标 + 少量 LLM 取代人工逐节点判断；前瞻收益评分验证策略 edge。

## 构建与测试
```
cargo build --release
cargo test
```

## 运行回测（纯量化）
```
cargo run --release -- backtest \
  --tree examples/trend_tree.yaml \
  --primary 15m.csv --context 1h.csv \
  --out report.json --traces traces.jsonl
```
CSV 表头：`time,open,high,low,close,volume`（time 形如 `2024-01-02 09:45:00`）。

## 启用 LLM 节点（OpenAI 标准，兼容 DashScope/DeepSeek）
设置 API key 环境变量并指定 model + base_url：
```
# DeepSeek
export RQUANT_LLM_API_KEY=sk-xxx
cargo run --release -- backtest --tree ... --primary ... --context ... \
  --llm-model deepseek-chat --llm-base-url https://api.deepseek.com/v1

# DashScope(通义)
--llm-model qwen-plus --llm-base-url https://dashscope.aliyuncs.com/compatible-mode/v1

# OpenAI
--llm-model gpt-4o-mini --llm-base-url https://api.openai.com/v1
```
三者(model+base_url+env key)缺一即回退：LLM 节点走 `default` 分支（纯量化照常）。

可选新闻输入：`--news news.csv`（表头 `time,score,headline`）→ 填入 Context.news，供 LLM 节点的 `news_score`/`recent_headlines` 使用。

## 复现性与缓存
LLM 调用 `temperature=0`，结论按 `hash(model+node_id+渲染输入)` 缓存于 `--llm-cache-dir`（默认 `.rquant-cache/llm/`）。首轮并发填缓存；重跑全命中 → 零网络、可复现。注意：LLM 即便 temp=0 也非严格确定，复现性由缓存保证。

> ⚠️ 每个到达 LLM 节点的决策点都会发起一次调用（未命中缓存时）；请让 LLM 节点处于被量化节点过滤后的稀疏位置，并控制回测区间，以免产生大量调用与费用。
```

- [ ] **Step 4: Commit**

```bash
git add tests/e2e.rs README.md
git commit -m "test(e2e): LLM stub changes path vs disabled; add README with provider presets" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## 附录 A：Spec 覆盖对照（自检）

| Spec 章节 | 实现于 |
|---|---|
| §4.1 异步化（traverse/runner/cli + buffered 有序并发）| Task 8 |
| §4.2 LlmEvaluator 枚举派发（OpenAi/Disabled/Stub）| Task 4(Stub/helpers)+6(OpenAi)+7(枚举) |
| §4.3 输入渲染（价格快照 + inputs + news/缺失）| Task 5 |
| §4.4 Context.news + 新闻加载器（防未来函数）| Task 2(loader)+3(Context/build_context) |
| §4.5 配置 + 启用判据（env key + model + base_url）| Task 8(cli) |
| §4.6 内容寻址缓存（键/原子写/get/put）| Task 4 |
| §4.7 复现性（temp=0/缓存/有序并发）| Task 4+6+8 |
| §4.8 错误处理（disabled/网络/非法 → default 回退）| Task 4(helpers)+6(client)+7 |
| §5 类型契约（Decision/请求体/响应解析）| Task 5+6 |
| §6 对 M1–M4 改动面 | Task 1/3/8 |
| §8 测试（缓存/prompt/解析/新闻/评估器/集成/确定性）| Task 2/4/5/6/7/8/9 |
| §9 防未来函数（news 闸门 + 有序并发）| Task 3(news 测试)+8 |

## 附录 B：明确不在范围（YAGNI / 后置）
- 新闻**采集/打分**（仅消费 CSV）。
- LLM `confidence` 用于软遍历（仅记录）。
- 真实端点 CI 测试（用 Stub + 边界单测；DashScope/DeepSeek 真连靠 README 手动 smoke）。
- M6 新浪 fetcher / Parquet/SQLite 缓存。
- 缓存淘汰/TTL（内容寻址、手动清理即可）。

> **并发与复现性**：runner 用 `buffered(N)` 有序产出 + 度量 `BTreeMap` 定序 + 缓存内容确定 → 同输入同输出（缓存填好后）。`#[tokio::main]` 默认多线程运行时；`eval_point` future 仅持共享只读引用，`Send` 成立。

