# rquant：E3 — 广义数据输入（aux 外部序列）— 设计文档

- **日期**：2026-06-10
- **状态**：设计已确认（写 spec + 计划）
- **关联**：master `9045825`。差距分析 G4：数据面只有 OHLCV×2 周期 + 新闻钩子；指数相对强弱、资金流、基本面无法接入。

---

## 1. 目标与非目标

### 目标
1. CLI `--aux name=path.csv`（可重复）挂载任意外部数值序列；树 DSL 经 `aux.<name>.<column>` 引用。
2. 通用 CSV：首列 `time`（`%Y-%m-%d %H:%M:%S` 或日频 `%Y-%m-%d`→00:00:00，严格递增），其余任意数值列，列名即 DSL 字段。
3. 与 primary 同走 `time ≤ t` 防未来闸门；低频序列经归约（取 last）自然得"最近已知值"；截断为空 → NaN → 比较弃权（与预热语义一致）。
4. **树不含文件路径**（树=策略 IP，文件装配=CLI 运行时，与 --primary/--context 同哲学）。

### 非目标（YAGNI）
- LLM `inputs` 开放 aux 字段（留后续）；aux 数据抓取器；多文件合并/重采样；树内声明 aux 依赖清单。

## 2. 锁定决策

| # | 维度 | 选定 |
|---|---|---|
| 1 | 格式 | 通用列（time + 任意数值列），新读取器 |
| 2 | 挂载 | CLI `--aux name=path`（重复），`BacktestConfig.aux_paths: Vec<(String, PathBuf)>` |
| 3 | 可见性 | 行时间 ≤ t 即可见；公告滞后由用户把行时间写成发布时刻（引擎不猜，文档注明）|
| 4 | 未挂载表引用 | 求值期 Err 冒泡（快速失败提示）；`aux.` 标识符**格式**（恰 3 段非空）在加载期左移校验 |

## 3. 架构

### 3.1 读取器（`src/data/aux_table.rs`，新）
```rust
pub struct AuxTable { pub times: Vec<NaiveDateTime>, pub cols: BTreeMap<String, Vec<f64>> }
pub fn read_aux_csv(path: &Path) -> Result<AuxTable>
```
校验：首列名必须 `time`；其余列名非空、不含 `.`/空白；时间两格式（先试带时分秒，再试日频补 00:00:00）、严格递增；数值 f64 解析失败报错（带行/列定位）。

### 3.2 Context（`src/features/context.rs`）
```rust
pub struct AuxView { pub cols: BTreeMap<String, Vec<f64>> }      // 已截断
pub struct Context { ..., pub aux: BTreeMap<String, AuxView> }
pub fn build_context(primary, context, news, aux: &BTreeMap<String, AuxTable>, t, window) -> Context
```
每表 `partition_point(|x| *x <= t)` 截 times，再按相同长度截各列。涟漪：`Context {}` 字面量（grep；约 5-6 个测试助手 + build_context 本体）补 `aux: BTreeMap::new()`；`build_context` 调用点（runner/soft/各测试）补空表实参。

### 3.3 DSL（`src/dsl/eval.rs` `resolve_series`）
`name.starts_with("aux.")` → split 恰 3 段 → `ctx.aux.get(table)` / `.cols.get(column)` → `Ok(series.clone())`；缺表 → `Error::Eval("aux table 'x' not mounted (use --aux x=path.csv)")`；缺列 → 同风格。空序列照常返回（归约 NaN 弃权）。

### 3.4 loader 左移（`src/tree/loader.rs` `check_no_unknown_idents`）
`aux.` 前缀分支升级：`name.split('.')` 必须恰 3 段且各段非空，否则 `Error::Tree`（表/列存在性留运行时）。

### 3.5 CLI / 编排
- clap：`#[arg(long = "aux", value_name = "NAME=PATH")] aux: Vec<String>`；解析 `name=path`（坏格式/重名 → 错误退出）；`BacktestConfig.aux_paths`（默认空；e2e 字面量涟漪 `aux_paths: vec![]`）。
- `run`/`run_soft`：启动时逐个 `read_aux_csv` → `BTreeMap<String, AuxTable>` → 传 `eval_point*` → `build_context`。

## 4. 测试
- 读取器：多列/日频/混合格式；非递增、坏值、坏列名（含 `.`）、首列非 time → Err。
- 闸门：t 切中 → 截断长度正确；低频表 last = 最近已知值；t 早于首行 → 空。
- DSL：`aux.idx.v > 0` 求值正确；缺表/缺列 Err 文案；空截断 → false 弃权。
- loader：`aux.x`（2 段）/`aux..v`（空段）→ 加载错；`aux.idx.close` 通过。
- e2e：相对强弱树（`close/close[-5] > aux.idx.v/aux.idx.v[-5]`）硬+软全链路。
- 文档：dsl-reference / cli-reference / tree-yaml-schema / README。

## 5. 里程碑
- **T1** `aux_table.rs` + 测试。
- **T2** Context/`build_context` 接 aux + 字面量/调用点涟漪 + 闸门测试。
- **T3** DSL 解析 + loader 格式左移 + 测试。
- **T4** CLI `--aux` + runner/run_soft + Config 涟漪 + e2e + 文档。
