import { useEffect, useState } from "react";
import { App as AntApp, Button, Card, Col, DatePicker, InputNumber, Row, Select, Tabs } from "antd";
import { listen } from "@tauri-apps/api/event";
import { useScreen } from "../stores/screen";
import type { ScreenResultDto } from "@bindings/ScreenResultDto";
import ScreenPickTable from "../components/ScreenPickTable";
import ScreenBacktestResult from "../components/ScreenBacktestResult";

export default function Screen() {
  const st = useScreen();
  const { message } = AntApp.useApp();
  const [config, setConfig] = useState<string>("");
  const [asOf, setAsOf] = useState<string>("");
  const [top, setTop] = useState<number>(50);
  const [asofResult, setAsofResult] = useState<ScreenResultDto | null>(null);
  const [running, setRunning] = useState(false);
  useEffect(() => { void st.loadConfigs(); void st.loadRuns(); }, []);

  async function runAsof() {
    if (!config || !asOf) { message.warning("请选择配置与 as-of 日期"); return; }
    setRunning(true);
    try {
      const taskId = await st.api.screenAsof(config, asOf, top);
      const un = await listen<{ id: string; status: string; result: ScreenResultDto | null }>("task://progress", (e) => {
        if (e.payload.id !== taskId) return;
        if (e.payload.status === "done") { setAsofResult(e.payload.result); setRunning(false); void un(); }
        else if (e.payload.status === "failed") { message.error("选股失败"); setRunning(false); void un(); }
      });
    } catch (e) { message.error(String(e)); setRunning(false); }
  }

  const left = (
    <Card size="small" title="选股配置">
      <Select style={{ width: "100%" }} placeholder="配置" value={config || undefined}
        onChange={setConfig} options={st.configs.map((c) => ({ value: c.path, label: c.name ?? c.path }))} />
      <DatePicker style={{ width: "100%", marginTop: 8 }} onChange={(_, s) => setAsOf((s ?? "") as string)} />
      <InputNumber style={{ width: "100%", marginTop: 8 }} addonBefore="Top" min={1} value={top} onChange={(v) => setTop(v ?? 50)} />
      <Button type="primary" block style={{ marginTop: 8 }} loading={running} disabled={!config} onClick={runAsof}>运行选股</Button>
    </Card>
  );
  return (
    <Row gutter={12}>
      <Col span={6}>{left}</Col>
      <Col span={18}>
        <Tabs items={[
          { key: "asof", label: "选股榜 (as-of)", children: asofResult ? <ScreenPickTable result={asofResult} /> : <span style={{ opacity: 0.6 }}>选配置并运行</span> },
          { key: "bt", label: "选股回测", children: <ScreenBacktestResult /> },
        ]} />
      </Col>
    </Row>
  );
}
