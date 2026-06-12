import { useEffect, useRef, useState } from "react";
import { Card, Col, Row, Segmented, Spin, Statistic, Typography } from "antd";
import * as echarts from "echarts";
import type { RunSummaryDto } from "@bindings/RunSummaryDto";
import type { EquityPointDto } from "@bindings/EquityPointDto";
import { api } from "../api/ipc";

function EquityChart({ points, money }: { points: EquityPointDto[]; money: boolean }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!ref.current) return;
    const chart = echarts.init(ref.current);
    chart.setOption({
      tooltip: { trigger: "axis" },
      xAxis: { type: "category", data: points.map((p) => p.t) },
      yAxis: { type: "value", scale: true, axisLabel: { formatter: money ? "¥{value}" : "{value}" } },
      series: [
        { name: money ? "资产" : "净值", type: "line", showSymbol: false,
          data: points.map((p) => (money ? p.equity : p.nav)) },
        { name: "仓位", type: "line", showSymbol: false, yAxisIndex: 0, lineStyle: { opacity: 0 },
          areaStyle: { opacity: 0.08 }, data: points.map((p) => (money ? p.pos * points[0].equity : p.pos)) },
      ],
      grid: { left: 72, right: 16, top: 24, bottom: 24 },
    });
    const onResize = () => chart.resize();
    window.addEventListener("resize", onResize);
    return () => {
      window.removeEventListener("resize", onResize);
      chart.dispose();
    };
  }, [points, money]);
  return <div ref={ref} style={{ height: 300 }} />;
}

const pct = (v: number | null | undefined) =>
  v == null ? "—" : `${(v * 100).toFixed(2)}%`;
const yuan = (v: number | null | undefined) =>
  v == null
    ? "—"
    : v.toLocaleString("zh-CN", { style: "currency", currency: "CNY", maximumFractionDigits: 0 });

export default function RunOverview({ summary }: { summary: RunSummaryDto }) {
  const [points, setPoints] = useState<EquityPointDto[]>([]);
  const [loading, setLoading] = useState(false);
  const [money, setMoney] = useState(true);
  const sim = summary.meta.kind.startsWith("sim");

  useEffect(() => {
    setPoints([]);
    if (sim) {
      setLoading(true);
      api.runEquity(summary.meta.id)
        .then(setPoints)
        .catch(() => {})
        .finally(() => setLoading(false));
    }
  }, [summary.meta.id, sim]);

  if (!sim) {
    return (
      <Card size="small" title={`打分结果 · ${summary.meta.tree_name}`}>
        <Typography.Paragraph type="secondary">
          打分模式概览为原样关键字段(完整内容见"原始"标签)。
        </Typography.Paragraph>
        <pre style={{ fontSize: 12, maxHeight: 360, overflow: "auto" }}>
          {JSON.stringify(summary.raw, null, 2)?.slice(0, 4000)}
        </pre>
      </Card>
    );
  }

  return (
    <div>
      <Row gutter={8} style={{ marginBottom: 12 }}>
        <Col span={4}>
          <Card size="small"><Statistic title="期末资产" value={yuan(summary.final_equity)} /></Card>
        </Col>
        <Col span={4}>
          <Card size="small"><Statistic title="净盈亏" value={yuan(summary.net_pnl)} /></Card>
        </Col>
        <Col span={4}>
          <Card size="small"><Statistic title="总收益" value={pct(summary.total_return)} /></Card>
        </Col>
        <Col span={4}>
          <Card size="small"><Statistic title="最大回撤" value={pct(summary.max_drawdown)} /></Card>
        </Col>
        <Col span={4}>
          <Card size="small"><Statistic title="Sharpe" value={summary.sharpe?.toFixed(2) ?? "—"} /></Card>
        </Col>
        <Col span={4}>
          <Card size="small"><Statistic title="bh对照" value={pct(summary.buy_and_hold)} /></Card>
        </Col>
      </Row>
      <Card
        size="small"
        title="资产曲线"
        extra={
          <Segmented
            options={[{ label: "金额", value: 1 }, { label: "净值", value: 0 }]}
            value={money ? 1 : 0}
            onChange={(v) => setMoney(v === 1)}
          />
        }
      >
        {loading ? (
          <Spin />
        ) : points.length ? (
          <EquityChart points={points} money={money} />
        ) : (
          <Typography.Text type="secondary">无曲线数据(traces 缺失)</Typography.Text>
        )}
      </Card>
      <Typography.Text type="secondary" style={{ fontSize: 12 }}>
        初始资金 {yuan(summary.config.initial_capital)};交易 {summary.n_round_trips ?? "—"} 笔,
        胜率 {pct(summary.win_rate)},换手 {summary.turnover?.toFixed(1) ?? "—"}
      </Typography.Text>
    </div>
  );
}
