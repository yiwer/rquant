import { useEffect, useState } from "react";
import { Button, Card, Col, DatePicker, InputNumber, Row, Select, Tabs } from "antd";
import { useScreen } from "../stores/screen";
import ScreenPickTable from "../components/ScreenPickTable";
import ScreenBacktestResult from "../components/ScreenBacktestResult";
import TaskRunning from "../components/TaskRunning";
import { useTaskInfo, useTaskStartedAt } from "../stores/tasks";

/** 选股榜（指定日）——自包含 tab：配置 + 日期 + 数量 + 运行 + 结果。 */
function AsofTab() {
  const st = useScreen();
  const [config, setConfig] = useState<string>("");
  const [asOf, setAsOf] = useState<string>("");
  const [top, setTop] = useState<number>(50);
  useEffect(() => { void st.loadConfigs(); }, []);

  const info = useTaskInfo(st.asofTaskId);
  const startedAt = useTaskStartedAt(st.asofTaskId);
  const running = info?.status === "running";

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
            <Button type="primary" loading={running} disabled={!config || running} onClick={() => { if (!config || !asOf) { return; } void st.runAsof(config, asOf, top); }}>运行选股</Button>
          </Col>
        </Row>
      </Card>
      {running && info ? (
        <TaskRunning info={info} startedAt={startedAt} onCancel={() => st.asofTaskId && void st.api.taskCancel(st.asofTaskId)} />
      ) : st.asofError ? (
        <span style={{ color: "#dc2626" }}>{st.asofError}</span>
      ) : st.asofResult ? (
        <ScreenPickTable result={st.asofResult} />
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
