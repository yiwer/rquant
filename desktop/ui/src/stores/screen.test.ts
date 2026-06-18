import { test, expect, afterEach } from "vitest";
import { useScreen } from "./screen";
const real = useScreen.getState().api;
afterEach(() => useScreen.setState({ api: real, configs: [], runs: [], report: null, indexRel: null, benchmark: "csi300" }));

test("setBenchmark refetches index-relative", async () => {
  let lastBench = "";
  useScreen.setState({ api: { ...real,
    screenIndexRelative: async (_id: string, b: string) => { lastBench = b; return { benchmark: b, excess_cum: 0.3, curve: [], per_regime: [] }; },
  } });
  await useScreen.getState().setBenchmark("scr-1", "csi500");
  expect(lastBench).toBe("csi500");
  expect(useScreen.getState().indexRel?.benchmark).toBe("csi500");
});
