import { render, screen } from "@testing-library/react";
import "@testing-library/jest-dom";
import { test, expect, afterEach } from "vitest";
import { App as AntApp } from "antd";
import ScreenBacktestResult from "./ScreenBacktestResult";
import { useScreen } from "../stores/screen";

const real = useScreen.getState().api;
afterEach(() =>
  useScreen.setState({
    api: real,
    runs: [],
    report: null,
    indexRel: null,
    configs: [],
    indices: [],
  })
);

test("shows index-relative excess and OOS by default", () => {
  useScreen.setState({
    api: {
      ...real,
      screenRunsList: async () => [],
      screenRunReport: async () => null as never,
      screenIndexRelative: async () => null as never,
      screenConfigsList: async () => [],
      indexList: async () => [],
    },
    benchmark: "csi300",
    report: {
      meta: {} as never,
      net_total_return: 3.24,
      gross_total_return: 3.5,
      abs_sharpe: 1.13,
      max_drawdown: 0.19,
      turnover: 2.4,
      break_even: 164,
      nav: [],
      tag_attribution: [],
      regime_slices: [],
      quality_layers: [],
    },
    indexRel: {
      benchmark: "csi300",
      excess_cum: 2.96,
      curve: [],
      per_regime: [{ label: "2024-26_OOS", excess: 0.64 }],
    },
  });
  render(
    <AntApp>
      <ScreenBacktestResult />
    </AntApp>
  );
  expect(screen.getByText(/指数相对/)).toBeInTheDocument();
  expect(screen.getByText("2024-26_OOS")).toBeInTheDocument();
});
