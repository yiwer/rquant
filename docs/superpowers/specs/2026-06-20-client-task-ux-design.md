# 客户端任务运行体验统一 设计(task-ux)

> 状态:已 brainstorm 定稿,待 writing-plans。日期:2026-06-20。
> 前序:客户端重构 sub-1/2a/3a 已合入 master。本项是横切 UX 修缮,复用其桌面范式。

## 0. 背景与问题(代码坐实)

用户反馈:`选股榜(指定日)` 跑起来"只显示选股中、没有额外信息、不知道要多久;切换页面任务中断展示";`部署` 页同样。排查结论:

1. **页内无进度**:`pages/Screen.tsx` 的 `AsofTab` 在运行时只渲染静态 `<Spin/>`「选股中…」,**丢弃了后端已发的 `progress{pct,stage,detail}`**。`Deploy.tsx` 预览同理(无运行进度)。
2. **切页即丢**:运行态(`running`)与结果(`asofResult`/`preview`)是**组件局部 `useState`**,`listen("task://progress")` 写在点击回调里。切顶层页 → 组件卸载 → 监听泄漏、`done` 事件落到已卸载组件 → 任务仍在后台跑但页面回来空白。
3. **后端已够用**:`task://progress` 事件携带全量 `TaskInfoDto{id,kind,status,progress{pct,stage,detail},error,result}`;`task_list` 可回捞;右上**全局「任务」抽屉**(`components/TaskDrawer.tsx`)已订阅事件并显示进度/取消。缺的是把这套接进各页。
4. **进度粒度偏粗(诚实约束)**:`screen_asof` 仅发 加载0.1/选股0.4 两档,长调用 `run_screen` 期间停在 40%;`deploy_run_month` 仅 0.3 一档;`factor_run` 0.2 一档。故页内**真百分比意义有限,"阶段+已耗时"才是诚实信号**。

> 相关但独立:选股慢的**根因是 debug 构建**(`cargo tauri dev` 桥未优化:同一选股 debug 16.5s vs release 1.8s,~9×)。本设计**不解决速度**(用 release 构建解决,见 §8),只让等待**可见、可取消、切页不丢**。

## 1. 决策(brainstorm 定论)

| 决策 | 结论 |
|---|---|
| 范围 | **铺到所有任务驱动页**:选股(指定日+回测)、部署、因子工作台、研究(跑轮)、回测中心。统一机制后逐页接入。 |
| 进度形态 | **阶段(中文化)+ 已耗时计时 + 粗进度条 + 取消**;长停滞阶段转 indeterminate,不伪造百分比。**不改后端进度粒度。** |
| 状态归属 | 运行态+结果**提到 store(模块单例,切页不丢)**;`task://progress` **全局只订阅一次**(常驻,即便切走也能捕获结果)。 |

## 2. 架构(单一真源)

- **全局任务 store `stores/tasks.ts`**:`task://progress` 的唯一订阅者 + 启动 `task_list()` 播种。持有 `tasks: Record<id, TaskInfoDto>` 与 `startedAt: Record<id, number>`。`init()` 幂等(重复调用只订阅一次)。在 `App.tsx` 启动时调一次。`startedAt[id]` 在**首次见到该 id 时**记 `Date.now()`(发起即见,或播种时见)——播种到的"启动前已在跑"任务以 firstSeen 近似,耗时标"约"。
- **域 store 持久化**:`stores/{screen,deploy,factor,research,backtest}.ts` 各自持有 `currentTaskId: string|null`、`result`(域类型)、`error: string|null`(均在 store,非组件)。`launch` 动作:调 `api.xxx()` 拿 id → 记 `currentTaskId` → **订阅全局 store**(`useTasks.subscribe`)看该 id:`done` → 从 `tasks[id].result` 反序列化进 `result` 并退订;`failed` → `error = friendlyError(...).title` 退订;`cancelled` → 清运行态退订。全局监听常驻 ⇒ 结果捕获不依赖页面是否挂载。
- **共享组件 `components/TaskRunning.tsx`**:`props { info: TaskInfoDto, startedAt?: number, onCancel?: ()=>void }`。渲染:阶段(`labels` 中文化 stage)、进度条(`pct>0 && pct<1` 用百分比,否则 indeterminate 流动)、**已耗时**(本地 `setInterval` 每秒,`now-startedAt`)、取消按钮、长任务提示文案。纯展示,无副作用。
- **TaskDrawer 去重**:`components/TaskDrawer.tsx` 改为读全局 `useTasks`(去掉自身的 `listen`+2s 轮询),与各页共用同一真源。

