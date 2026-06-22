# A股选股因子全集目录（计算方式 + 获取方式 + 本项目现状）

> 目的：穷尽式罗列能描述「股价 / 基本面 / 行业 / 趋势 / 量能 / 情绪」等行为的影响因子，含已证伪者。**仅罗列，不含回测结论**。证伪记录见 `docs/superpowers/2026-06-*` findings 与 `MEMORY.md`。
> 编制日期 2026-06-22。

## 图例

**本项目现状**：✅ 已有现成列 ｜ 🟡 现有数据可算（DSL 实时 / 建表脚本，无需新拉）｜ 🔴 需新拉数据。

**数据底座**
- `kday`：日线 `time,open,high,low,close,volume,amount,turn(换手率%),pctChg(涨跌幅%)`（baostock 前复权，2018+，全市场并集）
- `fund`：季频时点财务 `roe,np_yoy(净利同比),rev_yoy(营收同比),gross_margin(毛利率),eps,bps`（akshare/baostock，point-in-time）
- `pa_sector`：`pa_*`×11 价格行为 + `sec_*`×4 板块（build_pa_features.py / build_sector_factors.py）
- `indicators`：`atr14,boll_{up,mid,dn,bw,pctb},cci14,ema12,ema26,kdj_{k,d,j},macd_{dif,dea,hist},obv,roc12,rsi14,rvol20`（build_indicators.py）
- `index`：宽基指数日线（沪深300/500/1000）
- `DSL`：引擎内置函数（`sma ema wma rsi macd_line macd_signal macd_hist atr highest lowest std slope percentrank corr crossover crossunder barssince ref valuewhen count sigmoid abs exp log sqrt floor sign pow max min session_vwap hour minute dow`）+ `fund.<col>`

**新数据源（🔴 用）**：`baostock+`=baostock 加字段/季频接口（peTTM/pbMRQ/psTTM/pcfNcfTTM；query_profit/growth/balance/cash_flow/operation/dupont_data）；`akshare`/`tushare`=北向/龙虎榜/分析师/股东户数/解禁/融资融券/分红/限售/指数成分。

---

## A. 价格 / 收益（Price & Return）

| 因子 | 计算 | 获取/现状 |
|---|---|---|
| 收益率(N日) | `close/ref(close,N)-1`，N∈{1,5,10,20,60,120,250} | 🟡 DSL |
| 对数收益 | `log(close/ref(close,1))` | 🟡 DSL |
| 涨跌幅(当日) | `pctChg` | ✅ kday(需接入Bar) |
| 累计涨跌(区间) | `close/ref(close,N)-1` | 🟡 DSL |
| 距均线偏离 | `close/sma(close,N)-1` | 🟡 DSL |
| 距N日最高/最低 | `close/highest(high,N)-1`，`close/lowest(low,N)-1` | 🟡 DSL |
| 52周高点接近度 | `close/highest(high,250)` | 🟡 DSL |
| 通道位置(Donchian %) | `(close-lowest(low,N))/(highest(high,N)-lowest(low,N))` | 🟡 DSL |
| Bollinger %b | `boll_pctb` = `(close-boll_dn)/(boll_up-boll_dn)` | ✅ indicators |
| 缺口(跳空) | `open/ref(close,1)-1` | 🟡 DSL |
| 日内振幅 | `(high-low)/ref(close,1)` | 🟡 DSL |
| 日内位置(收盘强度) | `(close-low)/(high-low)` | 🟡 DSL |
| K线实体比 | `abs(close-open)/(high-low)` | 🟡 DSL |
| 上/下影线比 | `(high-max(close,open))/(high-low)`；`(min(close,open)-low)/(high-low)` | 🟡 DSL |
| VWAP偏离(滚动) | `close/(sma(close*volume,N)/sma(volume,N))-1` | 🟡 DSL |
| VWAP偏离(当日) | `close/session_vwap-1` | 🟡 DSL(日内) |
| 信号K(吞没/锤子等) | `pa_sig_with,pa_sig_cnt`（PA 形态计数） | ✅ pa_sector |

---

## B. 趋势（Trend）

