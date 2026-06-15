# 桌面客户端显示与交互优化 · 设计文档

- 日期：2026-06-15
- 状态：设计已与用户逐节确认，待审阅 → writing-plans
- 范围：对已实现的 3 个桌面页面（驾驶舱/回测中心/数据工作台 + 账本详情）做中文文案准确化 + 核心业务流程顺畅化。不加新功能、不重设计布局、不动根 rquant crate 业务逻辑。

## 1. 背景与目标

桌面 app（Tauri 2 + React18 + antd6）M1/M2 已交付 3 个核心页面，运行时冒烟通过。但 UI 探查发现 ~30 处中文文案问题（硬编码英文枚举、未译术语、实现路径泄漏、术语误用/不一致、AccountSnapshot 13 字段全英文）+ 多处流程摩擦（dry-run 无解释、模式歧义、异步无反馈、报错直抛 Rust 原文、首次无引导）。

**目标**：让显示中文准确、贴合核心用户故事、核心业务流程顺畅——把"能用"打磨到"顺手"。

**核心用户故事 / 业务流程**（探查自 desktop 设计 spec）：
1. **驾驶舱监控**（纸面盘）：看 3 账本状态 + 今日信号/持仓 diff + 运行状态，必要时手动触发 run（纯监控，不编辑）。
2. **回测迭代**：选树+数据+模式 → 跑回测 → 五视图看结果 → 改树 → 重跑。
3. **数据准备**：拉行情（新浪 qfq，节流）→ 查新鲜度 → 叠因子验数据质量。

## 2. 已确认决策（brainstorming）

| # | 决策 | 选择 |
|---|---|---|
| Q1 | 范围/深度 | **文案 + 核心流程顺畅（3 真页面 + 账本详情）**；5 个占位导航本 phase 不动 |
| Q2 | 术语哲学 | **准确 + 去错误，保留标准量化术语**（修错/译非标/不泄漏实现；保留 Sharpe/净值/sim·score 等 + 一次性解释） |
| 方案 | 文案组织 | **共享 labels 模块 + 就地修 + 逐页 must-fix 流程**（不做全量 i18n，YAGNI） |

## 3. 共享术语表 + labels 模块（`desktop/ui/src/labels.ts`）

重复枚举/术语的单一真相源，跨页一致。

**枚举/映射：**
- 动作枚举：`Buy→买入`、`Sell→卖出`、`Adjust→调整`、`Hold→持有`
- 模式标签：`sim_hard→模拟·硬`、`sim_soft→模拟·软`、`score_hard→打分·硬`、`score_soft→打分·软`（配一次性 popover：模拟=资金曲线 / 打分=相对排名；硬=取最优 / 软=概率加权）
- AccountSnapshot 13 字段中文映射：`pos→仓位`、`entry_price→建仓价`、`bars_held→持仓根数`、`nav→净值`、`peak_nav→峰值净值`、`max_drawdown→最大回撤`、`turnover→换手`、`last_increase_date→末次加仓日`、`max_price_since_entry→持仓最高价`、`min_price_since_entry→持仓最低价`、`bars_since_exit→离场后根数`、`last_trip_return→上轮回合收益`、`trip→回合数`

**术语表（修正/翻译）：**
- `bps→基点`、`warmup→热身期`、`window→回溯窗`、`bh对照→等权基准`、`bars→根数`
- `"弃权"（因子语境）→缺失/无效`、`schtask→计划任务`、`run.log→运行日志`
- 路径泄漏：`paper//.rquant-desktop/data/→"行情数据库"`、`deploy 只读→"内置"`

**保留的标准量化术语**（不强译）：Sharpe、净值(nav)、回撤、留档（既有一致术语，保留不 churn）。

## 4. 逐页显示 + 流程修复

### 4.1 驾驶舱 Cockpit（Cockpit.tsx + BookCard/DiffTable/RunStatusPanel）
- **显示**：清单动作→中文枚举；BookCard "回撤"→"最大回撤"、"入选 N 只"→"目标持仓 N 只"、密集统计行加标点；清单空状态 "等待账本3 run"→"持仓组合未运行"；状态面板 schtask→计划任务、"查看 run.log"→"查看运行日志"。
- **流程**：①手动触发 modal 加 dry-run 解释（"交易时段外/计划任务窗口冲突 → 仅模拟运行，不写持仓状态"）；②run 完成后自动刷新驾驶舱 + toast（订阅任务完成事件，消除手动刷新）；③空状态可执行引导（"账本未初始化，点'手动触发 run'建首个快照"）。

