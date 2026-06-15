# 基本面数据拉取脚本

## 依赖

```
pip install akshare
```

akshare 1.18.64+ (Python 3.10+). 数据源为东方财富 (EastMoney)，需联网。

## 用法

```bash
python scripts/fetch_fundamentals.py [--out data/fundamentals] [--from-year 2018]
```

- `--out`：输出目录，默认 `data/fundamentals`
- `--from-year`：拉取起始年份（含），默认 2018

示例（完整历史）：

```bash
python scripts/fetch_fundamentals.py --out data/fundamentals --from-year 2018
```

约 33 个季度（2018 Q1 → 当前），每季度全市场 ~5000–12000 行，全程约 5–10 分钟。

## 输出

`data/fundamentals/<symbol>.csv` — 每只股票一个 CSV 文件，按 **公告日升序** 排列，key = `最新公告日期`（point-in-time 锚点）：

```
time,roe,np_yoy,rev_yoy,gross_margin,eps,bps
2019-04-25,8.89,38.9309,32.2124,91.3055,6.77,79.5718
2019-07-18,15.87,40.1154,38.2741,90.9421,12.55,74.3495
...
```

symbol 命名规则：`sh` + 6位代码（上交所：60xxxx / 68xxxx）或 `sz` + 6位代码（深交所：00xxxx / 30xxxx）。

## 字段单位（铁律）

| 字段 | 中文来源 | 单位 |
|---|---|---|
| `roe` | 净资产收益率 | **百分数**（如 `8.89` = 8.89%） |
| `np_yoy` | 净利润-同比增长 | **百分数** |
| `rev_yoy` | 营业总收入-同比增长 | **百分数** |
| `gross_margin` | 销售毛利率 | **百分数** |
| `eps` | 每股收益 | **元** |
| `bps` | 每股净资产 | **元** |
| `time` | 最新公告日期 | YYYY-MM-DD（公告日，非报告期末） |

> **注意**：roe 等字段为百分数原值，DSL 中 `fund.roe > 15` 即 ROE > 15%，不需要乘 100。

## Point-in-time 说明

`time` 字段取自 akshare 返回的 `最新公告日期`，代表财务数据公开可得的日期，而非报告期截止日。这确保回测时不引入未来信息（前视偏差）。

## 数据再现性 / gitignore

`data/fundamentals/` 目录已被 `.gitignore` 排除，不入库。需要时重跑脚本即可再现。
