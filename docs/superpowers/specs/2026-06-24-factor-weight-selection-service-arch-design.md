# 因子-权重选股模块 · 服务化架构设计

> 状态:**已设计,待评审** · 日期:2026-06-24 · 范围:把"数据采集 → 训练 → 产品赋能"打通,聚焦**因子-权重选股模块**

## 1. 背景与目标

现状:`rquant` 的因子-权重选股链是**一堆 Python 脚本经磁盘 CSV/JSON 互通的批处理管线** + 一个 Tauri 桌面端**直接读文件**。数据采集挂在 **Windows 计划任务**(`fetch_guard.ps1` + `fetch_watchdog.py`,见运行记录)上跨会话自愈。这套对"个人单机研究"够用,但要演进为**可部署、多用户的产品**,有三个根本障碍:

1. **隐式文件契约**:服务边界靠"谁读哪个 CSV"约定,无版本、无血缘、无 schema 校验。
2. **共享重资产与按租户轻资产纠缠**:数据/因子面板(共享)与策略配置/权重/选股(按用户)混在同一批脚本里。
3. **计算与产品耦合**:桌面端读文件 = 单用户、与产物文件布局强绑定;无对外接口。
4. **采集依赖 OS 调度**:Windows 计划任务平台绑定、交互态、不可容器化。

**三条已确认的产品包络**(经逐条澄清):
- **轨道**:演进为**可部署产品 / 多用户**(真·服务拆分)。
- **产品形态**:**混合**——共享数据 + 策展因子库,用户在你的因子集上**选因子/调参**(top_n、成本、迟滞、因子子集、模型类型)得定制权重与选股;含**策略市场**(发布/fork)。
- **部署/规模**:**单机自托管 · 运维从简**——模块化多服务但同址(容器组),中小规模,异步训练 + 结果缓存,先不上编排集群。

**接缝策略(已选)**:**方案 A——契约优先的模块化单体 + 薄 API**。逻辑服务(模块 + 显式接口 + schema + 注册表),打包成少数进程/容器;边界是真的,契约到位后可无痛把某上下文升级为独立网络服务。

**成功标准**:
- MVP 经新脊柱产出的 as-of 选股,与现有 `paper_ridge.py --asof` **逐票/同分一致**(golden parity)。
- 数据采集**脱离 Windows 计划任务**,成为跨平台、容器原生、自愈的统一中心。
- 新增一个策略(不同因子子集/参数)→ 训练 → 验证 → 服务,**全程不碰冻结的现有策略、不中断每日选股**。
- 任一选股可经"模型版本 →(策略版本 + 面板版本 + 数据版本)"完整复现(项目第 1 约束:复现性第一)。

## 2. 范围

**In**:因子-权重选股模块的数据采集、因子计算、参数化训练、模型注册、验证闸、as-of 选股/前向纸面册、对外 API、编排调度。

**Out(本 spec 不含)**:模糊决策树引擎(Rust `rquant` 的 tree/DSL/LLM 节点)本身的重构——它作为**被 S5 验证闸调用的回测内核**保留;桌面端其他研究页(Backtest/Screen/Audit/Verdict 等)的产品化(后续各自 spec);实盘下单通路。

## 3. 架构总览

八个限界上下文(逻辑服务),按"数据 → 因子 → 训练 → 选股 → 产品"分层;**共享存储**与**编排**横贯;打包成少数容器。

```
                          ┌───────────── 客户端(桌面App/未来Web)─────────────┐
                          │                      ↑ HTTP                        │
  共享存储 ┊  ┌───────────────────────── S7 API 网关/BFF(鉴权·多租户)─────────────────────┐ ┊  应用编排器 S8
  Postgres ┊  │  S6 选股/信号(as-of选股·前向纸面册/NAV)  ←读 模型注册表 + 因子as-of      │ ┊ (in-process,跨平台)
  +对象存储┊  │  S3 策略/市场   S4 训练+模型注册表   S5 验证闸(内嵌Rust引擎)            │ ┊  由"数据就绪@D"
  (血缘·版本)┊ │  S2 因子(注册表·面板缓存)                                              │ ┊  事件触发下游DAG
           ┊  │  S1 数据采集中心(源适配器·内置调度·采集作业·多源回退·新鲜度)          │ ┊
           ┊  └──────────────────────────────────────────────────────────────────────┘ ┊
```

