import { render, screen, waitFor } from "@testing-library/react";
import "@testing-library/jest-dom";
import { HashRouter } from "react-router-dom";
import { App as AntApp } from "antd";
import Cockpit from "./Cockpit";
import { useCockpit } from "../stores/cockpit";
import type { OverviewDto } from "@bindings/OverviewDto";

const OVERVIEW: OverviewDto = {
  cards: [
    { book: "b1", title: "账本1 · sh600030 60m", kind: "single", status: "ok", advice: null,
      nav: 1.0539, total_return: 0.0539, max_drawdown: 0.0213, pos: 0, state_time: "2026-06-12T15:00:00",
      holdings: null, last_signal: null },
    { book: "b2", title: "账本2 · sh600036 60m", kind: "single", status: "empty",
      advice: "state 未建账:等待 15:35 schtask 首跑,或手动触发 run(收盘后)", nav: null, total_return: null,
      max_drawdown: null, pos: null, state_time: null, holdings: null, last_signal: null },
    { book: "b3", title: "账本3 · 组合 top3 日线", kind: "portfolio", status: "ok", advice: null,
      nav: null, total_return: null, max_drawdown: null, pos: null, state_time: "2026-06-11T15:00:00",
      holdings: [["sh600900", 0.5], ["sz000333", 0.5]], last_signal: null },
  ],
  diff: [{ symbol: "sh600900", action: "Hold", from_w: 0.5, to_w: 0.5 }],
  diff_t: "2026-06-12T15:00:00",
  runlog: { last_header: "==== Fri 06/12/2026 ====", ok: true, summary: "最近一次 run 正常收尾" },
  schtask: { next_run: "6/12/2026 3:35:00 PM", last_run: null, last_result: "267011", status: "Ready" },
};

test("cockpit renders three book cards, diff and run status", async () => {
  useCockpit.setState({
    api: { ...useCockpit.getState().api, cockpitOverview: async () => OVERVIEW },
  });
  render(
    <AntApp><HashRouter><Cockpit /></HashRouter></AntApp>
  );
  await waitFor(() => expect(screen.getByText("账本1 · sh600030 60m")).toBeInTheDocument());
  expect(screen.getAllByText(/未建账/).length).toBeGreaterThan(0);
  expect(screen.getByText(/sh600900 0.50/)).toBeInTheDocument();
  expect(screen.getByText("最近一次 run 正常收尾")).toBeInTheDocument();
});
