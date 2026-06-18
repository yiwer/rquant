# Claude-in-the-loop 选股树快速迭代 harness — 设计 spec

> 用户指令（2026-06-18）："设计完整的 回测→监控→快速迭代 的回测方案，方便 claude 用 /loop 多轮快速迭代；claude 参与观察结果、思考、策略决策树演进。" 决策：**全自治轮 / 横截面日频选股树（新数据集因子）/ 分层严格度**。沿用证伪文化与 §5.3。

## 1. 目标与角色

把此前 ad-hoc 跑了 27 轮的"回测→观察→改树→再回测"循环，**固化成可复用、LLM 友好、防重复、带 OOS 严格度**的 harness。

- **Claude 角色**：假设生成器 + 判读者 + 决策树作者。读账本与轮卡 → 提一个未试的新选股角度 → 写/改树 → 判读结果 → 记账 → 续轮。
- **脚本角色**：执行器 + 记录器 + §5.3 护栏执行器。跑回测、算诊断、自动旗标过拟合、追加账本、打印轮卡。
- **分工原则**：创造性（假设/判读/改树）归 Claude；机械+纪律（执行/记录/护栏）归脚本。脚本绝不"调参凑数"。

## 2. 架构

现有 `screen` 引擎 + baostock 数据集之上加一层 harness：`scripts/iterate.py`（轮驱动）+ 迭代账本 + `/loop` prompt。零引擎改动（v1）。

```
/loop 触发 → Claude 读账本尾+上轮卡 → 形成假设 → 写/改 树+配置
   → python scripts/iterate.py <config> --note "假设"
       → Tier-1 回测(gross/net, train/OOS) → [过门则 Tier-2 敏感性+时间二分]
       → 过拟合自动旗标 → 追加账本(md+jsonl) → 打印轮卡
   → Claude 读轮卡 → 判读 + 在账本记决策/下一假设 → ScheduleWakeup(续轮)
```

## 3. `scripts/iterate.py`（轮驱动）

`python scripts/iterate.py <config.yaml> --note "<假设>" [--axis daily|intraday] [--label NAME]`

- **Tier-1（每轮）**：`screen --backtest --rebalance 1` 跑 gross(0) + net(20bps)，universe + train/OOS regime 切分（见 §6）。解析 JSON → 轮卡诊断。
- **Tier-2（仅当 Tier-1 过 OOS 门）**：参数敏感性——top∈{30,50,100} × reb∈{1,5} 网格各跑 net；时间二分（OOS 前/后半）。检测**符号翻转**（任一参数下净超额变号 = 非稳健）。
- **过拟合自动旗标**（任一触发即记）：① 净 OOS 超额 ≤0；② 跨 top/reb 净超额符号翻转；③ 单 regime 集中（净超额仅来自一个 regime 窗口）；④ break-even < 2×真实成本(40bps)；⑤ 仅样本内（train>0 但 OOS≤0）。
- **裁决**：`PASS` 需全满足——毛超额>0 且 净 OOS 超额>0 且 净 Sharpe>0 且 (Tier-2)无符号翻转 且 break-even≥40bps 且 非单 regime。否则 `FALSIFIED(原因)`。
- **副作用**：追加账本（§4）；打印轮卡（§5）。**不**修改树/配置（那是 Claude 的活）。
- 复用 `daily_eval.py` 的 gross/net+break-even+regime 解析逻辑（抽取共享）。位置无关路径、UTF-8 输出（沿用 daily_eval 修法）。

## 4. 迭代账本

- **`docs/superpowers/iteration-ledger.md`**（人读，入库=研究记录）：
  - 运行表，每轮一行：`round | label | 假设 | net总 | net超额 | net-OOS超额 | netSharpe | 换手/d | tier | flags | 裁决`
  - **"已证伪角度（勿重试）"** 区：从 27 轮历史种入（反转/动量/低波/价值/中度反转/value×低波/value池内反转/MACD/道氏/RSI/布林/Brooks/规模代理…全证伪；唯一稳健边=价值-防御慢调仓）。
  - **"待试角度"** 区：Claude 维护的候选队列（新数据集解锁：日内微结构、扩展TA组合、板块相对、多因子AND、价值×质量…）。
- **`.iter/ledger.jsonl`**（机读，gitignored）：每轮全字段 JSON，供脚本"对比 prior-best"与防重复。

## 5. 轮卡格式（Claude 读的紧凑诊断）

