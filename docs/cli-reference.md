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
  portfolio   横截面组合：同一棵树逐标的打分，持仓 top-N 等权
  factor      横截面因子检验：IC/RankIC、衰减阶梯、分层回测、相关性矩阵
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
| `--folds <usize>` | usize | `0` | Walk-forward 折数，≥2 时启用（`--sim` 下忽略并打印提示） |
| `--soft` | bool | `false` | 启用软/概率遍历模式（可与 `--sim` 组合） |
| `--sim` | bool | `false` | 持仓状态模拟模式（顺序权益，见下方模式对照） |
| `--llm-model <string>` | string | `""` | LLM 模型名，如 `deepseek-chat` |
| `--llm-base-url <string>` | string | `""` | LLM API base URL |
| `--llm-cache-dir <PATH>` | PathBuf | `.rquant-cache/llm` | LLM 响应缓存目录 |
| `--aux NAME=PATH（可重复）` | string | — | 挂载外部 aux 序列；DSL 经 `aux.<name>.<column>` 引用 |

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

**`--aux NAME=PATH`（可重复）**

挂载任意外部数值序列（通用列 CSV），供决策树 DSL 通过 `aux.<name>.<column>` 引用。可多次指定，每次挂载一张表，名字不可重复。

aux CSV 格式要求：
- **首列必须命名为 `time`**，支持两种格式：`YYYY-MM-DD HH:MM:SS`（日内精确）与 `YYYY-MM-DD`（日频，自动展开为当天 00:00:00）；
- **其余列为任意数值（f64）列**，列名不得含 `.` 或空白；
- **时间严格递增**，否则加载时报错。

**低频序列**（如日线指数）与高频 primary（如 15m）挂载时，`build_context` 对每个决策点 `t` 取 `time ≤ t` 的所有行（最近已知值语义），不做重采样；DSL 中 `aux.idx.v[-1]` 即取该截断后的倒数第二行。

如果 `--aux name=path.csv` 中 `name` 对应的表未被任何 DSL 表达式引用，它不会产生错误；如果 DSL 引用了未挂载的表名，运行时报错并给出提示文案：`aux table '<name>' not mounted (use --aux <name>=path.csv)`。

**`--sim`**

切换为持仓状态模拟模式（`run_sim`）：顺序逐 bar 执行，树产出**目标仓位**而非前瞻打分，模拟器按差额交易、三段记账净值（prev_close→open 旧仓段 + 成本段 + open→close 新仓段），输出 `SimReport`（总收益/最大回撤/回合数/胜率/换手/buy&hold + 回合列表）。与 `--soft` 可自由组合：硬 sim（目标 = 叶 `stance×weight`）/ 软 sim（目标 = `E = Σ p·w·dir`）。`--folds` 在 sim 模式下被忽略（打印提示）。

---

### 模式对照

| 模式 | 触发 | 产出目标 | 执行语义 | 适用场景 |
|---|---|---|---|---|
| 前瞻打分（默认） | 无额外标志 | 前瞻收益期望 / hit rate | 逐点独立，并发，无顺序约束 | 信号质量研究，验证 edge 是否存在 |
| 软打分 | `--soft` | 期望净值 `E = Σp·w·dir` | 同上，叶子概率分布 | 连续信号强度研究 |
| 模拟（硬） | `--sim` | 目标仓位（叶 `stance×weight`）| 顺序权益，T+1，成本单边 rt/2 | 策略回测，测量资金曲线与回撤 |
| 模拟（软） | `--sim --soft` | 目标仓位（`E = Σp·w·dir`）| 同上，连续调仓 | 软权重驱动的连续仓位策略 |

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
| `--adjust <string>` | string | `none` | 复权方式：`none`（raw，不复权）/ `qfq`（前复权，via 腾讯日线） |
| `--base-url <string>` | string | `https://quotes.sina.cn/cn/api/json_v2.php` | 新浪 API 端点 base URL |

### 说明

`scale=240` 是日线的新浪别名（一个交易日 = 4 × 60 分钟 = 240 分钟）。

**端点说明（2026-06）**：旧端点 `money.finance.sina.com.cn` 回应"Service not valid"，当前可用端点为 `https://quotes.sina.cn/cn/api/json_v2.php`（已设为默认值）。如端点再次变更，用 `--base-url` 覆盖。

**`--adjust qfq` 三源合成原理（分钟线）**：当日复权因子 = 腾讯前复权日线 close ÷ 腾讯 raw 日线 close；该因子乘到新浪分钟 OHLC 各价格上（volume 不动）。日线（scale=240）则直接从腾讯 fqkline 拉取前复权日线，不经合成。

