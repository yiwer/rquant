import { render } from "@testing-library/react";
import "@testing-library/jest-dom";
import { vi } from "vitest";

const setOption = vi.fn();
vi.mock("echarts", () => ({ init: () => ({ setOption, resize: () => {}, dispose: () => {} }) }));

import KlineChart from "./KlineChart";
import type { BarDto } from "@bindings/BarDto";

const BARS: BarDto[] = [
  { t: "2026-01-01T10:00:00", open: 10, high: 10.5, low: 9.8, close: 10.2, volume: 100 },
  { t: "2026-01-01T11:00:00", open: 10.2, high: 10.8, low: 10.1, close: 10.6, volume: 120 },
];

test("kline builds candlestick + volume series with markers", () => {
  render(
    <KlineChart bars={BARS} markers={[{ t: "2026-01-01T11:00:00", price: 10.2, kind: "entry", label: "买" }]} />
  );
  expect(setOption).toHaveBeenCalled();
  const opt = setOption.mock.calls[0][0];
  expect(opt.series[0].type).toBe("candlestick");
  expect(opt.series.at(-1).type).toBe("bar");
  expect(opt.series[0].markPoint.data).toHaveLength(1);
});
