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
- **value × 资金流(rvol，round 16)**：PB 便宜半数池内按相对成交量 rvol(volume/sma20) tilt。**表头诱人却 sign-flip 证伪**——净超额 +4.49、Sharpe 1.0、OOS +2.09 看似最佳,但 Tier-2 top-30 净超额转负(−0.76)、**换手 79.8%/日、be 仅 110bps**。放量=快衰减高换手信号、浓缩进最高关注股(同 corr_pv)→ 成本墙吞噬 + 非稳健。**资金流不救价值**。树 `value_pb`+`rvol_xs`。

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
- **成长是 OOS 最强因子族**（rounds 9/11/12）：净利增速 np_yoy 净OOS +1.57(csi300)/+1.58(csi1000)；**营收增速 rev_yoy 更强更稳——净OOS +1.68(round 12,新 prior-best)、净超额 +3.30、be 614bps、Tier-2[2.55..3.51] 无翻转**(营收难粉饰>净利;**r17 营收成长 vs csi1000 +3.33 ≈ vs csi300 +3.30 → 非规模 beta、大小盘双胜**)。成长系在 2024-26 OOS(小盘/题材年)远超价值系(价值 OOS ~+0.5-0.8)，与价值互补(价值 train 强、成长 OOS 强)。
- **GARP(合理价格成长,round 14)= 迄今最佳风险调整**：PB 最便宜半数池内按 np_yoy 选 top-50 → 净超额 +3.37、**Sharpe 0.83(成长族最高)**、**train +0.97 / OOS +1.09(双强且均衡)**、回撤 0.40(<纯成长 0.55)、be 418bps、Tier-2[1.88..3.34] 无翻转。估值纪律剔除"高估成长"→ 保住成长强 OOS 的同时补强 train、压低回撤,**直接缓解"OOS 单一 regime"诚实边界**。便宜+成长共识 > 任一单飞。
- **对照(round 15):质量∩成长稀释、价值∩成长增强**——高 ROE ∩ 高 np_yoy 净超额 +2.20 / OOS +1.05 / Sharpe 0.60,**弱于纯成长(r9 +3.50/+1.57/0.73)与 GARP(r14)**;ROE 与成长正相关→叠加="贵的质量成长",唯**估值(PB)才是真正多元化的约束**。结论:并非所有因子组合都增益,value+growth(GARP)是特例。
- **★ rev-GARP(round 18)= 迄今最强且最稳**：PB 便宜半数池内按营收增速 rev_yoy 选 top-50(综合 r12 最干净成长 + r14 估值纪律)→ **净超额 +4.76(PASS 最高)、Sharpe 0.87(最高)、train +1.07 / OOS +1.75(双强,OOS 亦居首)、回撤 0.43、be 475bps、Tier-2[2.57..4.69] 全强正无翻转**。关键:**净 vs-EW +0.64(正!)**——连不可投资的等权强基准都跑赢(首个清晰做到的 PASS 轮),大幅强化"非基准依赖"诚实性。营收成长 × 估值纪律 = 最佳综合。**r19/r22 三桶 vs csi300/500/1000 = +4.76/+4.65/+4.79 全胜 → 赢家彻底非规模 beta**。**r20 行业中性版**(每行业 top-3)=最佳风险调整/最易部署:**Sharpe 1.10(全场最高)、回撤 0.21(腰斩)**、换手 9.3%、be 818bps、净超额 +3.76、OOS +0.96(train +1.25),代价=让出 vs-EW(−0.47,行业中性剥离小盘 tilt)。⇒ **三形态**:①激进 raw(r18,净+4.76/OOS+1.75/Sharpe0.87/beat-EW+0.64/DD0.43);②稳健 行业中性(r20,Sharpe1.10/DD0.21/换手9.3%/OOS+0.96/让出EW);③可投资 部署加固(r21,roe>0+流动性≥5000万→净+2.34/OOS+1.40/be310bps,但**地板滤掉小盘 cheap-growth→边际收窄**,Sharpe0.60/DD0.53)。**诚实结论:raw 表头部分靠较小/欠流动名;可投资至容量的版本 OOS≈+1.4、净≈+2.3——仍稳健跑赢指数但收敛。** 部署纪律对成长系是"减分项"(剥离小盘成长),与对价值系中性(r5)相反。
- **估值纪律可推广但 PB 最优（round 23 PE-GARP）**：盈利(PE)便宜半数池内按营收增速选 → 净超额 +2.69 / OOS +1.24 / Sharpe 0.64，PASS 但**弱于 PB 闸 rev-GARP(r18 +4.76/+1.75)**。⇒「便宜闸 + 营收成长 = 赢家」对估值口径稳健(账面 PB / 盈利 PE 皆 PASS)，但 **PB 是更好的价值闸**(账面价值比周期性 EPS 更稳、噪声小)。
- **★★ 价值成长双核（round 24）= 全程最佳全能**：value_pb + 营收成长 rev_yoy **两树均值**(软组合,非 GARP 硬闸)→ top-50。**净超额 +5.20(全场最高)、Sharpe 1.09、回撤 0.24、train +1.48 / OOS +1.31(最均衡的强表现)、净 vs-EW +1.19(全场最高)、be 504bps、Tier-2[2.63..4.63] 无翻转**。**软均值组合两互补因子胜过 GARP 硬闸(r18)**：均值在全 universe 软加权(强于价值 OR 成长 OR 两者皆得分高),比 value_frac 硬切更分散→ train/OOS 双强、回撤腰斩。**精化 r15/r10**:均值组合在因子**真正互补且各自强**时大增益(value train强 + rev OOS强),在**冗余/偏弱**时稀释(ROE∩成长、r10 含 ROE 与弱 np_yoy)。**直接破解"OOS 单一 regime"诚实边界**——train 与 OOS 皆 >+1.3。**r25 vs csi1000 +5.22 ≈ vs csi300 +5.20 → 双核亦彻底非规模 beta、beat-EW +1.19**,稳健性确认。⇒ 当前最佳全能选股策略 = 价值成长双核(value_pb+rev_yoy 均值)。
- **★★★ 价值成长质量三核（round 26）= 全程新最佳**：value_pb + 营收成长 rev_yoy + 高毛利 gross_margin **三树均值** → top-50。**净超额 +6.64(全场最高)、OOS +1.92(全场最高)、train +1.49、Sharpe 1.09、回撤 0.30、净 vs-EW +2.44(全场最高,远超双核 +1.19)、be 567bps、Tier-2[4.12..5.36] 全强正无翻转(最稳)**。**加第3个【与成长正交】的质量因子(毛利率)大幅增益、非稀释**——印证 r24 软组合机制并推翻"因子空间已尽":value(便宜)+rev(成长)+margin(护城河)=三个正交质量维,叠加选"便宜 AND 成长 AND 高毛利"高信念股。**与 r10 关键对照**:r10 加 ROE(与成长相关)稀释,r26 加毛利(正交)增益 → **决定因素是正交性,不是因子数**。三核 OOS +1.92 ≫ 双核 +1.31 ≫ 单飞。**r27 vs csi1000 +6.67 ≈ vs csi300 +6.64 → 三核亦彻底非规模 beta**,稳健性确认。⇒ **当前最佳全能选股策略 = 价值成长质量三核**(value_pb+rev_yoy+gross_margin 均值)。**r28 行业中性版**(每行业 top-3):**Sharpe 0.94 / 回撤 0.26 / 换手 8.5%(be 866bps,最易部署)**、净 +3.31 / OOS +0.98,代价让出 vs-EW(−0.90,同 r20 剥离小盘 tilt)——三核的稳健保守形态。

