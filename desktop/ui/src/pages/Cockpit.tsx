import { useCallback, useEffect, useRef, useState } from "react";
import { Alert, App as AntApp, Col, Drawer, Row, Spin, Typography } from "antd";
import { useCockpit } from "../stores/cockpit";
import BookCard from "../components/BookCard";
import DiffTable from "../components/DiffTable";
import RunStatusPanel from "../components/RunStatusPanel";
import ManualRunButton from "../components/ManualRunButton";
import { api } from "../api/ipc";
import { friendlyError } from "../errors";
import ValueBookCard from "../components/ValueBookCard";

export default function Cockpit() {
  const { overview, loading, error, load } = useCockpit();
  const { message } = AntApp.useApp();
  const [logOpen, setLogOpen] = useState(false);
  const [logText, setLogText] = useState("");
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);

  useEffect(() => {
    void load();
  }, [load]);

  const openLog = async () => {
    setLogText(await api.runlogTail(200));
    setLogOpen(true);
  };

  // Poll until the given task reaches done/failed/cancelled, then reload + toast.
  const watchTask = useCallback((taskId: string) => {
    if (pollRef.current !== null) clearInterval(pollRef.current);
    const deadline = Date.now() + 20_000;
    pollRef.current = setInterval(() => {
      void api.taskList().then((tasks) => {
        const t = tasks.find((x) => x.id === taskId);
        const finished = t?.status === "done" || t?.status === "failed" || t?.status === "cancelled";
        if (finished || Date.now() > deadline) {
          clearInterval(pollRef.current!);
          pollRef.current = null;
          void load().then(() => {
            if (t?.status === "done") {
              void message.success("运行完成，已刷新");
            } else if (t?.status === "failed") {
              const fe = friendlyError(t.error ?? "未知错误");
              void message.error(fe.title);
            } else if (!t) {
              // task not found after deadline – still reload silently
            }
          });
        }
      }).catch(() => {
        clearInterval(pollRef.current!);
        pollRef.current = null;
      });
    }, 1000);
  }, [load, message]);

  // Clean up on unmount
  useEffect(() => () => { if (pollRef.current !== null) clearInterval(pollRef.current); }, []);

  if (loading && !overview) return <Spin />;
  if (error) return <Alert type="error" message={error} />;
  if (!overview) return null;

  return (
    <div>
      <Row justify="space-between" align="middle" style={{ marginBottom: 12 }}>
        <Typography.Title level={4} style={{ margin: 0 }}>纸面盘驾驶舱</Typography.Title>
        <ManualRunButton onStarted={(id) => watchTask(id)} />
      </Row>
      <Row gutter={12} style={{ marginBottom: 12 }}>
        {overview.cards.map((c) => (
          <Col key={c.book} span={8}><BookCard card={c} /></Col>
        ))}
      </Row>
      <Row gutter={12} style={{ marginBottom: 12 }}>
        <Col span={24}><ValueBookCard /></Col>
      </Row>
      <Row gutter={12}>
        <Col span={14}><DiffTable rows={overview.diff} t={overview.diff_t} /></Col>
        <Col span={10}>
          <RunStatusPanel runlog={overview.runlog} schtask={overview.schtask} onOpenLog={() => void openLog()} />
        </Col>
      </Row>
      <Drawer title="运行日志（末 200 行）" open={logOpen} onClose={() => setLogOpen(false)} width={720}>
        <pre style={{ fontSize: 12, whiteSpace: "pre-wrap" }}>{logText}</pre>
      </Drawer>
    </div>
  );
}
