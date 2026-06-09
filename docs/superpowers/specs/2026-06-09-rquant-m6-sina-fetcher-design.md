# rquant M6：新浪 fetcher → 本地 CSV — 设计文档

- **日期**：2026-06-09
- **状态**：设计已确认（待 spec 评审 → 进实现计划）
- **关联**：扩展 `2026-06-09-rquant-decision-tree-backtest-design.md`（M1–M5 已实现并合并 master，HEAD `28b98f2`）。M1–M4 spec 决策 #6 即定："新浪 API → 落地缓存 → 回测只读缓存"。

---

## 1. 背景

M1–M5 已交付：纯量化回测 + LLM 节点。数据一直靠**手动丢 CSV**。M6 补上自动取数：一个 `fetch` 子命令从新浪财经拉 A股 K 线落成本地 CSV，`backtest` 照常读它。抓取与回测**解耦**（fetch 是独立一步，回测只读快照），既用上新浪又保住复现性。

复用 M5 已引入的 async `reqwest`，无新依赖。

## 2. 目标与非目标

### 目标
1. `fetch` 子命令：给 symbol + scale，从新浪拉最近 `datalen` 根 K 线 → 写本地 CSV（`time,open,high,low,close,volume`），可被现有 `read_bars_csv` 直接读。
2. 解析逻辑（JSON → Bar）纯函数、可单测、不联网；真实网络靠手动 smoke。
3. 端点可配（`--base-url`），以应对新浪接口变动。

### 非目标（YAGNI / 后置）
- Parquet/SQLite 列式缓存层（CSV 足够；"换源不动引擎"已由同一 CSV 契约满足）。
- 任意历史区间回溯（新浪只给最近 `datalen` 根——这是已知浅历史限制，不绕）。
- 一次拉多 symbol / 多 scale 的批量封装（用户跑多次或自行脚本）。
- 回测自动按需抓取（保持解耦：fetch 与 backtest 是两个独立子命令）。
- 增量追加 / 去重合并历史（每次 fetch 覆盖写出）。
- 复权处理（拉原始数据；复权属未来增强）。

## 3. 锁定决策

| # | 维度 | 选定 |
|---|---|---|
| 1 | 落地格式 | **CSV**（复用 `read_bars_csv`，零新依赖）|
| 2 | datalen 默认 | **1023**（新浪上限），`--datalen` 可调 |
| 3 | base_url 默认 | `https://money.finance.sina.com.cn/quotes_service/api/json_v2.php`，`--base-url` 可覆盖 |
| 4 | 重试 | **2 次** |
| 5 | scale | 透传整数（15/60/240 由用户给），不做多周期便利封装 |
| 6 | I/O | 复用 M5 的 async reqwest；`fetch` 在 `#[tokio::main]` 下跑 |

## 4. 架构

### 组件
- `src/data/sina.rs`（新）
  - `pub fn parse_sina_klines(json: &str) -> Result<Vec<Bar>>`：解析新浪 JSON 数组（字段为字符串）→ `Vec<Bar>`，**按 `time` 升序排序**后返回。纯函数、可单测。
  - `pub async fn fetch_sina_klines(http: &reqwest::Client, base_url: &str, symbol: &str, scale: u32, datalen: u32, max_retries: u32) -> Result<Vec<Bar>>`：拼 URL → GET（带重试）→ `parse_sina_klines`。联网，手动 smoke。
- `src/data/reader.rs`（改）：加 `pub fn write_bars_csv(bars: &[Bar], path: &Path) -> Result<()>`（与 `read_bars_csv` 配对；写 `time` 用 `%Y-%m-%d %H:%M:%S`）。
- `src/cli/mod.rs`（改）：`Cmd` 加 `Fetch` 变体并在 `main` 分发。

### 数据流
```
rquant fetch --symbol sh600000 --scale 15 --out 15m.csv
  → fetch_sina_klines(http, base_url, "sh600000", 15, 1023, 2)
     → GET {base_url}/CN_MarketDataService.getKLineData?symbol=sh600000&scale=15&ma=no&datalen=1023
     → parse_sina_klines(body) → Vec<Bar>(升序)
  → write_bars_csv(&bars, "15m.csv")
  → 打印 "wrote N bars to 15m.csv"

(之后) rquant backtest --primary 15m.csv ...  → read_bars_csv 照常读
```

## 5. 新浪端点契约