### 数据源表

| 数据源 | 用途 | 说明 |
|---|---|---|
| 新浪 `quotes.sina.cn` | 分钟 raw OHLCV（scale < 240） | 不复权原始价格，`--adjust none` 的唯一来源 |
| 腾讯 `web.ifzq.gtimg.cn` fqkline（`day` 键） | 日线 raw close（因子分母） | 仅在 `--adjust qfq` + 分钟线时拉取 |
| 腾讯 `web.ifzq.gtimg.cn` fqkline（`qfqday` 键） | 日线前复权 close（因子分子）/ scale=240 直接输出 | `--adjust qfq` 的前复权来源 |

---

## `report` 子命令

```
rquant report [OPTIONS]
```

### 标志一览

| 标志 | 类型 | 默认值 | 说明 |
|---|---|---|---|
| `--report <PATH>` | PathBuf | 必填 | 报告 JSON 文件路径（hard/soft/sim/portfolio 各对应其产出） |
| `--out <PATH>` | PathBuf | `report.html` | 输出 HTML 路径 |
| `--traces <PATH>` | PathBuf | 可选 | traces JSONL 路径（hard/soft/sim 模式用于画时间序列曲线） |
| `--primary <PATH>` | PathBuf | 可选 | primary CSV（硬模式绘曲线时需要，与 `--traces` 配套） |
| `--soft` | bool | `false` | 渲染软模式报告（`SoftReport` + `SoftStepRecord`） |
| `--sim` | bool | `false` | 渲染 sim 模式报告（`SimReport` + 可选 `SimStepRecord` traces） |
| `--portfolio` | bool | `false` | 渲染组合报告（`PortfolioReport`，自包含，无需 traces/primary） |

`--soft` / `--sim` / `--portfolio` 三个标志**互斥**；同时指定两个以上时命令返回错误。不指定任何模式标志则默认为 hard 模式。

### 四种渲染模式

| 模式标志 | 输入文件 | 需要 `--traces`？ | 需要 `--primary`？ | 产出图表 |
|---|---|---|---|---|
| 无（hard 默认） | `Report` JSON（`backtest` 产出） | 可选（与 `--primary` 配套才绘曲线） | 可选（与 `--traces` 配套） | 累计前瞻收益曲线、收益分布直方图、叶子/节点条形图、walk-forward 折条形 |
| `--soft` | `SoftReport` JSON（`backtest --soft` 产出） | 可选（有则绘净值曲线） | 忽略（会打印提示） | 累计期望收益曲线、expected_net 直方图、叶子平均概率条形、叶子概率堆叠面积 |
| `--sim` | `SimReport` JSON（`backtest --sim` 产出） | 可选（有则绘净值/仓位曲线） | 忽略（会打印提示） | 净值曲线、仓位轨迹、回合收益直方图、回合表（前 50） |
| `--portfolio` | `PortfolioReport` JSON（`portfolio` 产出） | 忽略（会打印提示） | 忽略（会打印提示） | 组合 vs 基准双线净值图、选中频率条形、持仓表（前 50） |

### 说明

**硬模式**：`--traces` 与 `--primary` 需**同时给出**才能绘时间序列曲线（前瞻收益累计曲线）；只给 `--report` 则仅渲染聚合图表。

**软模式（`--soft`）**：渲染 `SoftReport` JSON，`expected_net` 已内含于 traces 中，**不需要也不使用 `--primary`**。若指定了 `--primary`，会打印提示（`--primary ignored in --soft report`）但不报错。

**sim 模式（`--sim`）**：渲染 `SimReport` JSON（`backtest --sim` 的产出）。给出 `--traces`（`backtest --sim --traces steps.jsonl` 写出的 `SimStepRecord` JSONL）时绘净值曲线与仓位轨迹；否则仅显示汇总指标和回合表。`--primary` 在此模式下无意义，会打印忽略提示。

**组合模式（`--portfolio`）**：渲染 `PortfolioReport` JSON（`portfolio` 子命令的产出）。报告已自包含全部 holdings 序列，无需 `--traces` 或 `--primary`；两者均被静默忽略（打印提示）。

---

## `portfolio` 子命令

```
rquant portfolio [OPTIONS] --tree <TREE> --universe <UNIVERSE>
```

对 universe 内每只标的跑同一棵树，逐期取横截面 top-N 等权持仓，输出组合净值报告。

