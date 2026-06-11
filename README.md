# rquant — 模糊决策树 A股回测引擎

基于声明式 YAML 决策树的 A股离线回测工具：量化指标 + 可选 LLM 节点取代人工逐节点判断，前瞻收益评分验证策略 edge，支持硬遍历（单路径）与软/概率遍历（叶子分布）。

---

## Quick Start

```bash
# 1. 拉取 K 线（--adjust qfq 前复权，回测推荐）
cargo run --release -- fetch --symbol sh600000 --scale 15 --adjust qfq --out 15m.csv
cargo run --release -- fetch --symbol sh600000 --scale 60  --adjust qfq --out 1h.csv

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

## 参数寻优（`optimize`）

`optimize` 子命令对参数网格做**锚定扩展 Walk-Forward Optimization（WFO）**：样本内（IS）寻优 → 样本外（OS）验证，输出退化率、参数漂移、全样本对照基准，系统评估过拟合风险。

```bash
# 对 n_trend 和 k_trend 做 3×3 网格，4 折 WFO，预热 80 根 bar
cargo run --release -- optimize \
  --tree examples/regime_adaptive_1.yaml \
  --primary 60m.csv --context 60m.csv \
  --grid "n_trend=10,20,30" \
  --grid "k_trend=0.05,0.10,0.15" \
  --folds 4 --warmup 80 \
  --out opt_report.json
```

**与 `backtest --folds` 的区别**：`backtest --folds K` 对**固定树**（不搜索参数）做时间稳定性分折，回答"这棵树的 edge 是全程稳定还是一段行情撞的"。`optimize` 则在每 OS 折的 IS 窗口上**寻找最优参数**，再验证该参数在 OS 上能否泛化，回答"参数选择本身有没有过拟合"。

**研究循环**

```
factor 检验  →  入树  →  optimize 校准  →  backtest/sim 含成本复检
    ↑                                               |
    └─────────────── 因子/参数迭代调整 ──────────────┘
```

1. **factor 检验**：横截面 RankIC/ICIR 快速评估因子信号质量（gross 口径）。
2. **入树**：满足门槛的因子写入决策树 `when`/`strength` 条件与 `params` 块。
3. **optimize 校准**：对树中 `params` 做 WFO 网格寻优；判读退化率与漂移，确认参数稳健性。
4. **backtest/sim 含成本复检**：用 optimize 确定的参数运行完整回测，验证扣费后 edge 仍然显著再入生产。

**防过拟合快速判读**（详见 `docs/cli-reference.md` § optimize）

| 指标 | 红旗 | 说明 |
|---|---|---|
| `degradation`（退化率=OS/IS）| < 0.5 | OS 收益不足 IS 一半，强过拟合信号 |
| `drift.n_unique`（参数漂移）| 接近折数 | 参数乱跳，每折选不同值，无泛化规律 |
| IS top-5 形态 | 尖峰（#1远高于#5）| 孤立高峰，过拟合概率高；plateau 更可靠 |
| `full_sample_best` vs `os_mean` | 差距>30% | 事后偷看代价大，全样本"盈利"注水 |

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
年化收益  :     0.24
年化波动  :     0.18
Sharpe    :     1.31
Sortino   :     2.07
Calmar    :     0.80
VaR95     : -0.0182
CVaR95    : -0.0182
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

## 横截面组合（`portfolio`）

对 universe 内每只标的运行同一棵树，横截面取分数最高的 top-N 等权持仓，按调仓间隔换仓，输出组合净值与等权基准对比。

```bash
# 1. 准备 universe CSV（两列最少，第三列 context 可选）
cat > universe.csv <<'EOF'
symbol,primary
sh600000,data/sh600000_60m.csv
sh600036,data/sh600036_60m.csv
sz000001,data/sz000001_60m.csv
sz000002,data/sz000002_60m.csv
EOF

# 2. 运行组合回测（软打分，top-2，每 8 根 timeline bar 调仓）
cargo run --release -- portfolio \
  --tree examples/strength_tree.yaml \
  --universe universe.csv \
  --top 2 --rebalance 8 --warmup 60 \
  --soft --out port.json
