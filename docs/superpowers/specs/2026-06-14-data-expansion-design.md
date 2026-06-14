# 数据扩展（深历史）· 设计文档

- 日期：2026-06-14
- 状态：设计已与用户逐节确认，待审阅 → writing-plans
- 范围：为同 10 标的获取尽可能深的日线 qfq 历史 + 质量校验 + 诚实覆盖文档。仅数据获取与校验，不含策略重跑。

## 1. 背景与目标

纯量化决策树弧线证伪 3/4 棵树，头号根因是数据局限：**~550 日 bar × 10 标的太短**（触发树 WFO 折内信号过稀、IS 区分不出参数）且**纯牛市样本**（2024Q4-2025 上行段，2025-11 后一律击穿）。要让后续策略迭代站得住，先扩数据：更深的日线历史 + 覆盖一段系统性熊市。

**目标**：同 10 标的拉尽可能深的日线 qfq 历史，验证质量，诚实记录实际覆盖（含哪些 regime）——为后续"用 `rquant eval` 跑一轮"提供经熊市检验的数据底座。

## 2. 已确认决策（brainstorming 逐条敲定）

| # | 决策 | 选择 |
|---|---|---|
| Q1 | 扩展侧重 | **深度优先（regime 覆盖）**，universe 保持 ~10 |
| Q2 | 深度目标 | **探到 Tencent 源上限、诚实记录实际覆盖**（目标至少含 2022 回调，能到 2018 熊/2020 暴跌更好） |
| 方案 | 落地形态 | **A1：复用现有 `fetch` + 可复现批脚本 + 校验 + 覆盖报告**（不为一次性拉取建拉取子命令） |

**关键数据源约束（探查确认）**：
- Sina 分钟 API 硬上限 1023 根 → 60m ≈ 1 年、15m ≈ 64 天，**分钟无法做深历史**。
- 日线走 Tencent fqkline（qfq），`fetch --datalen N` **直通 Tencent、无 1023 硬卡**（`.min(1023)` 仅在分钟合成分支）→ 深度零改 fetch 代码，靠大 `--datalen` 实现，实际上限需实测。

## 3. 范围、产出、边界

**产出**：
1. `data/<symbol>.csv`——10 标的深日线 qfq（新目录，与 live 的 `paper/` 隔离）。
2. 可复现批脚本 `data/fetch_deep.cmd`。
3. 校验：`src/data/quality.rs` 纯函数 + `rquant validate-data` 薄 CLI 壳。
4. 覆盖报告 `docs/superpowers/2026-06-14-data-expansion-coverage.md`。

**边界（非目标，诚实声明）**：
- **仅日线**——分钟被 Sina 卡 ~1 年；两棵 60m 执行树（v4，唯一有严格 OS 证据者）拿不到深历史，本轮不碰。
- **同 10 标的**——sh600030/sh600036/sh600276/sh600519/sh600900/sh601088/sh601318/sz000333/sz000858/sz300750，与上一弧线可比，不扩 universe。
- **不在本任务重跑策略**——数据获取+校验为限；用 4 棵树 / eval 跑深数据是明确的下一阶段。
- **幸存者偏差接受 + 文档化**——Sina/Tencent 拿不到 A 股历史时点成分；同 10 大盘股多数 2018 前已上市，偏差较小但须声明。
- `data/*.csv` 大且脚本可复现 → **gitignore**；只提交脚本 + 校验码 + 覆盖报告（沿用 paper/ 产物 gitignore 纪律）。

## 4. 深度探测 + 批量拉取

### 4.1 深度探测
对一只老股（sh601398 工行 或 sh600519 茅台，2018 前上市）跑：
```
rquant fetch --symbol sh600519 --scale 240 --datalen 5000 --adjust qfq --out tmps/probe_sh600519.csv
```
看 Tencent fqkline 实际返回 bar 数 + 最早日期，定出可达深度 `D`。深度零改 fetch 代码。

### 4.2 批量拉取
`data/fetch_deep.cmd`（仿 `deploy/paper_run.cmd` ASCII 安全风格），对 10 标的各跑：
```
rquant fetch --symbol <s> --scale 240 --datalen <D> --adjust qfq --out data/<s>.csv
```
幂等（重跑覆盖）。脚本头部注释记录抓取日期与探测出的 `D`。