| 因子 | 计算 | 获取/现状 |
|---|---|---|
| 均线斜率 | `slope(sma(close,N),k)` | 🟡 DSL |
| 价在均线上方 | `close > sma(close,N)`（N∈{20,60,120,250}） | 🟡 DSL |
| 均线多头排列 | `sma(close,5)>sma(close,20) and sma(close,20)>sma(close,60)` | 🟡 DSL |
| MACD 快线/柱/信号 | `macd_line(close,12,26)`，`macd_hist(...)`，`macd_signal(...)` | ✅ indicators / 🟡 DSL |
| MACD 金叉 | `crossover(macd_line(...), macd_signal(...))` | 🟡 DSL |
| 道氏高低点(HH-HL) | `highest(high,n)>highest(high,m) and lowest(low,n)>lowest(low,m)` | 🟡 DSL（`dow_trend_xs` 已证伪@top3） |
| 价格结构方向 | `pa_dir,pa_struct,pa_regime`（PA 趋势态） | ✅ pa_sector |
| 线性回归斜率 | `slope(close,N)` 或 `slope(log(close),N)` | 🟡 DSL |
| ADX/DMI(趋势强度) | Wilder ADX：+DI/−DI 自 high/low/close | 🔴 DSL无ADX，需建表(可由atr+方向动量算) |
| Aroon | `(N-距N日最高bar)/N*100` 与下行对称 | 🟡 DSL(`barssince`变体)/建表 |
| TRIX | 三重EMA的ROC：`roc(ema(ema(ema(close))))` | 🟡 DSL组合 |
| 一目均衡(Ichimoku) | 转换/基准线 = (highest+lowest)/2 各周期 | 🟡 DSL组合 |
| SuperTrend | ATR 通道翻转线 | 🟡 DSL组合 |
| 趋势持续天数 | `barssince(crossover(close,sma(close,N)))` | 🟡 DSL |
| 通道宽窄(挤压) | `boll_bw` = `(boll_up-boll_dn)/boll_mid`；`pa_chan` | ✅ indicators/pa_sector |

---

## C. 动量 / 反转（Momentum / Reversal）

| 因子 | 计算 | 获取/现状 |
|---|---|---|
| 横截面动量(12-1) | `ref(close,20)/ref(close,250)-1`（剔近月反转） | 🟡 DSL |
| 短期反转(1月/1周) | `-(close/ref(close,20)-1)`；`-(close/ref(close,5)-1)` | 🟡 DSL |
| RSI | `rsi(close,14)` | ✅ indicators/DSL |
| 随机指标 KDJ | `kdj_k,kdj_d,kdj_j` | ✅ indicators |
| Williams %R | `(highest(high,N)-close)/(highest(high,N)-lowest(low,N))*-100` | 🟡 DSL |
| CCI | `cci14` = (TP-sma(TP))/(0.015·MD) | ✅ indicators |
| ROC | `roc12` = `close/ref(close,12)-1` | ✅ indicators/DSL |
| 相对强度(vs指数) | `(close/ref(close,N)) / (idx/ref(idx,N))` | 🟡 需指数对齐(index已有) |
| 相对强度(vs行业) | 个股动量 − 板块动量 `sec_mom20` | 🟡 pa_sector |
| 动量加速度 | `slope(close/sma(close,20),5)` | 🟡 DSL |
| 52周新高动量 | `close>=highest(close,250)` 事件 | 🟡 DSL |
| 路径效率(分形) | `abs(close-ref(close,N))/Σabs(逐日变动)` | 🟡 DSL/建表(15m native 已证伪) |

---

## D. 波动率 / 风险（Volatility / Risk）

| 因子 | 计算 | 获取/现状 |
|---|---|---|
| 已实现波动 | `std(close/ref(close,1)-1, N)` | 🟡 DSL |
| ATR(真实波幅) | `atr(14)`；归一 `atr(14)/close` | ✅ indicators/DSL |
| Bollinger 带宽 | `boll_bw` | ✅ indicators |
| 波动收缩 | `std(close,20) < std(close,60)` | 🟡 DSL(`value_volcontract` 已证伪) |
| Parkinson 波动(高低) | `sqrt(mean(ln(high/low)^2)/(4ln2))` | 🟡 DSL组合 |
| Garman-Klass 波动 | 用 OHLC 的日内波动估计 | 🟡 DSL组合 |
| 下行波动(半方差) | 仅负收益的 std | 🟡 建表 |
| 最大回撤 | `1 - close/highest(close,N)` | 🟡 DSL |
| Beta(对市场) | `corr(stock_ret, idx_ret, N) · std比` | 🟡 需指数对齐 |
| 特质波动(残差) | 个股收益对市场回归的残差 std | 🔴 需回归建表 |
| 波动率分位 | `percentrank(atr(14)/close, 250)` | 🟡 DSL |
| 振幅均值 | `sma((high-low)/ref(close,1), N)` | 🟡 DSL |

