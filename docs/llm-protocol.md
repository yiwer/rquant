# LLM 节点协议参考

本文档基于 `src/eval/llm/{mod,client,prompt,cache}.rs` 的实际代码整理。

---

## 要求的输出格式

LLM 必须返回一个 JSON 对象：

```json
{"probs": {"label_a": 0.6, "label_b": 0.3}, "reason": "简短原因"}
```

- `probs`：每个允许 label 对应一个 [0,1] 的概率值，建议各 label 之和等于 1。
- `reason`：自由文本，写入 Trace 的 `rationale` 字段，供审计使用。

---

## 系统提示词（`SYSTEM_PROMPT`）

以下是 `prompt.rs` 中 `SYSTEM_PROMPT` 常量的逐字内容：

```
You are a financial-analysis classifier. Assign a probability between 0 and 1 to EVERY allowed label; probabilities should sum to 1. Respond ONLY with a JSON object: {"probs": {<label>: <number 0..1>, ...}, "reason": <short string>}.
```

该提示词是缓存键的组成部分：修改提示词后，所有旧缓存条目自动失效。

---

## 用户消息渲染（`render_user`，`prompt.rs`）

用户消息按以下固定格式渲染（确定性，是缓存键的一部分）：

```
Question: <node.prompt>
Allowed labels: [<label_1>, <label_2>, ...]   # label 按字典序排序
Recent primary closes: [1.2345, 1.2678, ...]  # 最近 ≤20 根收盘价，4 位小数
Latest close: 1.2678

# 若 inputs 中包含对应字段：
news_score: 0.5000
recent_headlines: 标题A; 标题B
```

- `inputs` 中目前支持 `news_score`（最近一条新闻评分）与 `recent_headlines`（最近新闻标题，`;` 分隔）。
- 无对应数据时输出 `news_score: none` / `recent_headlines: none`。
- 未知 input 字段输出 `<field>: unavailable`。

---

## 清洗规则（`parse_answer`，`prompt.rs`）

拿到 LLM 原始输出后依次执行：

1. **JSON 解析**：内容必须是合法 JSON；否则 → `Err`（触发重试）。
2. **未知 label 丢弃**：`probs` 中不在 `node.labels` 键集合内的条目静默丢弃。
3. **NaN → 0**：概率值为 NaN 时替换为 0。
4. **clamp[0,1]**：概率值 clamp 到 [0,1]。
5. **零概率丢弃**：clamp 后 p=0 的条目不写入结果。
6. **Σ>1 归一化**：若所有已知 label 之和 > 1，整体除以该和（归一至 Σ=1）。
7. **空/全零检测**：若清洗后结果为空或所有概率均为 0 → `Err`（触发重试或 default 回退）。

清洗后 Σ ≤ 1（残余由消费方分配给 `default`）。

---

## 硬模式 argmax（`decision_from_dist`，`mod.rs`）

```
candidates = probs ∪ {"default": max(0, 1 - Σprobs)}
return argmax(candidates)   # 并列时取 BTreeMap 字典序最小的 label
```

胜出 label 通过 `node.labels` 映射到 goto；若 label 不在映射中，goto = `node.default`。

---

## 软模式多路分布（`dist_to_gotos`，`mod.rs`）

将 label 概率分布转换为 goto 概率分布：
1. 已知 label → 通过 `node.labels` 映射到 goto，同 goto 的概率合并。
2. 残余 `1 - Σprobs`（> 0 时）追加到 `node.default` 的 goto。
3. 产出按 goto 名字典序排列，Σ = 1。

---

## 缓存（`cache.rs`）

**缓存键**（SHA-256 hex，64 字符）：

```
sha256( model \0 base_url \0 system_prompt \0 node_id \0 rendered )
```

五个字段以 `\0` 分隔，共同决定缓存 key：

- `model`：`--llm-model` 参数
- `base_url`：`--llm-base-url` 参数
- `system_prompt`：`SYSTEM_PROMPT` 常量（代码内嵌）
- `node_id`：树中的节点 id
- `rendered`：`render_user` 的完整输出（含价格、labels、inputs）

**keys⊆labels 守卫**：缓存命中时额外检查缓存中所有 key 均在当前 `node.labels` 中；不满足则跳过缓存重新调用（防止树修改后读到旧 label 的缓存）。

**缓存存储**：每条记录写为一个 JSON 文件（`{key}.json`），包含 `probs`/`reason`/`model`。写入使用原子 rename（先写带 PID+计数器的临时文件，再 rename），并发写同一键安全。

**自动失效**：修改 `--llm-model`、`--llm-base-url`、`system_prompt`、节点 id、或 prompt / input 字段任意一项，对应缓存自动失效（键变化）。可手动删除 `.rquant-cache/` 目录清空所有缓存。

---

## 重试与回退（`client.rs`）

- 最大重试次数：`max_retries = 2`（即最多 3 次尝试）。
- 超时：`timeout_secs = 60`。
- 所有尝试均失败时 → `Err`，由调用层回退到 `default_decision`（`default` 分支，`confidence=0.0`）。

---

## `LlmEvaluator` 枚举

| 变体 | 说明 |
|---|---|
| `OpenAi(OpenAiLlm)` | 真实调用，带缓存和重试 |
| `Disabled` | LLM 三项参数任一为空时启用；所有 LLM 节点直接走 `default`（`confidence=0.0`，rationale 含 "LLM disabled"） |
| `Stub(StubLlm)` | 测试用；按 `answers: HashMap<node_id, answer>` 预置答案。格式：普通 label 字符串（confidence 0.9）、`"label_a:0.5,label_b:0.3"` 多 label 分布、`"ERROR"` 触发 default 回退 |

---

## 提供商配置预设（2026-06 实测可用）

| 提供商 | `--llm-base-url` | 示例 `--llm-model` |
|---|---|---|
| DeepSeek | `https://api.deepseek.com` | `deepseek-chat` |
| 阿里云 DashScope（通义千问） | `https://dashscope.aliyuncs.com/compatible-mode/v1` | `qwen-plus` |

两者均实现 OpenAI 标准 `/chat/completions` 接口，兼容 `response_format: {"type": "json_object"}`（强制 JSON 输出模式，`temperature=0`）。

---

## 环境变量

| 变量 | 说明 |
|---|---|
| `RQUANT_LLM_API_KEY` | Bearer token，与 `--llm-model` + `--llm-base-url` 三者同时非空时 LLM 生效 |