### 4.3 qfq 锚定纪律
qfq 锚定最新价 → **旧 `paper/pd_*.csv` 与新 `data/*.csv` 跨抓取日不可混用**；新 data/ 集内部一致（同日抓取）。覆盖报告记录抓取日期。

## 5. 校验（`src/data/quality.rs` 纯函数 + `validate-data` 薄壳）

### 5.1 纯函数
```rust
pub struct QualityReport {
    pub n_bars: usize,
    pub first: NaiveDateTime,
    pub last: NaiveDateTime,
    pub strictly_increasing: bool,
    pub max_abs_daily_return: f64,
    pub suspicious_jumps: Vec<(NaiveDateTime, f64)>,
    pub calendar_gaps: usize,
}
pub fn analyze(bars: &[Bar], jump_threshold: f64) -> QualityReport;
```
校验项：
1. **时间严格递增**（无重复、无逆序）。
2. **粗跳空**：`|日收益 close_t/close_{t-1} − 1| > jump_threshold`（默认 0.21，超主板 ±10% / 创业板 ±20% 之外即可疑——抓数据损坏或未复权残留；sz300750 为创业板 ±20% 注明）→ 收入 `suspicious_jumps`。
3. **缺口**：复用既有 AShareCalendar 缺口检测，统计意外缺交易日数（停牌 OK 但计数）。
4. **覆盖**：first/last 日期、n_bars。

### 5.2 薄 CLI
`rquant validate-data --csv <path>... [--jump 0.21]`：逐文件加载 → `analyze` → 打印 QualityReport；**任一文件 `strictly_increasing==false` 或 `suspicious_jumps` 非空 → 进程退出非零**（真闸）。

### 5.3 qfq 诚实说明
日线 qfq 由 **Tencent 直接返回已复权**（非本地合成；合成只用于分钟），F-7 已实证 Tencent qfq 消除除息跳空。故本校验是"qfq 序列健全性"（单调/粗跳空/缺口/覆盖），不重做逐除息日复权核对。

### 5.4 单测（合成 bar）
- 注入逆序行 → `strictly_increasing==false`。
- 注入 +30% 跳 → `suspicious_jumps` 含该日。
- 注入缺交易日 → `calendar_gaps` 计数。
- 干净序列 → 全清（jumps 空、单调真、gaps 0）。

## 6. 覆盖报告 + 诚实边界

`docs/superpowers/2026-06-14-data-expansion-coverage.md`：
- **每标的表**：日期范围（first→last）、bar 数、max|日收益|、可疑跳空数、缺口数（由 `validate-data` 输出回填）。
- **regime 标注**：按实际起始日期标覆盖哪些 regime——首日 ≤2018 → 含 2018 全年熊；≤2020-02 → 含 COVID 暴跌；≤2022 → 含 2022 回调。直接回答"是否拿到系统性熊市样本"。
- **诚实边界**：幸存者偏差（同 10 幸存大盘股）、qfq 锚定抓取日（跨日不可混用）、仅日线（分钟浅）、Tencent qfq 预复权（F-7 已验）、data/ 已 gitignore（脚本可复现）、记录抓取日期。

## 7. 改动文件

| 文件 | 改动 |
|---|---|
| `src/data/quality.rs` | 新模块：QualityReport + analyze() 纯函数 + 单测 |
| `src/data/mod.rs`（或 lib 模块表） | 注册 `pub mod quality;` |
| `src/cli/mod.rs` | `Cmd::ValidateData` 薄臂（读 CSV → analyze → 打印 → 退出码） |
| `data/fetch_deep.cmd` | 新建：批量深拉脚本（提交） |
| `.gitignore` | 加 `data/*.csv`（脚本/校验码/报告不忽略） |
| `docs/cli-reference.md` | `validate-data` 子命令文档 |
| `docs/superpowers/2026-06-14-data-expansion-coverage.md` | 覆盖报告（含实测 D + 每标的覆盖 + regime + 诚实边界） |

## 8. 诚实边界小结

- 本任务只做**数据获取 + 质量校验 + 覆盖文档**，不重跑策略（下一阶段）。
- 深历史是**日线专属**——60m 执行树不受益（Sina 分钟 ~1 年硬限）。
- 实际深度取 Tencent 所能给，**可能够不到 2018**——若如此照实记录（覆盖到 2022 回调即已是对纯牛市样本的实质改善）。
- 幸存者偏差不可消除（无历史时点成分数据），只能声明。
