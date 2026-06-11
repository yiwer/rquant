# rquant：F-2 — 参数寻优 WFO（optimize 子命令）— 设计文档

- **日期**：2026-06-12
- **状态**：设计已确认（写 spec + 计划）
- **关联**：master `8b4cb07`。成熟度差距分析 F-2：`params:` 块与 walk-forward 分折均已是入口，缺 optimizer 接环。现 `--folds` 仅"固定树分折看稳定性"，本设计补"滚动 IS 寻优 → OS 验证"的完整 WFO。

---

## 1. 目标与非目标

### 目标
1. `rquant optimize --tree t.yaml --primary --context --grid "name=start:stop:step|v1,v2,…"(重复) --folds K [--sim] [--soft] [--aux] [LLM 三件套] --max-combos 500 --out optimize_report.json`。
2. 锚定扩展 WFO：OS 折 k ∈ 2..=K，IS = 折 1..k−1 网格寻优 → best 组合 → OS 折 k 评估。
3. 防过拟合输出：每折 IS/OS/退化率、参数漂移表、OS 拼接 vs 全样本最优对照、每折 IS top-5。
4. loader 参数覆盖入口（校验链全保留）。

### 非目标（YAGNI）
- 随机/贝叶斯搜索；rolling 定长 IS；deflated Sharpe；HTML（follow-up）；portfolio 口径寻优；多目标。

## 2. 锁定决策
| # | 维度 | 选定 |
|---|---|---|
| 1 | IS 窗口 | 锚定扩展（IS = 折 1..k−1）|
| 2 | 口径 | 打分默认 + `--sim` 切换；目标自动：打分硬=active mean_net、打分软=engaged mean expected_net、sim=Sharpe（跨度<30 天退化 total_return）|
| 3 | 搜索 | 网格（笛卡尔积，CLI 声明序、末位变最快，确定性）；> max-combos 报错 |
| 4 | sim OS 起跑 | 空仓 nav=1 从 OS 段顺序起跑（不带 IS 持仓状态，诚实）|
| 5 | 无效目标 | 无可评估点 → 该组合记 −∞（全 −∞ → 该折 best=None 降级标记，不 panic）|

## 3. 语义约定（权威）
- **grid 语法**：`name=start:stop:step`（闭区间，step>0，start≤stop，浮点容差 `v ≤ stop+1e-9`）或 `name=v1,v2,…`；name 必须存在于树 `params:` 块；`--grid` 重复同名 → 错误；组合总数 = Π各参数取值数。
- **折切分**：可评估决策索引空间（打分 = `warmup..len`；sim = `warmup..len−1`）等分 K 连续折（mirror 现 walk-forward 索引切分；不足 K → Error::Data）。
- **evaluate(tree, data, 索引范围, 模式) → Option<f64>**：
  - 打分硬：范围内逐点 traverse → 叶 horizon/weight 的 forward_return **net 含成本**（与 backtest 同口径）→ active（非 flat 计分点）net 均值；0 个 → None。
  - 打分软：traverse_soft + score_soft → engaged 点 expected_net 均值；0 个 → None。
  - sim：范围内顺序模拟（fresh SimAccount，风控/T+1 照常）→ nav 点列 → `risk_metrics`：Sharpe Some 用之；否则 total_return；nav < 2 点 → None。
- **寻优**：IS 范围逐组合 evaluate，None → −∞；最大者 best（并列取网格序先者）；全 −∞ → best=None。
- **OS**：best Some → OS 范围 evaluate（None 容许，报告 null）。
- **退化率** = os/is（仅两者 Some 且 |is| > 1e-12；is<0 时不算比率给 null——负 IS 的比率无意义）。
- **全样本对照**：全部可评估范围一次网格 → best 单组合 + 目标值；与 OS 拼接均值（各折 OS Some 值的等权均值）并列展示。
- **漂移表**：每参数 best 值序列（按折）+ 唯一值计数。
- **IS top-5**：每折 IS 目标降序前 5（params + 值）。
- **LLM**：树含 llm 节点 → eprintln 警告一次（缓存跨组合复用价格输入，首轮全量）；评估顺序 await。

## 4. 架构
```
新增 src/optimize/grid.rs   # GridAxis 解析/expand_grid 笛卡尔积/上限校验（纯函数）
新增 src/optimize/mod.rs    # OptimizeConfig/evaluate/run_optimize/OptimizeReport/print_optimize_summary
改动 src/tree/loader.rs     # pub fn load_tree_str_with_overrides(yaml, &BTreeMap<String,f64>)（override 键必须存在于 params；既有 load_tree_str 改薄包装空覆盖）
改动 src/lib.rs             # + pub mod optimize;
改动 src/cli/mod.rs         # Cmd::Optimize
```
- 树 YAML 文本只读一次，每组合 `load_tree_str_with_overrides`（加载毫秒级，组合数 ≤500 可忽略）。
- 报告类型全 serde；print：参数空间/折表（IS/OS/退化/best params）/漂移表/全样本对照/top-5。

## 5. 测试
- grid：range 展开（含浮点容差闭端）、list、dup 名报错、不存在于 params 报错（loader 层）、超上限报错、笛卡尔序确定。
- loader overrides：覆盖生效（行为变化）、未知键报错、空覆盖 = 原行为。
- evaluate：合成数据三口径已知值；范围限制（两个不同范围给不同值）。
- WFO 黄金（植入已知最优）：上升趋势数据 + 树 `close > thr → long else flat`、`thr ∈ {5, 15, 100}` → 每折 best thr=5（全程做多吃满涨幅）、漂移唯一值=1、thr=100 该组合 active=0 → −∞ 不被选。
- e2e 全链路 + 真数据 smoke（regime_adaptive_1 小网格打分口径）。
- 文档：cli-reference（optimize 全表 + 防过拟合判读：退化率<0.5 红旗、漂移乱跳红旗、尖峰 vs plateau）、README 一节。

## 6. 里程碑
- **T1** grid 纯函数 + 测试。
- **T2** loader 覆盖入口 + 测试。
- **T3** evaluate 三口径 + 测试。
- **T4** run_optimize WFO 循环 + 报告/print + CLI。
- **T5** e2e 植入最优黄金 + 文档 + 真数据 smoke。
