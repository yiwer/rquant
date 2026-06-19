import { render, screen } from "@testing-library/react";
import "@testing-library/jest-dom";
import { test, expect } from "vitest";
import FactorReport from "./FactorReport";
import type { FactorReportDto } from "@bindings/FactorReportDto";
const R: FactorReportDto = { n_symbols: 1073, sample: 16, horizon: 16, layers_q: 5, corr: null,
  factors: [{ name: "value_pb", expr: "1/(1+fund.pb)", n_periods: 100, ic_mean: 0.04, icir: 0.5, ic_t: 3.1, ic_pos_share: 0.6, rank_ic_mean: 0.05, rank_icir: 0.6, ic_decay: [{horizon:8, rank_ic:0.05}], layers: { q:5, ann_returns:[0.2,0.1,0.05,0.0,-0.1], spread_total:0.3, spread_sharpe:1.0, monotonicity:0.9 } }] };
test("factor report shows IC table", () => {
  render(<FactorReport report={R} />);
  expect(screen.getByText("value_pb")).toBeInTheDocument();
  expect(screen.getByText("0.040")).toBeInTheDocument();
});