```

universe CSV 格式：首行 `symbol,primary[,context]`；`symbol` 非空且唯一；`primary` 为主周期 bar CSV 路径，`context` 缺省回退为 `primary`。

**诚实边界**

- 分数（硬：`dir×weight`；软：`E=Σp·w·dir`）是伪概率，仅用于**排序**，不是期望收益。
- 持仓**等权**：无分数加权、无波动率中性化。
- **纯多头**：仅持有分数 > 0 的标的；分数 ≤ 0 或停牌标的当期出局。
- **T+1 不强制**：组合层按 timeline bar 节奏调仓，无同日禁减仓约束（与 `--sim` 不同）。如相邻调仓点为同一自然日，会在 stderr 打印一次提示。
- **停牌出局**：不新鲜标的当期不纳入候选；若已持有则按最后已知价计价，贡献零收益。换仓时停牌持仓按模型假设可执行离场——实盘中停牌/跌停可能无法成交。
- **基准晚入**：数据起点较晚的标的在首个有价格的调仓点起纳入等权基准（不回溯）。

| 标志 | 默认值 | 说明 |
|---|---|---|
| `--tree` | 必填 | 决策树 YAML |
| `--universe` | 必填 | universe CSV |
| `--top` | `5` | 每期最多持仓数 |
| `--rebalance` | `16` | 调仓间隔（timeline bar 数） |
| `--warmup` | `100` | 跳过前 N 根 timeline bar |
| `--window` | `100` | Context 历史窗口大小 |
| `--cost-bps` | `10.0` | 换手成本（基点） |
| `--soft` | false | 软遍历打分 |
| `--out` | `portfolio.json` | 报告 JSON |
| `--traces` | — | 逐期 holdings JSONL（可选） |

---

## 因子检验（`factor`）

对 universe 中所有标的做横截面因子检验：IC/RankIC 汇总、IC 衰减阶梯、Q 分层回测（含 Top−Bottom 价差）、多因子相关性矩阵，输出 JSON + print + 可选 HTML。

```bash
# 准备 universe（与 portfolio 同格式）
cat > universe.csv <<'EOF'
symbol,primary
sh600000,data/sh600000_60m.csv
sh600036,data/sh600036_60m.csv
sz000001,data/sz000001_60m.csv
sz000002,data/sz000002_60m.csv
EOF

# 检验动量因子与 RSI
cargo run --release -- factor \
  --universe universe.csv \
  --factor "mom20=close/ref(close,20)-1" \
  --factor "rsi14=rsi(close,14)" \
  --sample 8 --horizon 8 --warmup 60 \
  --out factor_report.json --html factor_report.html
```

**研究循环定位**

```
factor 检验  →  入树  →  optimize 校准  →  backtest/portfolio 含成本复检
    ↑                                               |
    └─────────────── 因子/参数迭代调整 ──────────────┘
```

1. **factor 检验**：快速横截面评估因子的 RankIC/ICIR/单调性/冗余度（gross 口径，无成本）。
2. **入树**：满足门槛（`|RankIC| > 0.03` 且 `|ICIR| > 0.3`）的因子写入决策树 DSL（`when`/`strength` 条件）。
3. **optimize 校准**：对树中 `params` 做 WFO 参数网格寻优，验证参数稳健性（退化率/漂移/事后偷看代价）。
4. **backtest/portfolio 含成本复检**：加入 `--cost-bps` 后验证 edge 依然显著，再进入策略生产。

**判读要点**

| 指标 | 门槛 | 说明 |
|---|---|---|
| `rank_ic_mean` | `|.| > 0.03` | 横截面预测能力 |
| `rank_icir` | `|.| > 0.3` | 信号稳定性（剔除运气） |
| `monotonicity` | `|.| > 0.8` | 分层结构是否单调 |
| `spread_sharpe` | `|.| > 1` | 多空价差质量（强因子） |
| 两因子相关 | `> 0.7` | 冗余，留 `|ICIR|` 高者 |

负 `rank_ic_mean` = 反向因子，同样有效，进树时取负即可。

---

## fetch

```bash
cargo run --release -- fetch --symbol sh600000 --scale 15 --out 15m.csv
cargo run --release -- fetch --symbol sh600000 --scale 240 --out daily.csv  # scale=240 为日线别名

