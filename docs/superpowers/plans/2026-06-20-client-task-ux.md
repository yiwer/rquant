# 任务运行体验统一 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让所有任务驱动页(选股指定日/选股回测/部署/因子/研究跑轮/回测中心)在运行时显示阶段+已耗时+进度+取消,且切换页面不丢运行态与结果。

**Architecture:** 新建全局任务 store(`task://progress` 唯一订阅者 + `task_list` 播种,模块单例切页不丢)+ 共享 `<TaskRunning>` 进度组件 + `trackTask()` 把任务终态一次性回调给各域 store(运行态/结果提到 store)。各页改为从 store 读运行态/结果。后端零改动(进度粒度沿用现有,前端补"已耗时")。

**Tech Stack:** React18 + Zustand5 + antd6 + Vitest4 + Tauri event API;`@bindings/*` ts-rs 类型;`friendlyError`(errors.ts);`task_list`/`task_cancel`/`task://progress`(已存在)。

## Global Constraints

- 全局只订阅一次 `task://progress`(在新 `stores/tasks.ts` 内),其余组件/store 不得再 `listen` 同通道(去重,避免泄漏)。
- 运行态(`*TaskId`)与结果(`*Result`/`*Error`)一律放 store,**不得**用组件 `useState` 持有(切页持久的前提)。
- 进度展示:`pct∈(0,1)` 用百分比,否则 indeterminate;**已耗时**=`now − startedAt`;**不伪造百分比**;播种的旧任务耗时标"约"。
- 复用既有:`friendlyError(s).title` 做错误文案;`api.taskCancel(id)` 取消;`@bindings/TaskInfoDto`(字段 `id,kind,status,progress{pct,stage,detail},error,result`)。
- Tauri invoke 参数 JS camelCase 自动映射 Rust snake_case(沿用)。
- 验证三件套:`node desktop/ui/node_modules/typescript/bin/tsc --noEmit -p desktop/ui/tsconfig.app.json` 0 错;`npm --prefix desktop/ui run test -- --run` 全过;`npm --prefix desktop/ui run build` 成功。
- 英文 commit(`git commit -F -` heredoc);只 add 本任务文件;不 push。

---

### Task 1: 全局任务 store + trackTask

**Files:**
- Create: `desktop/ui/src/stores/tasks.ts`
- Test: `desktop/ui/src/stores/tasks.test.ts`

**Interfaces — Produces:**
- `useTasks` (zustand store): `{ tasks: Record<string,TaskInfoDto>, startedAt: Record<string,number>, inited: boolean, init(): void, ingest(info: TaskInfoDto): void }`
- `trackTask(id: string, handlers: { done?(info): void; failed?(info): void; cancelled?(info): void }): void` — 订阅全局 store,任务到终态时一次性触发对应回调并退订(若已是终态立即触发)。
- `useTaskInfo(id: string|null): TaskInfoDto|undefined`、`useTaskStartedAt(id: string|null): number|undefined`(React 选择器 hooks)。

- [ ] **Step 1: 写失败测试** `desktop/ui/src/stores/tasks.test.ts`:

```typescript
import { test, expect, vi, beforeEach } from "vitest";

// mock 事件与 ipc:listen 把回调存起来手动触发;taskList 返回播种数据
let emit: ((info: any) => void) | null = null;
vi.mock("@tauri-apps/api/event", () => ({
  listen: (_name: string, cb: (e: { payload: any }) => void) => {
    emit = (info) => cb({ payload: info });
    return Promise.resolve(() => {});
  },
}));
vi.mock("../api/ipc", () => ({
  api: { taskList: vi.fn().mockResolvedValue([{ id: "t9", kind: "seed", status: "running", progress: { pct: 0.5, stage: "选股", detail: "" }, error: null, result: null }]) },
}));

import { useTasks, trackTask } from "./tasks";

beforeEach(() => { useTasks.setState({ tasks: {}, startedAt: {}, inited: false }); emit = null; });

function info(id: string, status: string, extra: any = {}) {
  return { id, kind: "k", status, progress: { pct: 0, stage: "选股", detail: "" }, error: null, result: null, ...extra };
}

test("ingest records task and stamps startedAt once", () => {
  useTasks.getState().ingest(info("t1", "running"));
  const a = useTasks.getState().startedAt["t1"];
  expect(a).toBeGreaterThan(0);
  expect(useTasks.getState().tasks["t1"].status).toBe("running");
  useTasks.getState().ingest(info("t1", "done"));
  expect(useTasks.getState().startedAt["t1"]).toBe(a); // 不被覆盖
  expect(useTasks.getState().tasks["t1"].status).toBe("done");
});

test("init seeds from task_list once and subscribes", async () => {
  useTasks.getState().init();
  useTasks.getState().init(); // 幂等:第二次空操作
  await Promise.resolve(); await Promise.resolve();
  expect(useTasks.getState().tasks["t9"]?.status).toBe("running");
  expect(typeof emit).toBe("function"); // 已订阅
});

test("trackTask fires done once with result", () => {
  let got: any = null;
  useTasks.getState().init();
  trackTask("t2", { done: (i) => { got = i.result; } });
  useTasks.getState().ingest(info("t2", "running"));
  expect(got).toBeNull();
  useTasks.getState().ingest(info("t2", "done", { result: { ok: 1 } }));
  expect(got).toEqual({ ok: 1 });
});

test("trackTask fires failed", () => {
  let err: string | null = null;
  trackTask("t3", { failed: (i) => { err = i.error; } });
  useTasks.getState().ingest(info("t3", "failed", { error: "boom" }));
  expect(err).toBe("boom");
});
```

