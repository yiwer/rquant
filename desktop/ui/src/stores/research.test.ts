import { test, expect, afterEach } from "vitest";
import { useResearch } from "./research";
import { useTasks } from "./tasks";

const real = useResearch.getState().api;
afterEach(() => useResearch.setState({ api: real, ledger: [], queue: null, card: null, error: null, runTaskId: null, runError: null }));

test("runRound sets runTaskId on start", async () => {
  useResearch.setState({ api: { ...real, iterRunRound: async () => "tr1" }, runTaskId: null, runError: null });
  useTasks.setState({ tasks: {}, startedAt: {}, inited: true });
  await useResearch.getState().runRound("cfg.yaml", "note", "daily", 50, "csi300", 1);
  expect(useResearch.getState().runTaskId).toBe("tr1");
});

test("runRound triggers ledger refresh on done", async () => {
  let loadCalled = false;
  useResearch.setState({
    api: { ...real, iterRunRound: async () => "tr1", iterLedger: async () => { loadCalled = true; return []; }, iterQueue: async () => ({ pending: [], active: null }) as any },
    runTaskId: null, runError: null,
  });
  useTasks.setState({ tasks: {}, startedAt: {}, inited: true });
  await useResearch.getState().runRound("cfg.yaml", "note", "daily", 50, "csi300", 1);
  useTasks.getState().ingest({ id: "tr1", kind: "iter_round", status: "done", progress: { pct: 1, stage: "跑轮", detail: "" }, error: null, result: null });
  // allow microtask queue to flush
  await Promise.resolve();
  expect(loadCalled).toBe(true);
});

test("runRound sets runError on failed", async () => {
  useResearch.setState({
    api: { ...real, iterRunRound: async () => "tr2", iterLedger: async () => [], iterQueue: async () => ({ pending: [], active: null }) as any },
    runTaskId: null, runError: null,
  });
  useTasks.setState({ tasks: {}, startedAt: {}, inited: true });
  await useResearch.getState().runRound("cfg.yaml", "note", "daily", 50, "csi300", 1);
  useTasks.getState().ingest({ id: "tr2", kind: "iter_round", status: "failed", progress: { pct: 0, stage: "跑轮", detail: "" }, error: "跑轮失败", result: null });
  await Promise.resolve();
  expect(useResearch.getState().runError).toBeTruthy();
});