---

## E. 量能 / 资金流（Volume / Money Flow）

| 因子 | 计算 | 获取/现状 |
|---|---|---|
| 成交量 | `volume` | ✅ kday |
| 成交额 | `amount`（≈`close*volume`） | ✅ kday |
| 换手率 | `turn`（=volume/流通股本） | ✅ kday(需接入Bar) |
| 相对成交量 rvol | `volume/sma(volume,20)` | ✅ indicators(rvol20)/DSL（`rvol_xs` 已证伪） |
| 量能趋势 | `sma(volume,5)/sma(volume,60)-1` | 🟡 DSL（`vol_energy_xs` 已证伪） |
| OBV(能量潮) | `obv` = Σ sign(Δclose)·volume | ✅ indicators |
| 累积/派发线 A/D | Σ ((close-low)-(high-close))/(high-low)·volume | 🟡 DSL组合 |
| 资金流量 MFI | RSI 的成交额加权版 | 🟡 DSL组合/建表 |
| 蔡金资金流 CMF | N日 ((2close-high-low)/(high-low)·vol) / Σvol | 🟡 DSL组合 |
| 量价相关 | `corr(close, volume, N)` | 🟡 DSL |
| 量价背离 | 价创新高而量不创(`close≥highest` 且 `volume<sma`) | 🟡 DSL |
| 放量突破 | `close>highest(ref(high,1),N) and volume>sma(volume,N)*k` | 🟡 DSL |
| 量能波动 | `std(volume,N)/sma(volume,N)` | 🟡 DSL |
| 主力/大单净流入 | 分钟逐笔按单量分级聚合 | 🔴 需 L2/资金流(akshare 东财) |
| 北向持股/净买 | 沪深港通持股比例、净买额 | 🔴 akshare/tushare |
| 融资融券余额 | 两融余额、融资买入占比 | 🔴 akshare/tushare |
| 龙虎榜 | 上榜、机构净买、游资席位 | 🔴 akshare/tushare |
| Amihud 非流动性 | `mean(abs(ret)/amount, N)` | 🟡 DSL |

---

## F. 流动性 / 规模（Liquidity / Size）

| 因子 | 计算 | 获取/现状 |
|---|---|---|
| 流动性地板 | `sma(close*volume,20) >= 阈值`（成交额） | 🟡 DSL（部署在用） |
| 总市值 | close × 总股本 | 🔴 baostock+(总股本)/akshare |
| 流通市值 | close × 流通股本 | 🔴 baostock+/akshare |
| 对数市值(规模因子) | `log(市值)` | 🔴 同上 |
| 换手率均值 | `sma(turn, N)` | ✅ kday(turn) |
| Amihud ILLIQ | `mean(abs(ret)/amount, N)` | 🟡 DSL |
| 零成交天数比 | N日内 volume=0 占比 | 🟡 建表 |
| 买卖价差(估) | 高频或日内估计 | 🔴 需分钟/L2 |
| 上市天数/次新 | 当前日期 − 上市日 | 🔴 baostock+(上市日) |

---

## G. 价值（Value）

| 因子 | 计算 | 获取/现状 |
|---|---|---|
| PB(市净率) | `close/bps`；便宜度 `1/(1+close/bps)` | ✅ fund(bps)（部署在用 value_pb） |
| PE(市盈率) | `close/eps`(或 peTTM) | 🟡 fund(eps)/🔴 baostock+ peTTM |
| 盈利收益率 E/P | `eps/close` | 🟡 fund |
| PS(市销率) | psTTM = 市值/营收 | 🔴 baostock+ psTTM |
| PCF(市现率) | pcfNcfTTM = 市值/经营现金流 | 🔴 baostock+ pcfNcfTTM |
| 股息率 | 每股分红/close | 🔴 akshare/baostock 分红 |
| EV/EBITDA | (市值+净债)/EBITDA | 🔴 需资产负债+利润表 |
| EV/Sales | 企业价值/营收 | 🔴 同上 |
| PEG | PE / 净利增速 = `(close/eps)/np_yoy` | 🟡 fund 组合（已证伪弱） |
| FCF收益率 | 自由现金流/市值 | 🔴 需现金流量表 |
| 账面市值比 B/M | `bps/close`（PB 倒数） | ✅ fund |
| 横截面价值分位 | `value_frac` 两段闸（最便宜 X%） | 🟡 引擎已支持 |