# 前复权（推荐回测用）：消除除息假跳空
cargo run --release -- fetch --symbol sh600000 --scale 240 --adjust qfq --out daily_qfq.csv
cargo run --release -- fetch --symbol sh600000 --scale 60  --adjust qfq --out 60m_qfq.csv
```

`--datalen` 默认 1023（新浪上限）。默认端点 `https://quotes.sina.cn/cn/api/json_v2.php`（2026-06 可用）；旧端点 `money.finance.sina.com.cn` 已不可用，可用 `--base-url` 覆盖。

`--adjust qfq`：日线（scale=240）直取腾讯前复权日线；分钟线为三源合成——新浪分钟 raw × 腾讯 qfq/raw 日线因子，消除除息日假跳空。

**诚实边界（前复权 qfq）**

- **历史价随新除权整体重标**：前复权锚定最新交易日（因子≈1），每次重新拉取后历史价格会随新增除权事件整体下移；旧 CSV 与新 CSV 不可混用，需同批次拉取。
- **volume 单位不统一**：腾讯日线 volume 单位为手（100 股），新浪分钟 volume 单位为股；引擎内 volume 仅作相对量使用，跨数据源横向对比绝对成交量无意义。
- **建议**：回测一律使用 `--adjust qfq`，raw（不复权）仅用于与实时行情盘口对照。

---

## report

```bash
# 硬模式（--traces 和 --primary 同时给出才画时间曲线）
cargo run --release -- report --report report.json --out report.html \
  --traces traces.jsonl --primary 15m.csv

# 软模式（不需要 --primary）
cargo run --release -- report --soft --report soft_report.json \
  --traces soft_traces.jsonl --out soft.html

# sim 模式（用 --traces 提供净值/仓位曲线；无 traces 仅显示汇总与回合表）
cargo run --release -- report --sim --report sim_report.json \
  --traces sim_steps.jsonl --out sim.html

# 组合模式（PortfolioReport 自包含，无需 traces/primary）
cargo run --release -- report --portfolio --report portfolio.json --out portfolio.html
```

`--soft` / `--sim` / `--portfolio` 三个模式标志互斥；不指定则默认 hard 模式。
软/sim 模式下 `--primary` 被忽略（附提示）；组合模式下 `--traces` 与 `--primary` 均被忽略（附提示）。

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

## 纸面盘（`signal`）与每日一条命令

`signal` 子命令是研究闭环的最后一环：用当日最新 K 线增量重放历史、输出今日信号，结合人工下单形成完整的纸面交易流水。

**研究闭环**

```
factor 检验  →  入树  →  optimize 校准  →  backtest/sim 含成本复检  →  signal 纸面盘  →  （人工下单）
    ↑                                                                          |
    └──────────────────────────────── 因子/参数迭代调整 ──────────────────────┘
```

### 每日一条命令（PowerShell）

```powershell
cargo run --release -- signal `
  --tree examples/regime_adaptive_1.yaml `
  --fetch sh600519 --scale 60 --adjust qfq `
  --primary data\p.csv `
  --state paper.json --commit `
  --out signal.json
```

首次运行时 `paper.json` 不存在，自动以 fresh 状态初始化。`--commit` 将信号后的账户快照写回 `paper.json`；省略 `--commit` 则为 dry-run，只打印不落盘。

### Windows 任务计划程序（收盘后自动执行）

```
schtasks /create /sc daily /st 15:30 /tn rquant-signal ^
  /tr "cargo run --manifest-path E:\rust-app\rquant\Cargo.toml --release -- signal --tree examples/regime_adaptive_1.yaml --fetch sh600519 --scale 60 --adjust qfq --primary data\p.csv --state paper.json --commit --out signal.json"
```

A 股收盘时间 15:00，建议定时任务设为 15:30 确保数据已更新。可改用编译好的 `rquant.exe` 替换 `cargo run`，减少启动时间。

---

## 构建与测试

```bash
cargo build --release
cargo test
```
