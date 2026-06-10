# rquant LLM 完整 label 概率分布 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** LLM 节点输出升级为 `{"probs": {label: p, ...}, "reason": ...}` 概率分布；软遍历多路消费，硬遍历 argmax 派生 Decision；缓存改存分布（键含 SYSTEM_PROMPT，自动隔离旧缓存）。

**Architecture:** 在 master(HEAD `0f16a4c`)上扩展。T1 纯增量加 `dist_to_gotos`/`decision_from_dist`；T2 一次性切换 prompt/cache/client/mod（编译耦合链）；T3 软遍历 LLM 臂改 `eval_llm_dist` + 多路测试。Stub/Disabled 行为逐字保留 → 既有测试零改动是验收标准。

**Tech Stack:** Rust 2024 + 既有（serde/serde_json/reqwest/sha2）。

> 设计依据：`docs/superpowers/specs/2026-06-10-rquant-llm-label-dist-design.md`。提交信息用英文。

---

## 文件结构
```
改动: src/eval/llm/mod.rs     # T1: + dist_to_gotos / decision_from_dist；T2: eval_llm_dist + Stub probs_for + eval_llm 改派生（删 decision_from_answer）
改动: src/eval/llm/prompt.rs  # T2: 新 SYSTEM_PROMPT + LlmAnswer{probs} + parse_answer 清洗 + 测试重写
改动: src/eval/llm/cache.rs   # T2: Cached{probs, reason, model} + 测试更新
改动: src/eval/llm/client.rs  # T2: fetch_probs 共享路径 + eval(argmax) + eval_dist + 测试更新
改动: src/engine/soft.rs      # T3: LLM 臂改 eval_llm_dist + 3-label 多路测试
改动: README.md               # T3
```

---

## Task 1: dist_to_gotos + decision_from_dist（纯增量）

**Files:**
- Modify: `src/eval/llm/mod.rs`（两个新函数 + 测试；不动既有代码）
- Test: 同文件

- [ ] **Step 1: 在 `src/eval/llm/mod.rs` 的 `mod tests` 加失败测试**

（`mod tests` 已有 `ctx()`/`labels()` 助手；`labels()` 返回 `{"go" → "leaf_l"}`。）
```rust
    #[test]
    fn dist_to_gotos_maps_merges_and_fills_default() {
        use std::collections::BTreeMap;
        let lbl = HashMap::from([
            ("a".to_string(), "leaf_x".to_string()),
            ("b".to_string(), "leaf_x".to_string()),  // 同 goto，应合并
            ("c".to_string(), "leaf_y".to_string()),
        ]);
        let node = LlmNode { inputs: &[], prompt: "q", labels: &lbl, default: "leaf_f" };
        let probs = BTreeMap::from([("a".to_string(), 0.3), ("b".to_string(), 0.2), ("c".to_string(), 0.1)]);
        let dist = dist_to_gotos(&node, &probs);
        // leaf_x: 0.3+0.2=0.5, leaf_y: 0.1, 残余 0.4 → leaf_f；BTreeMap 序：leaf_f, leaf_x, leaf_y
        assert_eq!(dist.len(), 3);
        let m: std::collections::HashMap<_, _> = dist.iter().cloned().collect();
        assert!((m["leaf_x"] - 0.5).abs() < 1e-9);
        assert!((m["leaf_y"] - 0.1).abs() < 1e-9);
        assert!((m["leaf_f"] - 0.4).abs() < 1e-9);
        let sum: f64 = dist.iter().map(|(_, w)| w).sum();
        assert!((sum - 1.0).abs() < 1e-9);
    }

    #[test]
    fn decision_from_dist_argmax_and_default_remainder() {
        use std::collections::BTreeMap;
        let lbl = labels(); // {"go" → "leaf_l"}
        let node = LlmNode { inputs: &[], prompt: "q", labels: &lbl, default: "leaf_f" };
        // go=0.9 胜出
        let d = decision_from_dist(&node, &BTreeMap::from([("go".to_string(), 0.9)]), "r");
        assert_eq!(d.goto, "leaf_l");
        assert_eq!(d.label, "go");
        assert!((d.confidence - 0.9).abs() < 1e-9);
        // go=0.3 → 残余 0.7 给 default 胜出
        let d2 = decision_from_dist(&node, &BTreeMap::from([("go".to_string(), 0.3)]), "r");
        assert_eq!(d2.goto, "leaf_f");
        assert_eq!(d2.label, "default");
        assert!((d2.confidence - 0.7).abs() < 1e-9);
    }
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test --lib eval::llm::tests::dist_to_gotos_maps_merges_and_fills_default`
Expected: 编译失败（两函数未定义）。