- [ ] **Step 2: 跑确认失败** `npm --prefix desktop/ui run test -- --run src/stores/tasks.test.ts` → FAIL(模块不存在)。

- [ ] **Step 3: 实现** `desktop/ui/src/stores/tasks.ts`:

```typescript
import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import type { TaskInfoDto } from "@bindings/TaskInfoDto";
import { api } from "../api/ipc";

interface TasksState {
  tasks: Record<string, TaskInfoDto>;
  startedAt: Record<string, number>;
  inited: boolean;
  ingest: (info: TaskInfoDto) => void;
  init: () => void;
}

export const useTasks = create<TasksState>((set, get) => ({
  tasks: {},
  startedAt: {},
  inited: false,
  ingest: (info) =>
    set((s) => ({
      tasks: { ...s.tasks, [info.id]: info },
      startedAt: s.startedAt[info.id] ? s.startedAt : { ...s.startedAt, [info.id]: Date.now() },
    })),
  init: () => {
    if (get().inited) return;
    set({ inited: true });
    void api.taskList().then((list) => list.forEach((t) => get().ingest(t))).catch(() => {});
    void listen<TaskInfoDto>("task://progress", (e) => get().ingest(e.payload));
  },
}));

/** 订阅全局 store,任务到终态时一次性回调并退订(已终态则立即回调)。 */
export function trackTask(
  id: string,
  handlers: { done?: (info: TaskInfoDto) => void; failed?: (info: TaskInfoDto) => void; cancelled?: (info: TaskInfoDto) => void },
): void {
  const fire = (info: TaskInfoDto): boolean => {
    if (info.status === "done") { handlers.done?.(info); return true; }
    if (info.status === "failed") { handlers.failed?.(info); return true; }
    if (info.status === "cancelled") { handlers.cancelled?.(info); return true; }
    return false;
  };
  const cur = useTasks.getState().tasks[id];
  if (cur && fire(cur)) return;
  const unsub = useTasks.subscribe((s) => {
    const info = s.tasks[id];
    if (info && fire(info)) unsub();
  });
}

export const useTaskInfo = (id: string | null): TaskInfoDto | undefined =>
  useTasks((s) => (id ? s.tasks[id] : undefined));
export const useTaskStartedAt = (id: string | null): number | undefined =>
  useTasks((s) => (id ? s.startedAt[id] : undefined));
```

- [ ] **Step 4: 跑确认通过** `npm --prefix desktop/ui run test -- --run src/stores/tasks.test.ts` → PASS;`node desktop/ui/node_modules/typescript/bin/tsc --noEmit -p desktop/ui/tsconfig.app.json 2>&1 | tail -5` → 0。

- [ ] **Step 5: Commit**

```bash
git add desktop/ui/src/stores/tasks.ts desktop/ui/src/stores/tasks.test.ts
git commit -F - <<'EOF'
feat(ui): global task store (single task://progress listener, seed, trackTask)
EOF
```

---

### Task 2: 阶段中文化 + TaskRunning 组件

**Files:**
- Modify: `desktop/ui/src/labels.ts`(追加 `STAGE_ZH` + `stageZh`)
- Create: `desktop/ui/src/components/TaskRunning.tsx`
- Test: `desktop/ui/src/components/TaskRunning.test.tsx`

