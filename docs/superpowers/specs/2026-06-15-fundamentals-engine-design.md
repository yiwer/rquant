# 基本面进引擎（子项①：A 数据管线 + C 引擎集成）· 设计文档

- 日期：2026-06-15
- 状态：设计已与用户逐节确认，待审阅 → writing-plans
- 范围：大方向"基本面 + 全市场 2000"的**子项①**——把 A 股基本面（point-in-time）引入引擎，并在现有 20 标的上验证基本面因子真有预测力 + 接线正确。**不含** 子项② B（2000 OHLCV 拉取 + universe + 幸存者）、子项③ D（完整基本面×技术方法学）。

## 1. 背景与大方向拆解

四阶段收敛结论已证：纯 OHLCV 技术信号在这组大盘上难稳健跑赢买入持有（见 `2026-06-15-screen-tilt-loop-findings.md`）。用户定方向：**引入基本面 + 全市场扩到 ~2000**。这是 4 子系统大弧线，按依赖构建序拆：

- **①（A+C）基本面进引擎**（本 spec）：Python akshare 管线 → 逐股 point-in-time 财务 CSV；引擎支持逐股读财务、DSL 访问、time≤t 防前视；在 20 上验证。
- **② B 扩到 2000**：全市场 OHLCV 批量拉 + 2000 universe + 幸存者处理。
- **③ D 基本面×技术 选股方法学 + 验证**（在 2000 上）。

每子项各自 spec→plan→实现。本 spec 只覆盖 ①。

## 2. 已确认决策（brainstorming）

| # | 决策 | 选择 |
|---|---|---|
| 数据源 | 基本面从哪来 | **akshare（Python 免费，无 token）**；`ak.stock_yjbb_em(date=季末)` 一次返回全市场 + 公告日 |
| 规模 | 分期 | **全市场 2000 一把上**（不以 20 验证为门控）；但构建按依赖序，① 先打通 |
| 引擎集成 | 逐股基本面怎么进 | **universe 加逐股财务 CSV（第4列）+ 新 DSL 命名空间 `fund.<col>`**（复用 aux 的 time≤t 时点语义，但逐股）|
| 因子集 | 哪些基本面 | roe/np_yoy/rev_yoy/gross_margin/eps/bps（yjbb 直出）；PE/PB 在树里派生（close/eps、close/bps）|
| point-in-time | 前视防护 | 公告日（最新公告日期）为时点锚；首份财报公告前 = NaN 弃权 |
| 重述 | restatement | 用 yjbb 最新值 + 文档声明（roe/eps 重述罕见）|

## 3. 数据管线（A，`scripts/fetch_fundamentals.py` 新建，Python）

**抓取**：循环季度 `2018-03-31 … 2026-03-31`（~33 季，截至今 2026-06-15 Q2 未披露），每季 `ak.stock_yjbb_em(date="YYYYMMDD")` → 全市场 DataFrame（含 `股票代码`、`净资产收益率`、`净利润-同比增长`、`营业总收入-同比增长`、`销售毛利率`、`每股收益`、`每股净资产`、`最新公告日期`）。

**转换**：按 `股票代码` 分组 → **逐股 CSV** `data/fundamentals/<sym>.csv`（gitignored，可复现）：
```
time,roe,np_yoy,rev_yoy,gross_margin,eps,bps
2018-04-27,34.1,39.0,31.0,91.0,8.05,48.5
```
- **单位铁律（防量纲 bug，吸取流动性闸教训）**：`roe/np_yoy/rev_yoy/gross_margin` 按 yjbb 原样存 = **百分数**（如 ROE 34.1 表示 34.1%），`eps/bps` = 元；**树里写 `fund.roe > 15`（15%）非 `> 0.15`**。dsl-reference 显式记此约定。
- **`time` = 最新公告日（非报告期）= point-in-time 锚**；行按公告日升序；每季一行。
- 代码映射：6 位 → 交易所前缀（`60/68/9→sh`、`00/30/2→sz`；其余跳过/记日志）对齐 universe。
- 缺失字段 → 空（引擎侧 NaN 弃权）。
- 全市场一次拉 → 生成 ~5000 只 CSV（① 只用 20 验证，② 复用全量）。

**校验**：`time` 单调升、字段数值合理（roe∈合理区间、非全空）；坏数据告警。

## 4. 引擎集成（C，Rust）

