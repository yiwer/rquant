import { render, screen, waitFor } from "@testing-library/react";
import "@testing-library/jest-dom";
import { vi, test, expect, beforeAll } from "vitest";
import { App as AntApp } from "antd";

// Polyfill ResizeObserver — jsdom does not implement it; antd Table needs it.
beforeAll(() => {
  if (typeof window.ResizeObserver === "undefined") {
    window.ResizeObserver = class ResizeObserver {
      observe() {}
      unobserve() {}
      disconnect() {}
    };
  }
});

vi.mock("./KlineChart", () => ({ default: () => null }));

vi.mock("../api/ipc", () => ({
  api: {
    paperStockDetail: vi.fn(async () => ({
      symbol: "sh600208",
      name: "新湖中宝",
      kday_path: "data/baostock/kday/sh600208.csv",
      asof: "2026-06-20",
      factors: [
        { key: "mom_20", value: 0.0512 },
        { key: "vol_20", value: null },
      ],
    })),
    dataReadBars: vi.fn(async () => []),
  },
}));

import StockDetailDrawer from "./StockDetailDrawer";

test("renders stock name and factor key when symbol is set", async () => {
  render(
    <AntApp>
      <StockDetailDrawer symbol="sh600208" onClose={() => {}} />
    </AntApp>,
  );
  // The Drawer renders into a portal on document.body.
  await waitFor(() =>
    expect(screen.getByText(/新湖中宝/)).toBeInTheDocument(),
  );
  expect(screen.getByText("mom_20")).toBeInTheDocument();
});
