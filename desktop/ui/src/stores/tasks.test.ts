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
