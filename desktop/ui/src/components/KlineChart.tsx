import { useEffect, useRef } from "react";
import * as echarts from "echarts";
import type { BarDto } from "@bindings/BarDto";

export interface TradeMarker {
  t: string;
  price: number;
  kind: "entry" | "exit";
  label: string;
}

export interface Overlay {
  name: string;
  points: { t: string; value: number | null }[];
}

/** 通用 K 线:主图 candlestick(+overlay 线/markers),副图 volume。 */
export default function KlineChart({
  bars, markers = [], overlays = [], height = 420,
}: {
  bars: BarDto[];
  markers?: TradeMarker[];
  overlays?: Overlay[];
  height?: number;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!ref.current || !bars.length) return;
    const chart = echarts.init(ref.current);
    const times = bars.map((b) => b.t);
    const idx = new Map(times.map((t, i) => [t, i]));
    chart.setOption({
      tooltip: { trigger: "axis", axisPointer: { type: "cross" } },
      axisPointer: { link: [{ xAxisIndex: "all" }] },
      grid: [
        { left: 64, right: 16, top: 24, height: height - 200 },
        { left: 64, right: 16, top: height - 150, height: 80 },
      ],
      xAxis: [
        { type: "category", data: times, gridIndex: 0 },
        { type: "category", data: times, gridIndex: 1, axisLabel: { show: false } },
      ],
      yAxis: [
        { type: "value", scale: true, gridIndex: 0 },
        { type: "value", gridIndex: 1, axisLabel: { show: false } },
      ],
      dataZoom: [{ type: "inside", xAxisIndex: [0, 1] }, { type: "slider", xAxisIndex: [0, 1], top: height - 44 }],
      series: [
        {
          name: "K", type: "candlestick", xAxisIndex: 0, yAxisIndex: 0,
          data: bars.map((b) => [b.open, b.close, b.low, b.high]),
          itemStyle: { color: "#cf1322", color0: "#3f8600", borderColor: "#cf1322", borderColor0: "#3f8600" },
          markPoint: markers.length
            ? {
                data: markers
                  .filter((m) => idx.has(m.t))
                  .map((m) => ({
                    coord: [idx.get(m.t), m.price],
                    value: m.label,
                    symbol: m.kind === "entry" ? "arrow" : "pin",
                    symbolRotate: m.kind === "entry" ? 0 : 180,
                    itemStyle: { color: m.kind === "entry" ? "#1677ff" : "#fa8c16" },
                  })),
                label: { fontSize: 10 },
              }
            : undefined,
        },
        // overlay 用 Map 查值(O(1)),避免逐点 find 的 O(n²)
        ...overlays.map((o) => {
          const om = new Map(o.points.map((p) => [p.t, p.value]));
          return {
            name: o.name, type: "line" as const, xAxisIndex: 0, yAxisIndex: 0, showSymbol: false,
            data: times.map((t) => om.get(t) ?? null),
            connectNulls: false,
          };
        }),
        {
          name: "成交量", type: "bar", xAxisIndex: 1, yAxisIndex: 1,
          data: bars.map((b) => b.volume),
          itemStyle: { color: "rgba(22,119,255,.45)" },
        },
      ],
      legend: overlays.length ? { top: 0 } : undefined,
    });
    const onResize = () => chart.resize();
    window.addEventListener("resize", onResize);
    return () => {
      window.removeEventListener("resize", onResize);
      chart.dispose();
    };
  }, [bars, markers, overlays, height]);
  return <div ref={ref} style={{ height }} />;
}
