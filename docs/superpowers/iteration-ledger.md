# 选股树迭代账本 (iteration ledger)

Claude-in-the-loop 横截面日频选股树迭代的**单一事实源**。每轮一行（见底部运行表），
配套机读 `.iter/ledger.jsonl`（gitignored，供 `iterate.py` 防重复 + 对比 prior-best）。
设计见 [spec](specs/2026-06-18-iteration-harness-design.md) / [plan](plans/2026-06-18-iteration-harness.md)。

**诚实文化（§5.3）**：证伪是合法且有价值的产出。绝不调参凑过线、绝不重测已证伪族、绝不钓参数变体。

---

## /loop prompt（循环指令，自足；复制即用）

```
迭代选股树（全自治轮，§5.3 纪律）。本轮：
①读 docs/superpowers/iteration-ledger.md 尾部 + "已证伪/待试角度"；
②选一个【未证伪、未试】的横截面日频选股假设（优先待试队列；新数据集因子）；
③写/改 examples/screen/iter/<label>.yaml(+树 examples/trees/screen/<factor>.yaml)；
④python scripts/iterate.py examples/screen/iter/<label>.yaml --note "<假设>"；
⑤读轮卡：PASS→已自动 Tier-2，若稳健则记里程碑并上报；FALSIFIED→记原因；
⑥在账本追加本轮结论 + 更新待试/已证伪队列（绝不重复已证伪角度，不调参凑数）；
⑦里程碑(稳健过OOS赢家 / 连续~8轮跨多角度全证伪=空间穷尽)→PushNotification 上报并停；否则 ScheduleWakeup 续轮。
```

## 裁决严格闸（iterate.py `judge`）

`PASS` 需**全满足**：毛超额>0 且 净 OOS 超额>0 且 净 Sharpe>0 且 break-even≥40bps(2×成本) 且 (Tier-2)无符号翻转。
否则 `FALSIFIED` + 过拟合旗标：`gross-excess<=0` / `net-OOS<=0` / `net-sharpe<=0` / `in-sample-only`(train>0 但 OOS≤0) / `break-even<40bps` / `sign-flip`。

> 注：本闸比旧 `daily_eval.py` 更严（旧版仅查净 OOS>0，会把 biased 伪 alpha 判 PASS——见[日内 findings](2026-06-18-intraday-daily-selection-findings.md) §诚实局限3）。
> 回测口径：universe=`baostock_day`(~1073，survivorship-free 含退市)，**train 2018-01..2023-12 / OOS 2024-01..2026-06**，net 20bps，基准=universe 等权无成本（强基准，等权偏小盘，2018-26 累计 >+100%）。

---

## 已证伪角度（勿重试）

A股 top 流动股、2018-2026、横截面日频选股上，以下角度**均已证伪**。源头多为：① 极端因子选中者续走极端（接落刀/追高），② 中换手×日成本墙吞掉微弱毛信号，③ 单 regime 特异（OOS 归零）。详见
[日频框架 §6](2026-06-17-daily-selection-framework.md) / [基本面-技术 findings](2026-06-16-fundamental-technical-selection-findings.md) / [日内 findings](2026-06-18-intraday-daily-selection-findings.md)。

### 价格/技术类（2018 起全有数据）
- **短期反转**(5日跌幅最大)：毛−1.99/净−2.04，灾难证伪——极端跌者续跌。
- **纯动量**(N日涨幅最大)：毛−2.01/净−2.03，灾难证伪——极端涨者次日续跌。
- **纯低波**(20日波动最低)：毛+0.31 但净−1.35，**成本墙样板**——中换手×日成本吞掉 −60% 超额。
- **中度反转**(剔暴跌尾)：毛−1.02/净−2.02，2024 单年诱人=regime 特异；换手 85%/日。
- **MACD / 道氏趋势 / RSI 超卖 / 布林 %b / Brooks 价格行为 / 规模代理(小盘)**：27 轮循环全证伪（毛≤0 或净 OOS≤0）。对应树 `macd_xs/dow_trend_xs/rsi_os_xs/boll_pctb_xs/price_action_xs/small_xs`。
- **量价相关性 corr_pv（高确认向，round 2）**：选 corr(close,volume,20) 最高 top-50，**毛超额 −4.52 灾难证伪**——高量价确认股=高关注/接盘股，次日大幅跑输 EW（基准 +442%、组合 −10%）；换手 21%/日。背离向（低 corr_pv）同族低 EV（只会贴另一批股 EW，难过严格闸）。树 `corr_pv_xs`。