**打包(单机自托管,容器组):**
- **API 容器**(FastAPI)= S7 网关 + S6 选股读路径(延迟敏感)。
- **Worker 容器** = S2 因子重算 + S4 训练 + S5 验证(异步作业,先用 **DB 作业表**当队列)。
- **数据采集中心容器**(S1)= 独立服务,自带调度 + 看门狗(详见 §6);网络重、自愈,隔离运行。
- **Postgres**(元数据 + S3/S4/数据三类注册表)+ **对象存储**(大面板/产物,先本地目录,将来 MinIO)。
- Rust `rquant` 引擎由 S5 以**子进程/库**内嵌;现有 `test_*.py` 纯函数核被各服务复用。

**技术选型**:Python(FastAPI + 现有 pandas/numpy/sklearn)承载 S1–S6;保留 Rust `rquant` 藏在 S5 后;Postgres 元数据;对象存储先本地;Docker Compose 编排容器。**迁移以"重组 + 加契约 + 加 API"为主,几乎不重写。**

## 4. 服务详述

| | 服务 | 职责 | 现有资产归位 |
|---|---|---|---|
| **S1** | 数据采集中心 | 见 §6:源适配器注册表 + 内置调度 + 采集作业 + 多源回退 + 新鲜度;写规范 PIT 库 | `fetch_*.py`、`fetch_guard.ps1`、`fetch_watchdog.py`、新浪补数还原 |
| **S2** | 因子 | **因子注册表**:71 因子各自登记 `{name,fn,lookback,deps,category,pit_lag}`,从巨型 `compute_symbol_factors` 解耦;按**任意因子子集**算/缓存面板(全史训练 + as-of 选股) | `build_factor_matrix.py`、`build_sector_factors`、`build_pa_features` |
| **S3** | 策略/市场 | 策略=版本化配置对象(§5);CRUD + 版本 + 归属 + 发布/fork——混合形态的心脏 | 散在 `train_*`/`eval_*` 的硬编码超参 |
| **S4** | 训练 | 参数化训练器(多后端 ridge/nonlinear/gbdt):`train(策略版本, 面板)→模型版本`;异步作业;**模型注册表**(权重 + 元数据 + 血缘) | `train_ridge/nonlinear/gbdt/dropout_ensemble`、`*_weights.json` |
| **S5** | 验证闸 | 诚实闸门服务化:WFO/成本压力/容量/IC正交/placebo → `Verdict`;发布前必过;**内嵌 Rust 引擎**做树/组合回测 | `eval_ridge/blend/factor_orthogonal`、`daily_eval`、Rust `portfolio/signal/screen` |
| **S6** | 选股/信号 | 冻结模型套 as-of 截面 → top-N + Trace;**按策略/按用户**前向纸面册 + NAV;延迟敏感读路径 | `paper_ridge.py`(asof+journal)、`eval_blend --json` |
| **S7** | API 网关/BFF | REST 覆盖 S1–S6 + 鉴权 + 多租户 + 计费钩子;桌面端从读文件改调它 | 桌面 `paths.rs` + `dto_*.rs` + 各页(DTO 复用,改 HTTP) |
| **S8** | 应用编排器 | **in-process、跨平台**的跨服务 DAG:数据就绪 → 因子重建 → 到期重训 → 验证 → 发布;**非 OS 调度** | 现 Windows 计划任务的"跨步编排"职责(采集自身节奏归 S1) |

**设计原则**:每个服务可独立理解/测试,经 schema 接口通信;纯函数核与 I/O 边界分离(沿用现有 `test_*.py` 文化)。

## 5. 数据流与契约

**两条流,只在「模型注册表」交汇:**

**① 构建/训练流(写 · 异步 · Worker)**
```
策略 S3 → 训练 S4 →(读因子面板 S2)→ 模型注册表[frozen 权重+血缘]
                                       → 验证闸 S5 → Verdict.pass → status=published
```
**② 服务/选股流(读 · 同步 · API)**
```
客户端 → API S7(鉴权)→ 选股 S6 →(读 模型注册表取冻结模型 + 因子 as-of 截面 S2)
        → 打分→top-N + Trace → 前向纸面册/NAV → 返回
```
**唯一交汇点 = 模型注册表**(①写、②读)。训练慢/异步/可重算;选股快/只读冻结产物。这是"训练 ↔ 产品"解耦的关键一刀。

**每日/每周编排(S8,由 S1 的"数据就绪@D"事件驱动):**

| 时刻 | 步骤 | 服务 | 备注 |
|---|---|---|---|
| 收盘后傍晚 | 数据刷新到 D | S1 | baostock EOD;未出则新浪补当日(qfq 锚定,provisional),次日对账回填 |
| 数据就绪@D | 因子面板重建到 D | S2 | 全史(训练)+ as-of D 截面(选股) |
| 调仓日(周三) | 到期策略重训 | S4 | 仅未冻结/到期者;冻结策略跳过(honest-forward) |
| 重训后 | 验证闸 | S5 | 新模型版本必过 gauntlet 才 published |
| 调仓日收盘后 | 发布选股 | S6 | 推进各策略前向纸面册 → 客户端次日早盘可取 |

