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

/** 15m 选股（实验）——镜像 AsofTab，跑 15m universe；红字标注无验证 edge。 */
function Intraday15mTab() {
  const st = useScreen();
  const [config, setConfig] = useState<string>("");
  const [asOf, setAsOf] = useState<string>("");
  const [top, setTop] = useState<number>(50);
  useEffect(() => { void st.load15mConfigs(); }, []);
  const info = useTaskInfo(st.i15mTaskId);
  const startedAt = useTaskStartedAt(st.i15mTaskId);
  const running = info?.status === "running";
  return (
    <div>
      <div style={{ color: "#dc2626", fontSize: 12, marginBottom: 8 }}>
        ⚠️ 实验模块：15m 因子无验证 edge、数据有幸存者偏差/无 OOS。占位配置仅供迭代因子，勿当已验证策略。
      </div>
      <Card size="small" style={{ marginBottom: 8 }}>
        <Row gutter={8} align="middle">
          <Col flex="auto">
            <Select style={{ width: "100%" }} placeholder="15m 选股配置" value={config || undefined}
              onChange={setConfig} options={st.configs15m.map((c) => ({ value: c.path, label: c.name ?? c.path }))} />
          </Col>
          <Col><DatePicker placeholder="指定日" onChange={(_, s) => setAsOf((s ?? "") as string)} /></Col>
          <Col><InputNumber addonBefore="数量" min={1} value={top} onChange={(v) => setTop(v ?? 50)} /></Col>
          <Col><Button type="primary" loading={running} disabled={!config || running}
            onClick={() => { if (!config || !asOf) { return; } void st.run15mAsof(config, asOf, top); }}>运行选股</Button></Col>
        </Row>
      </Card>
      {running && info ? (
        <TaskRunning info={info} startedAt={startedAt} onCancel={() => st.i15mTaskId && void st.api.taskCancel(st.i15mTaskId)} />
      ) : st.i15mError ? (
        <span style={{ color: "#dc2626" }}>{st.i15mError}</span>
      ) : st.i15mResult ? (
        <ScreenPickTable result={st.i15mResult} />
      ) : (
        <span style={{ opacity: 0.6 }}>选 15m 配置与指定日，点「运行选股」查看 15m 选股榜（尾盘截面）。</span>
      )}
    </div>
  );
}

export default function Screen() {
  return (
    <Tabs items={[
      { key: "asof", label: "选股榜（指定日）", children: <AsofTab /> },
      { key: "bt", label: "选股回测", children: <ScreenBacktestResult /> },
      { key: "intraday15m", label: "15m选股（实验）", children: <Intraday15mTab /> },
    ]} />
  );
}
