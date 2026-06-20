import { test, expect, afterEach } from "vitest";
import { useDeploy } from "./deploy";
import { useTasks } from "./tasks";
const real = useDeploy.getState().api;
afterEach(() => useDeploy.setState({ api: real, book: null, preview: null, error: null }));
test("commit clears preview and reloads book", async () => {
  let committed = "";
  useDeploy.setState({ api: { ...real, deployCommitMonth: async (a: string) => { committed = a; },
    deployBookRead: async () => ({ status: "ok", nav: 1.05, excess_total: 0.02, last_rebalance: "2026-06-30", holdings: [], nav_history: [], months: [] }) },
    preview: { as_of: "2026-06-30" } as any });
  await useDeploy.getState().commit("2026-06-30");
  expect(committed).toBe("2026-06-30");
  expect(useDeploy.getState().preview).toBeNull();
  expect(useDeploy.getState().book?.nav).toBe(1.05);
});

test("runMonth tracks task and captures preview on done", async () => {
  const real = useDeploy.getState().api;
  useDeploy.setState({ api: { ...real, deployRunMonth: async () => "td1" }, runTaskId: null, preview: null, runError: null });
  useTasks.setState({ tasks: {}, startedAt: {}, inited: true });
  await useDeploy.getState().runMonth("2026-06-16");
  expect(useDeploy.getState().runTaskId).toBe("td1");
  useTasks.getState().ingest({ id: "td1", kind: "deploy_month", status: "done", progress: { pct: 1, stage: "选股", detail: "" }, error: null, result: { as_of: "2026-06-16", picks: [], diff: [], proj_nav: 1, proj_excess: 0, realized_ret: 0 } as any });
  expect(useDeploy.getState().preview?.as_of).toBe("2026-06-16");
});