**Interfaces:**
- Consumes: `@bindings/TaskInfoDto`、`stageZh`(本任务新增)。
- Produces: `export default function TaskRunning(props: { info: TaskInfoDto; startedAt?: number; onCancel?: () => void })`;`stageZh(stage: string): string`。

- [ ] **Step 1: labels.ts 追加**(置于文件末尾):

```typescript
/** 任务进度阶段标识 → 中文(后端任务体发的 stage:start/加载/选股/毛档/净档/归档/因子/...)。 */
export const STAGE_ZH: Record<string, string> = {
  start: "启动", 加载: "加载数据", 选股: "横截面选股", 毛档: "毛收益档",
  净档: "净收益档", 归档: "归档", 因子: "因子计算",
};
export const stageZh = (stage: string): string => STAGE_ZH[stage] ?? (stage || "运行中");
```

- [ ] **Step 2: 写失败测试** `desktop/ui/src/components/TaskRunning.test.tsx`:

```tsx
import { test, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import TaskRunning from "./TaskRunning";

const info = (pct: number, stage = "选股") => ({
  id: "t1", kind: "screen_asof", status: "running",
  progress: { pct, stage, detail: "" }, error: null, result: null,
});

test("shows stage in Chinese and elapsed seconds", () => {
  render(<TaskRunning info={info(0.4) as any} startedAt={Date.now() - 5000} />);
  expect(screen.getByText(/横截面选股/)).toBeTruthy();
  expect(screen.getByText(/已耗时/)).toBeTruthy();
});

test("determinate bar when pct in (0,1)", () => {
  const { container } = render(<TaskRunning info={info(0.4) as any} startedAt={Date.now()} />);
  expect(container.querySelector(".ant-progress")).toBeTruthy();
});
```

- [ ] **Step 3: 跑确认失败** `npm --prefix desktop/ui run test -- --run src/components/TaskRunning.test.tsx` → FAIL。

- [ ] **Step 4: 实现** `desktop/ui/src/components/TaskRunning.tsx`:

```tsx
import { useEffect, useState } from "react";
import { Button, Progress, Space, Spin, Typography } from "antd";
import type { TaskInfoDto } from "@bindings/TaskInfoDto";
import { stageZh } from "../labels";

export default function TaskRunning({ info, startedAt, onCancel }: { info: TaskInfoDto; startedAt?: number; onCancel?: () => void }) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, []);
  const elapsed = startedAt ? Math.max(0, Math.round((now - startedAt) / 1000)) : null;
  const pct = info.progress.pct;
  const determinate = pct > 0 && pct < 1;
  return (
    <div style={{ textAlign: "center", padding: 40 }}>
      <Space direction="vertical" size="middle" style={{ width: 360, maxWidth: "100%" }}>
        {determinate ? <Progress percent={Math.round(pct * 100)} status="active" /> : <Spin />}
        <Typography.Text>
          {stageZh(info.progress.stage)}
          {info.progress.detail ? ` · ${info.progress.detail}` : ""}
          {elapsed != null ? ` · 已耗时 ${elapsed}s` : ""}
        </Typography.Text>
        <Typography.Text type="secondary" style={{ fontSize: 12 }}>
          横截面计算中,通常数十秒;切换页面不会中断,可在右上「任务」查看。
        </Typography.Text>
        {onCancel && <Button size="small" onClick={onCancel}>取消</Button>}
      </Space>
    </div>
  );
}
```

- [ ] **Step 5: 跑确认通过** `npm --prefix desktop/ui run test -- --run src/components/TaskRunning.test.tsx` → PASS;`tsc --noEmit` → 0。

- [ ] **Step 6: Commit**

```bash
git add desktop/ui/src/labels.ts desktop/ui/src/components/TaskRunning.tsx desktop/ui/src/components/TaskRunning.test.tsx
git commit -F - <<'EOF'
feat(ui): TaskRunning progress component + stage labels
EOF
```

---

### Task 3: App 启动 init + TaskDrawer 改读全局 store

**Files:**
- Modify: `desktop/ui/src/App.tsx`(挂载时 `useTasks.init()`)
- Modify: `desktop/ui/src/components/TaskDrawer.tsx`(读全局 store,删自身 listen+poll)

**Interfaces:**
- Consumes: `useTasks`(Task 1)。

- [ ] **Step 1: App.tsx** — `import { useEffect } from "react";`(若未导入)+ `import { useTasks } from "./stores/tasks";`;在 `Shell()` 顶部加:

