import { render, screen, waitFor } from "@testing-library/react";
import "@testing-library/jest-dom";
import { vi } from "vitest";
import type { BarDto } from "@bindings/BarDto";
import type { TradeDto } from "@bindings/TradeDto";

const BARS: BarDto[] = [
  { t: "2026-01-01T10:00:00", open: 10, high: 10.5, low: 9.8, close: 10.2, volume: 100 },
  { t: "2026-01-01T11:00:00", open: 10.2, high: 10.8, low: 10.1, close: 10.6, volume: 120 },
];
const TRADES: TradeDto[] = [
  {
    entry_t: "2026-01-01T10:00:00",
    exit_t: "2026-01-01T11:00:00",
    entry_px: 10,
    exit_px: 10.6,
    max_abs_pos: 1,
    trip_return: 0.06,
    bars_held: 1,
    reason: "tree",
    pnl_amount: 6000,
  },
];

// 模拟真实时序：promise 下一拍 resolve → 组件经历 loading=true 首渲染 + 数据后二次渲染
// hooks 顺序违规会在二次渲染崩——本测试是白屏回归锁
vi.mock("../api/ipc", () => ({
  api: {
    dataReadBars: () => new Promise((r) => setTimeout(() => r(BARS), 0)),
    runTrades: () => new Promise((r) => setTimeout(() => r(TRADES), 0)),
  },
}));
vi.mock("echarts", () => ({
  init: () => ({ setOption: () => {}, resize: () => {}, dispose: () => {} }),
}));

import KlineSignalsView from "./KlineSignalsView";

test("kline signals survives loading->data two-phase render and shows counts", async () => {
  render(
    <KlineSignalsView
      runId="20260613-000000-0a1b-01"
      primaryPath="paper/p_x.csv"
      isSim={true}
    />
  );
  await waitFor(() =>
    expect(screen.getByText(/1 笔交易标注\/1 笔共计/)).toBeInTheDocument()
  );
});