- [ ] **Step 3: 实现（`src/eval/llm/mod.rs`，`decision_from_answer` 之后）**

顶部 `use std::collections::HashMap;` 改为 `use std::collections::{BTreeMap, HashMap};`。追加：
```rust
/// label 分布 → goto 分布：label→labels[label]（未知→default）、同 goto 合并、残余补 default。
/// 前置：probs 已清洗（Σ ≤ 1）。产出 Σ = 1，按 goto 名排序（确定性）。
pub fn dist_to_gotos(node: &LlmNode<'_>, probs: &BTreeMap<String, f64>) -> Vec<(String, f64)> {
    let mut acc: BTreeMap<String, f64> = BTreeMap::new();
    let mut sum = 0.0;
    for (label, &p) in probs {
        if p > 0.0 {
            let goto = node.labels.get(label).cloned().unwrap_or_else(|| node.default.to_string());
            *acc.entry(goto).or_insert(0.0) += p;
            sum += p;
        }
    }
    let rem = 1.0 - sum;
    if rem > 0.0 {
        *acc.entry(node.default.to_string()).or_insert(0.0) += rem;
    }
    acc.into_iter().collect()
}

/// 硬模式派生：在 (label, p) + ("default", 残余) 上取 argmax（并列取字典序小，BTreeMap 序保证）。
pub fn decision_from_dist(node: &LlmNode<'_>, probs: &BTreeMap<String, f64>, rationale: &str) -> Decision {
    let mut candidates: BTreeMap<String, f64> = probs.clone();
    let sum: f64 = probs.values().sum();
    let rem = 1.0 - sum;
    if rem > 0.0 {
        *candidates.entry("default".to_string()).or_insert(0.0) += rem;
    }
    let (label, confidence) = candidates
        .iter()
        .fold(("default".to_string(), 0.0), |best, (k, &v)| if v > best.1 { (k.clone(), v) } else { best });
    let goto = node.labels.get(&label).cloned().unwrap_or_else(|| node.default.to_string());
    Decision { goto, label, confidence, rationale: rationale.to_string() }
}
```

- [ ] **Step 4: 运行验证通过**

Run: `cargo test --lib eval::llm`
Expected: 既有 + 2 新测试 PASS。
Run: `cargo build`
Expected: 通过。

- [ ] **Step 5: Commit**

