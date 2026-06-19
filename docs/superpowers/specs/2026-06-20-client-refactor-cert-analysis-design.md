# 客户端重构 — 认证 & 分析 设计 (子项 2a/3,即 sub-2 第一刀)

> 状态:已 brainstorm 定稿,待 writing-plans。日期:2026-06-20。
> 前序:sub-1「选股 & 迭代研究台」已合入 master(`a474608` + 打磨 `67d46cb`)。本子项复用其全部桌面范式。

## 0. 背景与范围

sub-2「认证 & 分析」原捆 5 个面(eval / factor / optimize / portfolio / 三分析器),对单一 spec 偏大。按"认证&分析"语义切片:

- **2a(本文档)= 认证 & 分析**:eval 认证视图 + factor 因子分析 + 三分析器(行业归因/两腿/部署加固)。
- **2b(后续)= 做实时序占位**:optimize(WFO 网格)+ portfolio(组合回测)。

**关键决策(brainstorm 定论)**:
| 决策 | 结论 |
|---|---|
| eval 输入 | 消费**已有 optimize 报告 JSON**(2a 不生成;optimize 页在 2b)。直调 `rquant::verdict::certify`。 |
| 分析器实现 | **Rust 端口**(像 sub-1 的 `index_relative`):纯算术、无 Python 依赖、可交互、与 `analyze_*.py` 数值对拍。 |
| IA | **认证**=新顶层页;**因子工作台**=填实 `/factor` 占位;**分析器**=挂在 sub-1 的 `ScreenBacktestResult`(三者都跑在 screen run 上)。 |

**纪律(沿用 sub-1)**:新域代码进新文件,现有不动;直调库用 `tokio block_on`;**全中文**(保留 PB/PE/ROE/夏普/IC 等专业术语);命令同步壳 + 重计算走 `TaskRegistry`;英文 commit;`git add` 显式列文件;收尾 `--workspace` + ui 闸。

## 1. 信息架构

侧栏在 sub-1 基础上:`驾驶舱 / 回测中心 / 数据工作台 / 选股 / 研究 / 认证(新) / 因子工作台(占位→做实)`(其余占位 调参WFO/组合/档案 留 2b/后续)。

## 2. 页面设计

### 2.1 因子工作台(填实 `/factor`)
- **左栏**:universe 选择(默认 `data/baostock/universe_baostock_day.csv`)· 因子表达式列表(可增删,DSL 如 `fund.bps`、`1/(1+fund.pb)`、`sma(close,20)`)· horizon · 分层 Q · 采样间隔 · 可选 membership · `运行分析`。
- **主区**:每因子一行的 IC 表(IC 均值/ICIR/RankIC 均值/RankICIR/IC t 值/正 IC 占比)+ 选中因子的 **IC 衰减曲线** + **分层收益**(Q1..Q5 + 单调性 + 价差/价差夏普)+ **因子相关阵**(多因子时)。
- 直调 `rquant::factor::run_factor`(任务跑,结果 `FactorReportDto`)。

### 2.2 认证(新顶层页)
- **左栏**:可选 optimize 报告列表(`eval_list_reports` 扫 `.daily_runs/` + 仓库根可解析为 OptimizeReport 的 `*.json`)· 多选 1+ · 策略名 · `运行认证`。
- **主区**:**Verdict 矩阵** —— 顶部 `已认证 ✓ / 未通过` 总章 + n_symbols;5 门槛逐条表(门槛名 / 状态 ✓·✗·不定 / 值 / 阈值 / 说明);失败门槛高亮。
- 直调 `rquant::verdict::certify(&[(name, OptimizeReport)], strategy, &GateThresholds::default())`(读所选 JSON 反序列化为 `OptimizeReport`)。**不重判、不改门槛**;阈值用引擎默认。

### 2.3 选股回测结果 → 「分析」tab 组(扩 `ScreenBacktestResult`)
三者都对一个已归档 screen run 后验(纯算术,Rust 端口,数值对拍 `analyze_*.py`):
- **行业归因**:把 vs 指数的超额拆成 **配置效应(持有便宜板块)/ 选择效应(板块内选股)**。读 run holdings + `data/baostock/sector_membership.csv`(symbol→行业)+ `data/baostock/sector/<行业>.csv`(板块等权日线)。输出累计 r_p / r_alloc / r_bench + 配置%/选择% 拆分。对拍 `analyze_sector.py`。
- **两腿**:再选一个成长 run(value_run × growth_run)+ **w 滑杆**;按 w 在 nav 段层混合(`br=w·v+(1−w)·g`,每调仓再平衡),扫 w∈{1,.8,.7,.6,.5,.4,.3,0} 出表(净总/超额/样本外超额/年化夏普/最大回撤)+ Sharpe+OOS 均衡最优 w;滑杆即时重算(复用 `index_relative` 算超额)。对拍 `analyze_twoleg.py`。
- **部署加固**:**T+1 执行**(决策 close[T] vs 滞后 1 bar 成交)的超额拖累(lag0 vs lag1)+ **容量**(持仓名 ADV 中位,来自 kday `amount` 列,按 %ADV → 最大 AUM 表)。读 run holdings + `data/baostock/kday/<sym>.csv` amount。对拍 `analyze_deploy.py`。

## 3. 桥架构 & 命令

