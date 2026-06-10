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

## 软/概率遍历（`--soft`，可选）

默认是**硬遍历**：每节点选一支、走单路径到单叶。加 `--soft` 切换为**置信度加权软遍历**：
每节点按 `(选中支: confidence, 残余 1-c → default)` 把概率质量沿决策 DAG 传播，得**叶子概率分布**，
再按期望打分 `expected_net = Σ p(leaf)·net(leaf.stance)`，输出 `SoftReport`（`soft.engaged` 为参与点的期望净收益统计）。

```bash
cargo run --release -- backtest --tree examples/trend_tree.yaml \
  --primary 15m.csv --context 1h.csv --soft --out soft_report.json
```

说明：软效果目前体现在 **LLM 节点**（量化节点 confidence=1.0 仍硬）；软模式会评估所有可达节点
（含 LLM `default` 子树里的 LLM 节点），LLM 调用比硬模式多（有缓存兜底）。LLM 的 confidence
是"伪概率"、未做校准，叶子分布请谨慎解读。

软模式也支持 `--traces <file>`：写出逐点 JSONL（每决策点 `{t, leaf_probs, expected_net}`，未计分点 `expected_net` 为 null），可用于离线分析软遍历的叶子分布（report 软曲线消费为后续）。

### 软量化谓词（`strength`）

软模式下，量化分支可选 `strength`（标量 DSL 表达式，clamp[0,1]）表达"命中强度"。
节点按 `when` 选真分支，按 `strength` 做**首真泄漏**：`w_i = remaining·strength_i`，残余给 `default`。
不写 `strength` 则 `strength=1` —— 软模式退化为硬首真（渐进采用）。

```yaml
branches:
  - when: "close > sma(close,20)"
    strength: "sigmoid((close - sma(close,20)) / (0.02 * sma(close,20)))"  # 高于均线 2% 处≈0.88
    goto: leaf_long
```

`sigmoid(x)=1/(1+e^-x)` 是内置 DSL 函数；尺度（`margin/scale`）由作者按指标量纲选定。
（见 `examples/strength_tree.yaml`。）

## 取数（新浪 fetcher）

从新浪财经拉 A股 K 线到本地 CSV（再喂给 backtest）：

    # 小周期 15min
    cargo run --release -- fetch --symbol sh600000 --scale 15 --out 15m.csv
    # 大周期 1h
    cargo run --release -- fetch --symbol sh600000 --scale 60 --out 1h.csv

`--symbol` 形如 `sh600000`(沪) / `sz000001`(深)；`--scale` 为分钟数（15/60/240=日线）；`--datalen` 最多 1023（新浪只给最近这么多根，浅历史）。端点可用 `--base-url` 覆盖。

抓取与回测解耦：fetch 出 CSV 后，照常 `cargo run -- backtest --primary 15m.csv --context 1h.csv ...`。

## 报告可视化（`rquant report`）

把回测产物渲染成**自包含 HTML**（内联 SVG，离线可分享）：

```bash
cargo run --release -- report --report report.json --out report.html \
  --traces traces.jsonl --primary 15m.csv
```

- 含累计前瞻收益曲线、逐点净收益直方图、各叶子平均净收益条形、节点命中条形、headline 表。
- `--traces`/`--primary` 二者都给才画时间序列（可视化器用 `forward_return` 重算逐点 net）；只给 `--report` 则仅画聚合图。
- 累计曲线因前瞻窗口重叠是**信号质量曲线、非可交易净值**（HTML 内有标注）。
