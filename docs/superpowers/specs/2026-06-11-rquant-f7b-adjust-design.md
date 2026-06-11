# rquant：F-7b — 复权数据通路（fetch --adjust qfq）— 设计文档

- **日期**：2026-06-11
- **状态**：设计已确认（写 spec + 计划）
- **关联**：master `723f49f`。成熟度差距分析 F-7a 实测确认新浪数据不复权（工行/神华除息日 −4%~−10% 假跳空），正确性级别修复，优先级在 F-4 之前。

---

## 1. 目标与非目标

### 目标
1. `fetch --adjust qfq|none`（默认 `none` 完全现状）：
   - **scale=240 + qfq**：腾讯 fqkline 前复权日线直取。
   - **scale<240 + qfq**：三源合成——新浪分钟 raw + 腾讯日线 raw + 腾讯日线 qfq → **当日复权因子 = qfq_close/raw_close** → 分钟 OHLC × 因子（volume 不动）。
2. 新 `data/tencent.rs`（URL/解析/重试）与 `data/adjust.rs`（因子计算/应用纯函数）。
3. 黄金验证：除息日跳空在 qfq 序列消失（单测构造 + 真数据 smoke 神华 2025-07-07）。

### 非目标（YAGNI）
- hfq 后复权（qfq 是回测标准）；分红明细表/自算因子；指数（无除权，--adjust 对指数无意义但不禁止——比值恒 1 自然无操作）；增量更新/本地库（F-7 后续）；新浪日线 raw 路径改动（保留，仅 qfq 走腾讯）。

## 2. 已验证事实（2026-06-11 实测）
- 端点 `https://web.ifzq.gtimg.cn/appstock/app/fqkline/get?param={symbol},day,{start},{end},{count},{adjust}` 可用；`adjust=qfq` → 响应键 `qfqday`，`adjust=`（空）→ 键 `day`。
- 行格式 **`[day, open, close, high, low, volume]`**（字符串数组）——⚠️ 与新浪 `open/high/low/close` 字段序不同，解析陷阱必须有测试钉住。
- 神华 2025-07-07 raw 跳空 −5.51% → qfq 序列 +0.00%（平滑确认）。
- 行尾可能附加非字符串元素（信息对象），解析取前 6 个元素、容忍多余。

## 3. 架构

### 3.1 `data/tencent.rs`（新）
```rust
pub fn tencent_fqkline_url(base_url: &str, symbol: &str, start: &str, end: &str, count: u32, adjust: &str) -> String
pub fn parse_tencent_klines(json: &str, symbol: &str, adjust: &str) -> Result<Vec<Bar>>
pub async fn fetch_tencent_daily(http, base_url, symbol, datalen, adjust) -> Result<Vec<Bar>>
```
- 解析：`data.{symbol}.{qfqday|day}`（adjust="qfq"→qfqday，否则 day；两键都查容错）；行经 `serde_json::Value` 取前 6 元素转字符串；date-only → `15:00:00`（与 sina 日线约定一致）；按 time 升序；价格/量 parse 错误 → Error::Data。
- `fetch_tencent_daily`：end=本地今日、start=end−ceil(datalen×1.7) 自然日（覆盖节假日空隙）、count=datalen；重试逻辑 mirror sina（截断重试同款）。
- 默认 base_url `https://web.ifzq.gtimg.cn/appstock/app/fqkline/get`（CLI 不暴露 v1，常量即可）。

### 3.2 `data/adjust.rs`（新，纯函数）
```rust
/// 按日期交集对齐，factor(d) = qfq_close(d) / raw_close(d)。空交集 → Error。
pub fn adjust_factors(raw_daily: &[Bar], qfq_daily: &[Bar]) -> Result<BTreeMap<NaiveDate, f64>>

/// bar.date 查因子；缺日回退最近前值（因子是阶梯函数，前值语义正确）；
/// 早于因子表起点 → Error（拒绝静默错数据）。OHLC×因子，volume 不动。
pub fn apply_factors(bars: &[Bar], factors: &BTreeMap<NaiveDate, f64>) -> Result<Vec<Bar>>
```
因子校验：比值有限且 > 0，否则 Error（防腾讯坏行）。

### 3.3 CLI（fetch 臂）
`--adjust <ADJUST>`（默认 "none"；只接受 "none"/"qfq"，其它 → 错误）：
- none → 现状（新浪，任何 scale）。
- qfq + scale==240 → `fetch_tencent_daily(..., "qfq")` 写 CSV。
- qfq + scale<240 → 新浪分钟 raw（现有路径）+ `fetch_tencent_daily(raw)` + `fetch_tencent_daily(qfq)` → `adjust_factors` → `apply_factors` → 写 CSV；打印一行合成说明（含因子表覆盖天数）。

## 4. 诚实边界（文档必写）
- 前复权锚定最新交易日（因子≈1），历史价非当时真实成交价（高分红长历史可出现极低甚至负价——qfq 的固有性质）；每次重新拉取后历史值会随新除权事件整体重标（缓存的旧 CSV 与新 CSV 不可混用）。
- 腾讯日线 volume 单位与新浪不同（手 vs 股）；引擎内 volume 仅作相对量使用，单位以数据源为准。
- 建议：回测一律 `--adjust qfq`；raw 仅用于盘口对照。

## 5. 测试
- tencent 解析：字段序（open/close/high/low！）、qfqday/day 双键、date-only→15:00、行尾多余元素容忍、坏价格报错、升序。
- adjust 黄金：构造一次除息（raw d1=10/d2=10，qfq d1=9.5/d2=10 → 因子 0.95/1.0）→ 分钟 bar d1 close 10.2 → 9.69；d2 不变；缺日回退前值；早于起点报错；空交集报错。
- 端到端单测：合成"除息日分钟序列"经合成路径后隔夜跳空消失（表达式断言）。
- CLI：--adjust 非法值报错；none 路径回归（既有 e2e 不动）。
- 真数据 smoke（手动）：`fetch sh601088 --scale 240 --adjust qfq` → 2025-07-07 无跳空；`--scale 60 --adjust qfq` → 除权日首 bar 平滑。
- 文档：cli-reference（--adjust + 三源合成说明）、README（诚实边界 + 推荐 qfq）。

## 6. 里程碑
- **T1** `data/tencent.rs` URL/解析 + 测试。
- **T2** `data/adjust.rs` 因子纯函数 + 黄金测试。
- **T3** fetch `--adjust` 编排（日线直取/分钟三源合成）+ fetch_tencent_daily 重试 + CLI 校验。
- **T4** e2e + 文档 + 真数据 smoke。
