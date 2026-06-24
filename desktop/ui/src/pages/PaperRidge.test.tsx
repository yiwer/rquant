import { render, screen, waitFor } from "@testing-library/react";
import { vi, test, expect, beforeEach } from "vitest";
import { App as AntApp } from "antd";
import PaperRidge from "./PaperRidge";

vi.mock("../components/StockDetailDrawer", () => ({ default: () => null }));

const status = {
  initialized: true, strategy: "ridge-on-gauss / 去相关岭组合",
  train_lo: "2018-02-06", train_hi: "2026-06-04", n_train_dates: 404,
  delta: 0.05, top_n: 3, cost_bps: 20,
  open_picks: ["sh600208","sz000039","sz301316"], closed: [],
  cum_net: 0, cum_excess: null, blend: null,
  names: { "sh600208": "新湖中宝", "sz000039": "中集集团", "sz301316": "海外通" },
};
vi.mock("../api/ipc", () => ({ api: {
  paperRidgeStatus: vi.fn(() => Promise.resolve(status)),
  paperRidgeAdvance: vi.fn(), paperRidgeRetrain: vi.fn(), paperBlendRecompute: vi.fn(),
  paperStockDetail: vi.fn(), dataReadBars: vi.fn(),
}}));

beforeEach(() => {
  vi.clearAllMocks();
});

test("renders frozen meta + open picks with names", async () => {
  render(<AntApp><PaperRidge /></AntApp>);
  await waitFor(() => expect(screen.getByText(/去相关岭组合/)).toBeTruthy());
  expect(screen.getByText(/sh600208 新湖中宝/)).toBeTruthy();
});