### 标志一览

| 标志 | 类型 | 默认值 | 说明 |
|---|---|---|---|
| `--tree <PATH>` | PathBuf | 必填 | 决策树 YAML 文件路径（与 `backtest` 同格式） |
| `--universe <PATH>` | PathBuf | 必填 | universe CSV 文件路径（见下方格式） |
| `--top <usize>` | usize | `5` | 每期最多持仓标的数（top-N 等权） |
| `--rebalance <usize>` | usize | `16` | 调仓间隔（timeline bar 数）；`warmup` 后每隔 `rebalance` 根 bar 调仓一次 |
| `--warmup <usize>` | usize | `100` | 跳过前 N 根 timeline bar，与 `backtest --warmup` 语义一致 |
| `--window <usize>` | usize | `100` | 传入 Context 的窗口大小（每时点最多取最近 N 根 bar） |
| `--cost-bps <f64>` | f64 | `10.0` | 单次调仓换手成本（基点）；`nav *= 1 − rate × turnover` |
| `--soft` | bool | `false` | 启用软遍历打分（`E = Σp·w·dir`）；否则用硬叶分数（`dir × weight`）|
| `--aux NAME=PATH（可重复）` | string | — | 挂载外部 aux 序列；DSL 经 `aux.<name>.<column>` 引用 |
| `--out <PATH>` | PathBuf | `portfolio.json` | 输出 `PortfolioReport` JSON 路径 |
| `--traces <PATH>` | PathBuf | 可选 | 若给出则写逐期 holdings JSONL（每行一个 `HoldingsRecord`） |
| `--llm-model <string>` | string | `""` | LLM 模型名（与 `backtest` 同；空则 Disabled） |
| `--llm-base-url <string>` | string | `""` | LLM API base URL |
| `--llm-cache-dir <PATH>` | PathBuf | `.rquant-cache/llm` | LLM 响应缓存目录 |

### universe CSV 格式

```
symbol,primary[,context]
sh600000,data/sh600000_60m.csv
sh600036,data/sh600036_60m.csv,data/sh600036_daily.csv
sz000001,data/sz000001_60m.csv
```

- **首行必须是表头**：至少两列 `symbol,primary`；可选第三列 `context`（大周期 bar，缺省回退为 primary）。
- `symbol` 非空且全局唯一（重复报错）。
- `primary` / `context` 为相对于当前工作目录的路径（与命令行其他 `--xxx PATH` 一致）。
- 读入后按 `symbol` 字典序排序（影响并列分数的确定性打破顺序）。

### 新鲜度与停牌语义

**新鲜（fresh）**：某标的在调仓时间点 `t` 恰有 bar（即 `time == t`）时视为可交易。

- **不新鲜（停牌）**：当期评分跳过，不进入候选集；若之前已持有，按最后已知收盘价（`last_close_at(t)`）计价，不产生收益也不计换手。
- **选股池**：每调仓点仅在新鲜标的中取分数 > 0 的 top-N；若新鲜标的不足 N 只，实际持仓数为新鲜且分数 > 0 的标的数（可 < N）。

### 基准口径

基准为 **universe 等权组合**（每调仓点对所有 `last_close_at(t)` 存在的标的等权重置，无成本）。与策略组合采用完全相同的 timeline 和调仓节奏，结果可直接相减得超额收益。

### 输出摘要示例

```
=== rquant PORTFOLIO: strength_demo ===
cost_bps=10  top_n=2  rebalance=8
总收益率    : -0.1650
基准收益率  : -0.2504
超额收益    : 0.0854
最大回撤    : 0.2979
换手率      : 106.0000
调仓次数    : 121
平均成员数  : 1.40
```

---

## `factor` 子命令

```
rquant factor [OPTIONS] --universe <UNIVERSE>
```

横截面单/多因子检验：对 universe 内所有标的按指定采样间隔取截面，计算 IC/RankIC 汇总、IC 衰减阶梯、Q 分层回测（含 Top−Bottom 价差）、因子相关性矩阵，输出 JSON + print + 可选 HTML。

### 标志一览