### 价值/基本面类（有效窗 2019-05 起；基本面 CSV 最早 2019-04-30）
- **纯价值**(PB 最低)：毛−0.62/净−0.78，**净 Sharpe +0.15**（唯一正）——仅防御（换手 0.062 极低、回撤 0.28 小），但 lag 强基准、无超额。
- **value×低波**(双防御)：毛−0.62/净−1.14——更防御但无超额；低波 tilt 反推高换手。
- **value 池内反转**：毛−1.56/净−1.95——便宜跌者=续跌价值陷阱，价值地板救不了反转。
- **价值+长趋势 / 价值×动量 / 质量(ROE) / 成长 / PE / PE+PB**：27 轮全证伪（OOS 超额≤0 或成本墙）。对应树 `value_longtrend/value_pb_trend/quality_v1/value_pe/value_pepb/value_roe`。
- **value ∩ quality（Greenblatt，round 3）**：便宜 30% PB 池内按 ROE 取 top-50，**净超额 −2.39 / 净 OOS −1.13 证伪**。最干净防御画像（净 Sharpe 0.72、换手 3.6%、回撤 0.28、train 近平 −0.15）但仍 lag 强基准；**质量倾斜浓缩大盘价值 → OOS(2024-26 小盘暴动)比纯价值 −0.66 更落后**。质量在价值之上不添 OOS alpha。树 `value_pb`+`roe_xs`。

### 日内微结构类（日内轴，6mo/sina/幸存者，**无 OOS**）
- **6 因子 × 正反两向 = 12 全证伪**：last_leg 尾盘动量 / intraday_rev 日内反转 / close_vs_vwap 收盘强度 / intraday_range 日内波幅 / vol_tilt 量能后移 / overnight 隔夜跳空。唯二净微正者前后半翻号(非稳健) + break-even<2×成本。
- ⚠️ 本轮最大价值=**抓住一个 survivorship 陷阱**（按窗口末成交额选 universe → 前视选中赢家 → 假 Sharpe 18）。任何"惊艳"结果先做合理性检查。

## 稳健边（里程碑：换框架后确认）

**最便宜 PB 价值 = 对可交易基准的稳健跑赢者**（round 4，[完整发现](2026-06-18-value-vs-tradeable-benchmark-finding.md)）。
此前 ~30 轮"价值证伪"是**基准选错**：vs 不可投资的 EW 全集(+442%)价值落后；但 vs **可交易宽基指数**
(CSI300/500/1000，2018-26 仅 +21~36%)，`value_pb`(top-50,net20bps) **净总 +324%、绝对 Sharpe 1.13、回撤 0.19、换手 2.4%/日**：

- 超额 vs CSI300/500/1000 = **+2.96/+2.85/+2.97**，**三规模桶全胜（含小盘 → 非规模 beta）**；
- **洁净窗**(去 2018 无基本面空仓)：clean-train(2019-05..2023-12) 超额 **+1.53**（CSI300 当期 −7%）、**OOS(2024-26) +0.64**——train+OOS 皆正且数据洁净；
- Tier-2 敏感性 `[2.1..4.01]` 全正无符号翻转；break-even 164bps。
- **PASS**（毛>0 且 净OOS>0 且 净Sharpe>0 且 be≥40bps 且无翻转，全满足）。
- **部署加固**（round 5，月频+质量/流动性地板）：回撤腰斩 0.10、break-even 514bps、换手 1/3.5；T+1 执行拖累可忽略；容量 ~2.5亿(10%ADV)。逐年：2021-26 连赢 6 年(含全 OOS)、成长年 2019-20 输 → 真价值因子。
- **行业中性**（round 6，引擎 group-select 已建）：每行业 top-3，**OOS 超额 +0.81 > 全局 +0.51、train/OOS 趋平衡 → 更少 regime 依赖**（坐实"半 sector 押注半选股"中选股是稳健半）；代价 回撤 0.10→0.21、Sharpe 1.13→0.89。