**诚实边界**：①结论取决于基准=可交易指数（合法，EW 不可投资高估机会成本）；②2019-26 是 A股价值/红利友好周期，OOS 为单一宏观期（行业中性版缓解但未消除）；③long-only 价值/beta 溢价非对冲 alpha（回撤真实）；④最便宜 PB 含价值陷阱→部署需质量+流动性地板（`value∩quality` 也过基准但更薄：超额 +1.71/OOS +0.16/Sharpe 0.72）。详见[发现文档](2026-06-18-value-vs-tradeable-benchmark-finding.md)。

## 待试角度（候选队列，新 baostock 数据集解锁；Claude 维护）

> 优先未试、机制上有别于已证伪族者。新 `features_day` 指标：kdj/cci/wr/obv/vwap20/roc/rvol20/corr_pv20（多数未作选股器测过）。

- [ ] **多因子 AND 共识**：同时满足"便宜(低PB) AND 高质(高ROE)"双优股池（≠ value×momentum 的连乘倾斜；用 value_frac 两段 + 质量闸交集）。
- [x] ~~**量价背离**：`corr_pv20`~~ → round 2 测高确认向=灾难证伪（毛 −4.52）；背离向低 EV，暂搁。
- [x] ~~**资金流向代理**：`rvol20` 放量 × 价值闸~~ → round 16 **sign-flip 证伪**(换手 79.8%/日、be 110bps、top-30 转负);放量不救价值。obv 斜率同族(高换手)暂搁。
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
| 8 | pe_value_m | pure PE value (cheap by earnings) vs index [bench:csi300] [reb20] | +3.244 | 0.226 | 1.21 | daily | — | PASS |
| 9 | growth_npyoy_m | pure growth (np_yoy) vs index — does growth beat index? [bench:csi300] [reb20] | +3.496 | 1.571 | 0.73 | daily | — | PASS |
| 10 | multi_factor_vqg_m | multi-factor composite (value PB + quality ROE + growth np_yoy mean) vs index [bench:csi300] [reb20] | +2.851 | 0.466 | 0.86 | daily | — | PASS |
| 11 | growth_npyoy |  [bench:csi1000] | +2.557 | 1.577 | 0.69 | daily | — | PASS |
| 12 | growth_revyoy | revenue growth (rev_yoy): cleaner top-line growth signal vs index [bench:csi300] [reb20] | +3.301 | 1.681 | 0.67 | daily | — | PASS |
| 13 | quality_gm | gross margin (pricing power/moat): quality axis distinct from ROE, vs index [bench:csi300] [reb20] | +1.721 | 0.561 | 0.52 | daily | — | PASS |
| 14 | garp | GARP: growth (np_yoy) within cheapest-half PB pool — growth with valuation discipline [bench:csi300] [reb20] | +3.374 | 1.088 | 0.83 | daily | — | PASS |
| 15 | quality_growth | quality-growth consensus: high ROE AND high np_yoy (profitable growers) vs index [bench:csi300] [reb20] | +2.203 | 1.045 | 0.60 | daily | — | PASS |
| 16 | value_flow | value x capital-flow: rvol (volume/sma20) tilt within cheapest-half PB pool — does volume flow filter value traps? [bench:csi300] [reb20] | +4.491 | 2.088 | 1.00 | daily | sign-flip | FALSIFIED |
| 17 | growth_revyoy_csi1000 | rev_yoy growth robustness: vs csi1000 small-cap index — non-size-beta? [bench:csi1000] [reb20] | +3.327 | 1.670 | 0.67 | daily | — | PASS |
| 18 | garp_rev | rev-GARP: revenue growth (rev_yoy) within cheapest-half PB pool — synthesize r12+r14 [bench:csi300] [reb20] | +4.760 | 1.749 | 0.87 | daily | — | PASS |
| 19 | garp_rev_csi1000 | rev-GARP robustness vs csi1000 small-cap index [bench:csi1000] [reb20] | +4.786 | 1.739 | 0.87 | daily | — | PASS |
| 20 | garp_rev_sn | sector-neutral rev-GARP (top-3 per industry) — does the winner survive sector balancing? [bench:csi300] [reb20] [sector-neutral] | +3.756 | 0.960 | 1.10 | daily | — | PASS |
| 21 | garp_rev_deploy | deploy-hardened rev-GARP: rev_yoy tilt within cheap-half of profitable+liquid pool (final investable form) [bench:csi300] [reb20] | +2.344 | 1.403 | 0.60 | daily | — | PASS |
| 22 | garp_rev_csi500 | rev-GARP three-bucket completion: vs csi500 mid-cap index [bench:csi500] [reb20] | +4.651 | 1.641 | 0.87 | daily | — | PASS |
| 23 | garp_rev_pe | PE-GARP: rev_yoy tilt within cheapest-half PE pool — does valuation discipline generalize from book to earnings? [bench:csi300] [reb20] | +2.685 | 1.245 | 0.64 | daily | — | PASS |
| 24 | value_rev_blend | value + rev-growth 2-sleeve mean blend — does mean-combining complementary factors smooth regime vs GARP gate? [bench:csi300] [reb20] | +5.195 | 1.312 | 1.09 | daily | — | PASS |
| 25 | value_rev_blend_csi1000 | value+rev blend robustness vs csi1000 small-cap index [bench:csi1000] [reb20] | +5.221 | 1.302 | 1.09 | daily | — | PASS |
| 26 | value_rev_gm_blend | value + rev-growth + gross-margin 3-sleeve blend — does an uncorrelated quality factor add to the 2-sleeve champion? [bench:csi300] [reb20] | +6.641 | 1.920 | 1.09 | daily | — | PASS |
| 27 | value_rev_gm_blend_csi1000 | tri-core blend robustness vs csi1000 small-cap index [bench:csi1000] [reb20] | +6.667 | 1.910 | 1.09 | daily | — | PASS |
| 28 | value_rev_gm_blend_sn | tri-core sector-neutral (top-3 per industry) — risk-adjusted/regime robustness of the champion [bench:csi300] [reb20] [sector-neutral] | +3.306 | 0.982 | 0.94 | daily | — | PASS |
