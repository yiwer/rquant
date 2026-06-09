# rquant M5：接入 LLM 节点（OpenAI 标准，异步）— 设计文档

- **日期**：2026-06-09
- **状态**：设计已确认（待 spec 评审 → 进实现计划）
- **关联**：扩展 `2026-06-09-rquant-decision-tree-backtest-design.md`（M1–M4 已实现并合并 master，HEAD `0b51889`）

---

## 1. 背景

M1–M4 已交付纯量化端到端回测；其中 **LLM 节点暂走 `default` 分支**（`engine/traversal.rs` 的 `Node::Llm => default`，rationale "LLM deferred (M5)"）。M5 把 LLM 节点接上真实模型，使树里"少量不可量化"的判断真正由 LLM 完成。

主流平台（DashScope/通义、DeepSeek、OpenAI）均提供 **OpenAI 标准 `chat/completions` 兼容端点**，因此用**单一客户端 + 可配 `base_url`** 即可覆盖全部。

## 2. 目标与非目标

### 目标
1. LLM 节点经 OpenAI 标准 API 求值，强制结构化输出 `{label, confidence, reason}`，`temperature=0`。
2. `base_url` 可配 → 兼容 DashScope / DeepSeek / OpenAI。
3. **内容寻址文件缓存**实现回测可复现 + 省钱（LLM 调用昂贵）。
4. 引擎**异步化**，runner 以有序并发（`buffered(N)`）跑各决策点，加速首轮填缓存。
5. 新增**可选新闻输入**：用户提供按时间戳的新闻文件 → `Context.news`（只消费不采集），喂给 LLM 节点。
6. LLM 不可用/出错时**优雅回退 `default`**——纯量化树无需任何 LLM 配置即可照跑。

### 非目标（YAGNI / 后置）
- 新闻**采集/打分**（仍只消费用户提供的文件）。
- LLM `confidence` 用于软/概率遍历（仅记录，硬遍历不变）。
- 真实网络的 CI 测试（用 Stub + 边界单测；真实端点靠文档化手动 smoke）。
- M6 新浪 fetcher / Parquet/SQLite 缓存。
- 多模型投票、视觉读图。

## 3. 锁定决策

| # | 维度 | 选定 |
|---|---|---|
| 1 | I/O 模型 | **异步**（tokio + reqwest）；traverse/runner 改 async；runner 用 `buffered(N)` 有序并发 |
| 2 | LLM 输入 | **价格上下文 + 可选新闻文件**（`Context.news`，只消费不采集） |
| 3 | 缓存后端 | **内容寻址文件缓存**（`<sha256>.json`，增量持久化、并发安全、无原生依赖）|
| 4 | 并发度默认 | `--concurrency` = **8** |
| 5 | 价格快照默认 | 最近 **20** 根 primary 收盘 + 最新价（定宽格式化；可配）|
| 6 | 缓存目录默认 | `.rquant-cache/llm/`（加入 `.gitignore`）|
| 7 | 新闻文件格式 | CSV `time,score,headline`（M5 仅 CSV）|
| 8 | LLM 启用判据 | `model` + `base_url` + `api_key` 三者齐备 → 启用，否则回退 `default` |

## 4. 架构

### 4.1 异步化（动到 M1–M4，明确改动面见 §6）
- `engine/traversal.rs`：`pub async fn traverse(tree, ctx, llm: &LlmEvaluator) -> Result<Trace>`。Quant 节点同步求值（`eval_quant`，无 await）；Llm 节点 `llm.eval_llm(...).await`。
- `backtest/runner.rs`：`pub async fn run(cfg: &BacktestConfig, llm: &LlmEvaluator) -> Result<Report>`（`LlmEvaluator` 含 reqwest client，不便 Clone/Debug，故作为独立参数而非塞进 `BacktestConfig`）。对 `start..primary.len()` 各决策点构 future（`build_context → traverse → forward_return`），`futures::stream::iter(...).buffered(cfg.concurrency)` **有序**并发执行，按序收集 `Vec<(Trace, Option<ForwardResult>)>` → `compute_metrics`。有序收集保证复现性。
- `cli/mod.rs`：`#[tokio::main] async fn main`（经 `lib::cli::main` 暴露）。

### 4.2 LLM 评估器（枚举派发；不引 async-trait、不用 dyn）
```rust
pub enum LlmEvaluator {
    OpenAi(OpenAiLlm),   // 真实客户端（含 config + cache）
    Disabled,            // 永远走 default（= M1–M4 行为；无 API 配置时）
    Stub(StubLlm),       // 测试用，按 node_id 返回预设 label
}
impl LlmEvaluator {
    pub async fn eval_llm(&self, node_id: &str, node: &LlmNode, ctx: &Context) -> Result<Decision>;
}
```
`LlmNode` = traverse 传入的 LLM 节点视图（`inputs: &[String]`, `prompt: &str`, `labels: &HashMap<String,String>`, `default: &str`）。

`OpenAi::eval_llm` 流程：构造缓存键 → 命中则直接还原 Decision；未命中 → 渲染 prompt → POST → 解析校验 → 落缓存 → 返回。失败/非法（重试后）→ `default`。

