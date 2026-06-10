# rquant：LLM 返回完整 label 概率分布 — 设计文档

- **日期**：2026-06-10
- **状态**：设计已确认（写 spec + 计划）
- **关联**：软遍历多路边模型（`270c8a2`）与 M5 LLM 节点已合并 master（HEAD 谱系 `6c2866a`）。本设计把 LLM 节点输出从 `{label, confidence}` 二分升级为对所有 label 的概率分布。

---

## 1. 背景

LLM 节点现在返回单 `{label, confidence}`：硬遍历走该 label，软遍历按 `(chosen:c, default:1-c)` 二分。对 ≥3 个 label 的节点，这丢失了"次优 label 也有质量"的信息。软遍历的边模型已是多路 `Vec<(goto, weight)>`——LLM 分布可直接喂入。**关键便利**：缓存键含 `SYSTEM_PROMPT`，改提示词自动隔离新旧缓存，无需迁移。

## 2. 目标与非目标

### 目标
1. LLM 输出协议升级为 `{"probs": {label: p, ...}, "reason": ...}`（全面替换，不兼容旧格式）。
2. 解析+清洗为标准化 label 分布（丢未知 label、clamp、Σ>1 归一、残余→default）。
3. 新统一出口 `eval_llm_dist` → goto 分布（Σ=1）；软遍历多路消费；硬遍历从分布 argmax 派生 `Decision`（行为接口不变）。
4. 缓存改存 label 分布；Stub/Disabled 行为保留（既有测试零改动）。

### 非目标（YAGNI）
- 旧 `{label, confidence}` 双格式兼容（缓存键自动隔离，无需）。
- 概率校准 / 温度缩放（probs 仍是伪概率）。
- 树 YAML / `LlmNode` 结构改动（labels/default 不变）。

## 3. 锁定决策

| # | 维度 | 选定 |
|---|---|---|
| 1 | 协议 | 全面换 `{"probs": {...}, "reason": ...}`；新 SYSTEM_PROMPT；不兼容旧格式 |
| 2 | 清洗 | 丢未知 label；p clamp[0,1]、NaN→0；Σ>1 整体归一；Σ≤1 保留（残余→default）；全空/全零 → Err |
| 3 | 硬模式 | 对 goto 分布取 argmax（default 残余参与竞争；并列取字典序小）→ `Decision{confidence=p_max}` |
| 4 | Stub | `[(labels[label], 0.9), (default, 0.1)]`——现行为逐字保留 |
| 5 | 缓存 | `Cached{probs: BTreeMap<label,p>, reason, model}`（存清洗后 label 分布，命中重映射 goto）|
| 6 | 软消费 | `engine/soft.rs` LLM 臂直接用 `eval_llm_dist` 的多路 vec |

## 4. 架构

### 4.1 prompt.rs
- 新 `SYSTEM_PROMPT`：要求对每个 allowed label 给 0..1 概率、和≈1，`Respond ONLY with {"probs": {<label>: <p>, ...}, "reason": <short string>}`。
- `LlmAnswer { probs: BTreeMap<String, f64>, #[serde(default)] reason: String }`。
- `parse_answer(content, allowed) -> Result<LlmAnswer>`：
  1. JSON 解析失败 → Err。
  2. 保留 `allowed` 内的 label；p NaN→0、clamp[0,1]。
  3. `sum = Σp`；`sum > 1.0` → 每项 ÷sum；`sum == 0` 或清洗后为空 → Err（上层回退）。
  4. 返回清洗后的 `LlmAnswer`（Σ ≤ 1；残余 1-Σ 由消费方隐式归 default）。

### 4.2 mod.rs — eval_llm_dist + argmax 派生
```rust
/// label 分布 → goto 分布：label→labels[label] 映射、同 goto 合并、补 (default, 1-Σ)（>0 时）。Σ=1。
pub fn dist_to_gotos(node: &LlmNode<'_>, probs: &BTreeMap<String, f64>) -> Vec<(String, f64)>

impl LlmEvaluator {
    /// 统一出口：goto 分布（Σ=1）+ rationale。
    pub async fn eval_llm_dist(&self, node_id, node, ctx) -> Result<(Vec<(String, f64)>, String)>
    // OpenAi → client 调用→parse→dist_to_gotos；Disabled → [(default,1.0)]；
    // Stub → answers[node]=label 命中: [(labels[label],0.9),(default,0.1)]（同 goto 合并），"ERROR"/未命中: [(default,1.0)]。
}

/// 硬模式派生：goto 分布 argmax（并列取字典序小）→ Decision{goto, confidence=p_max, label, rationale}。
/// label：argmax goto 来自哪个 label 则用之；default 残余胜出 → "default"。
pub fn decision_from_dist(node, dist_label_probs, rationale) -> Decision
```
- `eval_llm`（现有签名）保留，内部改为"取分布 → argmax 派生 Decision"；硬遍历（`engine/traversal.rs`）零改动。
- argmax 在 **label 分布 + default 残余**上做（而非合并后的 goto 上），以保留 label 名；实现上可对 `[(label,p)...,("default",残余)]` 取 max。