**核心契约(完整字段在实现期细化):**

策略配置(S3):
```json
{ "strategy_id":"uuid", "version":7, "owner":"user_id", "visibility":"private|published",
  "factor_subset":["f_bm","f_mom20","..."],
  "normalization":"gauss|rank|winz",
  "model":{"type":"ridge|nonlinear|gbdt|blend","ridge_a":0.1},
  "selection":{"top_n":3,"cost_bps":20,"hysteresis_delta":0.05,"rebalance":5,"horizon":1},
  "universe":{"membership":"top2000","eligibility":["non_st","roe>0","bm>0","logamt>=floor"]} }
```
模型注册表(S4,用 status + 不可变 `train_hi` **结构性强制**冻结契约):
```
models(model_id PK, strategy_id, strategy_version,
       status[training→frozen→validated→published],
       train_lo, train_hi, factor_cols_hash, weights_ref→对象存储,
       data_version, panel_version, metrics_json, verdict_json, created_at)
```
核心服务接口(REST 与内部函数同构):
```
S1  refresh(source, data_type, date_range) ;  freshness() -> {(source,data_type): latest_date}
S2  asof_cross_section(universe, date, factor_set) -> matrix
    build_panel(universe, date_range, factor_set) -> panel_ref
S4  submit_training(strategy_version) -> job_id ;  get_model(model_id)
S5  validate(model_id, suite) -> verdict
S6  asof_pick(model_id, date, {hysteresis}) -> {picks[], scores, trace}
    advance_journal(strategy_id) ;  get_nav(strategy_id)
```
**血缘链**:选股 → 模型版本 →(策略版本 + 面板版本 + 数据版本),全链可复现。

## 6. 数据采集中心(统一,取代 Windows 计划任务)

**动机**:采集不应依赖 OS 级调度(平台绑定、交互态、不可容器化)。建立**单一、跨平台、容器原生、自愈**的采集中心,统管所有源。

**组成:**
- **源适配器注册表**:每个源(baostock / 新浪 / 掘金 gm / akshare / 腾讯 / 雪球)登记在统一接口后:
  ```
  Source.capabilities() -> {data_types, latency_class, history_depth, adjust}
  Source.fetch(data_type, scope, date_range) -> records
  ```
- **内置调度器**(APScheduler,**应用内 cron,跨平台**):定义各源采集节奏(如 baostock 日线@傍晚、新浪当日补@收盘后、掘金尾盘@14:46、margin@每日)。**取代 `fetch_guard.ps1` + Windows 计划任务**。
- **采集作业表**:`fetch_jobs(job_id, source, data_type, scope, date_range, status, attempts, heartbeat, rows_written, error, lineage)`——**断点续传、幂等、看门狗**(停滞/崩溃检测 → kill+resume)。**取代 `fetch_watchdog.py` + PID 锁文件**(改用作业表 status + 心跳)。
- **多源回退策略(声明式)**:把今天的手工动作变成规则。例:`data_type=daily_eod, date=D` → 主源 baostock;若 deadline 前未出 → 新浪 qfq 补当日(provisional);baostock 落地后**自动对账回填**(替换 provisional、去重)。沿用 qfq 锚定使 raw 拼接一致的事实。
- **新鲜度监控 + SLA**:跟踪每 `(source,data_type)` 最新日期,暴露 `freshness()`;下游选股前**前置校验数据到 D**,缺失则**结构化报错而非静默出旧票**。
- **写规范 PIT 库**:去重 `(symbol,time)`、blob 血缘(source/job/version)、PIT 纪律;**不污染 qfq 历史**(provisional 与 reconcile 分离)。

**部署**:独立容器,自调度自愈,无需任何外部调度器;"数据就绪@D" 经 Postgres 事件/通知触发 S8 下游 DAG。

## 7. 迁移路径与 MVP

**三条铁律**:① 绞杀者模式(脚本先包接口再换内壳,新老并存);② 绝不中断每日选股(文件路径活到 API 路径逐票一致);③ 采纳而非重训(`paper_ridge_weights.json` 原样导入为第一个 frozen 模型,保纸面册连续)。

**分阶段:**