- **`src/data/fundamentals.rs`（新）**：
  - `FundamentalSeries`（按公告日升序的行：`(NaiveDate, BTreeMap<String,f64>)`）。
  - `load_fundamentals_csv(path) -> Result<FundamentalSeries>`（首列 time = `%Y-%m-%d`，其余数值列；空单元 → 该列缺失）。
  - `as_of(&self, t: NaiveDateTime) -> BTreeMap<String,f64>`：取**公告日 ≤ t.date() 的最近一行**；无则空 map（→ DSL 该列 NaN）。
- **`src/data/universe.rs`**：`UniverseEntry` 加 `pub fundamentals: Option<PathBuf>`；CSV 第 4 列可选（`symbol,primary[,context[,fundamentals]]`）；缺列 → None。
- **`src/features/context.rs`**：`build_context(...)` 加 `fundamentals: Option<&FundamentalSeries>` 参；在时点 t 解析 `as_of(t)` → `Context.fundamentals: BTreeMap<String,f64>`（as-of-t 快照标量；None → 空）。所有现有调用点补 `None`（行为冻结）。
- **`src/dsl/{lexer,eval}.rs`**：`fund.<col>` 命名空间——词法接受 `fund.` 两段点标识符（类比 `aux.<name>.<col>`）；eval `fund.roe` → `Context.fundamentals.get("roe")`，有则 Scalar、无则 NaN（弃权语义，比较恒 false）。`fund.*` 是 **as-of-t 标量**（非序列；季频不做滚动，YAGNI）。

**point-in-time 铁律**：`fund.*` 在 t 只能见公告日 ≤ t 的财报；首报公告前 NaN。这是基本面回测有效性的命根——引擎层强制，与 aux time≤t 同闸。

## 5. 在 20 上验证（D-subset，proof）

- `data/universe_20_fund.csv`：在 universe_20 基础上加第 4 列指向 `data/fundamentals/<sym>.csv`。
- 复用 **`rquant factor --universe data/universe_20_fund.csv --factor "roe=fund.roe" --factor "npyoy=fund.np_yoy" --factor "pe=close/fund.eps" …`** → IC/RankIC/分层：**point-in-time 下基本面是否预测前瞻收益**（F-1 门槛 |RankIC|>0.03 ∧ |ICIR|>0.3）。
- **point-in-time 正确性单测（关键回归锁）**：构造一只股财务（公告日 D），断言决策 bar t<D 时 `fund.roe`=NaN（弃权）、t≥D=值；公告日推进取最近——防前视回归。

## 6. 文件

| 文件 | 改动 |
|---|---|
| `scripts/fetch_fundamentals.py` | 新建：yjbb 循环季度 → 全市场逐股 CSV + 代码映射 + 校验 |
| `scripts/requirements.txt` 或文档 | 记 `akshare`（pip 安装说明；引擎不依赖 Python，仅数据管线）|
| `src/data/fundamentals.rs` | 新建：FundamentalSeries + load + as_of + 单测 |
| `src/data/universe.rs` | UniverseEntry 加 fundamentals 第4列 + 解析 + 测试 |
| `src/features/context.rs` | build_context 加 fundamentals 参 + as-of-t 快照；所有调用点补 None |
| `src/dsl/lexer.rs` / `src/dsl/eval.rs` | `fund.<col>` 词法 + 求值（as-of-t 标量 / NaN 弃权）+ 测试 |
| `data/universe_20_fund.csv` | 新建：20 标的 + fundamentals 第4列 |
| `docs/dsl-reference.md` / `docs/cli-reference.md` | `fund.` 节 + universe 第4列说明 |
| 闸 | **`cargo test --workspace` + `cargo clippy --workspace --all-targets`**（UniverseEntry/build_context/DSL 是引擎公共 API/共享类型）+ point-in-time 单测 |

## 7. 诚实边界（非目标）

- 子项① = 基本面进引擎 + 全市场财务 CSV 生成 + **20 验证**；**不**做 2000 OHLCV/universe/幸存者（②）、不做完整方法学（③）。
- **point-in-time 强制**（公告日闸、首报前 NaN）——基本面回测命根。
- **重述** = 最新值 + 声明（罕见）。
- **幸存者偏差**：① 的财务 CSV 含已退市股则部分缓解；正式处理留 ②（届时 universe 须含退市股 + 声明）。
- `build_context` 加参是引擎公共 API 变 → 闸必 `--workspace`（防桥接 crate 漏编译复发）。
- Python akshare 是新数据管线依赖；引擎仍纯 Rust，脚本独立（同既有 data fetch 脚本模式，只是 Python）。
- 引擎现有行为冻结：fundamentals=None 时与改造前逐字一致（所有现有调用点补 None；真数据冻结闸可加）。
