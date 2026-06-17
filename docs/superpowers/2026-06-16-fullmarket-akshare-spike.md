# akshare 退市数据可行性 spike（子项② Task 0）

- 日期：2026-06-16
- 目的：全量抓取前确认 survivorship-free 的命根——akshare 能否提供 (a) 退市股清单+退市日、(b) 退市股的 qfq OHLCV 历史。
- 结论：**survivorship-free 完全可行，无需降级。**

## 1. 退市清单接口（可用 + 含退市日）

| 接口 | 行数 | 列 | 退市日字段 |
|---|---|---|---|
| `ak.stock_info_a_code_name()` | 5528（在市） | `code, name` | — |
| `ak.stock_info_sh_delist()` | 152（沪退市） | `公司代码, 公司简称, 上市日期, 暂停上市日期` | `暂停上市日期` |
| `ak.stock_info_sz_delist()` | 204（深退市） | `证券代码, 证券简称, 上市日期, 终止上市日期` | `终止上市日期` |

- 全市场 roster ≈ 5528 在市 + 356 退市 ≈ **5884 只**。
- SH 代码列 `公司代码`、SZ 代码列 `证券代码`，均含「代码」二字 → `build_roster.py` 的 `next((c for c in d.columns if "代码" in c))` 探测对两者通用。✓

## 2. 退市股 qfq OHLCV（可取，到最后交易日）

- 探针：`ak.stock_zh_a_hist(symbol="000005", period="daily", start_date="20180101", end_date="20240501", adjust="qfq")`
  （000005 = ST星源，2024-04-26 退市，窗口内）
- 结果：**shape=(1496, 12)**，`日期` 2018-01-02 → 2024-03-05（退市前最后交易日），列含 `日期/开盘/收盘/最高/最低/成交量/成交额/...`。
- 即退市股在其活跃期的完整 qfq 日线**可得**——这正是 survivorship-free 所需：退市股在活跃月份能进 top-2000、退市后无 bar 自动出。✓
- 附带发现：hist 直出 `成交额`（元）列；但引擎 primary CSV 格式只收 `time,open,high,low,close,volume`，membership builder 用 `close×volume` 近似排名（scale-invariant，量纲不影响序）——无需引入 amount 列。

## 3. 降级判定

**不降级**——退市 OHLCV 可得，走全 survivorship-free。

**残余偏差声明（诚实边界）：**
- 退市覆盖 = 上述两清单 356 只。SH 清单（152）多为旧「暂停上市」机制条目，**可能欠覆盖近年沪市退市新规下的退市股**；任何未进清单的退市股不在 roster → 不被抓取 → 不进任何 top-2000 → 构成**残余幸存者偏差**（量级：相对 ~5900 全集的退市尾部，偏小）。Task 10 findings 量化实际纳入的退市股数 + 占比。
- eastmoney 接口偶发 SSLError（探针中 600001 触发）；`fetch_ohlcv.py` 以 try/except + resume（已最新则跳过、失败续跑）兜底，全量抓取可断点续。

## 4. 对后续任务的影响

- Task 5 `build_roster.py`：在市 + 两退市清单合并，代码列「含代码」探测通用——按既定脚本即可。
- Task 6 `fetch_ohlcv.py`：退市股与在市股同接口（`stock_zh_a_hist`），无需特殊分支；缺数据股天然跳过。
- Task 9 `build_membership.py`：`close×volume` 排名口径确认可用。
