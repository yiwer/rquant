import { test, expect, afterEach } from "vitest";
import { useDeploy } from "./deploy";
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