| 阶段 | 交付 | 涉及 |
|---|---|---|
| **0 基座** | Compose 起 Postgres + 对象存储;元数据/血缘骨架;开始版本化(与文件并存) | 存储/目录 |
| **1 MVP·选股读路径** | 现有冻结 ridge 经新脊柱出 as-of 票 | S1/S2/S6/S7 + 注册表 |
| **2 构建 + 采集中心 + 编排** | 数据采集中心(脱离 Windows 计划任务)+ 参数化训练作业 + 验证闸 + S8 DAG | S1 全/S4/S5/S8 |
| **3 多租户 + 市场** | 鉴权/租户、策略发布/fork、Web 端、按用户纸面册/计费 | S3 全/S7 全 |
| **4 按需扩缩** | 把热点上下文(因子/训练)升级为独立容器服务 | 局部 A→B |

**MVP(阶段 1)精确定义**——最薄端到端竖切,只证明架构、不造新 alpha:
- **范围**:单用户、单策略(现有 ridge)、**只做流②(选股读路径)**。
- **动作**:① 导入冻结权重为第一个 frozen 模型;② 现有 ridge 配置登记为一条 strategy;③ `build_factor_matrix --asof` 包成 `S2.asof_cross_section`;④ `paper_ridge.py --asof` 包成 `S6.asof_pick`;⑤ 当日补数(新浪)+ baostock 读取包成 `S1` 最小读路径;⑥ 端点 `GET /strategies/{id}/picks?asof=D`;⑦ 桌面 PaperRidge 页改调 API。
- **出口标准**:`GET picks?asof=2026-06-24` 与 `paper_ridge.py --asof 2026-06-24` **逐票/同分一致**。
- **不做(YAGNI)**:鉴权/多租户、市场、Web、K8s、消息队列、nonlinear/gbdt 训练、多因子集、自动重训。**采集中心的完整调度/看门狗/多源回退留到阶段 2**:MVP 不触碰采集,现有采集方式临时原样保留(MVP 只读已落地数据);**目标态(阶段 2)由数据采集中心彻底取代 Windows 计划任务**,二者不矛盾——MVP 阶段只是尚未动到这块。

**现有资产 → 服务模块(基本是重组):**

| 现有 | → 模块 |
|---|---|
| `fetch_baostock_update`/`fetch_index`/新浪补数/`fetch_guard.ps1`/`fetch_watchdog.py` | S1 数据采集中心(适配器 + 调度 + 作业 + 回退) |
| `build_factor_matrix`(+sector/pa) | S2 因子注册表 + 构建器 |
| `train_ridge/nonlinear/gbdt` | S4 训练器(多后端) |
| `eval_ridge/blend/factor_orthogonal`、Rust `portfolio/signal` | S5 验证闸 |
| `paper_ridge`(asof+journal)、`eval_blend --json` | S6 选股/信号 |
| 桌面 `paths.rs` + `dto_*` + PaperRidge 页 | S7 客户端(DTO 复用,改 HTTP) |

## 8. 验证、错误处理、可观测性

- **黄金平价测试**:新路径产物 == 老脚本产物(同日同票/同权重/同 NAV)才 cutover——延续项目 golden-invariant 文化。各服务保留纯函数核,现有 `test_*.py` 直接复用。
- **数据新鲜度守卫**:选股前 `freshness()` 校验数据到 D;缺失 → 结构化报错。
- **优雅降级有痕**:缺 turn/sector → rank 中性化(沿用现有),但**记进 Trace 元数据**(标注降级因子),不静默。
- **作业幂等**:采集/训练/验证走作业表(status + 重试 + 心跳 + 幂等),`signal` 幂等重放已有先例。
- **血缘即可观测性**:每产物带 data/panel/model 版本,问题可溯源。

## 9. 风险与取舍

- **逻辑边界靠纪律**(方案 A 固有):一仓库需守边界,以接口 + schema + 注册表强约束;契约到位后可升级为 B(独立网络服务)。
- **大面板序列化成本**:对象存储用 `panel_ref`(引用而非内联传输),避免 API 搬运 300MB CSV。
- **Python/Rust 双栈**:刻意保留——ML 侧 Python 投资巨大且正确,回测内核 Rust 已验证;以 S5 子进程边界隔离,不强行统一。
- **YAGNI**:多租户/市场/Web/K8s/消息队列**全部后置**;MVP 单用户单策略只读。

## 10. 开放问题(实现期定)

- 对象存储:本地目录起步,何时上 MinIO?
- 作业队列:DB 作业表够用到多大规模?何时换轻量队列(如 Redis/RQ)?
- 鉴权方案(阶段 3):自建 vs 托管(影响多租户数据隔离粒度)。
- 因子注册表的 `fn` 表达:纯 Python 函数 vs 可序列化 DSL(后者利于用户自定义因子,属阶段 3+)。