```bash
git add src/eval/llm/mod.rs
git commit -m "feat(eval/llm): dist_to_gotos + decision_from_dist (label-distribution helpers)" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: 协议切换（prompt + cache + client + mod，编译耦合，一次完成）

> 改 `LlmAnswer`/`Cached` 字段会立刻破坏 `client.rs`；本任务把四个文件一次切完并保持全绿。**Stub/Disabled 行为逐字保留**（既有 mod/soft/e2e 测试必须不改而过）。

**Files:**
- Modify: `src/eval/llm/prompt.rs`、`src/eval/llm/cache.rs`、`src/eval/llm/client.rs`、`src/eval/llm/mod.rs`
- Test: 各文件内

- [ ] **Step 1: `prompt.rs` — 新协议**

(a) `SYSTEM_PROMPT` 替换为：
```rust
pub const SYSTEM_PROMPT: &str = "You are a financial-analysis classifier. Assign a probability between 0 and 1 to EVERY allowed label; probabilities should sum to 1. Respond ONLY with a JSON object: {\"probs\": {<label>: <number 0..1>, ...}, \"reason\": <short string>}.";
```
(b) `use serde::Deserialize;` 下加 `use std::collections::BTreeMap;`。`LlmAnswer` 替换为：
```rust
#[derive(Debug, Deserialize)]
pub struct LlmAnswer {
    pub probs: BTreeMap<String, f64>,
    #[serde(default)]
    pub reason: String,
}
```
(c) `parse_answer` 替换为：
```rust
/// 解析+清洗：丢未知 label；p NaN→0、clamp[0,1]、0 丢弃；Σ>1 整体归一；清洗后空/全零 → Err。
/// 产出 Σ ≤ 1（残余由消费方归 default）。
pub fn parse_answer(content: &str, allowed: &std::collections::HashMap<String, String>) -> Result<LlmAnswer> {
    let raw: LlmAnswer = serde_json::from_str(content.trim())
        .map_err(|e| Error::Eval(format!("LLM output not valid JSON: {e}")))?;
    let mut probs: BTreeMap<String, f64> = BTreeMap::new();
    for (k, v) in raw.probs {
        if allowed.contains_key(&k) {
            let p = if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) };
            if p > 0.0 {
                probs.insert(k, p);
            }
        }
    }
    let sum: f64 = probs.values().sum();
    if probs.is_empty() || sum <= 0.0 {
        return Err(Error::Eval("LLM probs empty or all-zero after cleaning".into()));
    }
    if sum > 1.0 {
        for v in probs.values_mut() {
            *v /= sum;
        }
    }
    Ok(LlmAnswer { probs, reason: raw.reason })
}
```
(d) 重写 `parse_answer_valid_invalid_and_label_check` 测试为：
```rust
    #[test]
    fn parse_answer_cleans_normalizes_and_rejects() {
        let allowed = HashMap::from([("go".to_string(), "x".to_string()), ("hold".to_string(), "y".to_string())]);
        // 合法分布，Σ≤1 保留
        let ok = parse_answer("{\"probs\":{\"go\":0.6,\"hold\":0.3},\"reason\":\"r\"}", &allowed).unwrap();
        assert!((ok.probs["go"] - 0.6).abs() < 1e-9);
        assert!((ok.probs["hold"] - 0.3).abs() < 1e-9);
        // Σ>1 → 归一
        let n = parse_answer("{\"probs\":{\"go\":0.8,\"hold\":0.4}}", &allowed).unwrap();
        let sum: f64 = n.probs.values().sum();
        assert!((sum - 1.0).abs() < 1e-9);
        // 未知 label 丢弃（剩余合法）
        let u = parse_answer("{\"probs\":{\"go\":0.5,\"nope\":0.5}}", &allowed).unwrap();
        assert_eq!(u.probs.len(), 1);
        // 非 JSON / 全未知 → Err
        assert!(parse_answer("not json", &allowed).is_err());
        assert!(parse_answer("{\"probs\":{\"nope\":1.0}}", &allowed).is_err());
    }
```

- [ ] **Step 2: `cache.rs` — Cached 改存分布**

(a) `use std::path::PathBuf;` 旁加 `use std::collections::BTreeMap;`。`Cached` 替换为：
```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cached {
    pub probs: BTreeMap<String, f64>,
    pub reason: String,
    pub model: String,
}
```
(b) `put_then_get_roundtrips` 测试里的 `Cached` 字面量改为：
```rust
        let c = Cached { probs: BTreeMap::from([("go".to_string(), 0.7)]), reason: "ok".into(), model: "m".into() };