```
=== ITER round N · <label> (axis=daily) ===
hypothesis : <note>
universe   : baostock_day (1073 stocks)   train 2018-01..2023-12 / OOS 2024-01..2026-06   cost 20bps
              gross     net
total        +x.xxxx   +x.xxxx
excess       +x.xxxx   +x.xxxx
sharpe        x.xx      x.xx
maxDD         x.xx      x.xx
turnover/d    xx.x%     break-even  xx.xbps
regime net excess : train +x.xxxx | OOS +x.xxxx
quality layers Q1→Q5 (mean fwd-ret) : [.. .. .. .. ..]  mono=±x.xx
tag attribution : <tag> picks=.. hit=.. fwd=±..
-- tier2 (only if OOS gate passed) --
sensitivity net-excess : top{30/50/100}=[.. .. ..]  reb{1/5}=[.. ..]  sign-flip=Y/N
time-split net-excess  : OOS-1st=+x.xx  OOS-2nd=+x.xx
flags   : [none | net-OOS≤0 | sharpe-flip | single-regime | be<2x | in-sample-only]
VERDICT : PASS / FALSIFIED(<reason>)
vs prior-best (net-OOS-excess) : <+/− Δ>
```

## 6. 回测口径（默认，可调）

- **universe**：`data/baostock/universe_baostock_day.csv`（核心~1073，渐扩），primary=日线，fund→真财务 `data/fundamentals/`。
- **日线轴（主）**：因子 = DSL 在日线 bar 上算 TA/价格（sma/ema/rsi/macd/boll/atr/kdj/cci/wr/roc/std…全内置）+ `fund.*` 价值/基本面。train 2018-01..2023-12 / **OOS 2024-01..2026-06**。
- **日内轴（次）**：因子 = 预存"日频化日内微结构"表（尾盘动量/日内反转/收盘vsVWAP…，date-only 戳）经 `fund.*` 喂入一个独立 universe（fund→日内因子表）。primary=日线、rebalance 1。train 2021..2023 / OOS 2024..2026。
- 基准 = universe 等权、无成本（screen 引擎既有口径）。

## 7. `/loop` prompt 模板（循环指令，自足）

```
迭代选股树（全自治轮，§5.3 纪律）。本轮：
①读 docs/superpowers/iteration-ledger.md 尾部 + "已证伪/待试角度"；
②选一个【未证伪、未试】的横截面日频选股假设（优先待试队列；新数据集因子）；
③写/改 examples/screen/iter/<label>.yaml(+树/DSL)；
④python scripts/iterate.py examples/screen/iter/<label>.yaml --note "<假设>"；
⑤读轮卡：PASS→已自动 Tier-2，若稳健则记里程碑并上报；FALSIFIED→记原因；
⑥在账本追加本轮结论 + 更新待试/已证伪队列（绝不重复已证伪角度，不调参凑数）；
⑦里程碑(稳健过OOS赢家 / 连续~8轮跨多角度全证伪=空间穷尽)→PushNotification 上报并停；否则 ScheduleWakeup 续轮。
```

## 8. 候选配置布局

`examples/screen/iter/<label>.yaml`（迭代配置，与生产 examples/screen/ 分开）；树 `examples/trees/screen/<factor>.yaml`。每轮 Claude 新建/改这些数据文件，无引擎改码。

## 9. 已知引擎缺口（v1 不阻塞，标注）

**行业中性 / 板块相对选股**需引擎"按行业分组 select"能力（当前 select_top 是全局）。v1 用 DSL 可算因子 + fund.* 因子表覆盖；sector-neutral 留作后续引擎特性（届时单独 spec）。

## 10. 测试

- `iterate.py`：裁决 + 过拟合旗标 + break-even + prior-best 比较 = 纯函数，pytest 钉死（含符号翻转检测、单 regime 检测的合成用例）。
- 账本追加：写一条 → 读回字段一致。
- e2e smoke：对 `value_pb`（已知防御基线）跑一轮 → 轮卡含 train>0/OOS lag、裁决合理、账本+1。

## 11. 成功标准 & 诚实预期

- **harness 成功** = 每轮 <数分钟出可信轮卡 + 账本防重复 + 过拟合自动旗标拦截 + Claude 能据卡演进。
- **研究预期（诚实）**：前 27 轮几乎全证伪；多数轮将继续证伪。harness 的价值=**快速、不重复、带 OOS 严格度的证伪**，以及偶有真 edge 被金标准抓住而非被单年/样本内假象骗过。证伪是合法且有价值的产出。

---

**自审**：占位符无；裁决门槛/旗标阈值具体（OOS>0、be≥40bps、符号翻转、单regime）；轮卡/账本 schema 明确；factor 接线（DSL+fund.*）与引擎缺口(sector-neutral)标注；范围聚焦单一 harness。待用户审阅。
