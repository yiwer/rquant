# baostock 5 年回测数据集 — 设计与参考

> 用户指令（2026-06-18）："拉取5年内市场标的15min的行情数据+每天的板块数据，清洗、建模、处理、计算各项常规指标，保存并建立回测数据集。" 范围决策：universe=top-2000(survivorship-free)、格式=引擎原生+预存指标特征、指标=扩展集。

## 数据源决策（实测定）

| 需求 | 源 | 实测 |
|---|---|---|
| 多年 15m | **baostock**(`query_history_k_data_plus` freq=15, adjustflag=2 qfq) | ✅ 5yr 21k bar/股；含退市(survivorship-free)。~23s/股顺序（并发不可行：4 并发 3min 未完成）|
| 日线 | baostock freq=d（含 turn 换手率/pctChg） | ✅ 2018+，~2s/股 |
| sina 15m | `stock_zh_a_minute` | ❌ 仅 ~6 个月历史（弃用）|
| eastmoney 分钟/板块 | `stock_zh_a_hist_min_em`/`stock_board_*` | ❌ 限频 ConnectionError（弃用）|
| 行业归属 | baostock `query_stock_industry` | ✅ 5207 股 / 83 行业（证监会分类）|

**关键现实**：top-2000 是月度 survivorship-free，**5 年并集 = 5115 股 ≈ 全市场**（churn 巨大）→ 15m 顺序抓 **~33h**。按近期成交额排序抓取，使"可用 top-2000"先落地、survivorship-free 长尾后补，可在任意检查点止损。

## 数据集布局（`data/baostock/`，gitignored，全可复现）

| 路径 | 内容 | 用途 |
|---|---|---|
| `kday/<sym>.csv` | 日线 qfq OHLCV + turn + pctChg（2018+） | rquant **primary**（日频回测）|
| `k15m/<sym>.csv` | 15m qfq OHLCV + amount（2021+） | rquant **primary**（日内→日频）|
| `features_day/<sym>.csv` | 日线扩展 TA 指标（~30 列） | **独立分析存档**（"保存指标"诉求）|
| `features_15m/<sym>.csv` | 15m 扩展 TA 指标 | 同上 |
| `sector/<industry>.csv` | 各行业等权日线序列 time,ret,index,n,breadth | 板块日线数据 |
| `sector_membership.csv` | symbol,industry,classification,update_date | 行业归属 |
| `sector_daily_panel.csv` | time,industry,ret,n,breadth（长表） | 横截面板块分析 |
| `universe_baostock_{day,15m}.csv` | symbol,primary,context,fundamentals(真财务) | rquant 引擎入口 |
| `dataset_manifest.json` | 覆盖/日期范围/总条数/质量旗标/来源 | 索引 |

## 扩展指标集（features_*，全部因果·无前视，单测钉死）

MA(5/10/20/60)、EMA(12/26)、volMA(5/20)、ret(涨跌幅)、amplitude(振幅)、MACD(dif/dea/hist)、RSI(14 Wilder)、BOLL(20,2: mid/up/dn/%b/bandwidth)、ATR(14 Wilder)、KDJ(9,3,3)、CCI(14)、WR(14)、OBV、VWAP(20 滚动)、ROC(12)、已实现波动率(20 对数收益std)、量价相关(20)。
> 引擎 DSL 亦内置 sma/ema/rsi/macd/boll/atr 等可现算；features 为预存独立存档，不进 universe 的 fundamentals 列（该列指向真财务 `data/fundamentals/`，供 `fund.*`）。

## 板块日线方法学

eastmoney 板块指数限频不通 → **自算**：baostock 行业归属 + 现成日线，每行业**等权**聚合成分股日收益（clip |ret|<0.5 防除权污染）→ ret/净值index/成分数n/上涨广度breadth。survivorship-controllable，比抓板块指数更干净透明。

## 复现命令

```
python scripts/build_universe_union.py                 # → universe_5yr_symbols.txt (5115, 流动性序)
python scripts/fetch_baostock.py                       # → kday/ + k15m/ (~33h, resumable)
python scripts/build_indicators.py --bars-dir data/baostock/kday  --out-dir data/baostock/features_day
python scripts/build_indicators.py --bars-dir data/baostock/k15m  --out-dir data/baostock/features_15m
python scripts/build_sectors.py                        # → membership + sector/ + panel
python scripts/build_dataset_manifest.py               # → universe_* + manifest.json
```

## 已知边界（诚实）

- **survivorship-free 但需声明**：baostock 含退市，但当前在市筛选/分类用最新快照；行业归属为近期分类（非逐期历史）。
- **qfq 锚定**：baostock qfq 锚至最新；日内/日线同源同锚（一致）。日收益跨除权日反映分红。
- **抓取耗时**：5115 股 ~33h；按流动性序，可在任意 30min 检查点止损（前 ~2000 即覆盖主流流动股）。
- **板块等权**：等权（非市值加权）；可后续加市值加权变体。

## 状态

抓取后台进行中（30min 监控+异常自恢复）。脚本 + 单测 + 板块 + manifest 均已建并验证；features/manifest 待抓取完成后全量重跑。完成后更新本文档的最终覆盖数。
