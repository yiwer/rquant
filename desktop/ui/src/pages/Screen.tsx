import { useEffect, useState } from "react";
import { App as AntApp, Button, Card, Col, DatePicker, InputNumber, Row, Select, Spin, Tabs } from "antd";
import { listen } from "@tauri-apps/api/event";
import { useScreen } from "../stores/screen";
import type { ScreenResultDto } from "@bindings/ScreenResultDto";
import ScreenPickTable from "../components/ScreenPickTable";
import ScreenBacktestResult from "../components/ScreenBacktestResult";

/** 选股榜（指定日）——自包含 tab：配置 + 日期 + 数量 + 运行 + 结果。 */
function AsofTab() {
  const st = useScreen();
  const { message } = AntApp.useApp();
  const [config, setConfig] = useState<string>("");
  const [asOf, setAsOf] = useState<string>("");
  const [top, setTop] = useState<number>(50);
  const [asofResult, setAsofResult] = useState<ScreenResultDto | null>(null);
  const [running, setRunning] = useState(false);
  useEffect(() => { void st.loadConfigs(); }, []);

  async function runAsof() {
    if (!config || !asOf) { message.warning("请选择配置与指定日日期"); return; }
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

  return (
    <div>
      <Card size="small" style={{ marginBottom: 8 }}>
        <Row gutter={8} align="middle">
          <Col flex="auto">
            <Select style={{ width: "100%" }} placeholder="选股配置" value={config || undefined}
              onChange={setConfig} options={st.configs.map((c) => ({ value: c.path, label: c.name ?? c.path }))} />
          </Col>
          <Col>
            <DatePicker placeholder="指定日" onChange={(_, s) => setAsOf((s ?? "") as string)} />
          </Col>
          <Col>
            <InputNumber addonBefore="数量" min={1} value={top} onChange={(v) => setTop(v ?? 50)} />
          </Col>
          <Col>
            <Button type="primary" loading={running} disabled={!config} onClick={runAsof}>运行选股</Button>
          </Col>
        </Row>
      </Card>
      {running ? (
        <div style={{ textAlign: "center", padding: 48 }}>
          <Spin />
          <div style={{ marginTop: 12, opacity: 0.6 }}>选股中…</div>
        </div>
      ) : asofResult ? (
        <ScreenPickTable result={asofResult} />
      ) : (
        <span style={{ opacity: 0.6 }}>选择配置与指定日日期，点「运行选股」查看当日选股榜。</span>
      )}
    </div>
  );
}

export default function Screen() {
  return (
    <Tabs items={[
      { key: "asof", label: "选股榜（指定日）", children: <AsofTab /> },
      { key: "bt", label: "选股回测", children: <ScreenBacktestResult /> },
    ]} />
  );
}
