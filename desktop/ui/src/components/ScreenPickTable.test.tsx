import { render, screen, fireEvent } from "@testing-library/react";
import "@testing-library/jest-dom";
import { test, expect } from "vitest";
import ScreenPickTable from "./ScreenPickTable";
import type { ScreenResultDto } from "@bindings/ScreenResultDto";

const mk = (rank: number, symbol: string, selected: boolean) => ({
  rank, symbol, quality_score: 0.9, speculative_score: 0.05, combined_score: 0.9 - rank * 0.01,
  tags: ["质量"], selected, reasons: [],
});
const R: ScreenResultDto = { config: "c.yaml", as_of: "2026-06-12", n_universe: 1073, top: 50, rows: [
  mk(1, "sh601398", true),  // 工商银行 (not ST, selected)
  mk(2, "sh600759", true),  // ST洲际 (ST, selected → should be filtered by default)
  mk(3, "sh600000", false), // 浦发银行 (not selected)
] };

test("renders selected picks with 中文名称, hides unselected", () => {
  render(<ScreenPickTable result={R} />);
  expect(screen.getByText("sh601398")).toBeInTheDocument();
  expect(screen.getByText("工商银行")).toBeInTheDocument();          // name column
  expect(screen.queryByText("sh600000")).not.toBeInTheDocument();   // unselected → 数量 fix
});

test("ST/*ST excluded by default", () => {
  render(<ScreenPickTable result={R} />);
  expect(screen.queryByText("sh600759")).not.toBeInTheDocument();   // ST洲际 filtered out
  expect(screen.getByText(/已剔除 1 只 ST/)).toBeInTheDocument();
});

test("toggling the ST switch off shows ST again", () => {
  render(<ScreenPickTable result={R} />);
  expect(screen.queryByText("sh600759")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("switch"));
  expect(screen.getByText("sh600759")).toBeInTheDocument();
});
