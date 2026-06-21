import { test, expect, afterEach } from "vitest";
import { useScreen } from "./screen";
import { useTasks } from "./tasks";
const real = useScreen.getState().api;
afterEach(() => useScreen.setState({ api: real, configs: [], runs: [], report: null, indexRel: null, benchmark: "csi300", configs15m: [], i15mTaskId: null, i15mResult: null, i15mError: null }));

test("runAsof tracks task and captures result on done", async () => {
  const real = useScreen.getState().api;
  useScreen.setState({ api: { ...real, screenAsof: async () => "ta1" }, asofTaskId: null, asofResult: null, asofError: null });
  useTasks.setState({ tasks: {}, startedAt: {}, inited: true });
  await useScreen.getState().runAsof("cfg", "2026-06-16", 50);
  expect(useScreen.getState().asofTaskId).toBe("ta1");
  useTasks.getState().ingest({ id: "ta1", kind: "screen_asof", status: "done", progress: { pct: 1, stage: "选股", detail: "" }, error: null, result: { config: "cfg", as_of: "2026-06-16", n_universe: 10, top: 50, rows: [] } as any });
  expect(useScreen.getState().asofResult?.n_universe).toBe(10);
});

test("runBacktest tracks task and captures run id on done", async () => {
  const real = useScreen.getState().api;
  useScreen.setState({ api: { ...real, screenBacktestRun: async () => "tb1" }, btTaskId: null, btRunId: null, btError: null });
  useTasks.setState({ tasks: {}, startedAt: {}, inited: true });
  await useScreen.getState().runBacktest("cfg", "2024-01-01", "2026-01-01", 50, 5, 20);
  expect(useScreen.getState().btTaskId).toBe("tb1");
  useTasks.getState().ingest({ id: "tb1", kind: "screen_backtest", status: "done", progress: { pct: 1, stage: "归档", detail: "" }, error: null, result: "run_123" as any });
  expect(useScreen.getState().btRunId).toBe("run_123");
});

test("setBenchmark refetches index-relative", async () => {
  let lastBench = "";
  useScreen.setState({ api: { ...real,
    screenIndexRelative: async (_id: string, b: string) => { lastBench = b; return { benchmark: b, excess_cum: 0.3, curve: [], per_regime: [] }; },
  } });
  await useScreen.getState().setBenchmark("scr-1", "csi500");
  expect(lastBench).toBe("csi500");
  expect(useScreen.getState().indexRel?.benchmark).toBe("csi500");
});

test("run15mAsof sets task id from screen15mAsof", async () => {
  useScreen.setState({ api: { ...real, screen15mAsof: async () => "t15m" } });
  await useScreen.getState().run15mAsof("examples/screen/intraday/15m_placeholder.yaml", "2026-06-18", 50);
  expect(useScreen.getState().i15mTaskId).toBe("t15m");
});
test("load15mConfigs fills configs15m", async () => {
  useScreen.setState({ api: { ...real, screen15mConfigsList: async () => [{ path: "p", name: "n", frozen: false, error: null }] } });
  await useScreen.getState().load15mConfigs();
  expect(useScreen.getState().configs15m.length).toBe(1);
});