## 3. 文件

**新建**(`desktop/ui/src/`):
- `stores/tasks.ts` — 全局任务 store + `init()` + 选择器 `useTaskInfo(id)`、`useTaskStartedAt(id)`。
- `components/TaskRunning.tsx` — 进度展示组件。
- `hooks/useTaskLauncher.ts`(可选,薄封装)— 给域 store/页复用"启动→跟踪→done/failed"接线;若域 store 内联即可则省去。

**修改**:
- `App.tsx` — 启动 `useTasks.getState().init()`(挂载一次)。
- `stores/{screen,deploy,factor,research,backtest}.ts` — 加 `currentTaskId/result/error` + `launch` 接线全局 store。
- `pages/{Screen,Deploy,Factor,Research,Backtest}.tsx`(及相关子组件如 `ScreenBacktestResult`/`RunRoundForm`)— 运行态/结果改从域 store 读;运行中渲染 `<TaskRunning>`;有结果渲染结果;空态引导。
- `components/TaskDrawer.tsx` — 改读全局 store。
- `labels.ts` — stage 中文映射(`start/加载/选股/毛档/净档/归档/因子/...`)+ 长任务提示文案。

## 4. 数据流

- **启动**:`App` → `useTasks.init()` → `task_list()` 播种 + `listen("task://progress")` 常驻。
- **跑任务**:页调域 store `launch()` → `api.xxx()` 得 `taskId` → 域 store 记 `currentTaskId`、全局 store 记 `startedAt` → 全局 store 持续更新 `tasks[taskId]` → 页用 `useTaskInfo(currentTaskId)` 渲染 `<TaskRunning>` → `done`:域 store 从 `info.result` 取结果存入 → 页渲染结果。
- **切页**:组件卸载,但 store 单例 + 全局监听常驻不动 → 切回:`currentTaskId` 仍在 → 仍 running 显进度、已 done 显结果。
- **取消**:`<TaskRunning>` 取消 → `api.taskCancel(id)` → 任务转 `cancelled` → 域 store 清运行态(回空态或保留上次结果)。

## 5. 错误处理(诚实)

- `failed`:`friendlyError` 友好文案在页内显著显示 + 抽屉红标;不静默。
- 进度粗/长停滞:indeterminate + 已耗时 + 提示("横截面打分中,通常数十秒;debug 构建会更慢"),**不伪造百分比**。
- 播种的旧任务无精确开始时刻:耗时按 firstSeen 近似并标"约",不谎报。
- 重复发起防护:`launch` 时若本域已有 running 的 `currentTaskId`,禁用发起按钮(沿用既有 loading 语义,改读 store)。

## 6. 测试

- **vitest**:
  - `stores/tasks.ts`:`task_list` 播种;`task://progress` 事件更新 `tasks[id]`;`startedAt` 首见记录;`init` 幂等(只订阅一次)。注入 mock `api` + mock event(模块级 `listen` 用可注入封装或 vi.mock)。
  - 域 store 接线:发起→`currentTaskId` 置位;模拟全局 store `done` 事件→`result` 捕获;`failed`→`error`;`cancelled`→清运行态。**关键:模拟"组件卸载后" done 仍被捕获**(store 层断言,不依赖渲染)。
  - `TaskRunning.tsx`:给定 `info`+`startedAt` 渲染阶段/百分比 vs indeterminate/耗时/取消回调。
- **收尾**:`tsc --noEmit` 0 + `vitest --run` 全过 + `npm run build`;GUI 冒烟:指定日跑→显示阶段+已耗时→切到部署页→切回→仍显示进度/结果;部署预览同;抽屉与页内进度一致。

## 7. 范围边界(YAGNI)

不含:改后端任务体进度粒度(决策:沿用现有阶段 + 前端耗时);任务历史持久化(进程内 registry 已足);多任务并行 UI(重任务独占槽);占位页(策略树/调参/组合/档案,无任务)。

## 8. 关联后续(非本设计实现)

- **release 构建**:选股慢的根因。UX 落地后,桌面 app 改用 `cargo tauri dev --release`(桥优化,选股 ~2s);分发用 `cargo tauri build`。属独立运维动作,本项不含代码改动。
- **抓取资源竞争**:后台 baostock 抓取与选股抢磁盘/CPU;可在交互期暂停抓取,亦属运维,不在本设计。