```
（测试内已 `use super::*;`，BTreeMap 经 cache.rs 顶部导入可见。）

- [ ] **Step 3: `mod.rs` — eval_llm_dist + Stub 分布 + eval_llm 改派生**

(a) **删除** `decision_from_answer` 函数（已被 `decision_from_dist` 取代）。
(b) `StubLlm` 增加分布助手并改写 `eval`（行为保留；另支持 `"label:p,label:p"` 多 label 语法供多路测试用，普通 label 字符串仍是 `{label: 0.9}`）：
```rust
/// 测试用 stub：node_id -> 答案。普通 label → {label: 0.9}（残余 0.1 → default）；
/// "ERROR"/未命中 → 回退 default；"a:0.5,b:0.3" 语法 → 显式多 label 分布。
pub struct StubLlm {
    pub answers: HashMap<String, String>,
}
impl StubLlm {
    fn probs_for(&self, node_id: &str, node: &LlmNode<'_>) -> Option<BTreeMap<String, f64>> {
        let ans = self.answers.get(node_id)?;
        if ans == "ERROR" {
            return None;
        }
        if ans.contains(':') {
            let mut m = BTreeMap::new();
            for pair in ans.split(',') {
                let (l, p) = pair.split_once(':')?;
                if node.labels.contains_key(l) {
                    m.insert(l.to_string(), p.trim().parse::<f64>().ok()?);
                }
            }
            return if m.is_empty() { None } else { Some(m) };
        }
        if node.labels.contains_key(ans) {
            Some(BTreeMap::from([(ans.clone(), 0.9)]))
        } else {
            None
        }
    }
    pub fn eval(&self, node_id: &str, node: &LlmNode<'_>, _ctx: &Context) -> Result<Decision> {
        match self.probs_for(node_id, node) {
            Some(p) => Ok(decision_from_dist(node, &p, "LLM: stub")),
            None => Ok(default_decision(node, "LLM stub error/no-answer")),
        }
    }
}
```
(c) `impl LlmEvaluator` 增加：
```rust
    /// 统一分布出口：goto 分布（Σ=1）+ rationale。
    pub async fn eval_llm_dist(&self, node_id: &str, node: &LlmNode<'_>, ctx: &Context) -> Result<(Vec<(String, f64)>, String)> {
        match self {
            LlmEvaluator::OpenAi(c) => c.eval_dist(node_id, node, ctx).await,
            LlmEvaluator::Disabled => Ok((vec![(node.default.to_string(), 1.0)], "LLM disabled: default".into())),
            LlmEvaluator::Stub(s) => Ok(match s.probs_for(node_id, node) {
                Some(p) => (dist_to_gotos(node, &p), "stub".into()),
                None => (vec![(node.default.to_string(), 1.0)], "stub default".into()),
            }),
        }
    }