```tsx
  useEffect(() => { useTasks.getState().init(); }, []);
```

(放在 `const nav = useNavigate();` 之前或之后均可,确保在 `Shell` 组件体内、`return` 之前。)

- [ ] **Step 2: TaskDrawer.tsx 改实现** — 删掉自身的 `useEffect`/`listen`/`setInterval`/`refresh`/本地 `tasks` state,改读全局 store:

```tsx
import { useState } from "react";
import { Badge, Button, Drawer, List, Progress, Typography } from "antd";
import { useTasks } from "../stores/tasks";
import { api } from "../api/ipc";

const STATUS_BADGE: Record<string, string> = {
  running: "processing", done: "success", failed: "error", cancelled: "default",
};

export default function TaskDrawer() {
  const [open, setOpen] = useState(false);
  const tasks = useTasks((s) => Object.values(s.tasks).sort((a, b) => a.id.localeCompare(b.id)));
  const running = tasks.filter((t) => t.status === "running").length;
  return (
    <>
      <Badge count={running} size="small">
        <Button size="small" onClick={() => setOpen(true)}>任务</Button>
      </Badge>
      <Drawer title="任务" open={open} onClose={() => setOpen(false)} width={420}>
        <List
          dataSource={tasks}
          locale={{ emptyText: "暂无任务" }}
          renderItem={(t) => (
            <List.Item
              actions={t.status === "running" ? [<Typography.Link key="c" onClick={() => void api.taskCancel(t.id)}>取消</Typography.Link>] : []}
            >
              <List.Item.Meta
                title={<Badge status={(STATUS_BADGE[t.status] ?? "default") as never} text={`${t.kind} · ${t.id}`} />}
                description={
                  <>
                    <Progress percent={Math.round(t.progress.pct * 100)} size="small" />
                    <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                      {t.progress.stage} {t.progress.detail} {t.error ?? ""}
                    </Typography.Text>
                  </>
                }
              />
            </List.Item>
          )}
        />
      </Drawer>
    </>
  );
}
```

- [ ] **Step 3: 验证** `tsc --noEmit` → 0;`npm --prefix desktop/ui run test -- --run 2>&1 | tail -6` → 全过(既有 TaskDrawer 相关测试若 mock 了 taskList/listen,改注入全局 store——见下注)。

> 注:若存在 `TaskDrawer.test.tsx` 且依赖旧的 listen/poll,改为 `useTasks.setState({ tasks: { t1: {...} } })` 后断言渲染。无则跳过。

- [ ] **Step 4: Commit**

```bash
git add desktop/ui/src/App.tsx desktop/ui/src/components/TaskDrawer.tsx
git commit -F - <<'EOF'
feat(ui): init global task store at app start; TaskDrawer reads it
EOF
```

---

### 共享接入模式(Task 4–7 通用,每页照此改)

每个任务驱动页/子组件做同一变换。**每个域 store** 增三字段 + 一动作:

```typescript
// 在 interface 中(<X> 替换为该域前缀,<Result> 为结果 DTO 类型)
<x>TaskId: string | null;
<x>Result: <Result> | null;
<x>Error: string | null;
run<X>: (...args) => Promise<void>;
```

```typescript
// 在 create(...) 中
<x>TaskId: null, <x>Result: null, <x>Error: null,
run<X>: async (...args) => {
  set({ <x>Error: null });
  try {
    const id = await get().api.<apiMethod>(...args);
    set({ <x>TaskId: id });
    trackTask(id, {
      done: (info) => set({ <x>Result: info.result as <Result> }),
      failed: (info) => set({ <x>Error: friendlyError(info.error ?? "运行失败").title }),
    });
  } catch (e) { set({ <x>Error: friendlyError(String(e)).title }); }
},
```

**每个页**:删本地 `running`/结果 `useState` 与 `listen`;改:

```tsx
import { useTaskInfo, useTaskStartedAt } from "../stores/tasks";
import TaskRunning from "../components/TaskRunning";
// ...
const info = useTaskInfo(st.<x>TaskId);
const startedAt = useTaskStartedAt(st.<x>TaskId);
const running = info?.status === "running";
// 运行按钮:loading={running} disabled={running||...} onClick={() => void st.run<X>(...)}
// 区域渲染:
{running && info
  ? <TaskRunning info={info} startedAt={startedAt} onCancel={() => st.<x>TaskId && void st.api.taskCancel(st.<x>TaskId)} />
  : st.<x>Error
  ? <Typography.Text type="danger">{st.<x>Error}</Typography.Text>
  : st.<x>Result
  ? <结果组件 .../>
  : <空态引导/>}
```