**新文件**(`desktop/src-tauri/src/`):`factor_cmds.rs`、`eval_cmds.rs`、`analyze.rs`(三分析器纯算术 + 单测)、`dto_factor.rs`、`dto_eval.rs`、`dto_analyze.rs`。复用 `paths.rs`(加 `daily_runs_dir`/`sector_dir`/`sector_membership` 路径)、`screen_runs.rs`(读 screen run)、`index_relative.rs`(两腿超额)、`TaskRegistry`。

**命令**:
- `factor_run(universe, factors: Vec<(name,expr)>, horizon, layers, sample, membership?) -> String(task)` → `FactorReportDto`。
- `eval_list_reports() -> Vec<OptimizeReportInfoDto>`(扫候选 JSON,解析成功才列,带 n_combos/folds/错误)。
- `eval_certify(paths: Vec<String>, name: String) -> Result<VerdictDto,String>`(同步;读 JSON→OptimizeReport→certify)。
- `analyze_sector(run_id) -> Result<SectorAttribDto,String>`。
- `analyze_twoleg(value_run_id, growth_run_id, w: f64) -> Result<TwoLegDto,String>`(w 用于"当前选中"高亮;表始终全 w 谱)。
- `analyze_deploy(run_id) -> Result<DeployDto,String>`。

factor 直调 `rquant::factor::run_factor`(确认 async 与否,按签名 block_on 或直调);eval 直调 `rquant::verdict::certify`;分析器纯 Rust。

## 4. DTO(新增,`#[derive(Debug,Clone,Serialize,TS)] #[ts(export)]`)

- `FactorStatsDto { name, expr, ic_mean, ic_std, icir, ic_t, ic_pos_share, rank_ic_mean, rank_icir, ic_decay: Vec<DecayPointDto>, layers: Option<LayerStatsDto> }`;`DecayPointDto { horizon: i32, rank_ic: f64 }`;`LayerStatsDto { q: i32, ann_returns: Vec<f64>, spread_total: f64, spread_sharpe: f64, monotonicity: f64 }`。
- `CorrDto { names: Vec<String>, values: Vec<Vec<Option<f64>>> }`。
- `FactorReportDto { n_symbols, sample: i32, horizon: i32, layers_q: i32, factors: Vec<FactorStatsDto>, corr: Option<CorrDto> }`。
- `GateOutcomeDto { gate: String, status: String, value: f64, threshold: f64, note: String }`。
- `VerdictDto { strategy, n_symbols, certified: bool, gates: Vec<GateOutcomeDto>, failed_gates: Vec<String> }`。
- `OptimizeReportInfoDto { path: String, name: Option<String>, n_combos: Option<i32>, folds: Option<i32>, error: Option<String> }`。
- `SectorAttribDto { excess_total: f64, alloc_pct: f64, select_pct: f64, cum: Vec<SectorCumDto> }`;`SectorCumDto { t: String, r_p: f64, r_alloc: f64, r_bench: f64 }`。
- `TwoLegCellDto { w: f64, net_total: f64, excess: f64, oos_excess: Option<f64>, sharpe: f64, max_dd: f64 }`;`TwoLegDto { rows: Vec<TwoLegCellDto>, best_w: f64 }`。
- `DeployDto { lag0_excess: f64, lag1_excess: f64, drag: f64, adv_median: f64, capacity: Vec<CapacityRowDto> }`;`CapacityRowDto { adv_pct: f64, max_aum: f64 }`。

(DTO 子结构需被命令端 `serde_json::from_value` 或手工映射;factor/verdict 直接由库结构体转 DTO。)

## 5. 数据流

- factor:UI→`factor_run`→task 调 `run_factor`→`FactorReportDto`(经 task result)。
- eval:UI→`eval_list_reports`(列候选)→选→`eval_certify`→`VerdictDto`。
- 分析器:UI(在选股回测结果选中一个 run)→`analyze_{sector,twoleg,deploy}`→各 DTO;两腿 w 滑杆变更→重调 `analyze_twoleg`(轻量 Rust 重算)。
- **唯一真源**:认证用引擎 `certify`、默认阈值,不在 GUI 重判;分析器纯后验算术不改任何裁决。

## 6. 错误处理

`errors.ts` 扩:optimize JSON 解析失败(非 OptimizeReport)→"非有效 optimize 报告";factor 表达式非法→"因子表达式无效";分析器缺数据(sector 面板/kday amount/第二个 run)→引导;两腿两 run 调仓时间线不一致→"两腿对齐点太少,需同 universe/区间/调仓"。诚实文化:未通过认证明确呈现;缺数据给引导。

## 7. 测试

- Rust:`analyze.rs` 三分析器纯算术 TDD(固定 fixture,数值对拍 `analyze_*.py` 的口径);`eval_list_reports` 解析筛选;`eval_certify` 用 fixture optimize JSON → 已知 Verdict;factor 命令走 fixture/真数据冒烟。
- 前端:vitest 覆盖 `Factor.tsx`/`Verdict.tsx` 两页 + 新组件(FactorReport/VerdictMatrix/SectorAttrib/TwoLegBlend/DeployHardening),注入 store mock。
- 收尾:`cargo test --workspace` + ui build/vitest + 真数据冒烟(对一个真 screen run 跑三分析器,核对与 `analyze_*.py` 一致;对一个真 optimize JSON 跑 eval 核对 Verdict)。

## 8. 边界(YAGNI)

不含:optimize(WFO 网格)/ portfolio(组合回测)做实(留 2b);eval 在 GUI 内生成 optimize 报告(只消费已有);分析器对非 screen run 的通用化。