### 4.3 输入渲染（`eval/llm/prompt.rs`）
- **system**：`You are a financial-analysis classifier. Choose exactly one label. Respond ONLY with JSON {"label": <one of allowed>, "confidence": <0..1>, "reason": <short>}.`
- **user**：
  - `Question: <node.prompt>`
  - `Allowed labels: [<node.labels 的键>]`
  - `Price context: recent <=20 primary closes = [..定宽..], latest = <close>`
  - 对每个 `inputs` 名：`news_score: <最新可见新闻分 or "none">`、`recent_headlines: <最近K条 or "none">`、未知名 → `<name>: unavailable`
- 价格浮点用定宽（`{:.4}`）渲染，保证缓存键稳定。

### 4.4 `Context.news` + 新闻加载器
- `features/context.rs`：`Context` 加 `pub news: Option<NewsView>`（**只被 LLM 渲染读**；量化 DSL 的 `resolve_series` 不变）。
- `data/news.rs`：
  ```rust
  pub struct NewsRecord { pub time: NaiveDateTime, pub score: f64, pub headline: String }
  pub struct NewsView { pub recent: Vec<NewsRecord> } // time<=t 的最近 K 条
  pub fn read_news_csv(path: &Path) -> Result<Vec<NewsRecord>>; // 表头 time,score,headline；时间升序校验
  ```
- `build_context(primary, context, news: &[NewsRecord], t, window)` → `news = Some(NewsView{ time<=t 的最近 5 条 })`（新闻固定取最近 5 条；更多对 LLM 判断无益且涨提示长度）；传入空切片 → `news = None`。**同 partition_point 防未来函数闸门。**

### 4.5 配置（`eval/llm/mod.rs`）
```rust
pub struct LlmConfig {
    pub base_url: String, pub api_key: String, pub model: String,
    pub timeout_secs: u64, pub max_retries: u32, pub cache_dir: PathBuf,
}
```
- CLI 新增：`--llm-model`、`--llm-base-url`、`--llm-cache-dir`（默认 `.rquant-cache/llm`）、`--news <file>`、`--concurrency <N>`（默认 8）。
- API key 走环境变量 `RQUANT_LLM_API_KEY`。
- 启用判据：`model` 非空 ∧ `base_url` 非空 ∧ env key 非空 → `LlmEvaluator::OpenAi`，否则 `Disabled`。
- 预设（文档/示例）：DashScope `https://dashscope.aliyuncs.com/compatible-mode/v1`、DeepSeek `https://api.deepseek.com/v1`、OpenAI `https://api.openai.com/v1`。

### 4.6 缓存（`eval/llm/cache.rs`）
- 键 = `sha256_hex(model + "\0" + node_id + "\0" + rendered_user_message)`。
- 文件 `cache_dir/<key>.json`，内容 `{ "label": String, "confidence": f64, "reason": String, "model": String }`。
- `get(key) -> Option<Cached>`（读文件）；`put(key, cached)`（原子写：写临时文件再 rename）。增量持久化、并发各写各的。

### 4.7 复现性
`temp=0` + model 进键 + 内容寻址缓存。首轮并发填缓存；重跑全命中 → 零网络、`buffered(N)` 有序 → 逐次一致。**诚实说明**：LLM 即便 temp=0 跨调用/跨版本也非严格确定；保证的是"**缓存填好后，同一回测可复现**"。

### 4.8 错误处理（spec §11 一致）
| 情况 | 处理 |
|---|---|
| LLM 未配置（Disabled）| 走 `default`，rationale "LLM disabled" |
| 网络/超时/5xx | 重试至 `max_retries` → 仍失败走 `default`，rationale 记错误 |
| 响应非 JSON / 缺字段 | 重试 1 次 → 走 `default` |
| `label` 不在 `node.labels` | 重试 1 次 → 走 `default` |
LLM 任何失败都**不中断回测**。

## 5. 关键类型契约

- `Decision`（复用 `eval/mod.rs`）：LLM 节点产出 `{ goto: node.labels[label], label, confidence, rationale }`，rationale 形如 `"LLM: <reason>"` / `"LLM(cached): <reason>"` / `"LLM fallback(<err>): default"`。
- OpenAI 请求体：`{ model, temperature: 0, response_format: {"type":"json_object"}, messages: [system, user] }`，头 `Authorization: Bearer <key>`。
- OpenAI 响应：取 `choices[0].message.content`（JSON 字符串）→ 解析为 `{label, confidence, reason}`。

## 6. 对 M1–M4 的明确改动面