`friendlyError` 从 `../errors` 引入。每页测试:注入 mock `api`,断言 `run<X>` 置 `*TaskId`;`useTasks.getState().ingest({id, status:"done", result})` 后断言 `*Result` 捕获;`status:"failed"` 后断言 `*Error`。

---

### Task 4: 选股「指定日」接入(screen store + Screen.tsx)

**Files:**
- Modify: `desktop/ui/src/stores/screen.ts`
- Modify: `desktop/ui/src/pages/Screen.tsx`(`AsofTab`)
- Test: `desktop/ui/src/stores/screen.test.ts`(新增或追加 asof 用例)

**Interfaces:**
- Consumes: `trackTask`,`useTaskInfo`,`useTaskStartedAt`(Task 1);`TaskRunning`(Task 2);`api.screenAsof(config,asOf,top)→string`;`@bindings/ScreenResultDto`;`friendlyError`。
- Produces: screen store `asofTaskId/asofResult/asofError/runAsof(config,asOf,top)`。

- [ ] **Step 1: 写失败测试**(追加到 `desktop/ui/src/stores/screen.test.ts`;若无则新建并 `import { useScreen } from "./screen"`):

```typescript
import { test, expect } from "vitest";
import { useScreen } from "./screen";
import { useTasks } from "./tasks";

test("runAsof tracks task and captures result on done", async () => {
  const real = useScreen.getState().api;
  useScreen.setState({ api: { ...real, screenAsof: async () => "ta1" }, asofTaskId: null, asofResult: null, asofError: null });
  useTasks.setState({ tasks: {}, startedAt: {}, inited: true });
  await useScreen.getState().runAsof("cfg", "2026-06-16", 50);
  expect(useScreen.getState().asofTaskId).toBe("ta1");
  useTasks.getState().ingest({ id: "ta1", kind: "screen_asof", status: "done", progress: { pct: 1, stage: "选股", detail: "" }, error: null, result: { config: "cfg", as_of: "2026-06-16", n_universe: 10, top: 50, rows: [] } as any });
  expect(useScreen.getState().asofResult?.n_universe).toBe(10);
});
```

- [ ] **Step 2: 跑确认失败** `npm --prefix desktop/ui run test -- --run src/stores/screen.test.ts` → FAIL。

- [ ] **Step 3: 实现** — `stores/screen.ts`:`import { trackTask } from "./tasks";`、`import { friendlyError } from "../errors";`、`import type { ScreenResultDto } from "@bindings/ScreenResultDto";`(若未引)。在接口与 `create` 中按"共享接入模式"加 `asofTaskId/asofResult/asofError/runAsof`(`<x>`=`asof`,`<Result>`=`ScreenResultDto`,`<apiMethod>`=`screenAsof`,args=`config,asOf,top`)。

  `pages/Screen.tsx` `AsofTab`:删 `asofResult`/`running` 的 `useState` 与 `listen`、`runAsof` 本地函数;`import TaskRunning from "../components/TaskRunning";`、`import { useTaskInfo, useTaskStartedAt } from "../stores/tasks";`;按模式渲染:

```tsx
  const info = useTaskInfo(st.asofTaskId);
  const startedAt = useTaskStartedAt(st.asofTaskId);
  const running = info?.status === "running";
  // 按钮:<Button type="primary" loading={running} disabled={!config || running} onClick={() => { if (!config || !asOf) { message.warning("请选择配置与指定日日期"); return; } void st.runAsof(config, asOf, top); }}>运行选股</Button>
  // 结果区:
  {running && info ? (
    <TaskRunning info={info} startedAt={startedAt} onCancel={() => st.asofTaskId && void st.api.taskCancel(st.asofTaskId)} />
  ) : st.asofError ? (
    <span style={{ color: "#dc2626" }}>{st.asofError}</span>
  ) : st.asofResult ? (
    <ScreenPickTable result={st.asofResult} />
  ) : (
    <span style={{ opacity: 0.6 }}>选择配置与指定日日期,点「运行选股」查看当日选股榜。</span>
  )}
```

- [ ] **Step 4: 跑确认通过** `npm --prefix desktop/ui run test -- --run src/stores/screen.test.ts` → PASS;`tsc --noEmit` → 0。

- [ ] **Step 5: Commit**

