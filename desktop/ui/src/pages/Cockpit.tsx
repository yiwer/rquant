import { useEffect, useState } from "react";
import { Alert, Col, Drawer, Row, Spin, Typography } from "antd";
import { useCockpit } from "../stores/cockpit";
import BookCard from "../components/BookCard";
import DiffTable from "../components/DiffTable";
import RunStatusPanel from "../components/RunStatusPanel";
import ManualRunButton from "../components/ManualRunButton";
import { api } from "../api/ipc";

export default function Cockpit() {
  const { overview, loading, error, load } = useCockpit();
  const [logOpen, setLogOpen] = useState(false);
  const [logText, setLogText] = useState("");

  useEffect(() => {
    void load();
  }, [load]);

  const openLog = async () => {
    setLogText(await api.runlogTail(200));
    setLogOpen(true);
  };

  if (loading && !overview) return <Spin />;
  if (error) return <Alert type="error" message={error} />;
  if (!overview) return null;

  return (
    <div>
      <Row justify="space-between" align="middle" style={{ marginBottom: 12 }}>
        <Typography.Title level={4} style={{ margin: 0 }}>纸面盘驾驶舱</Typography.Title>
        <ManualRunButton onStarted={() => void load()} />
      </Row>
      <Row gutter={12} style={{ marginBottom: 12 }}>
        {overview.cards.map((c) => (
          <Col key={c.book} span={8}><BookCard card={c} /></Col>
        ))}
      </Row>
      <Row gutter={12}>
        <Col span={14}><DiffTable rows={overview.diff} t={overview.diff_t} /></Col>
        <Col span={10}>
          <RunStatusPanel runlog={overview.runlog} schtask={overview.schtask} onOpenLog={() => void openLog()} />
        </Col>
      </Row>
      <Drawer title="run.log(末 200 行)" open={logOpen} onClose={() => setLogOpen(false)} width={720}>
        <pre style={{ fontSize: 12, whiteSpace: "pre-wrap" }}>{logText}</pre>
      </Drawer>
    </div>
  );
}
