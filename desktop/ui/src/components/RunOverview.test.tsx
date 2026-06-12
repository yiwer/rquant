import { render, screen } from "@testing-library/react";
import "@testing-library/jest-dom";
import { vi } from "vitest";
import type { RunSummaryDto } from "@bindings/RunSummaryDto";

vi.mock("../api/ipc", () => ({ api: { runEquity: async () => [] } }));
vi.mock("echarts", () => ({ init: () => ({ setOption: () => {}, resize: () => {}, dispose: () => {} }) }));

import RunOverview from "./RunOverview";

const SUMMARY: RunSummaryDto = {
  meta: {
    id: "20260612-210000-0a1b-01",
    kind: "sim_hard",
    name: "n",
    tree_name: "t",
    created: "2026-06-12T21:00:00",
    ok: true,
    error: null,
  },
  config: {
    tree_path: "examples/x.yaml",
    primary_path: "paper/p.csv",
    mode: "sim_hard",
    cost_bps: 10,
    warmup: 80,
    window: 100,
    initial_capital: 100000,
    fetch: null,
  },
  total_return: 0.246,
  max_drawdown: 0.137,
  n_round_trips: 33,
  win_rate: 0.52,
  avg_hold_bars: 9.1,
  turnover: 21.4,
  buy_and_hold: -0.232,
  sharpe: 1.21,
  final_equity: 124600,
  net_pnl: 24600,
  raw: null,
};

test("overview shows money metrics from initial capital", () => {
  render(<RunOverview summary={SUMMARY} />);
  expect(screen.getByText("期末资产")).toBeInTheDocument();
  expect(screen.getByText(/124,600/)).toBeInTheDocument();
  expect(screen.getByText("24.60%")).toBeInTheDocument();
});