```bash
git add desktop/ui/src/stores/screen.ts desktop/ui/src/pages/Screen.tsx desktop/ui/src/stores/screen.test.ts
git commit -F - <<'EOF'
feat(ui): screen as-of run via global task store (progress + cross-page persist)
EOF
```

---

### Task 5: 选股回测接入(screen store + ScreenBacktestResult.tsx)

**Files:**
- Modify: `desktop/ui/src/stores/screen.ts`
- Modify: `desktop/ui/src/components/ScreenBacktestResult.tsx`
- Test: `desktop/ui/src/stores/screen.test.ts`(追加 backtest 用例)

**Interfaces:**
- Consumes: `api.screenBacktestRun(config,from,to,top,rebalance,costBps)→string`;回测 run id 是 `string`(任务 result 为新归档 run 的 id 或报告)。先 READ `ScreenBacktestResult.tsx` 确认其当前如何取结果(是否 run 完再 `screenRunReport(id)`)。
- Produces: screen store `btTaskId/btRunId/btError/runBacktest(...)`(结果是 run id 字符串;done 后照原逻辑再拉报告)。

- [ ] **Step 1: READ** `desktop/ui/src/components/ScreenBacktestResult.tsx` —— 摸清它现在的运行/结果/listen 模式与"跑完取报告"的衔接(回测 task 的 `result` 是什么:run id 还是报告 DTO)。据此设 `btTaskId` + 结果字段(若 result 是 run id 字符串,则 `btRunId: string|null`,done 后触发既有 `screenRunReport`/列表刷新逻辑)。

- [ ] **Step 2: 写失败测试**(追加 `screen.test.ts`):

```typescript
test("runBacktest tracks task and captures run id on done", async () => {
  const real = useScreen.getState().api;
  useScreen.setState({ api: { ...real, screenBacktestRun: async () => "tb1" }, btTaskId: null, btRunId: null, btError: null });
  useTasks.setState({ tasks: {}, startedAt: {}, inited: true });
  await useScreen.getState().runBacktest("cfg", "2024-01-01", "2026-01-01", 50, 5, 20);
  expect(useScreen.getState().btTaskId).toBe("tb1");
  useTasks.getState().ingest({ id: "tb1", kind: "screen_backtest", status: "done", progress: { pct: 1, stage: "归档", detail: "" }, error: null, result: "run_123" as any });
  expect(useScreen.getState().btRunId).toBe("run_123");
});
```

- [ ] **Step 3: 跑确认失败** → FAIL。

- [ ] **Step 4: 实现** — `stores/screen.ts` 按模式加 `btTaskId/btRunId/btError/runBacktest`(`<apiMethod>`=`screenBacktestRun`;done:`set({ btRunId: info.result as string })`)。`ScreenBacktestResult.tsx`:删本地 running/listen;运行中渲染 `<TaskRunning info=... onCancel=...>`;done(`btRunId` 变化)后照既有逻辑拉报告/刷新归档列表(用 `useEffect([st.btRunId])` 触发原 `screenRunReport`)。错误显示 `btError`。

- [ ] **Step 5: 跑确认通过** `npm --prefix desktop/ui run test -- --run src/stores/screen.test.ts` → PASS;`tsc --noEmit` → 0;`npm --prefix desktop/ui run test -- --run 2>&1 | tail -6` 全过。

- [ ] **Step 6: Commit**

```bash
git add desktop/ui/src/stores/screen.ts desktop/ui/src/components/ScreenBacktestResult.tsx desktop/ui/src/stores/screen.test.ts
git commit -F - <<'EOF'
feat(ui): screen backtest run via global task store
EOF
```

---

### Task 6: 部署接入(deploy store + Deploy.tsx)

**Files:**
- Modify: `desktop/ui/src/stores/deploy.ts`
- Modify: `desktop/ui/src/pages/Deploy.tsx`
- Test: `desktop/ui/src/stores/deploy.test.ts`(追加 run-month 用例)

**Interfaces:**
- Consumes: `api.deployRunMonth(asOf)→string`;`@bindings/DeployMonthDto`;现有 `deploy.ts` 有 `preview`/`commit`/`load`(沿用),新增 run 跟踪。
- Produces: deploy store `runTaskId/runError/runMonth(asOf)`;`preview` 仍为预览结果(done 时 `set({ preview: info.result as DeployMonthDto })`)。

- [ ] **Step 1: 写失败测试**(追加 `deploy.test.ts`):

