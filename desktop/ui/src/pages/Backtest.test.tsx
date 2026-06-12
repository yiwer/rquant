import { render, screen, waitFor } from "@testing-library/react";
import "@testing-library/jest-dom";
import { HashRouter } from "react-router-dom";
import { App as AntApp } from "antd";
import Backtest from "./Backtest";
import { useBacktest } from "../stores/backtest";
import type { RunMetaDto } from "@bindings/RunMetaDto";

const RUNS: RunMetaDto[] = [
  { id: "20260612-210000-0a1b-01", kind: "sim_hard", name: "m2-mini × bars.csv",
    tree_name: "m2-mini", created: "2026-06-12T21:00:00", ok: true, error: null },
];

const realApi = useBacktest.getState().api;
afterEach(() => useBacktest.setState({ api: realApi, runs: [], selectedId: null, summary: null, compareIds: [] }));

test("backtest page lists archived runs", async () => {
  useBacktest.setState({
    api: { ...realApi, runsList: async () => RUNS, treeList: async () => [], dataCsvList: async () => [] },
  });
  render(
    <AntApp><HashRouter><Backtest /></HashRouter></AntApp>
  );
  await waitFor(() => expect(screen.getByText(/m2-mini × bars.csv/)).toBeInTheDocument());
  expect(screen.getByText(/历史留档\(1\)/)).toBeInTheDocument();
});
