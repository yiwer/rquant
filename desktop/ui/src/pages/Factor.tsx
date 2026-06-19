import { useState } from "react";
import { App as AntApp, Button, Card, Col, Input, InputNumber, Row, Space } from "antd";
import { listen } from "@tauri-apps/api/event";
import { useFactor } from "../stores/factor";
import type { FactorReportDto } from "@bindings/FactorReportDto";
import FactorReport from "../components/FactorReport";
export default function Factor() {
  const st = useFactor(); const { message } = AntApp.useApp();
  const [exprs, setExprs] = useState<[string, string][]>([["价值BP", "fund.bps/close"]]);
  const [horizon, setH] = useState(16); const [layers, setL] = useState(5); const [sample, setS] = useState(16);
  const [running, setRunning] = useState(false);
  async function run() {
    const valid = exprs.filter(([n, e]) => n && e);
    if (!valid.length) { message.warning("请添加因子表达式"); return; }
    setRunning(true);
    try {
      const taskId = await st.api.factorRun(valid, horizon, layers, sample);
      const un = await listen<{ id: string; status: string; result: FactorReportDto | null }>("task://progress", (ev) => {
        if (ev.payload.id !== taskId) return;
        if (ev.payload.status === "done") { st.setReport(ev.payload.result); setRunning(false); void un(); }
        else if (ev.payload.status === "failed") { message.error("因子分析失败"); setRunning(false); void un(); } });
    } catch (e) { message.error(String(e)); setRunning(false); }
  }
  return (
    <Row gutter={12}>
      <Col span={8}><Card size="small" title="因子工作台">
        <Space direction="vertical" style={{ width: "100%" }}>
          {exprs.map((e, i) => (<Space key={i}>
            <Input placeholder="名" value={e[0]} style={{ width: 90 }} onChange={(ev) => setExprs(x => x.map((y, j) => j === i ? [ev.target.value, y[1]] : y))} />
            <Input placeholder="DSL 表达式" value={e[1]} onChange={(ev) => setExprs(x => x.map((y, j) => j === i ? [y[0], ev.target.value] : y))} /></Space>))}
          <Button size="small" onClick={() => setExprs(x => [...x, ["", ""]])}>+ 因子</Button>
          <Space><InputNumber addonBefore="horizon" value={horizon} onChange={v => setH(v ?? 16)} />
            <InputNumber addonBefore="层" value={layers} onChange={v => setL(v ?? 5)} /></Space>
          <InputNumber addonBefore="采样间隔" value={sample} onChange={v => setS(v ?? 16)} />
          <Button type="primary" block loading={running} onClick={run}>运行分析</Button>
        </Space>
      </Card></Col>
      <Col span={16}>{st.report ? <FactorReport report={st.report} /> : <span style={{ opacity: .6 }}>添加因子并运行</span>}</Col>
    </Row>
  );
}
