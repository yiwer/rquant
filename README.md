# rquant — 模糊决策树 A股回测引擎

基于声明式 YAML 决策树的 A股离线回测工具：量化指标 + 可选 LLM 节点取代人工逐节点判断，前瞻收益评分验证策略 edge，支持硬遍历（单路径）与软/概率遍历（叶子分布）。

---

## Quick Start

```bash
# 1. 拉取 K 线
cargo run --release -- fetch --symbol sh600000 --scale 15 --out 15m.csv
cargo run --release -- fetch --symbol sh600000 --scale 60 --out 1h.csv

# 2. 回测（可选 --aux 挂载外部序列，如指数 CSV，DSL 用 aux.idx.close 引用）
cargo run --release -- backtest \
  --tree examples/trend_tree.yaml \
  --primary 15m.csv --context 1h.csv \
  --out report.json --traces traces.jsonl \
  --aux idx=index.csv

# 3. 渲染 HTML 报告
cargo run --release -- report \
  --report report.json --out report.html \
  --traces traces.jsonl --primary 15m.csv
```

---

## CSV 格式

```
time,open,high,low,close,volume
2024-01-02 09:45:00,10.5,10.8,10.4,10.7,123456
```

时间格式 `YYYY-MM-DD HH:MM:SS`（本地时间，无时区后缀），按时间严格升序排列。

新闻 CSV（可选）：`time,score,headline`

---

## `backtest` 标志表

| 标志 | 默认值 | 说明 |
|---|---|---|
| `--tree` | 必填 | 决策树 YAML 路径 |
| `--primary` | 必填 | 主周期 CSV（如 15m） |
| `--context` | 必填 | 大周期 CSV（如 1h） |
| `--news` | — | 新闻 CSV（可选） |
| `--out` | `report.json` | 输出报告 JSON |
| `--traces` | — | 逐点 JSONL trace（可选） |
| `--cost-bps` | `10.0` | 往返成本（基点），10 bps = 0.1% |
| `--warmup` | `100` | 跳过前 N 根 bar（指标预热） |
| `--window` | `100` | Context 窗口大小（每时点最多取 N 根 bar） |
| `--concurrency` | `8` | 异步并发度 |
| `--holidays` | — | A股节假日文件（一行一个 YYYY-MM-DD，# 注释） |
| `--folds` | `0` | Walk-forward 折数（≥2 启用，兼容 `--soft`） |
| `--soft` | false | 软/概率遍历模式 |
| `--llm-model` | `""` | LLM 模型名（空则 LLM disabled） |
| `--llm-base-url` | `""` | LLM API base URL |
| `--llm-cache-dir` | `.rquant-cache/llm` | LLM 缓存目录 |
| `--aux NAME=PATH` | — | 挂载外部 aux 序列（可重复）；DSL: `aux.<name>.<column>` |

`--warmup` 控制跳过多少根 bar 再开始出决策；`--window` 控制 Context 里能看到多少根历史 bar。两者独立：`--window` 应 ≥ 树中最长指标窗口参数（否则预热后仍可能遇到 NaN）。

---

## LLM 节点

三项同时非空时 LLM 生效：`--llm-model`、`--llm-base-url`、环境变量 `RQUANT_LLM_API_KEY`。

```bash
# DeepSeek（2026-06 实测可用）
export RQUANT_LLM_API_KEY=sk-xxx
cargo run --release -- backtest --tree ... --primary ... --context ... \
  --llm-model deepseek-chat --llm-base-url https://api.deepseek.com

# DashScope 通义千问（2026-06 实测可用）
--llm-model qwen-plus --llm-base-url https://dashscope.aliyuncs.com/compatible-mode/v1
```

任一为空则以 `Disabled` 运行：LLM 节点走 `default` 分支，纯量化照常。

LLM 节点输出协议、缓存机制、清洗规则见 [docs/llm-protocol.md](docs/llm-protocol.md)。

---

## 软遍历与 `strength: "auto"`

加 `--soft` 启用置信度加权软遍历：概率质量沿决策 DAG 传播，得叶子概率分布，按期望 `Σ p(leaf)·net` 打分。

```bash
cargo run --release -- backtest --tree examples/trend_tree.yaml \
  --primary 15m.csv --context 1h.csv --soft --out soft_report.json --traces soft_traces.jsonl
```

quant 分支可选 `strength`（标量 DSL 表达式，clamp[0,1]）控制软模式下的概率分配比例。`strength: "auto"` 对 `when` 表达式做模糊求值（sigmoid 软比较，默认 scale=0.02）；`strength: "auto(0.05)"` 指定 scale。不写 `strength` 等价于 `strength=1`（软模式退化为硬首真）。

软报告含两套口径：`engaged`（逐腿期望收益）与 `position`（净仓位 E = Σ p·dir 后的净额收益）；long/flat 树下数学等价，启用 short 后 `position` 更贴近实际执行。

详见 [docs/tree-yaml-schema.md](docs/tree-yaml-schema.md) 与 [docs/dsl-reference.md](docs/dsl-reference.md)。`examples/factor_tree.yaml` 展示了 `params`/`factors` 命名块与叶子 `weight`/`horizon` 的完整用法。

