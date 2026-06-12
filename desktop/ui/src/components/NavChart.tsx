import { useEffect, useRef } from "react";
import * as echarts from "echarts";
import type { JournalPointDto } from "@bindings/JournalPointDto";

export default function NavChart({ points, portfolio }: { points: JournalPointDto[]; portfolio: boolean }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!ref.current) return;
    const chart = echarts.init(ref.current);
    chart.setOption({
      tooltip: { trigger: "axis" },
      xAxis: { type: "category", data: points.map((p) => p.state_time) },
      yAxis: { type: "value", scale: true },
      series: [
        portfolio
          ? { name: "成员数", type: "line", step: "end", data: points.map((p) => p.members ?? null) }
          : { name: "nav", type: "line", data: points.map((p) => p.nav ?? null) },
      ],
      grid: { left: 48, right: 16, top: 24, bottom: 24 },
    });
    const onResize = () => chart.resize();
    window.addEventListener("resize", onResize);
    return () => {
      window.removeEventListener("resize", onResize);
      chart.dispose();
    };
  }, [points, portfolio]);
  return <div ref={ref} style={{ height: 260 }} />;
}