```typescript
import { test, expect } from "vitest";
import { useDeploy } from "./deploy";
import { useTasks } from "./tasks";

test("runMonth tracks task and captures preview on done", async () => {
  const real = useDeploy.getState().api;
  useDeploy.setState({ api: { ...real, deployRunMonth: async () => "td1" }, runTaskId: null, preview: null, runError: null });
  useTasks.setState({ tasks: {}, startedAt: {}, inited: true });
  await useDeploy.getState().runMonth("2026-06-16");
  expect(useDeploy.getState().runTaskId).toBe("td1");
  useTasks.getState().ingest({ id: "td1", kind: "deploy_month", status: "done", progress: { pct: 1, stage: "选股", detail: "" }, error: null, result: { as_of: "2026-06-16", picks: [], diff: [], proj_nav: 1, proj_excess: 0, realized_ret: 0 } as any });
  expect(useDeploy.getState().preview?.as_of).toBe("2026-06-16");
});
```

- [ ] **Step 2: 跑确认失败** → FAIL。

- [ ] **Step 3: 实现** — `stores/deploy.ts`:加 `runTaskId: string|null`、`runError: string|null`、`runMonth(asOf)`(按模式;done→`set({ preview: info.result as DeployMonthDto })`,failed→`runError`)。`Deploy.tsx`:删本地 `running` + `listen`(`runMonth` 本地函数);`import TaskRunning` + `useTaskInfo/useTaskStartedAt`;预览区:运行中 `<TaskRunning info onCancel>`,否则原"预览卡 + 确认调仓"。运行按钮 `loading={running}`。

- [ ] **Step 4: 跑确认通过** `npm --prefix desktop/ui run test -- --run src/stores/deploy.test.ts` → PASS;`tsc --noEmit` → 0。

- [ ] **Step 5: Commit**

```bash
git add desktop/ui/src/stores/deploy.ts desktop/ui/src/pages/Deploy.tsx desktop/ui/src/stores/deploy.test.ts
git commit -F - <<'EOF'
feat(ui): deploy run-month via global task store (progress + cross-page persist)
EOF
```

---

### Task 7: 因子 / 研究跑轮 / 回测中心 接入

**Files:**
- Modify: `desktop/ui/src/stores/factor.ts` + `desktop/ui/src/pages/Factor.tsx`
- Modify: `desktop/ui/src/stores/research.ts` + `desktop/ui/src/components/RunRoundForm.tsx`(研究跑轮)
- Modify: `desktop/ui/src/stores/backtest.ts`(若存在;否则 `pages/Backtest.tsx` 内的运行态)+ `desktop/ui/src/pages/Backtest.tsx`
- Test: 各自 store test 追加一条 done-capture 用例

**Interfaces:**
- 因子:`api.factorRun(factors,horizon,layers,sample)→string`,result=`@bindings/FactorReportDto`。
- 研究:`api.iterRunRound(config,note,axis,top,benchmark,rebalance)→string`,result 形态先 READ `research.ts`/`RunRoundForm.tsx` 确认(可能是轮次号/台账刷新)。
- 回测:`api.backtestRun(config)→string`,result=run id(done 后拉 `runSummary` 等,READ `Backtest.tsx` 确认现有衔接)。

- [ ] **Step 1: READ** 三处当前实现(`stores/{factor,research,backtest}.ts` 若有、`pages/{Factor,Backtest}.tsx`、`components/RunRoundForm.tsx`),确认各自 result 形态与"跑完后续"衔接。

- [ ] **Step 2: 因子** — 写失败测试(`factor.test.ts` 追加):

```typescript
import { test, expect } from "vitest";
import { useFactor } from "./factor";
import { useTasks } from "./tasks";
test("factor run captures report on done", async () => {
  const real = useFactor.getState().api;
  useFactor.setState({ api: { ...real, factorRun: async () => "tf1" }, runTaskId: null, report: null, runError: null });
  useTasks.setState({ tasks: {}, startedAt: {}, inited: true });
  await useFactor.getState().runFactor([["v", "fund.bps/close"]], 16, 5, 16);
  expect(useFactor.getState().runTaskId).toBe("tf1");
  useTasks.getState().ingest({ id: "tf1", kind: "factor", status: "done", progress: { pct: 1, stage: "因子", detail: "" }, error: null, result: { n_symbols: 1, factors: [] } as any });
  expect(useFactor.getState().report).toBeTruthy();
});
```