---

## H. 成长（Growth）

| 因子 | 计算 | 获取/现状 |
|---|---|---|
| 净利同比 | `np_yoy` | ✅ fund（部署在用） |
| 营收同比 | `rev_yoy` | ✅ fund |
| EPS 同比 | eps 同比 | 🟡 fund(跨期) |
| 单季净利环比 QoQ | 季度净利 t/t-1 | 🔴 baostock+ 季频 |
| 净利/营收 CAGR | 多年复合增速 | 🔴 季频历史 |
| 可持续增长率 | ROE×(1−分红率) | 🔴 需分红率 |
| 毛利同比变化 | gross_margin 同比差 | 🟡 fund(跨期) |
| 增速加速度 | np_yoy 的环比变化 | 🟡 fund(跨期) |
| 营收增速稳定性 | rev_yoy 的滚动 std(反向) | 🔴 季频历史 |

---

## I. 质量 / 盈利 / 财务健康（Quality / Profitability / Health）

| 因子 | 计算 | 获取/现状 |
|---|---|---|
| ROE | `roe`（净资产收益率） | ✅ fund（部署用作地板） |
| ROA | 净利/总资产 | 🔴 baostock+ dupont/balance |
| ROIC | NOPAT/投入资本 | 🔴 需利润+资产负债 |
| 毛利率 | `gross_margin` | ✅ fund |
| 净利率 | npMargin = 净利/营收 | 🔴 baostock+ profit |
| 营业利润率 | 营业利润/营收 | 🔴 baostock+ |
| 资产周转率 | 营收/总资产 | 🔴 baostock+ operation |
| 杜邦分解 | ROE = 净利率×周转×杠杆 | 🔴 baostock+ dupont_data |
| 资产负债率 | liabilityToAsset | 🔴 baostock+ balance |
| 流动比率/速动比率 | 流动资产/流动负债 | 🔴 baostock+ balance |
| 利息保障倍数 | EBIT/利息费用 | 🔴 需利润表 |
| 经营现金流/净利 | CFOToNP（盈利质量/应计） | 🔴 baostock+ cash_flow |
| 应计项(Accruals) | (净利−经营现金流)/总资产 | 🔴 需现金流+资产 |
| 盈利稳定性 | 历史 EPS/ROE 的 std(反向) | 🔴 季频历史 |
| Piotroski F-score | 9项财务健康打分(0-9) | 🔴 需多张报表 |
| Altman Z-score | 破产风险综合分 | 🔴 需资产负债+市值 |
| 商誉占比 | 商誉/净资产 | 🔴 baostock+ balance |
| 股息支付率 | 分红/净利 | 🔴 akshare 分红 |

---

## J. 行业 / 板块（Industry / Sector）

| 因子 | 计算 | 获取/现状 |
|---|---|---|
| 行业归属 | 申万/证监会行业 membership | ✅ build_sectors.py(sector_membership) |
| 板块动量 | `sec_mom20`（板块N日收益） | ✅ pa_sector（`sector_strength` 已证伪@top3） |
| 板块趋势 | `sec_trend`（板块均线态） | ✅ pa_sector |
| 板块宽度(breadth) | `sec_breadth`（板块内上涨占比） | ✅ pa_sector |
| 板块热度(成交额) | `sec_heat`（板块成交额占比/分位） | ✅ pa_sector |
| 行业相对强度 | 个股收益 − 行业收益 | 🟡 pa_sector 组合 |
| 行业中性化 | 因子减行业均值/排名 | 🟡 引擎 per_sector / 建表 |
| 板块轮动位置 | 板块动量的横截面排名变化 | 🟡 建表 |
| 龙头溢价 | 个股市值/行业中位市值 | 🔴 需市值 |
| 行业景气(宏观) | 行业 PMI/价格/库存 | 🔴 外部宏观 |

---

## K. 情绪 / 事件 / 资金面 / 另类（Sentiment / Event / Flow / Alt）

