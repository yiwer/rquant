# rquant

基于模糊决策树的 A股离线回测引擎。决策树（YAML + 表达式 DSL）由用户提供，量化指标 + 少量 LLM 取代人工逐节点判断；前瞻收益评分验证策略 edge。

## 构建与测试

    cargo build --release
    cargo test

## 运行回测（纯量化）

    cargo run --release -- backtest \
      --tree examples/trend_tree.yaml \
      --primary 15m.csv --context 1h.csv \
      --out report.json --traces traces.jsonl

CSV 表头：`time,open,high,low,close,volume`（time 形如 `2024-01-02 09:45:00`）。

## 启用 LLM 节点（OpenAI 标准，兼容 DashScope/DeepSeek）

设置 API key 环境变量并指定 model + base_url：

    # DeepSeek
    export RQUANT_LLM_API_KEY=sk-xxx
    cargo run --release -- backtest --tree ... --primary ... --context ... \
      --llm-model deepseek-chat --llm-base-url https://api.deepseek.com/v1

    # DashScope (通义千问)
    --llm-model qwen-plus --llm-base-url https://dashscope.aliyuncs.com/compatible-mode/v1

    # OpenAI
    --llm-model gpt-4o-mini --llm-base-url https://api.openai.com/v1

三者（model + base_url + 环境变量 key）缺一即回退：LLM 节点走 `default` 分支（纯量化照常）。

可选新闻输入：`--news news.csv`（表头 `time,score,headline`）→ 填入 Context.news，供 LLM 节点的 `news_score` / `recent_headlines` 使用。

## 复现性与缓存

LLM 调用 `temperature=0`，结论按 `hash(model + node_id + 渲染输入)` 缓存于 `--llm-cache-dir`（默认 `.rquant-cache/llm/`）。首轮并发填缓存；重跑全命中 → 零网络、可复现。注意：LLM 即便 temp=0 也非严格确定，复现性由缓存保证。

> 每个到达 LLM 节点的决策点都会发起一次调用（未命中缓存时）。请让 LLM 节点处于被量化节点过滤后的稀疏位置，并控制回测区间，以免产生大量调用与费用。
