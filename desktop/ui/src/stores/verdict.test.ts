import { test, expect, afterEach } from "vitest";
import { useVerdict } from "./verdict";

const real = useVerdict.getState().api;
afterEach(() => useVerdict.setState({ api: real, reports: [], verdict: null, error: null }));

test("certify stores verdict", async () => {
  useVerdict.setState({
    api: {
      ...real,
      evalCertify: async () => ({
        strategy: "x",
        n_symbols: 3,
        certified: true,
        gates: [],
        failed_gates: [],
      }),
    },
  });
  await useVerdict.getState().certify(["a.json"], "x");
  expect(useVerdict.getState().verdict?.certified).toBe(true);
});