---

## Walk-forward

```bash
cargo run --release -- backtest ... --folds 5
cargo run --release -- backtest ... --folds 5 --soft
```

`--folds K`（K≥2）把决策点按时间等分 K 折，逐折输出 n/mean_net/hit/buy&hold 与汇总（positive 折数、最差折均值），回答"edge 是全程稳定还是一段行情撞的"。这是**固定树的时间稳定性分析**，不含参数寻优。前瞻窗口跨折边界未裁剪（与全局重叠警告同口径）。

---

## 持仓模拟（`--sim`）

`--sim` 启用顺序权益模拟模式，与前瞻打分模式互补：

```bash
# 硬 sim：树目标 = stance × weight，risk 块提供止损/止盈/最大持仓
cargo run --release -- backtest \
  --tree examples/sim_tree.yaml \
  --primary 60m.csv --context 60m.csv \
  --sim --warmup 30 --out sim_report.json

# 软 sim：目标 = E = Σ p·w·dir（连续仓位），可与 --soft 组合
cargo run --release -- backtest \
  --tree examples/sim_tree.yaml \
  --primary 60m.csv --context 60m.csv \
  --sim --soft --out sim_soft_report.json
```

输出摘要示例：
```
总收益率:     +12.34%
最大回撤:      -6.78%
回合数:            15
胜率:           53.3%
平均持仓 bar:   18.2
换手:           8.40
Buy & Hold:    +9.01%
```

**定位区别**

| 维度 | 前瞻打分（默认） | 模拟（`--sim`） |
|---|---|---|
| 核心问题 | 信号是否有统计 edge？ | 策略在历史上是否盈利？ |
| 执行模型 | 逐点独立，可并发 | 顺序，T+1，有持仓状态 |
| 评分口径 | 前瞻 N bar 收益 | 实际净值曲线（三段记账）|
| 成本 | 往返 `cost_bps` | 单边 `cost_bps/2`（一进一出合计 round-trip）|
| 风控 | 无 | 树顶层 `risk:` 块（stop/tp/max_hold）|

**诚实边界**

- Bar 粒度：用 open/close 模拟，无盘中价位成交，不能捕捉跳空行情内的逐笔滑点。
- 无涨跌停过滤：涨停板买入 / 跌停板卖出被视为可执行，实盘中不一定成交。
- 成本模型：单边 `cost_bps/2`，不含印花税差异（A 股卖出单边 0.1%）——如需精确，请将 `--cost-bps` 设为 `stamp_tax×2 + commission×2`。

---

## fetch

```bash
cargo run --release -- fetch --symbol sh600000 --scale 15 --out 15m.csv
cargo run --release -- fetch --symbol sh600000 --scale 240 --out daily.csv  # scale=240 为日线别名
```

`--datalen` 默认 1023（新浪上限）。默认端点 `https://quotes.sina.cn/cn/api/json_v2.php`（2026-06 可用）；旧端点 `money.finance.sina.com.cn` 已不可用，可用 `--base-url` 覆盖。

---

## report

```bash
# 硬模式（--traces 和 --primary 同时给出才画时间曲线）
cargo run --release -- report --report report.json --out report.html \
  --traces traces.jsonl --primary 15m.csv

# 软模式（不需要 --primary）
cargo run --release -- report --soft --report soft_report.json \
  --traces soft_traces.jsonl --out soft.html
```

软模式下 `--primary` 被忽略（附提示）；`expected_net` 已内含于 traces。

---

## 缓存与复现性

LLM 调用 `temperature=0`，按 `sha256(model, base_url, system_prompt, node_id, rendered)` 缓存到 `--llm-cache-dir`。首轮并发填缓存，重跑全命中，零网络且可复现。修改 prompt / model / base_url 任意一项时旧缓存自动失效；可手动删除 `.rquant-cache/` 清空。

回测输出的字段顺序、度量排序、HTML 内容对相同输入字节稳定（`BTreeMap`、确定性 serde、原子写缓存）。

---

## 文档索引

| 文档 | 内容 |
|---|---|
| [docs/dsl-reference.md](docs/dsl-reference.md) | DSL 语法、标识符、运算符、函数完整表、NaN 弃权、模糊语义 |
| [docs/tree-yaml-schema.md](docs/tree-yaml-schema.md) | YAML schema 完整字段、strength 三种形式、校验规则 |
| [docs/cli-reference.md](docs/cli-reference.md) | 全部 CLI 标志与默认值表，非显而易见的标志详解 |
| [docs/architecture.md](docs/architecture.md) | 九层架构职责、数据流、与原始 spec 的偏离、复现性不变量 |
| [docs/llm-protocol.md](docs/llm-protocol.md) | LLM 输出格式、系统提示词、清洗规则、缓存键、提供商预设 |
| [docs/superpowers/specs/](docs/superpowers/specs/) | 历史设计 spec（记录原始决策，部分功能尚未实现，以当前代码为准） |

---

## 构建与测试

```bash
cargo build --release
cargo test
```
