import { test, expect, afterEach } from "vitest";
import { useAudit } from "./audit";

const real = useAudit.getState().api;
afterEach(() => useAudit.setState({ api: real, records: [], error: null }));

test("load fills records from api", async () => {
  useAudit.setState({
    api: {
      ...real,
      auditList: async () =>
        ([
          {
            id: "t1",
            kind: "screen_asof",
            params: {},
            started_at: "x",
            ended_at: "y",
            duration_ms: 1200,
            stages: [],
            files: [],
            status: "done",
            error: null,
            result_summary: "top-50",
            artifact: null,
          },
        ] as any),
    },
  });
  await useAudit.getState().load();
  expect(useAudit.getState().records[0].kind).toBe("screen_asof");
});
