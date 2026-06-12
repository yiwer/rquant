# rquant 桌面端（Tauri）设计规格

日期：2026-06-12 ｜ 状态：已与用户逐节确认 ｜ 头脑风暴记录：可视化伴侣会话（功能版图全选 / 驾驶舱优先首页 / YAML+DAG 树工作台 / 方案一架构）

## 1. 背景与定位

为 rquant（A股模糊决策树回测引擎，Rust 库 + CLI）构建桌面端工作台，覆盖研究闭环全链路：数据 → 策略树 → 回测 → 调参/WFO → 因子 → 组合 → 纸面盘监控。

**定位假设（已确认）**：

- 个人本机工具（Windows 11），不做安装包分发、不做自动更新、不做多用户。
- UI 中文。
- schtask 15:35 每日纸面盘 **CLI 通路保持原样**——桌面端是第二入口，共享同一引擎与同一批文件，不替代不干扰。

## 2. 范围与建设顺序

V1 愿景包含全部 8 个模块（用户全选），一份规格、五期实现，**每期一份实现计划逐期交付**：

| 期 | 内容 | 理由 |
|---|---|---|
| M1 | 骨架（workspace/桥接层/任务系统/导航壳）+ G 驾驶舱 | 先把每天要看的做出来；驾驶舱全只读、引擎零改动，风险最低 |
| M2 | C 回测中心 + A 数据工作台 | 回测要选数据，天然耦合；留档/对比机制在此建立 |
| M3 | B 策略树工作台 | 编辑器 + lint + DAG 组件（DAG 组件被决策回放复用） |
| M4 | D WFO 实验室 + E 因子工作台 + F 组合回测台 | 研究三件套，共享热力图/表格组件；引擎进度回调在此引入 |
| M5 | H 档案馆 + 对比互链 + 打磨 | 收尾 |

## 3. 总体架构

**方案一（已确认）：进程内库调用**。Tauri 2 单体应用，桥接层直接依赖 `rquant = { path = "../.." }`，每个 `#[tauri::command]` 包引擎调用；不走 sidecar CLI（通信税高、细粒度数据如逐 bar 回放 CLI 输出面不够）、不走本地 HTTP（单机过度设计）。

### 3.1 仓库布局

```
rquant/                      # 根 crate（引擎）不动，根 Cargo.toml 加 [workspace]
├─ src/ …                    # 引擎 12 模块
├─ desktop/
│  ├─ src-tauri/             # 桥接层 crate（workspace 成员）
│  └─ ui/                    # React 18 + TS + Vite 前端
├─ .rquant-desktop/          # 桌面端留档（gitignore）
└─ paper/ deploy/ examples/ docs/   # 照旧
```

### 3.2 三层职责

1. **前端**：8 个模块页 + 共享组件（ECharts K线/净值/热力、React Flow 只读 DAG + dagre 布局、CodeMirror 6 YAML 编辑、antd 表格表单、zustand 状态、任务抽屉、toast）。注意：决策树是 DAG（goto 可汇聚），ECharts tree 不支持多父，故树图用 React Flow。
2. **桥接层**（零业务逻辑）：① DTO 转换——DTO 在桥接层定义并派生 ts-rs 生成 TS 类型，引擎结构体不加 derive；② 任务调度——`spawn_blocking` + `catch_unwind` 包所有引擎调用，长任务进 TaskRegistry（进度事件 `task://progress/{id}`、可取消、状态可查）；③ 工作区路径解析。若某逻辑只存在于 CLI 私有函数，**提升为 lib pub 函数**，不复制粘贴。
3. **引擎**：唯一事实源。配合改动见 §4，**重放/回测语义冻结**，黄金不变量与现有全量测试是底线闸。

### 3.3 双入口并存

schtask（CLI）与桌面端共享 paper/。桌面端写 paper/ 的纪律集中于 §7；CLI 侧零改动。

## 4. 引擎配合改动清单（全部 TDD，语义冻结）

| # | 改动 | 性质 |
|---|---|---|
| 1 | optimize/portfolio 扫参长循环加**可选进度回调 + 取消标志**（CLI 传 None，行为不变） | M4 必做 |
| 2 | 个别 CLI 私有 glue（如 resolve_target、fetch-to-csv 落盘）按需提升 lib pub | 按需 |
| 3 | 逐 bar 遍历轨迹**可选 trace 开关**（路径节点、分支强度、因子值；默认关）。加"默认关时输出 bit-for-bit 等价"锁测试 | M2 决策回放必做（若现有 per-bar 决策记录已含路径细节则免） |
| 4 | lint 结构化输出（节点/分支定位信息） | 可选低优先；V1 前端用启发式行定位 |

