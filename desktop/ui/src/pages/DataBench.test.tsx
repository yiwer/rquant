import { render, screen, waitFor } from "@testing-library/react";
import "@testing-library/jest-dom";
import { vi } from "vitest";

vi.mock("../api/ipc", () => ({
  api: {
    dataCsvList: async () => [
      { path: "paper/p_sh600030.csv", rows: 942, first_t: "2025-06-01T10:00:00", last_t: "2026-06-12T15:00:00" },
      { path: "paper/broken.csv", rows: null, first_t: null, last_t: null },
    ],
    universeList: async () => [
      { path: "deploy/universe_10.csv", name: "universe_10", frozen: true,
        entries: [{ symbol: "sh600519", primary: "paper/pd_sh600519.csv" }] },
    ],
    dataReadBars: async () => [],
    dataEvalFactor: async () => [],
    fetchBatch: async () => "t1",
  },
}));
vi.mock("echarts", () => ({ init: () => ({ setOption: () => {}, resize: () => {}, dispose: () => {} }) }));

import { App as AntApp } from "antd";
import DataBench from "./DataBench";

test("data bench lists csvs with freshness and universes", async () => {
  render(<AntApp><DataBench /></AntApp>);
  await waitFor(() => expect(screen.getByText("paper/p_sh600030.csv")).toBeInTheDocument());
  expect(screen.getByText(/942 根/)).toBeInTheDocument();
  expect(screen.getByText("解析失败")).toBeInTheDocument();
  expect(screen.getByText("universe_10")).toBeInTheDocument();
  expect(screen.getByText("deploy 只读")).toBeInTheDocument();
});
