import { test, expect, afterEach } from "vitest";
import { useFactor } from "./factor";
import { useTasks } from "./tasks";

const real = useFactor.getState().api;
afterEach(() => useFactor.setState({ api: real, runTaskId: null, report: null, runError: null }));

test("runFactor sets runTaskId on start", async () => {
  useFactor.setState({ api: { ...real, factorRun: async () => "tf1" }, runTaskId: null, report: null, runError: null });
  useTasks.setState({ tasks: {}, startedAt: {}, inited: true });
  await useFactor.getState().runFactor([["v", "fund.bps/close"]], 16, 5, 16);
  expect(useFactor.getState().runTaskId).toBe("tf1");
});

test("factor run captures report on done", async () => {
  useFactor.setState({ api: { ...real, factorRun: async () => "tf1" }, runTaskId: null, report: null, runError: null });
  useTasks.setState({ tasks: {}, startedAt: {}, inited: true });
  await useFactor.getState().runFactor([["v", "fund.bps/close"]], 16, 5, 16);
  expect(useFactor.getState().runTaskId).toBe("tf1");
  useTasks.getState().ingest({ id: "tf1", kind: "factor", status: "done", progress: { pct: 1, stage: "因子", detail: "" }, error: null, result: { n_symbols: 1, factors: [] } as any });
  expect(useFactor.getState().report).toBeTruthy();
});

test("factor run sets runError on failed", async () => {
  useFactor.setState({ api: { ...real, factorRun: async () => "tf2" }, runTaskId: null, report: null, runError: null });
  useTasks.setState({ tasks: {}, startedAt: {}, inited: true });
  await useFactor.getState().runFactor([["v", "fund.bps/close"]], 16, 5, 16);
  useTasks.getState().ingest({ id: "tf2", kind: "factor", status: "failed", progress: { pct: 0, stage: "因子", detail: "" }, error: "boom", result: null });
  expect(useFactor.getState().runError).toBeTruthy();
  expect(useFactor.getState().report).toBeNull();
});