## 5. 模块设计

### 5.1 G 驾驶舱（启动落点，已确认"驾驶舱优先"）

- **首页总览**：三账本状态卡（nav/持仓/最新信号/bars_replayed/state 时间）+ 今日组合清单 diff + 运行状态灯 + schtask 下次触发时间。
- **账本详情**：纸面净值曲线、信号历史时间线（悬挂决策标注）、AccountSnapshot 全 13 字段只读展示。
- **数据来源全只读**：paper/*.json、sig_*.json、run.log、`schtasks /query` 解析。
- **净值历史**：PaperState 只存最新快照 → 桌面端自建 journal（`.rquant-desktop/paper-journal.jsonl`，打开驾驶舱或 run 完成后 append 三账本快照，按 state 时间去重）。**历史从桌面端启用日开始积累，既往不补**。
- **清单 diff 语义**：sig_portfolio.json 目标 vs holdings_top3.json 持仓 → HOLD/BUY/SELL/ADJUST。
- **手动触发 run**：参数与 deploy/paper_run.cmd 严格一致（deploy 冻结树）；交易时段（9:30–15:00）默认禁 commit（可跑 dry）；commit 必弹确认；15:30–15:40 与 schtask 窗口重叠时警告。

### 5.2 C 回测中心

- **配置面板**：树（examples/ + deploy/ 自动发现）/ 标的或 CSV / 周期 / 复权 / sim 或打分模式 / 成本 / warmup。
- **运行即留档**：config.json + result.json + meta.json 落 `.rquant-desktop/runs/<id>/`；历史列表可命名、打标签、重跑、删除。
- **五视图**：概览（指标卡 + 净值/回撤带 vs buy&hold）/ K 线信号（进出场 markPoint + 持仓区间 markArea）/ 交易明细（每笔 trip_return、持有 bars）/ 决策回放 / 原始 JSON。
- **决策回放**：时间轴滑块选 bar → 该 bar 遍历路径 + 因子值/分支强度表（依赖引擎改动 #3）。**M2 先出表格式路径回放；DAG 高亮视图待 M3 DAG 组件就绪后接入**（期序约束：DAG 组件属 M3）。
- **对比视图**：历史勾任意两次 → 净值叠加 + 指标差异表（树版本迭代核心回路）。

### 5.3 A 数据工作台

universe 清单管理（deploy/universe_10.csv + 自定义清单增删）；标的新鲜度灯（最新 bar 时间/行数/周期/复权）；单标的或按清单批量拉取（**串行 + 节流延迟**防 sina 封禁；任务系统可取消）；K 线浏览器打开任意行情 CSV，支持**因子叠加**（输入 DSL 表达式即调引擎 eval 画主图/副图；组件与 5.4 因子预览共享）。

### 5.4 B 策略树工作台（已确认"YAML 主编辑 + 只读 DAG 预览"）

- YAML 是唯一事实源（注释/格式/git diff 全保留），**不做**可视化双向编辑。
- CodeMirror 编辑，保存即引擎 load + lint；警告面板按节点名启发式定位 YAML 行（精确定位待引擎改动 #4，V1 不承诺行内精确标注）。
- load 成功渲染 React Flow 只读 DAG：stance 着色叶、when/strength 悬浮、点节点跳 YAML 行（同启发式）。
- 因子预览：选标的数据 + 因子表达式 → 序列图。
- **deploy/ 冻结树默认只读**，解锁需确认弹窗（保护实盘账本 meta.name 纪律）。

### 5.5 D 调参/WFO 实验室

参数网格（from/to/step 或枚举）+ IS/OS 切分 + 目标指标；长任务组合级进度、可取消（引擎改动 #1）；结果四件套：双参热力图、参数面表格、每折最优漂移表、IS/OS 退化率。**红旗自检面板**（WFO 判读纪律产品化）自动标黄：边界最优（建议扩网格）、漂移≈折数、尖峰面、退化率 <0.5，每条一句解释。留档 kind=wfo。

### 5.6 E 因子工作台

标的池（多选拼池）+ 因子表达式列表 + 前瞻窗；输出 RankIC/t/分位单调性表格、IC 衰减曲线（多 horizon）、因子相关矩阵热力、分位收益柱状；F-1 判据内置（|t|>2 入选线、相关 >0.8 冗余提示）。

### 5.7 F 组合回测台

universe + 树 + top-N + 调仓节奏 + 软/硬 + 成本；输出组合净值 vs 等权基准、超额曲线、成员数曲线、换手统计；**敏感性矩阵一键跑**（top × reb 网格 → 热力格，超额/Sharpe 双值）。留档对比同回测中心。

### 5.8 H 档案馆

docs/superpowers/ 文件树 + markdown 渲染（表格/代码高亮）；文件名 + 全文搜索；回测留档页可挂"相关报告"链接（V1 单向互链）。

## 6. 任务系统（桥接层核心机制）

- TaskRegistry：`HashMap<TaskId, {handle, cancel: AtomicBool, progress, status}>`。
- 命令面：`task_start(kind, params) → TaskId`、`task_cancel(id)`、`task_list()`。
- 进度经 Tauri 事件 `task://progress/{id}` 推送 `{pct, stage, detail}`；前端任务抽屉统一展示运行中/失败/完成。
- 取消协作式：引擎回调间隙检查标志（组合间/折间粒度）。
- panic 经 `catch_unwind` 转 Engine 错误，任务标 failed，app 不崩。

## 7. 数据流、存储与并发纪律

- 工作区 = 仓库根（默认 `E:\rust-app\rquant`，设置页可改）；所有相对路径基于工作区解析。
- `.rquant-desktop/runs/<id>/`：config.json（完整可重跑）、result.json（引擎输出原样）、meta.json（kind/名称/标签/创建时间/关联报告）。run id = 时间戳 + 短随机。该目录进 .gitignore。
- 应用偏好（窗口状态、最近选择）→ Tauri app_data_dir，不入工作区。
- **所有状态/留档写盘 temp + rename 原子替换**。
- **paper/ 并发纪律**：应用内全局写互斥（同一时刻至多一个 commit 型任务）；schtask 窗口警告（§5.1）；真撞上靠二跑幂等（bars_replayed=0）兜底；CLI 侧零改动。

## 8. 错误处理

- 引擎 `Error`（thiserror 九类）→ 桥接层 `{kind, message}` DTO → 前端按类决定形态：toast / 任务抽屉详情 / 可操作建议（如 state corrupt → "删除重建，重放幂等"）。
- 应用日志：tauri-plugin-log → app_data_dir/logs 滚动。

## 9. 安全

- LLM key 维持现有纪律：**只读机器级环境变量 `RQUANT_LLM_API_KEY`**，桌面端不存储、不展示、不提供输入框；设置页仅显示"已检测到/未检测到"。
- 不开监听端口；Tauri fs scope 收敛到工作区 + app_data_dir；唯一外部网络调用是引擎内 sina 拉取（节流）。

## 10. 测试策略

- 引擎配合改动全 TDD；现有全量测试 + 黄金不变量为底线闸；trace 开关加 bit-for-bit 等价锁。
- 桥接层：DTO 转换单测；TaskRegistry 生命周期单测（启动/进度/取消/panic 恢复）；paper 写互斥并发测试。
- 前端：vitest + testing-library，重点锁"配置表单 → invoke 参数正确性"（mock IPC）；E2E 不承诺自动化，V1 手动烟雾清单（启动/驾驶舱渲染/一次快速回测）。
- 检查面：`cargo clippy + test`（workspace）+ `tsc + eslint + vitest`。

## 11. 技术栈（已确认）

Tauri 2 / React 18 + TypeScript + Vite / ECharts / CodeMirror 6 / React Flow（只读）+ dagre / Ant Design / zustand / ts-rs / tauri-plugin-log。

## 12. 风险与诚实边界

1. 纸面净值历史从桌面端启用日开始积累，既往不补（run.log 仅粗略可考）。
2. lint 警告行定位 V1 是按节点名的启发式，不保证精确（引擎结构化输出列为可选改动）。
3. 决策回放依赖引擎 trace 能力（改动 #3）——若实现成本超预期，M2 先出"叶子级回放"（现有决策记录），路径级回放后补。
4. 跨进程 paper/ 竞态没有锁文件级硬保证，靠运行纪律 + 幂等兜底（与现状一致，未恶化）。
5. WFO/组合扫参在 UI 线程外运行但仍占满 CPU——重任务（网格扫描/敏感性矩阵/批量拉取）并发数限 1，轻命令（单次回测/lint/读状态）不限。