### 4.3 cache.rs
`Cached { probs: BTreeMap<String, f64>, reason: String, model: String }`（替换 label/confidence 字段）。命中校验：probs 的 keys ⊆ node.labels（否则视为不命中重新调用——防树改 label 后误用）。键含 SYSTEM_PROMPT → 新旧缓存自动隔离。

### 4.4 client.rs（OpenAi）
`eval` 流程不变（缓存→调用(重试)→落缓存→失败回退 default），但内容换 probs：命中 → `Cached.probs`；调用成功 → `parse_answer` 清洗 → 存 `Cached{probs,...}`。对外提供给 `eval_llm_dist` 用的"取分布"路径与给 `eval_llm` 用的 argmax 派生共用同一份分布获取逻辑（仅出口不同）。

### 4.5 engine/soft.rs
LLM 臂：
```rust
Node::Llm { inputs, prompt, labels, default } => {
    let ln = LlmNode { inputs, prompt, labels, default };
    let (dist, _rationale) = llm.eval_llm_dist(&id, &ln, ctx).await?;
    dist
}
```
（多路边模型已就绪；2-label 节点行为与现状一致，≥3 label 首次真正多路。）

## 5. 错误处理
- 解析失败/全零分布 → Err → client 现有 retry，重试尽 → 回退 `default_decision`/`[(default,1.0)]`（语义同现状）。
- 缓存 probs 含未知 label（树改过）→ 视为缓存不命中，重新调用。
- `dist_to_gotos` 数学保证 Σ=1（清洗后 Σ≤1 + 残余补齐）；软遍历 `debug_assert(sum≈1)` 继续兜底。

## 6. 行为兼容性
- **Stub/Disabled 行为逐字保留** → 既有全部软/硬测试与 e2e 不变。
- 硬遍历接口（`eval_llm -> Decision`）不变；真实 OpenAi 路径下单 label 节点 argmax ≈ 旧"选中 label"语义。
- 旧缓存文件不被读取（键不同），留在 `.rquant-cache/` 无害（可手动清理）。

## 7. 测试
- `parse_answer`：合法分布；Σ>1 归一（断言归一后 Σ=1）；未知 label 丢弃；NaN/越界 clamp；全零/非 JSON/空 → Err。
- `dist_to_gotos`：label→goto 映射、同 goto 合并、残余→default、Σ=1。
- `decision_from_dist`：argmax 正确（含 default 残余胜出 → label="default"；并列字典序）。
- `eval_llm_dist`：Stub 命中/ERROR/未命中；Disabled。
- cache：`Cached{probs}` 往返；未知 label 视为不命中。
- `engine/soft.rs`：3-label LLM 节点（Stub 扩展或注入分布）→ 多路叶子分布、Σ=1；既有 5+ 软测试不变。
- e2e：既有 Stub 全链路全部不变（行为保留是验收标准）。

## 8. 风险
1. **模型不照协议输出**：JSON mode + 明确 prompt 缓解；解析失败有 retry+default 兜底。
2. **probs 仍是伪概率**（未校准）——沿用既有警告口径。
3. **argmax 派生的 label 语义**：default 残余可能胜出（label="default"）——与"模型对所有 label 都不确信→走 default"语义一致。
4. **真实端点未实测**：协议变更后建议跑一次真实 smoke（后续）。

## 9. 里程碑

> 编译耦合提示：改 `LlmAnswer`/`Cached` 字段会立刻破坏 `client.rs` 的使用点，故 prompt/cache/client/mod 的切换必须在同一任务内完成。

- **T1**（纯增量，独立编译）`mod.rs`：`dist_to_gotos` + `decision_from_dist`（皆为新函数，操作 `BTreeMap<label,p>`）+ 测试。
- **T2**（耦合切换，一次完成）`prompt.rs` 新 SYSTEM_PROMPT + `LlmAnswer{probs}` + `parse_answer` 清洗；`cache.rs` `Cached{probs}`（含未知 label 不命中校验）；`client.rs` OpenAi 流程接 probs；`mod.rs` `eval_llm_dist`（OpenAi/Disabled/Stub）+ `eval_llm` 改为 argmax 派生（`decision_from_answer` 移除或内化）+ 测试。全量须保持绿（Stub/Disabled 行为保留）。
- **T3** `engine/soft.rs` LLM 臂改 `eval_llm_dist` + 3-label 多路测试 + 全量回归 + README。
