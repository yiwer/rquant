# value_pb_deploy 纸面盘跟踪（第4账本：价值选股）

迭代 harness round 4/5 验证通过的**唯一可部署正策略**。完整验证 = [finding](../docs/superpowers/2026-06-18-value-vs-tradeable-benchmark-finding.md)。
本目录是**冻结部署副本**（examples/ 同名树继续迭代不影响此处）。**纸面盘，非投资建议。**

## 策略一句话
A股最便宜 PB（+盈利地板 roe>0 +流动性地板 20日均成交额≥5000万）top-50 等权、**月频(reb≈20)**调仓，
对标可交易宽基指数（CSI300/500）。它是 **long-only 价值/beta 溢价**——成长大年会跑输（2019-20 那样），价值/红利年大胜（2021-25）。

## 部署件
- `value_pb_deploy_frozen.yaml` — 配置（top-50, λ=0 纯价值）
- `value_pb_deploy_tree_frozen.yaml` — 价值树（便宜PB + 盈利/流动性地板）
- `momentum_xs_frozen.yaml` — 惰性 setup（λ=0，仅满足校验）
- `value_pb_deploy_picks_<date>.md` — 当期推荐持仓快照（首版 2026-06-12）

## 月度跟踪流程
每月最后交易日**收盘后**（数据稳定后）：

1. **拉当月数据**（universe 日线 + 基本面到最新）——`scripts/fetch_baostock.py`（或增量），财务 `scripts/fetch_fundamentals.py`。
2. **出当期 top-50**（as-of 选股）：
   ```
   target/release/rquant.exe screen \
     --universe data/baostock/universe_baostock_day.csv \
     --config deploy/value_pb_deploy_frozen.yaml \
     --as-of <YYYY-MM-DD> --top 50 --warmup 60 --out deploy/value_pb_deploy_picks_<date>.json
   ```
3. **对比上月持仓 → 调仓清单**（买入新进、卖出移出；等权 1/50）。可用 `rquant signal --universe ... --top 50`（组合清单引擎，输出 Buy/Sell/Hold）+ `--commit` 落持仓状态（仿三账本纸面盘）。
4. **记账**：当月组合 nav vs CSI300/CSI500 同期收益，累计跟踪超额（与回测口径一致：对指数算超额，非等权全集）。

## 激活为自动账本（用户 go-live 时）
仿现有纸面盘三账本（`deploy/paper_run.cmd` + Windows 排程 `rquant-paper`）：新增月频任务调用上述 as-of+signal --commit，
状态落 `paper/`（gitignored）。**注意**：①只在收盘后跑（盘中新浪/baostock 返成形中 bar）；②月频=每月一次即可；
③首次建仓用当期快照。**本文档不自动建排程——由用户明确启动**（涉及真实跟踪承诺 + 需先恢复数据抓取）。

## 诚实纪律（必读）
- **价值押注，非对冲 alpha**：有股票 beta，回撤约 10%（部署加固版），成长主导期会跑输指数——这是价值因子的本性，不是 bug。
- **超额口径=可交易指数**（CSI300/500），**不是**等权全集（那个 +442% 不可投资、是 harness 内建基准）。
- **regime 依赖**：边集中在价值/红利友好周期（2021-25 中特估）；约一半超额来自"持有便宜板块"（sector 配置），一半来自板块内选股（见 finding §5.6）。
- **容量**：当前 5000 万流动性地板 → 约 2.5 亿（10%ADV/1日建仓），提高地板换更大容量。
- **幸存者**：universe survivorship-free（含退市）；基本面 point-in-time（≤t 公告）。
- **执行**：回测 close[T] 决策+成交略乐观；实盘 T+1 开盘成交，实测拖累可忽略（月频低换手）。