```
（`eval_llm` 本身不动——OpenAi/Disabled/Stub 各臂签名未变。）

- [ ] **Step 4: `client.rs` — fetch_probs 共享路径**

(a) 顶部 import：`use crate::eval::llm::{decision_from_dist, default_decision, dist_to_gotos, LlmConfig, LlmNode};`（替换 `decision_from_answer` 的导入）；加 `use crate::eval::llm::prompt::LlmAnswer;` 与 `use std::collections::BTreeMap;`。
(b) `eval`/`call_with_retries`/`call_once` 替换为：
```rust
    /// 取分布（label 概率）：缓存命中（且 keys ⊆ labels）→还原；未命中→调用(重试)→落缓存。
    async fn fetch_probs(&self, node_id: &str, node: &LlmNode<'_>, ctx: &Context) -> Result<(BTreeMap<String, f64>, String, bool)> {
        let rendered = render_user(node, ctx);
        let key = FileCache::key(&self.cfg.model, &self.cfg.base_url, SYSTEM_PROMPT, node_id, &rendered);
        if let Some(c) = self.cache.get(&key)
            && !c.probs.is_empty()
            && c.probs.keys().all(|k| node.labels.contains_key(k))
        {
            return Ok((c.probs, c.reason, true));
        }
        let ans = self.call_with_retries(&rendered, node).await?;
        let _ = self.cache.put(&key, &Cached {
            probs: ans.probs.clone(), reason: ans.reason.clone(), model: self.cfg.model.clone(),
        });
        Ok((ans.probs, ans.reason, false))
    }

    /// 硬模式：分布 argmax 派生 Decision；失败回退 default。
    pub async fn eval(&self, node_id: &str, node: &LlmNode<'_>, ctx: &Context) -> Result<Decision> {
        match self.fetch_probs(node_id, node, ctx).await {
            Ok((probs, reason, cached)) => {
                let tag = if cached { "LLM(cached)" } else { "LLM" };
                Ok(decision_from_dist(node, &probs, &format!("{tag}: {reason}")))
            }
            Err(e) => Ok(default_decision(node, &format!("LLM fallback({e})"))),
        }
    }

    /// 软模式：goto 分布；失败回退 [(default, 1.0)]。
    pub async fn eval_dist(&self, node_id: &str, node: &LlmNode<'_>, ctx: &Context) -> Result<(Vec<(String, f64)>, String)> {
        match self.fetch_probs(node_id, node, ctx).await {
            Ok((probs, reason, _)) => Ok((dist_to_gotos(node, &probs), reason)),
            Err(e) => Ok((vec![(node.default.to_string(), 1.0)], format!("LLM fallback({e})"))),
        }
    }

    async fn call_with_retries(&self, rendered: &str, node: &LlmNode<'_>) -> Result<LlmAnswer> {
        let mut last = String::from("no attempt");
        for _ in 0..=self.cfg.max_retries {
            match self.call_once(rendered, node).await {
                Ok(a) => return Ok(a),
                Err(e) => last = e.to_string(),
            }
        }
        Err(Error::Eval(last))
    }

    async fn call_once(&self, rendered: &str, node: &LlmNode<'_>) -> Result<LlmAnswer> {
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
        parse_answer(&content, node.labels)
    }
```
(c) `parses_openai_style_response` 测试的 content 改为新协议：
```rust
        let raw = r#"{"choices":[{"message":{"role":"assistant","content":"{\"probs\":{\"go\":0.9},\"reason\":\"ok\"}"}}]}"#;
        // ...
        let ans = crate::eval::llm::prompt::parse_answer(&content, &allowed).unwrap();
        assert!((ans.probs["go"] - 0.9).abs() < 1e-9);
```

- [ ] **Step 5: 全量验证**

Run: `cargo test`
Expected: **全绿**——既有 mod/soft/e2e 测试（Stub 行为保留）必须不改而过；prompt/cache/client 的改写测试过。
Run: `cargo clippy --all-targets`
Expected: 无告警（平铺执行，勿用 `2>&1`）。

- [ ] **Step 6: Commit**

```bash
git add src/eval/llm/prompt.rs src/eval/llm/cache.rs src/eval/llm/client.rs src/eval/llm/mod.rs
git commit -m "feat(eval/llm): probs-distribution protocol (prompt/cache/client switch, argmax for hard mode)" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: 软遍历多路消费 + README

**Files:**
- Modify: `src/engine/soft.rs`（LLM 臂改 `eval_llm_dist` + 3-label 多路测试）
- Modify: `README.md`

- [ ] **Step 1: 加 3-label 多路失败测试（`src/engine/soft.rs` 的 `mod tests`）**