1. **`features/context.rs`**：`Context` 加 `news` 字段；`build_context` 加 `news` 参数。→ 更新其 2 个测试（传 `&[]`，断言不变）。
2. **构造 `Context{}` 字面量的测试助手**：`dsl/eval.rs`、`eval/quant.rs`、`engine/traversal.rs` 的 `ctx(...)` 助手补 `news: None`。
3. **`engine/traversal.rs`**：`traverse` 改 `async` + 加 `llm` 参数；`Node::Llm` 分支改为调用评估器。2 个测试改 `#[tokio::test]` 并传 `LlmEvaluator::Disabled`（quant 测试行为不变）/ `Stub`（llm 测试走具体 label）。
4. **`backtest/runner.rs`**：`run` 改 `async` + 并发，签名 `run(cfg: &BacktestConfig, llm: &LlmEvaluator)`；`BacktestConfig` 加 `news_path: Option<PathBuf>`、`concurrency: usize`（**不**含 LlmEvaluator）；读新闻文件并传入 `build_context`。
5. **`cli/mod.rs`**：`#[tokio::main]`；新增上述 flags；按启用判据构造评估器。
6. **`Cargo.toml`**：加 tokio/reqwest/futures/sha2。
7. **`.gitignore`**：确保忽略 `.rquant-cache/`（及 `target/`）。
8. **`tests/e2e.rs`**：现有 e2e 调 `run()` 处适配 async（`#[tokio::test]` + 新字段）。

> 原则：异步化是机械包裹，量化路径逻辑不变；所有既有断言保持，仅签名/await/新字段调整。

## 7. 模块布局
```
src/eval/llm/
  mod.rs      # LlmEvaluator 枚举 + LlmConfig + eval_llm 派发
  client.rs   # OpenAiLlm：reqwest 调用 + 请求构造 + 响应解析 + 重试 + 回退
  cache.rs    # 内容寻址文件缓存（键/get/put 原子写）
  prompt.rs   # render(node, ctx) -> messages；parse_response
  stub.rs     # StubLlm（#[cfg(test)] 或普通，测试用）
src/data/news.rs   # NewsRecord / NewsView / read_news_csv
```
改动：`features/context.rs`、`engine/traversal.rs`、`backtest/runner.rs`、`cli/mod.rs`、`eval/mod.rs`(加 `pub mod llm;`)、`data/mod.rs`(加 `pub mod news;`)。

## 8. 测试策略
- **单元**：缓存（键稳定性、put→get 往返、原子写）；prompt 渲染（含 prompt+labels+价格+news/缺失渲染）；响应解析（合法 / 非 JSON / 缺字段 / label 越权）；新闻加载 + 防未来函数（`time<=t`）；config 启用判据。
- **评估器（无网络）**：`Stub` 下 `traverse` 到 LLM 节点走**非 default** 分支；`Stub` 报错 → `default`；`Disabled` → `default`。
- **集成（async, 无真实网络）**：扩展/新增 e2e，用 `Stub` 跑 async 全链路，确认 LLM 节点改变路径并影响度量。
- **确定性**：`Stub`/暖缓存下两跑 Report 一致。
- **真实 OpenAi**：仅在"建请求/解析响应"边界单测；真实端点（DashScope/DeepSeek）靠 README 文档化的手动 smoke，不进 CI。

## 9. 复现性与防未来函数
- 新闻经 `build_context` 的 `time<=t` 闸门，**不泄漏未来新闻**（加一条属性/边界测试）。
- 并发 `buffered(N)` 有序收集 + `BTreeMap` 度量 + 缓存内容确定 → 同输入同输出（缓存填好后）。

## 10. 风险与诚实说明
1. **每 bar 调 LLM 很贵/慢**：LLM 节点应被上层量化节点过滤到稀疏；并发 + 缓存缓解；长回测仍需控制 LLM 节点的可达频率与日期范围。
2. **LLM 非严格确定**：靠缓存固化；跨 model 版本结果会变（model 在键里，换版本=换缓存）。
3. **缓存键依赖渲染稳定**：价格定宽格式化、inputs 渲染必须确定，否则缓存命中率塌陷。
4. **reqwest/tokio 体量**：异步选择的既定代价。
5. **新闻文件质量/时点正确性**由用户负责（消费接口）。

## 11. 已确认默认参数
并发 8 · 价格快照=最近 20 根 primary 收盘+最新价 · 缓存目录 `.rquant-cache/llm/` · 新闻 CSV `time,score,headline` · 启用判据=model+base_url+key 齐备。

## 12. 里程碑切分（实现顺序，关键路径）
- **M5.1** 依赖 + `.gitignore` + `data/news.rs`（加载器+防未来函数）
- **M5.2** `Context.news` + `build_context` 改造（含既有测试适配）
- **M5.3** `eval/llm/cache.rs`（内容寻址缓存）
- **M5.4** `eval/llm/prompt.rs`（渲染 + 响应解析）
- **M5.5** `eval/llm/{mod,stub}.rs`（LlmEvaluator 枚举 + LlmConfig + Disabled/Stub）
- **M5.6** `eval/llm/client.rs`（OpenAiLlm：调用/重试/回退/缓存接线）
- **M5.7** `traverse` 异步化 + 接评估器（既有测试转 async）
- **M5.8** `runner` 异步化 + 并发 + 新闻/LLM 接线
- **M5.9** `cli` tokio 化 + 新 flags + 启用判据
- **M5.10** e2e（Stub 全链路）+ README 手动 smoke 文档
