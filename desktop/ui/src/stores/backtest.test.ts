import { test, expect, afterEach } from "vitest";
import { useBacktest } from "./backtest";
import { useTasks } from "./tasks";
import type { BacktestConfigDto } from "@bindings/BacktestConfigDto";

const real = useBacktest.getState().api;
afterEach(() => useBacktest.setState({ api: real, runs: [], selectedId: null, summary: null, selectError: null, compareIds: [], runTaskId: null, runError: null }));

const dummyConfig: BacktestConfigDto = {
  tree_path: "tree.yaml",
  primary_path: "data.csv",
  mode: "sim_hard",
  cost_bps: 10,
  warmup: 80,
  window: 100,
  initial_capital: 100000,
  fetch: null,
};

test("backtestRun sets runTaskId on start", async () => {
  useBacktest.setState({ api: { ...real, backtestRun: async () => "tb1" }, runTaskId: null, runError: null });
  useTasks.setState({ tasks: {}, startedAt: {}, inited: true });
  await useBacktest.getState().backtestRun(dummyConfig);
  expect(useBacktest.getState().runTaskId).toBe("tb1");
});

test("backtestRun triggers loadRuns and select on done", async () => {
  let loadCalled = false;
  useBacktest.setState({
    api: {
      ...real,
      backtestRun: async () => "tb1",
      runsList: async () => { loadCalled = true; return []; },
      runSummary: async (_id: string) => null as any,
    },
    runTaskId: null, runError: null,
  });
  useTasks.setState({ tasks: {}, startedAt: {}, inited: true });
  await useBacktest.getState().backtestRun(dummyConfig);
  useTasks.getState().ingest({ id: "tb1", kind: "backtest", status: "done", progress: { pct: 1, stage: "归档", detail: "" }, error: null, result: "run_abc" });
  await Promise.resolve();
  expect(loadCalled).toBe(true);
});

test("backtestRun sets runError on failed", async () => {
  useBacktest.setState({ api: { ...real, backtestRun: async () => "tb2" }, runTaskId: null, runError: null });
  useTasks.setState({ tasks: {}, startedAt: {}, inited: true });
  await useBacktest.getState().backtestRun(dummyConfig);
  useTasks.getState().ingest({ id: "tb2", kind: "backtest", status: "failed", progress: { pct: 0, stage: "归档", detail: "" }, error: "回测失败", result: null });
  expect(useBacktest.getState().runError).toBeTruthy();
});
