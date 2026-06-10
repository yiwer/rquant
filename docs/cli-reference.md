# CLI 参考手册

本文档基于 `src/cli/mod.rs` 的实际代码整理。所有默认值以代码中的 `default_value` / `default_value_t` 为准。

---

## 概览

```
rquant <SUBCOMMAND>

子命令：
  backtest    在本地 CSV K 线上运行回测（可选 LLM 节点）
  fetch       从新浪财经拉取 K 线到本地 CSV
  report      把回测产物（JSON + traces）渲染为自包含 HTML
```

---

## `backtest` 子命令

```
rquant backtest [OPTIONS]
```

### 标志一览

| 标志 | 类型 | 默认值 | 说明 |
|---|---|---|---|
| `--tree <PATH>` | PathBuf | 必填 | 决策树 YAML 文件路径 |
| `--primary <PATH>` | PathBuf | 必填 | 主周期 CSV（如 15m.csv） |
| `--context <PATH>` | PathBuf | 必填 | 大周期 CSV（如 1h.csv） |
| `--news <PATH>` | PathBuf | 可选 | 新闻 CSV（表头 `time,score,headline`） |
| `--out <PATH>` | PathBuf | `report.json` | 输出报告 JSON 路径 |
| `--traces <PATH>` | PathBuf | 可选 | 若给出则写逐点 JSONL trace 文件 |
| `--cost-bps <f64>` | f64 | `10.0` | 往返成本（基点），10 bps = 0.1% |
| `--warmup <usize>` | usize | `100` | 跳过前 N 根 primary bar（指标预热） |
| `--window <usize>` | usize | `100` | 传入 Context 的窗口大小（每个时点最多取最近 N 根 bar） |
| `--concurrency <usize>` | usize | `8` | 异步并发度（同时运行的遍历任务数） |
| `--holidays <PATH>` | PathBuf | 可选 | A 股节假日文件，用于缺口检测 |
| `--folds <usize>` | usize | `0` | Walk-forward 折数，≥2 时启用 |
| `--soft` | bool | `false` | 启用软/概率遍历模式 |
| `--llm-model <string>` | string | `""` | LLM 模型名，如 `deepseek-chat` |
| `--llm-base-url <string>` | string | `""` | LLM API base URL |
| `--llm-cache-dir <PATH>` | PathBuf | `.rquant-cache/llm` | LLM 响应缓存目录 |

### 非显而易见的标志说明

**`--warmup` vs `--window`**

两者独立控制不同维度：
- `--warmup N`：跳过前 N 根 bar，不产生决策点。目的是让指标（如 sma(close,20)）完成预热，避免早期 NaN。典型值等于树中最长窗口参数。
- `--window N`：每个决策时点向 `Context` 注入最多 N 根历史 bar。`--window` 应 ≥ 树中最大的指标窗口参数，否则即便过了预热期也会因 Series 过短而得到 NaN。

**`--folds`**

`--folds K`（K≥2）把决策点按时间等分 K 个连续折，对每折独立计算 n/mean_net/hit_rate/buy_and_hold，汇总 positive 折数与最差折均值。这是**固定树的时间稳定性分析**，不含样本内参数寻优。**同时兼容 `--soft` 模式**，软模式下以 expected_net 作为每点净收益。

**`--soft`**

切换为置信度加权软遍历（`traverse_soft`）。每个节点的概率质量沿 DAG 传播，得叶子概率分布，再按期望打分 `expected_net = Σ p(leaf)·net(leaf.stance)`。输出写入 `--out`（`SoftReport` JSON），traces 每点包含 `{t, leaf_probs, expected_net}`。

**`--holidays`**

文件格式：每行一个 `YYYY-MM-DD`，空行与以 `#` 开头的行忽略（注释）。用于 `AShareCalendar` 的缺口检测；未提供时周末/节假日可能被误报为缺失交易日。

**LLM 启用条件**

三项同时非空时 LLM 生效：
1. `--llm-model`（非空字符串）
2. `--llm-base-url`（非空字符串）
3. 环境变量 `RQUANT_LLM_API_KEY`（非空字符串）

任一为空，引擎打印提示并以 `LlmEvaluator::Disabled` 运行；LLM 节点走其 `default` 分支，纯量化部分照常。

---

## `fetch` 子命令

```
rquant fetch [OPTIONS]
```

### 标志一览

| 标志 | 类型 | 默认值 | 说明 |
|---|---|---|---|
| `--symbol <string>` | string | 必填 | 股票代码，如 `sh600000`（沪）/ `sz000001`（深） |
| `--scale <u32>` | u32 | 必填 | K 线周期（分钟）：`15`、`60`、`240`（日线别名） |
| `--out <PATH>` | PathBuf | 必填 | 输出 CSV 路径 |
| `--datalen <u32>` | u32 | `1023` | 最多拉取的 bar 数，新浪上限 1023 |
| `--base-url <string>` | string | `https://quotes.sina.cn/cn/api/json_v2.php` | 新浪 API 端点 base URL |

### 说明

`scale=240` 是日线的新浪别名（一个交易日 = 4 × 60 分钟 = 240 分钟）。

**端点说明（2026-06）**：旧端点 `money.finance.sina.com.cn` 回应"Service not valid"，当前可用端点为 `https://quotes.sina.cn/cn/api/json_v2.php`（已设为默认值）。如端点再次变更，用 `--base-url` 覆盖。

---

## `report` 子命令

```
rquant report [OPTIONS]
```

### 标志一览

| 标志 | 类型 | 默认值 | 说明 |
|---|---|---|---|
| `--report <PATH>` | PathBuf | 必填 | `report.json` 或 `soft_report.json` 路径 |
| `--out <PATH>` | PathBuf | `report.html` | 输出 HTML 路径 |
| `--traces <PATH>` | PathBuf | 可选 | traces JSONL 路径（硬模式用于画时间序列曲线） |
| `--primary <PATH>` | PathBuf | 可选 | primary CSV（硬模式绘曲线时需要，与 `--traces` 配套） |
| `--soft` | bool | `false` | 渲染软模式报告（`SoftReport` + `SoftStepRecord`） |

### 说明

**硬模式**：`--traces` 与 `--primary` 需**同时给出**才能绘时间序列曲线（前瞻收益累计曲线）；只给 `--report` 则仅渲染聚合图表。

**软模式（`--soft`）**：渲染 `SoftReport` JSON，`expected_net` 已内含于 traces 中，**不需要也不使用 `--primary`**。若指定了 `--primary`，会打印提示（`--primary ignored in --soft report`）但不报错。

---

## 环境变量

| 变量 | 说明 |
|---|---|
| `RQUANT_LLM_API_KEY` | LLM API 密钥（bearer token），与 `--llm-model` + `--llm-base-url` 三者同时非空时 LLM 生效 |
