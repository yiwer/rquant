import { useEffect, useRef, useState } from "react";
import { Card, Table, Typography } from "antd";
import * as echarts from "echarts";
import type { RunSummaryDto } from "@bindings/RunSummaryDto";
import type { EquityPointDto } from "@bindings/EquityPointDto";
import { api } from "../api/ipc";

function OverlayChart({ a, b, an, bn }: { a: EquityPointDto[]; b: EquityPointDto[]; an: string; bn: string }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!ref.current) return;
    const chart = echarts.init(ref.current);
    // 各自时间轴可能不同——以并集为类目轴,缺位 null 断线
    const times = Array.from(new Set([...a.map((p) => p.t), ...b.map((p) => p.t)])).sort();
    const ma = new Map(a.map((p) => [p.t, p.nav]));
    const mb = new Map(b.map((p) => [p.t, p.nav]));
    chart.setOption({
      tooltip: { trigger: "axis" },
      legend: { top: 0 },
      xAxis: { type: "category", data: times },
      yAxis: { type: "value", scale: true },
      series: [
        { name: an, type: "line", showSymbol: false, connectNulls: false, data: times.map((t) => ma.get(t) ?? null) },
        { name: bn, type: "line", showSymbol: false, connectNulls: false, data: times.map((t) => mb.get(t) ?? null) },
      ],
      grid: { left: 56, right: 16, top: 28, bottom: 24 },
    });
    const onResize = () => chart.resize();
    window.addEventListener("resize", onResize);
    return () => {
      window.removeEventListener("resize", onResize);
      chart.dispose();
    };
  }, [a, b, an, bn]);
  return <div ref={ref} style={{ height: 280 }} />;
}

const pct = (v: number | null | undefined) => (v == null ? "—" : `${(v * 100).toFixed(2)}%`);

export default function CompareView({ ids }: { ids: [string, string] }) {
  const [sums, setSums] = useState<RunSummaryDto[]>([]);
  const [curves, setCurves] = useState<EquityPointDto[][]>([[], []]);
  const [loadErr, setLoadErr] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setSums([]);
    setCurves([[], []]);
    setLoadErr(null);
    void Promise.all(ids.map((id) => api.runSummary(id))).then((s) => {
      if (!cancelled) setSums(s);
    }).catch((e) => {
      if (!cancelled) setLoadErr(String(e));
    });
    void Promise.all(
      ids.map((id) => api.runEquity(id).catch((): EquityPointDto[] => []))
    ).then((c) => {
      if (!cancelled) setCurves(c);
    });
    return () => { cancelled = true; };
  }, [ids[0], ids[1]]); // eslint-disable-line react-hooks/exhaustive-deps

  if (loadErr) return <Typography.Text type="danger">对比加载失败: {loadErr}</Typography.Text>;
  if (sums.length < 2) return <Typography.Text type="secondary">加载对比…</Typography.Text>;
  const [a, b] = sums;
  const rows = [
    { k: "总收益", a: pct(a.total_return), b: pct(b.total_return) },
    { k: "最大回撤", a: pct(a.max_drawdown), b: pct(b.max_drawdown) },
    { k: "Sharpe", a: a.sharpe?.toFixed(2) ?? "—", b: b.sharpe?.toFixed(2) ?? "—" },
    { k: "交易数", a: a.n_round_trips ?? "—", b: b.n_round_trips ?? "—" },
    { k: "胜率", a: pct(a.win_rate), b: pct(b.win_rate) },
    { k: "换手", a: a.turnover?.toFixed(1) ?? "—", b: b.turnover?.toFixed(1) ?? "—" },
    { k: "bh对照", a: pct(a.buy_and_hold), b: pct(b.buy_and_hold) },
  ];
  return (
    <div>
      <Card size="small" title="净值曲线叠加(nav 口径,资金无关)" style={{ marginBottom: 12 }}>
        {curves[0].length || curves[1].length ? (
          <OverlayChart a={curves[0]} b={curves[1]} an={a.meta.name} bn={b.meta.name} />
        ) : (
          <Typography.Text type="secondary">至少一侧无曲线(打分模式)</Typography.Text>
        )}
      </Card>
      <Table
        size="small"
        rowKey="k"
        pagination={false}
        dataSource={rows}
        columns={[
          { title: "指标", dataIndex: "k" },
          { title: a.meta.name, dataIndex: "a" },
          { title: b.meta.name, dataIndex: "b" },
        ]}
      />
    </div>
  );
}