| 标志 | 类型 | 默认值 | 说明 |
|---|---|---|---|
| `--universe <PATH>` | PathBuf | 必填 | universe CSV 文件路径（与 `portfolio` 同格式：`symbol,primary[,context]`） |
| `--factor NAME=EXPR`（可重复） | string | 必填（至少一个） | 因子定义，格式 `名称=DSL表达式`；`name` 唯一非空，`expr` 加载期 DSL 解析校验 |
| `--sample <usize>` | usize | `16` | 采样间隔 K（每隔 K 根 timeline bar 取一个横截面） |
| `--horizon <usize>` | usize | `16` | 主前瞻期 H（forward_return gross 的主档距离；IC/分层使用此档） |
| `--layers <usize>` | usize | `5` | 分层数 Q（横截面按因子值升序分 Q 等分，前 n%Q 层 +1） |
| `--warmup <usize>` | usize | `100` | 跳过 timeline 前 N 根 bar（指标预热） |
| `--window <usize>` | usize | `100` | Context 历史窗口大小（每时点最多取最近 N 根 bar） |
| `--out <PATH>` | PathBuf | `factor_report.json` | 输出 `FactorReport` JSON 路径 |
| `--html <PATH>` | PathBuf | 可选 | 若给出则写自包含 HTML 报告（衰减折线/分层条形/spread 净值/相关矩阵） |

### 输出字段表（FactorReport JSON）

| 字段 | 类型 | 说明 |
|---|---|---|
| `n_symbols` | usize | universe 标的数 |
| `n_sample_points` | usize | 实际采样期数（warmup 后 step K） |
| `sample` | usize | 参数回显 |
| `horizon` | usize | 参数回显 |
| `layers_q` | usize | 参数回显 |
| `factors` | Vec\<FactorStats\> | 每因子统计（见下方 FactorStats 子表） |
| `corr` | Option\<CorrMatrix\> | ≥2 因子时的横截面相关性矩阵（`names` + `values`） |

**FactorStats 子表**

| 字段 | 说明 | None 语义 |
|---|---|---|
| `name` / `expr` | 因子名 / DSL 表达式 | — |
| `n_periods` | 进入 IC 统计的有效期数 | 全期跳过 → 0 |
| `n_skipped` | 有效对 < max(Q,5) 被跳过的期数 | — |
| `ic_mean` / `ic_std` / `icir` / `ic_t` / `ic_pos_share` | Pearson IC 汇总 | 无有效期 → 全 None |
| `rank_ic_mean` / `rank_ic_std` / `rank_icir` / `rank_ic_t` / `rank_ic_pos_share` | Spearman RankIC 汇总 | 同上 |
| `ic_decay` | `Vec<(horizon, Option<f64>)>` | 每阶梯均值 RankIC；无有效期 → None |
| `layers` | `Option<LayerStats>` | n_periods=0 时为 None |

**LayerStats 子表**

| 字段 | 说明 | None 语义 |
|---|---|---|
| `q` | 分层数 | — |
| `ann_returns` | 各层（低→高因子）年化收益 | 时间跨度 < 30 天 → None |
| `spread_total` | top−bottom 累计净值 −1（带符号） | — |
| `spread_ann` / `spread_sharpe` | spread 年化收益 / Sharpe | 时间跨度 < 30 天 → None |
| `monotonicity` | Spearman(层序号, 层期均收益)，[-1,1] | n < 2 时 None |
| `spread_nav` | spread 净值时间序列（用于 HTML 曲线） | — |

### 判读标准（spec §5）

**入树门槛**
- `|RankIC| > 0.03` 且 `|ICIR| > 0.3` → 因子统计显著，值得纳入决策树。
- 两条均未满足的因子通常为噪声，不建议入树。

**强因子**
- `|单调性（monotonicity）| > 0.8` 且 `|spread Sharpe| > 1` → 分层结构稳定，多头因子可直接作为 `when` 条件或 `strength` 权重。

**方向**
- `rank_ic_mean > 0` → 正向使用（高因子值 → 高收益）。
- `rank_ic_mean < 0` → 反向使用（低因子值 → 高收益）；此类因子同样有效，进树时 DSL 表达式取负或用反向比较即可。

**冗余剔除**
- 两因子横截面 Spearman 相关 `> 0.7` → 高度冗余；同时入树对信号贡献有限，建议仅保留 `|ICIR|` 更高者。

### Gross 口径提醒

`factor` 子命令收益口径为 **forward_return gross（无成本）**。Gross RankIC 测量的是因子信号强度，**不反映含成本的实际可交易收益**。**入树后必须经 `backtest` 或 `portfolio` 含成本复检**（`--cost-bps` 涵盖往返滑点与佣金），确认扣费后 edge 依然显著再投入策略生产。

---

## 风险指标（F-4 RiskMetrics）

`--sim` 和 `portfolio` 产出的 JSON 报告含 `risk` 字段（`Option<RiskMetrics>`，旧 JSON 兼容反序列化为 `null`）。