- [ ] **Step 3** 实现因子:`stores/factor.ts` 加 `runTaskId/report/runError/runFactor` 按模式(done→`set({ report: info.result as FactorReportDto })`);`Factor.tsx` 删本地 `running`+`listen`,改 store + `<TaskRunning>`。跑测试 PASS。

- [ ] **Step 4** 研究跑轮:同模式接 `stores/research.ts` + `RunRoundForm.tsx`(`<apiMethod>`=`iterRunRound`;done 后照原逻辑刷新台账 `iterLedger`)。加一条 store test(发起置 taskId、done 触发刷新标志)。跑测试 PASS。

- [ ] **Step 5** 回测中心:同模式接 `stores/backtest.ts`(或页内)+ `Backtest.tsx`(`<apiMethod>`=`backtestRun`;done→run id,照原逻辑拉结果)。加一条 store test。跑测试 PASS。

- [ ] **Step 6: 验证 + Commit** `tsc --noEmit` 0;`npm --prefix desktop/ui run test -- --run 2>&1 | tail -6` 全过。

```bash
git add desktop/ui/src/stores/factor.ts desktop/ui/src/pages/Factor.tsx desktop/ui/src/stores/research.ts desktop/ui/src/components/RunRoundForm.tsx desktop/ui/src/stores/backtest.ts desktop/ui/src/pages/Backtest.tsx desktop/ui/src/stores/factor.test.ts desktop/ui/src/stores/research.test.ts desktop/ui/src/stores/backtest.test.ts
git commit -F - <<'EOF'
feat(ui): factor / research-round / backtest runs via global task store
EOF
```

> 注:实际 `git add` 仅列出存在且被改的文件(部分 store/test 可能页内态而无独立 store;READ 后据实调整)。

---

### Task 8: 收尾闸 + 文档 + 记忆

- [ ] **Step 1: 前端全量闸** `node desktop/ui/node_modules/typescript/bin/tsc --noEmit -p desktop/ui/tsconfig.app.json` → 0;`npm --prefix desktop/ui run test -- --run` → 全过;`npm --prefix desktop/ui run build` → 成功。
- [ ] **Step 2: 后端不变核验** 本项零后端改动:`grep -rn 'listen(\"task://progress\"' desktop/ui/src` 应**仅** `stores/tasks.ts` 一处(去重达成)。
- [ ] **Step 3: GUI 交互冒烟**(需图形界面,**release 构建跑** `cargo tauri dev --release`):选股指定日跑→显示阶段+已耗时+(粗)进度→**切到部署页再切回**→仍显示进度/结果;部署跑本月同;右上「任务」抽屉与页内进度一致;取消可用。
- [ ] **Step 4: 文档 + 记忆** `docs/desktop-screen-research.md` 加"任务运行体验"一节(全局任务态/页内进度/切页持久);更新记忆 `rquant-project.md`(task-ux 落地 + 工程教训:全局单监听去重、运行态/结果须在 store 否则切页丢)。
- [ ] **Step 5: Commit**

```bash
git add docs/ && git commit -F - <<'EOF'
docs(desktop): task-ux (global task state + in-page progress) usage; finalize
EOF
```

- [ ] **Step 6: finishing** 调用 superpowers:finishing-a-development-branch 收口。

---

## 自审备忘(写计划时已校)

- **spec 覆盖**:§2 全局 store→T1;§2 TaskRunning→T2;§2 TaskDrawer 去重 + App init→T3;§1 决策"所有任务页"→T4(选股 asof)/T5(选股回测)/T6(部署)/T7(因子+研究+回测);§5 错误/取消→各页 `*Error`+TaskRunning onCancel;§6 测试→各 store/组件 vitest + T8 闸/冒烟;§8 release→T8 Step3 冒烟用 release。
- **类型一致**:`useTasks`/`trackTask`/`useTaskInfo`/`useTaskStartedAt` 命名贯穿 T1→T2/T3/T4–7;域字段命名 `<x>TaskId/<x>Result/<x>Error`(asof/bt/run 等)前后一致;result 经 `info.result as <Dto>`。
- **粗粒度诚实**:TaskRunning 在 `pct∈(0,1)` 才用百分比,否则 Spin + 已耗时(T2),符合 spec §1/§5 不伪造。
- **去重**:仅 `stores/tasks.ts` 一处 `listen`(T3 删 TaskDrawer 的;各页本就要删),T8 Step2 grep 守住。
- **YAGNI**:不改后端进度;release 构建/抓取暂停属运维(spec §8),不在本计划代码内。