### 4.2 回测中心 Backtest（config/history/overview/trades/replay/compare）
- **显示**：模式选择器 popover 解释（模拟/打分、硬/软）；warmup→热身期、window→回溯窗（+tooltip）；成本bps→成本(基点)；"bh对照"→"等权基准"、Sharpe 保留 + 夏普 tooltip；模式 tag→中文；交易表 "持有bars"→"持仓根数"、盈亏额 tooltip 友好化；回放 "NaN/弃权"→"缺失"；对比 "(nav口径,资金无关)"下沉副标题、"一侧"→"至少一侧数据缺失"；K线 "末2000根" 加 info（"完整数据已参与回测"）。
- **流程**：①树下拉 (加载失败)→展开错误详情；②报错友好化（见 §5）；③run 启动后任务进度（已有抽屉）+ 完成 toast。

### 4.3 数据工作台 DataBench
- **显示**：路径抽象（卡标题去 paper//.rquant-desktop/，CSV 列表提取标的当标题 "sh600030 · 60m"）；"deploy 只读"→"内置"；"串行+500ms节流"→"逐个拉取(节流)"；因子描述 "弃权"→"NaN 无法计算，显示断线"。
- **流程**：①因子叠加 loading 反馈（按钮 spinner）；②批量拉取输入校验 + 预览（"将拉取: sh600030, sz000001"）。

### 4.4 账本详情 BookDetail
- **显示**：AccountSnapshot 13 字段→中文映射（用 labels）；标题元数据下沉副标题（"持仓快照"标题 + "只读·13字段"副标题；journal 元数据同理）。

### 4.5 少量桥接源字符串
个别用户可见串来自 Rust 桥接（dry-run gate message、账本 advice 文案）——修这些＝小幅 `desktop/src-tauri` 改动，仍属"客户端显示"范畴、在范围内。故收尾闸必须带 `--workspace`。

## 5. 报错友好化 + 测试 + 边界

### 5.1 报错友好化（`desktop/ui/src/errors.ts`）
常见后端报错模式→友好中文（树解析失败→"策略树解析失败"、文件未找到→"文件未找到"、网络/拉取失败→"数据拉取失败（网络或数据源）"）；**兜底保留原文于可折叠"详情"**（量化用户仍能看 Rust 原文排查）。各 `message.error` 处统一接入。

### 5.2 测试
- `labels.ts` 单测（动作/模式/13 字段映射完整正确）；`errors.ts` 映射单测（已知模式→友好、未知→兜底）；关键组件测试（清单表渲染中文动作、账本详情渲染中文字段、模式选择器显示 gloss）。
- **闸门：`cargo test --workspace` + `cargo clippy --workspace`**（可能改桥接源字符串，必须 --workspace——吸取上次桥接 crate 漏编译教训）+ `npm --prefix desktop/ui run build`（tsc）+ vitest。

### 5.3 边界（非目标）
- 不加新功能（5 占位导航不动）；不做布局/视觉重设计（纯文案 + 反馈打磨，保持 antd 布局）；不做全量 i18n（仅共享 labels 模块）；**根 rquant crate 业务逻辑不动**（仅 desktop/ui + 个别 desktop/src-tauri 字符串）；回测/驾驶舱/数据的业务逻辑零改动（只改显示与反馈）。

## 6. 改动文件

| 文件 | 改动 |
|---|---|
| `desktop/ui/src/labels.ts` | 新建：枚举/术语映射单一真相源 + 单测 |
| `desktop/ui/src/errors.ts` | 新建：后端报错→友好中文 + 单测 |
| `desktop/ui/src/pages/Cockpit.tsx` + BookCard/DiffTable/RunStatusPanel | 文案中文化 + dry-run 解释 + run 完成自动刷新/toast + 空状态引导 |
| `desktop/ui/src/pages/Backtest.tsx` + config/history/overview/trades/replay/compare | 文案 + 模式 gloss + 报错友好 + tree 错误详情 + tooltip |
| `desktop/ui/src/pages/DataBench.tsx` | 路径抽象 + 标的提取 + 因子 loading + 批量拉取校验 |
| `desktop/ui/src/pages/BookDetail.tsx` | 13 字段中文映射 + 标题元数据下沉 |
| `desktop/ui/src/api/ipc.ts`（或 catch 处） | 接入 errors.ts 友好映射 |
| `desktop/src-tauri/src/*`（个别） | gate message / advice 文案中文化 |
| `docs/cli-reference.md` 或桌面文档 | 如有面向用户的术语变更，同步（可选） |

## 7. 诚实边界小结
- 纯显示 + 交互打磨，业务逻辑/引擎零改动；改动集中 desktop/ui + 个别桥接字符串。
- 5 个占位导航（策略树/因子/WFO/组合/档案馆）本 phase 不处理（M3-M5 功能）。
- 量化术语保留（Sharpe/净值/留档），只修错误/非标/泄漏——"准确"非"全译"。
- 收尾闸必须 `--workspace`（防桥接 crate 漏编译复发）。