### 指标表

| 字段 | 说明 | None 语义 |
|---|---|---|
| `ann_return` | 年化复合收益（CAGR），首末 nav 比的年化幂次 | 时间跨度 < 30 天时为 None |
| `ann_vol` | 年化波动率（样本标准差 × √bpy），bpy = n\_rets / 首末跨度年数 | 时间跨度 < 30 天，或 n < 2 时为 None |
| `sharpe` | Sharpe = ann\_return / ann\_vol（无风险利率 = 0） | 同 ann\_vol；或 ann\_vol ≈ 0 时为 None（拒绝假 Sharpe）|
| `sortino` | Sortino = ann\_return / (下行波动 × √bpy)，下行仅含负收益 | 无负收益，或 ann\_return None 时为 None |
| `calmar` | Calmar = ann\_return / max\_drawdown（模拟器最大回撤） | ann\_return None，或 max\_drawdown ≈ 0 时为 None |
| `var95` | 历史 VaR（95%）= 升序排列第 ⌈5%·n⌉ 个分位点收益（通常为负值） | 永不为 None（仅需 ≥1 个收益点） |
| `cvar95` | CVaR95 = VaR95 及更差尾部的均值（期望损失） | 同 var95 |

`SignalStat.t_stat`（出现在 `metrics.active` / `metrics.engaged` 等字段）= mean / (sample\_std / √n)；n < 2 或 std ≈ 0 时为 None。

**公式权威**：详见 `docs/superpowers/specs/2026-06-11-rquant-f4-risk-metrics-design.md` §3。

---

## `optimize` 子命令

```
rquant optimize [OPTIONS]
```

锚定扩展 Walk-Forward Optimization（WFO）：对参数网格做 IS 寻优 → OS 验证，输出退化率、参数漂移、全样本对照、每折 IS top-5。

### 标志一览

| 标志 | 类型 | 默认值 | 说明 |
|---|---|---|---|
| `--tree <PATH>` | PathBuf | 必填 | 决策树 YAML 文件路径 |
| `--primary <PATH>` | PathBuf | 必填 | 主周期 CSV |
| `--context <PATH>` | PathBuf | 必填 | 大周期 CSV |
| `--news <PATH>` | PathBuf | 可选 | 新闻 CSV |
| `--grid NAME=VALUES`（可重复）| string | 必填（至少一个）| 参数轴：`name=start:stop:step`（闭区间，容差 1e-9）或 `name=v1,v2,...` |
| `--folds <usize>` | usize | `5` | Walk-forward 折数，必须 ≥ 2 |
| `--sim` | bool | `false` | 用 sim Sharpe/total_return 作为目标（与 `--soft` 互斥）|
| `--soft` | bool | `false` | 用 engaged expected_net 均值作为目标（打分软口径）|
| `--max-combos <usize>` | usize | `500` | 网格上限（超出则拒绝并提示缩格）|
| `--warmup <usize>` | usize | `100` | 跳过前 N 根 bar（指标预热）|
| `--window <usize>` | usize | `100` | Context 历史窗口大小 |
| `--cost-bps <f64>` | f64 | `10.0` | 往返成本（基点）|
| `--aux NAME=PATH`（可重复）| string | — | 挂载外部 aux 序列 |
| `--out <PATH>` | PathBuf | `optimize_report.json` | 输出 `OptimizeReport` JSON 路径 |
| `--llm-model <string>` | string | `""` | LLM 模型名（空则 Disabled）|
| `--llm-base-url <string>` | string | `""` | LLM API base URL |
| `--llm-cache-dir <PATH>` | PathBuf | `.rquant-cache/llm` | LLM 缓存目录 |

### 输出字段表（OptimizeReport JSON）

| 字段 | 类型 | 说明 |
|---|---|---|
| `mode` | string | `"score_hard"` / `"score_soft"` / `"sim"` |
| `objective_name` | string | 目标函数名称（对应 mode）|
| `folds` | usize | 折数参数 |
| `n_combos` | usize | 展开的参数组合数 |
| `fold_results` | Vec\<FoldResult\> | 每 OS 折结果（见下表）|
| `os_mean_objective` | Option\<f64\> | OS 折目标均值（WFO 主要评价指标）|
| `full_sample_best` | Option\<ComboScore\> | 全样本最优组合（事后偷看基准）|
| `drift` | Vec\<ParamDrift\> | 每参数最优值跨折漂移情况 |
| `is_top5` | Vec\<Vec\<ComboScore\>\> | 每 OS 折 IS 前 5 组合（IS 降序）|