```rust
    const LLM3_TREE: &str = r#"
meta: { name: t, forward_window: 3, stances: [long, flat] }
root: a
nodes:
  a:
    type: llm
    prompt: "x"
    labels: { up: leaf_x, dn: leaf_y }
    default: leaf_f
leaves:
  leaf_x: { stance: long }
  leaf_y: { stance: flat }
  leaf_f: { stance: flat }
"#;

    #[tokio::test]
    async fn llm_multi_label_distribution_splits_three_ways() {
        let tree = load_tree_str(LLM3_TREE).unwrap();
        let ev = LlmEvaluator::Stub(StubLlm { answers: HashMap::from([("a".to_string(), "up:0.5,dn:0.3".to_string())]) });
        let st = traverse_soft(&tree, &ctx(&[1.0, 2.0, 3.0]), &ev).await.unwrap();
        assert!((st.leaf_probs["leaf_x"] - 0.5).abs() < 1e-9);
        assert!((st.leaf_probs["leaf_y"] - 0.3).abs() < 1e-9);
        assert!((st.leaf_probs["leaf_f"] - 0.2).abs() < 1e-9);
        let sum: f64 = st.leaf_probs.values().sum();
        assert!((sum - 1.0).abs() < 1e-9);
    }
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test --lib engine::soft::tests::llm_multi_label_distribution_splits_three_ways`
Expected: FAIL——当前 LLM 臂走 `eval_llm`(Decision argmax) 的 2 元拆分（up 0.5 胜出 → `[(leaf_x,0.5),(leaf_f,0.5)]`），`leaf_y` 缺失。

- [ ] **Step 3: 改 `engine/soft.rs` LLM 臂**

把阶段一 `Node::Llm` 臂（当前 eval_llm → Decision → 手工 2 元 vec）替换为：
```rust
            Node::Llm { inputs, prompt, labels, default } => {
                let ln = LlmNode { inputs, prompt, labels, default };
                let (dist, _rationale) = llm.eval_llm_dist(&id, &ln, ctx).await?;
                dist
            }
```
（顶部 `use crate::eval::llm::{LlmEvaluator, LlmNode};` 不变；若 `Decision` 相关 import 残留未用则删。）

- [ ] **Step 4: 全量验证**

Run: `cargo test --lib engine::soft`
Expected: 既有 6 个软测试（Stub 单 label 仍 0.9/0.1、disabled、quant 等）+ 新 3 路测试全 PASS。
Run: `cargo test`
Expected: 全量全绿。
Run: `cargo clippy --all-targets`
Expected: 无告警。

- [ ] **Step 5: README**（LLM/soft 相关小节补两句）

````markdown
LLM 节点现要求模型返回**完整 label 概率分布**：`{"probs": {<label>: <0..1>, ...}, "reason": "..."}`。
软遍历按分布多路传播（未覆盖的残余 → `default`）；硬遍历取 argmax。改了系统提示词 → LLM 缓存键自动失效（旧缓存文件无害，可手动清 `.rquant-cache/`）。
````

- [ ] **Step 6: Commit**

```bash
git add src/engine/soft.rs README.md
git commit -m "feat(engine): soft traversal consumes full LLM label distribution (multi-way)" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## 附录 A：Spec 覆盖对照（自检）
| Spec § | 实现于 |
|---|---|
| §4.1 新 SYSTEM_PROMPT + LlmAnswer{probs} + parse_answer 清洗 | Task 2 |
| §4.2 dist_to_gotos + decision_from_dist + eval_llm_dist + eval_llm argmax 派生 | Task 1 / Task 2 |
| §4.3 Cached{probs} + 未知 label 不命中 | Task 2 |
| §4.4 client fetch_probs 共享路径 + eval/eval_dist | Task 2 |
| §4.5 engine/soft LLM 臂多路 | Task 3 |
| §6 行为兼容（Stub/Disabled 保留、既有测试零改动）| Task 2/3 验收标准 |
| §7 测试 | Task 1/2/3 |

## 附录 B：明确不在范围（YAGNI）
- 旧 `{label,confidence}` 双格式兼容；概率校准；树 YAML/LlmNode 结构改动；真实端点 smoke（后续单独做）。
- Stub 的 `"label:p"` 语法仅为测试便利，不进文档协议。