| 因子 | 计算 | 获取/现状 |
|---|---|---|
| 北向资金持股 | 沪深港通持股比例及变化 | 🔴 akshare/tushare |
| 龙虎榜资金 | 机构/游资净买、上榜频率 | 🔴 akshare |
| 融资融券 | 两融余额、融资余额变化率 | 🔴 akshare/tushare |
| 分析师评级 | 评级、目标价、上调/下调 | 🔴 tushare/东财 |
| 盈利预期修正 | 一致预期 EPS 的修正方向 | 🔴 tushare/Wind |
| 股东户数变化 | 户数环比(集中度) | 🔴 akshare |
| 股权质押比例 | 质押股/总股本(风险) | 🔴 akshare |
| 限售解禁 | 解禁市值/流通市值、解禁日临近 | 🔴 akshare |
| 大股东增减持 | 净增持金额/方向 | 🔴 akshare |
| 业绩预告/快报 | 预增/预减、披露事件 | 🔴 akshare |
| 分红送转事件 | 高送转、除权除息 | 🔴 akshare/baostock |
| 新闻情绪 | 标题/正文情感打分(time≤t) | 🔴 引擎已留 news 接口,需采集 |
| 停复牌/ST 状态 | 是否 ST/*ST/停牌 | ✅ st_symbols.csv(ST)/🔴 停牌 |
| 指数纳入/调出 | 沪深300/500 成分调整事件 | 🔴 akshare 成分 |
| 回购/增发事件 | 回购金额、定增折价 | 🔴 akshare |

---

## L. 日历 / 季节性（Calendar / Seasonality）

| 因子 | 计算 | 获取/现状 |
|---|---|---|
| 月份效应 | 月份(1-12) one-hot | 🟡 DSL(可扩月份)/日期 |
| 星期效应 | `dow`(1=周一) | ✅ DSL |
| 小时/分钟(日内) | `hour,minute` | ✅ DSL(日内) |
| 财报披露窗 | 距最近报告期天数 | 🟡 fund 时点 |
| 节前/节后效应 | 距长假交易日 | 🔴 交易日历建表 |
| 月初/月末效应 | 当月第几个交易日 | 🟡 建表 |

---

## M. 市场 / 制度环境（Market Regime / Macro）

| 因子 | 计算 | 获取/现状 |
|---|---|---|
| 大盘趋势 regime | 指数 `close>sma(idx,200)`（牛/熊） | 🟡 index 序列(需入树, 引擎特性) |
| 市场波动 regime | 指数波动分位(高/低波) | 🟡 index |
| 市场宽度 | 全市场上涨家数占比 | 🟡 建表 |
| 风格(大小盘) | 沪深300 vs 中证1000 相对强度 | 🟡 index 组合 |
| 无风险利率/流动性 | 国债收益率、SHIBOR | 🔴 外部宏观 |
| 北向总净流入 | 全市场北向净流入 | 🔴 akshare |
| 涨跌停家数 | 涨停/跌停统计(情绪) | 🔴 建表 |

---

## 横截面工程（用因子前的标准处理）

| 处理 | 说明 |
|---|---|
| 去极值(winsorize) | 分位/MAD 截尾，防异常值主导 |
| 标准化(z-score/rank) | 截面 z 或排名归一，统一量纲后再合成 |
| 行业/市值中性化 | 因子对行业、log市值回归取残差，剥离风格 beta |
| 缺失处理 | NaN→截面中位 或 弃权(DSL 预热弃权语义) |
| 时点纪律(PIT) | 财务用披露日生效、技术用 ≤t、滞后1日防前视；membership 去幸存者 |
| 合成方式 | 加性均值(项目结论:正交因子加性最优) / 乘性 / 两段闸(GARP) / 打分加权 |

---

## 与本项目已验证结论的对应（避免重复踩坑）

- **已部署有效**：value_pb(便宜度) + np_yoy(净利增速) 加性均值 + roe>0/流动性地板 + 月频 top-3 + ST 过滤。
- **已系统证伪**（top-3，多为稀释或过拟合）：纯技术(创新高/突破/均线/MACD/KDJ)、PA/K线结构、道氏趋势、量能/相对成交量、板块强度、低波动、FF3 alpha、反转取反、15m 微结构、roe 地板收紧(杀 edge)、周频(砍 OOS 96%)。详见 `MEMORY.md` ⑤–⑧ 与 `docs/superpowers/2026-06-*findings`。
- **未试、需新工程**（潜在正交增量）：经营现金流/应计、ROIC/杜邦、负债率、北向资金、分析师预期修正、股东户数、市值规模、持仓内逐 bar 真择时(signal/sim)、市场 regime 条件选股(指数序列入树)。
