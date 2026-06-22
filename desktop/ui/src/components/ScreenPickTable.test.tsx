import { render, screen, fireEvent } from "@testing-library/react";
import "@testing-library/jest-dom";
import { test, expect } from "vitest";
import ScreenPickTable from "./ScreenPickTable";
import type { ScreenResultDto } from "@bindings/ScreenResultDto";

const mk = (rank: number, symbol: string, selected: boolean) => ({
  rank, symbol, quality_score: 0.9, speculative_score: 0.05, combined_score: 0.9 - rank * 0.01,
  tags: ["质量"], selected, reasons: [],
});
// top-2 集中口径：引擎原始 top-2 = rank1(非ST) + rank2(ST)；rank3(非ST,未入选) 用于验证回补。
const R: ScreenResultDto = { config: "c.yaml", as_of: "2026-06-12", n_universe: 1073, top: 2, rows: [
  mk(1, "sh601398", true),  // 工商银行 (not ST, selected)
  mk(2, "sh600759", true),  // ST洲际 (ST, selected → filtered by default)
  mk(3, "sh600000", false), // 浦发银行 (not ST, NOT selected → backfills top-2 after ST dropped)
] };

test("renders picks with 中文名称", () => {
  render(<ScreenPickTable result={R} />);
  expect(screen.getByText("sh601398")).toBeInTheDocument();
  expect(screen.getByText("工商银行")).toBeInTheDocument();          // name column
});

test("ST/*ST excluded by default and top-N backfills from full ranking", () => {
  render(<ScreenPickTable result={R} />);
  expect(screen.queryByText("sh600759")).not.toBeInTheDocument();   // ST洲际 filtered out
  expect(screen.getByText("sh600000")).toBeInTheDocument();         // 浦发银行 promoted into the vacated top-2 slot
  expect(screen.getByText(/剔除 1 只 ST，回补至 top-2/)).toBeInTheDocument();
});

test("toggling the ST switch off shows raw top-N (ST back, backfill gone)", () => {
  render(<ScreenPickTable result={R} />);
  expect(screen.queryByText("sh600759")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("switch"));
  expect(screen.getByText("sh600759")).toBeInTheDocument();         // raw selected top-2 restored
  expect(screen.queryByText("sh600000")).not.toBeInTheDocument();   // unselected → not shown when ST filter off
});
