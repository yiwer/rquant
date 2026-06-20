import { test, expect, afterEach } from "vitest";
import { useScreen } from "./screen";
import { useTasks } from "./tasks";
const real = useScreen.getState().api;
afterEach(() => useScreen.setState({ api: real, configs: [], runs: [], report: null, indexRel: null, benchmark: "csi300" }));

test("runAsof tracks task and captures result on done", async () => {
  const real = useScreen.getState().api;
  useScreen.setState({ api: { ...real, screenAsof: async () => "ta1" }, asofTaskId: null, asofResult: null, asofError: null });
  useTasks.setState({ tasks: {}, startedAt: {}, inited: true });
  await useScreen.getState().runAsof("cfg", "2026-06-16", 50);
  expect(useScreen.getState().asofTaskId).toBe("ta1");
  useTasks.getState().ingest({ id: "ta1", kind: "screen_asof", status: "done", progress: { pct: 1, stage: "选股", detail: "" }, error: null, result: { config: "cfg", as_of: "2026-06-16", n_universe: 10, top: 50, rows: [] } as any });
  expect(useScreen.getState().asofResult?.n_universe).toBe(10);
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
