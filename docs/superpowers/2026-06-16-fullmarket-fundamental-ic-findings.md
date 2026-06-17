# 宽截面基本面 IC 验证 · findings（子项②）

- 日期：2026-06-16
- 范围：在 survivorship-free 的 **月末 top-2000-按成交额** 宽截面上，诚实检验基本面因子的前瞻预测力。这是子项①（20 名欠采样，inconclusive）做不了的正经检验。
- 诚实纪律（§5.3）：works / inconclusive / falsified 均如实记录；**未为好看数字调参**。

## 1. 数据与口径

- **universe**：`data/universe_full.csv` 主板+科创+创业 roster **5529**（已排除 B 股、北交所——illiquid 且 sina 不供日线）。
- **OHLCV**：sina `stock_zh_a_daily` qfq（单源，volume 统一为**股**；退市股 eastmoney ×100 归一）。覆盖 **5217/5529 ≈ 94.4%**（缺 312：多为 **2018 前退市**＝窗口外无关，少量窗口内退市 eastmoney 限频未抓到）。
- **fundamentals**：①管线重生成全市场逐股 point-in-time 财务 CSV **5615**（公告日锚，`fund.roe/np_yoy/rev_yoy/gross_margin/eps/bps`）。
- **membership**：`scripts/build_membership.py` 月末按近 20 日均成交额（≤d 排名 + 近 14 日在市窗）取 top-2000 → `data/membership_top2000.csv`，**102 月 × 2000 = 204000 行**。survivorship-free：退市股活跃且够流动时纳入、退市后自动出；茅台 sh600519 全 102 月在列；ST 类低流动股在池但排不进 top-2000（正确）。
- **IC 运行**：`rquant factor --universe universe_membership.csv --membership membership_top2000.csv --sample 20 --horizon 20 --layers 5 --warmup 60 --window 120`，加载并集 5194 股；有效期数 **n_periods=86**（衰减阶梯 5/10/20/40/80 交易日）。

## 2. 结果（RankIC across 当期 top-2000，86 期）

| 因子 | 表达式 | RankIC | RankICIR | RankIC_t | pos% | 分层单调 | spread Sharpe | 判定（F-1: \|RankIC\|>0.03 ∧ \|ICIR\|>0.3）|
|---|---|---|---|---|---|---|---|---|
| **pb** | close/fund.bps | **−0.0459** | −0.27 | **−2.49** | 40.7% | **−0.90** | −0.35 | **borderline**：过 RankIC 幅度 + t 显著 + 强单调，**ICIR 0.27 微差 0.30** |
| pe | close/fund.eps | −0.0177 | −0.17 | −1.54 | 46.5% | −0.10 | −0.27 | 弱（同向价值效应，未达门槛）|
| roe | fund.roe | +0.0094 | 0.07 | 0.68 | 50.0% | −0.10 | −0.38 | **证伪**（近零）|
| npyoy | fund.np_yoy | −0.0001 | −0.00 | −0.01 | 48.8% | −0.30 | −0.22 | **证伪**（零）|
| revyoy | fund.rev_yoy | +0.0043 | 0.05 | 0.45 | 48.8% | +0.70 | 0.18 | **证伪**（RankIC 可忽略）|
| gm | fund.gross_margin | −0.0038 | −0.04 | −0.34 | 51.2% | −0.70 | −0.29 | **证伪**（近零）|

**IC 衰减（RankIC × horizon）**——价值因子随前瞻期**增强**（持久非瞬时）：

| 因子 | h=5 | h=10 | h=20 | h=40 | h=80 |
|---|---|---|---|---|---|
| pb | −0.030 | −0.034 | −0.046 | −0.067 | **−0.089** |
| pe | −0.010 | −0.012 | −0.018 | −0.025 | −0.028 |

**相关性**：roe–npyoy 0.45、npyoy–revyoy 0.51（质量/成长族内相关）；pe–pb 0.34（估值族内相关）；估值与质量族间近正交（roe–pe −0.06）。

## 3. 判读（诚实）

- **价值效应（PB）是唯一站得住的信号**：低 PB → 高前瞻收益，RankIC −0.046（过 0.03 幅度门槛）、t=−2.49（统计显著）、分层单调 −0.90（Q1 0.104 → Q5 0.022）、且随 horizon 单调增强（h=80 达 −0.089，持久）。**唯一未达项是 RankICIR −0.27（差 0.30 一点点）**——即方向稳健、幅度达标、但期间波动使稳定性略欠门槛。综合判 **promising / borderline-pass**，非"达标可用"，也非"证伪"。
- **PE 同向但更弱**（RankIC −0.018），与 PB 共同指向 A 股**价值/低估值**效应在宽截面上真实存在但温和。
- **质量/成长因子（ROE、净利同比、营收同比、毛利率）在该口径/前瞻期上证伪**：|RankIC|<0.01、ICIR≈0、分层不单调。说明单季 yjbb 质量/成长指标对 20 日前瞻横截面收益无稳健线性预测力。
- **对比子项①**（20 名，RankIC~0.02，统计欠采样 inconclusive）：宽截面把 PB 解析到**统计显著**（t=−2.49），这正是②的价值——宽横截面让弱信号可判定。

## 4. 诚实边界 / 局限

- **幸存者残差**：覆盖 94.4%；缺失 312 多为 2018 前退市（窗口外无关），少量窗口内退市未抓到（eastmoney 限频）。残差偏差方向：缺失多为退市前低迷股，纳入或略压低整体收益、对**横截面相对排名 IC 影响有限**；不声称零偏差。
- **量纲**：volume 统一为股（sina 原生；eastmoney ×100）；成交额排名 scale-invariant，未复发迭代#2 量纲 bug。
- **point-in-time 双闸**：membership 排名只用 ≤d 数据 + `fund.as_of(t)` 只见 ≤t 公告——无前视。
- **未调参**：上述为单次默认口径（sample/horizon=20、top-2000、lookback-20）直出，未择优。PB 的 borderline 结论**如实保留**，不调 horizon/口径去"凑过线"。
- **方法学**：本子项只验证**单因子线性 IC**；PB 的可用性需子项③在 2000 上做多因子/择时/分层组合的完整方法学（IC 显著 ≠ 扣费后可交易）。

## 5. 交付结论

子项② 达成：**全市场数据基建打通**（5217 主板股 sina 单源 + 5615 point-in-time 财务）+ **survivorship-free top-2000 membership 机制**（引擎 `--membership` 点时 mask）+ **宽截面 IC 被诚实测量**。

**因子结论**：A 股宽截面上 **价值因子（PB 为主、PE 为辅）是唯一有迹象的基本面信号**（PB borderline-pass：显著 + 单调 + 持久，仅 ICIR 微欠），**质量/成长因子证伪**。这为子项③（基本面×技术方法学）定调：**以价值（低 PB/PE）为基本面侧主轴**，质量/成长不作为独立 alpha。