**FoldResult 子表**

| 字段 | 说明 | None 语义 |
|---|---|---|
| `fold` | OS 折号（从 2 起，1-based）| — |
| `is_from` / `is_to` | IS 区间首末时间 | — |
| `os_from` / `os_to` | OS 区间首末时间 | — |
| `best_params` | IS 最优参数组合 | 全组合均无可评估点时为 None |
| `is_objective` | IS 最优目标值 | 同上 |
| `os_objective` | 最优参数在 OS 上的目标值 | IS best 为 None 则 None |
| `degradation` | `os/is`（退化率）| `is <= 1e-12` 或任一 None 时为 None |

**ParamDrift 子表**

| 字段 | 说明 |
|---|---|
| `name` | 参数名 |
| `values` | 每 OS 折最优值（None = 该折无 best）|
| `n_unique` | Some 值中 distinct 的 f64 个数（bit-pattern 去重）|

### 防过拟合判读指南

#### 退化率（degradation = OS/IS）
- **退化率 < 0.5（红旗）**：OS 收益不足 IS 的一半，强过拟合信号。参数对历史数据严重过优化，OS 期间几乎没有泛化能力。建议扩大网格步长、减少参数数量或增大 `--folds`。
- **退化率 0.5–0.8**：中等退化，常见于复杂树，可接受但需结合样本量判断。
- **退化率 > 0.8**：参数稳健，优化效果主要来自因子本身而非曲线拟合。

#### 参数漂移（drift n_unique）
- **n_unique 接近折数（红旗）**：每折选出不同的参数值，说明"最优参数"随时间剧烈跳动，无规律性。此时 IS 寻优基本等同于随机选参，OS 预测能力极弱。
- **n_unique = 1**：所有折均收敛到同一参数值，参数稳健。
- **n_unique 远小于折数（如折数=5 时 n_unique=2）**：参数在少数几个值之间切换，可接受；结合 OS 目标正负判断。

#### IS top-5 尖峰 vs plateau
- **top-5 尖峰（第一名远高于其余）**：IS 目标在某参数值处存在孤立高峰，典型过拟合形态。这类情况下 OS 降幅往往更大，因为曲线是"偶然"的。
- **plateau（top-5 目标相近）**：多个参数值效果相近，说明有真实且宽泛的 edge。此时即使选不到最优，OS 仍能保持较好表现。
- 实操：如果 top-5 中 #1 与 #5 的 IS 目标差距超过 #5 绝对值的 50%，视为尖峰，需谨慎。

#### WFO 拼接（OS mean）vs 全样本最优（full_sample_best）
- **差距 = full_sample_best.objective − os_mean_objective**，即**事后偷看代价**：全样本最优天然拥有对全部历史的知情权，而 WFO OS 均值仅反映"在不同时间窗口上实际可获得的表现"。
- 差距越大，说明真实可交易 edge 远低于全样本回测暗示的水平——这是最常见的回测虚高来源。
- **判读规则**：若 `os_mean > 0` 且 `os_mean` 接近 `full_sample_best.objective`（差距 < 30%），历史 edge 具有较好跨期稳定性；若 `os_mean <= 0` 而 `full_sample_best > 0`，则全样本"盈利"纯属事后偷看，该参数组合无实际价值。

### LLM 树成本提示

决策树包含 `type: llm` 节点时，`optimize` 会在每个参数组合 × 每个 IS 折的每个决策点调用 LLM，总调用次数约为：

```
n_combos × (folds − 1) × is_points + n_combos × full_sample_points
```

LLM 缓存按 `(model, base_url, system_prompt, node_id, rendered_inputs)` 键去重，同一问题仅调用一次，**相同 LLM 参数下不同参数网格组合不会复用缓存**（树参数影响前置 quant 节点路由，不同组合到达 LLM 节点的 bar 集合可能不同）。

建议：
1. 先用 `LlmEvaluator::Disabled`（不传 `--llm-model`）对参数 edge 做初步验证，确认纯量化部分有意义后再引入 LLM。
2. 若必须包含 LLM，缩小 `--grid` 步长以减少组合数，并确保 `--llm-cache-dir` 指向可持久化目录。

---

## 环境变量

| 变量 | 说明 |
|---|---|
| `RQUANT_LLM_API_KEY` | LLM API 密钥（bearer token），与 `--llm-model` + `--llm-base-url` 三者同时非空时 LLM 生效 |