- **URL**：`{base_url}/CN_MarketDataService.getKLineData?symbol={symbol}&scale={scale}&ma=no&datalen={datalen}`
- **symbol**：`sh600000`（沪）/ `sz000001`（深）。
- **scale**：分钟数；15=15min、60=1h、240=日线（M6 主用 15/60）。
- **datalen**：≤ 1023；返回**最近** datalen 根（不支持任意起止区间——浅历史限制）。
- **响应**：JSON 数组，元素形如
  ```json
  {"day":"2024-01-02 15:00:00","open":"10.000","high":"10.500","low":"9.800","close":"10.200","volume":"123456"}
  ```
  字段均为**字符串**：`day`→`time`（按 `%Y-%m-%d %H:%M:%S` 解析；M6 面向 intraday，含时分秒），`open/high/low/close/volume`→`f64`（从字符串 parse）。
- 解析后**按 time 升序排序**（不假设新浪返回顺序），使写出的 CSV 满足 `read_bars_csv` 的"时间严格递增"校验。

## 6. 类型契约

```rust
// data/sina.rs
#[derive(serde::Deserialize)]
struct SinaRow { day: String, open: String, high: String, low: String, close: String, volume: String }

pub fn parse_sina_klines(json: &str) -> Result<Vec<Bar>>;
pub async fn fetch_sina_klines(http: &reqwest::Client, base_url: &str, symbol: &str,
                               scale: u32, datalen: u32, max_retries: u32) -> Result<Vec<Bar>>;

// data/reader.rs
pub fn write_bars_csv(bars: &[Bar], path: &Path) -> Result<()>;
```
`Bar` 复用 `data/bar.rs`。错误用 crate `Error`（解析/数据问题 → `Error::Data`；reqwest → `Error::Eval` 或 `Error::Data`，统一用 `Error::Data("sina ...")` 即可）。

## 7. 错误处理

| 情况 | 处理 |
|---|---|
| HTTP 非 2xx / 网络错 | 重试至 `max_retries`（2）→ 仍失败 `Error::Data("sina http ...")` |
| 响应非合法 JSON | `Error::Data("sina bad json ...")` |
| 空数组（错误 symbol / 无数据）| `Error::Data("sina returned no bars for <symbol>")` |
| 某行字段无法 parse（time/float）| `Error::Data` 带行内容 |
| 写文件失败 | 经 `?` 冒泡为 `Error::Io` |
fetch 失败即非零退出、清晰报错，用户重试。

## 8. 测试

- **单元（无网络）**：
  - `parse_sina_klines`：喂样例 JSON（字符串字段、含乱序两行）→ 断言条数、数值、**输出升序**；喂 `[]` → 返回空 `Vec`（或按 §7 由调用方判空——见下）；喂坏 JSON → Err。
  - `write_bars_csv` → `read_bars_csv` **往返**：写若干 Bar 再读回，断言相等。
- **空判定位置**：`parse_sina_klines` 解析 `[]` 返回空 `Vec`（不报错，纯解析）；"空→报错"的判定放在 `fetch_sina_klines`（业务层）。
- **网络**：`fetch_sina_klines` 不进 CI；README 文档化手动 smoke（真实 symbol）。
- **无 e2e**（fetch 是网络操作）。

## 9. 风险与诚实说明

1. **浅历史**：新浪只给最近 ~1023 根；长回测会被数据量卡住——M6 不解决，文档说明。
2. **端点可能变**：新浪非官方 API 可能改路径/字段；故 `--base-url` 可配、解析逻辑独立可测；真坏了改 base_url 或 parse 即可。
3. **时间格式假设**：假定 intraday `day` 含 `HH:MM:SS`；日线（scale=240）为日期，M6 不专门支持（如需，后续在 parse 里兼容 date-only）。
4. **限频**：高频拉取可能被新浪限；重试 + 用户自控频率。
5. **复权**：拉原始数据，未做前/后复权。

## 10. 里程碑（实现顺序）

- **M6.1** `data/sina.rs`：`parse_sina_klines`（解析 + 升序）—— 纯函数 TDD。
- **M6.2** `data/reader.rs`：`write_bars_csv` —— 写→读往返 TDD。
- **M6.3** `data/sina.rs`：`fetch_sina_klines`（async + 重试）—— 边界单测（解析复用 M6.1），网络手动。
- **M6.4** `cli`：`Fetch` 子命令 + README 一节（用法 + 手动 smoke）。