**诚实边界**：①结论取决于基准=可交易指数（合法，EW 不可投资高估机会成本）；②2019-26 是 A股价值/红利友好周期，OOS 为单一宏观期（行业中性版缓解但未消除）；③long-only 价值/beta 溢价非对冲 alpha（回撤真实）；④最便宜 PB 含价值陷阱→部署需质量+流动性地板（`value∩quality` 也过基准但更薄：超额 +1.71/OOS +0.16/Sharpe 0.72）。详见[发现文档](2026-06-18-value-vs-tradeable-benchmark-finding.md)。

## 待试角度（候选队列，新 baostock 数据集解锁；Claude 维护）

> 优先未试、机制上有别于已证伪族者。新 `features_day` 指标：kdj/cci/wr/obv/vwap20/roc/rvol20/corr_pv20（多数未作选股器测过）。

- [ ] **多因子 AND 共识**：同时满足"便宜(低PB) AND 高质(高ROE)"双优股池（≠ value×momentum 的连乘倾斜；用 value_frac 两段 + 质量闸交集）。
- [x] ~~**量价背离**：`corr_pv20`~~ → round 2 测高确认向=灾难证伪（毛 −4.52）；背离向低 EV，暂搁。
- [ ] **资金流向代理**：`obv` 斜率 / `rvol20` 放量 × 价值闸——是否过滤价值陷阱。
- [ ] **regime 条件选股**：按市场波动/趋势 regime 切换因子（防御 regime 用价值、趋势 regime 用动量）——单棵树内 gate。
- [ ] **板块相对强弱**：需引擎 sector-neutral / 分组 select（当前 select_top 全局）——[引擎缺口](specs/2026-06-18-iteration-harness-design.md#9-已知引擎缺口-v1-不阻塞标注)，v1 不阻塞，留后续 spec。
- [ ] **日内微结构（正经 OOS）**：仅当有多年 survivorship-free 日内数据时（当前 EV 低）。

---

## 运行表

| round | label | 假设 | net超额 | net-OOS超额 | netSharpe | axis | flags | 裁决 |
|---|---|---|---|---|---|---|---|---|
| 1 | value_pb_base | baseline: pure-PB value defensive (smoke) | -1.184 | -0.659 | 1.13 | daily | gross-excess<=0,net-OOS<=0,in-sample-only,break-even<40bps | FALSIFIED |
| 2 | corr_pv_hi | corr_pv hi: select top-50 by price-volume correlation (volume confirms price) | -5.035 | -1.599 | -0.36 | daily | gross-excess<=0,net-OOS<=0,net-sharpe<=0,break-even<40bps | FALSIFIED |
| 3 | value_quality_and | value AND quality (Greenblatt): cheapest 30% PB, then top-50 by ROE | -2.394 | -1.134 | 0.72 | daily | gross-excess<=0,net-OOS<=0,break-even<40bps | FALSIFIED |
| 4 | value_pb_csi300 | pure-PB value vs tradeable index (reframe) [bench:csi300] | +2.957 | 0.636 | 1.13 | daily | — | PASS |
| 5 | value_pb_deploy_m | deploy-hardened value (roe>0 + liq>=50M floor) monthly vs index [bench:csi300] [reb20] | +2.795 | 0.512 | 1.13 | daily | — | PASS |
| 6 | value_pb_sn_m | sector-neutral value (top-3 per industry) vs index [bench:csi300] [reb20] [sector-neutral] | +2.585 | 0.806 | 0.89 | daily | — | PASS |
| 7 | quality_roe_m | quality (high ROE) standalone vs tradeable index — does quality beat index like value? [bench:csi300] [reb20] | +2.883 | 0.832 | 0.72 | daily | — | PASS |
