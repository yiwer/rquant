import { render, screen, waitFor } from "@testing-library/react";
import "@testing-library/jest-dom";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { vi } from "vitest";
import type { BookDetailDto } from "@bindings/BookDetailDto";

const DETAIL: BookDetailDto = {
  card: { book: "b1", title: "账本1 · sh600030 60m", kind: "single", status: "ok", advice: null,
    nav: 1.05, total_return: 0.05, max_drawdown: 0.02, pos: 1, state_time: "2026-06-12T15:00:00",
    holdings: null, last_signal: null },
  snapshot: { pos: 1, entry_price: 6.1, bars_held: 4, nav: 1.05, peak_nav: 1.06, max_drawdown: 0.02,
    turnover: 2.4, last_increase_date: "2026-06-09", max_price_since_entry: 6.3, min_price_since_entry: 6.0,
    bars_since_exit: null, last_trip_return: null, trip: null },
  journal: [{ state_time: "2026-06-12T15:00:00", nav: 1.05, pos: 1, members: null }],
};

vi.mock("../api/ipc", () => ({ api: { bookDetail: async () => DETAIL } }));
// echarts 在 jsdom 无布局,mock 掉渲染细节
vi.mock("echarts", () => ({
  init: () => ({ setOption: () => {}, resize: () => {}, dispose: () => {} }),
}));

import BookDetail from "./BookDetail";

test("book detail renders snapshot fields", async () => {
  render(
    <MemoryRouter initialEntries={["/cockpit/b1"]}>
      <Routes><Route path="/cockpit/:book" element={<BookDetail />} /></Routes>
    </MemoryRouter>
  );
  await waitFor(() => expect(screen.getByText(/账本1/)).toBeInTheDocument());
  expect(screen.getByText("bars_held")).toBeInTheDocument();
  expect(screen.getByText("1.050000")).toBeInTheDocument();
});
