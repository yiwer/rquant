import { render, screen } from "@testing-library/react";
import "@testing-library/jest-dom";
import { test, expect } from "vitest";
import ScreenPickTable from "./ScreenPickTable";
import type { ScreenResultDto } from "@bindings/ScreenResultDto";

const R: ScreenResultDto = { config: "value_pb_base.yaml", as_of: "2026-06-12", n_universe: 1073, top: 50, rows: [
  { rank: 1, symbol: "sh601398", quality_score: 0.91, speculative_score: 0.05, combined_score: 0.91, tags: ["质量"], selected: true, reasons: [{ tree: "value_pb", leaf: "L2", score: 0.9 }] },
  { rank: 2, symbol: "sh600000", quality_score: 0.80, speculative_score: 0.02, combined_score: 0.80, tags: ["价值"], selected: false, reasons: [] },
] };
test("renders ranked picks with scores", () => {
  render(<ScreenPickTable result={R} />);
  expect(screen.getByText("sh601398")).toBeInTheDocument();
  expect(screen.getAllByText("0.91").length).toBeGreaterThan(0);
  expect(screen.getByText("质量")).toBeInTheDocument();
});
test("shows 中文名称 from the bundled name map", () => {
  render(<ScreenPickTable result={R} />);
  expect(screen.getByText("工商银行")).toBeInTheDocument(); // sh601398 → name column
});
test("only selected picks shown so 数量(top-N) takes effect", () => {
  render(<ScreenPickTable result={R} />);
  expect(screen.queryByText("sh600000")).not.toBeInTheDocument(); // selected:false → filtered out
});
