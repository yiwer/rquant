import { useState } from "react";
import { App as AntApp, Button, Input, Select } from "antd";
import { useScreen } from "../stores/screen";
import { useResearch } from "../stores/research";
import { useTaskInfo, useTaskStartedAt } from "../stores/tasks";
import { api } from "../api/ipc";
import { indexZh } from "../labels";
import TaskRunning from "./TaskRunning";

export default function RunRoundForm() {
  const sc = useScreen();
  const rs = useResearch();
  const { message } = AntApp.useApp();
  const [config, setConfig] = useState("");
  const [note, setNote] = useState("");
  const [bench, setBench] = useState("csi300");

  const taskInfo = useTaskInfo(rs.runTaskId);
  const startedAt = useTaskStartedAt(rs.runTaskId);
  const running = taskInfo?.status === "running";

  async function run() {
    if (!config) { message.warning("请选择配置"); return; }
    await rs.runRound(config, note, "daily", 50, bench, 1);
  }

  if (running && taskInfo) {
    return (
      <TaskRunning
        info={taskInfo}
        startedAt={startedAt}
        onCancel={() => void api.taskCancel(rs.runTaskId!)}
      />
    );
  }

  return <div>
    <Select style={{ width: "100%" }} placeholder="配置" value={config || undefined} onChange={setConfig}
      options={sc.configs.map((c) => ({ value: c.path, label: c.name ?? c.path }))} />
    <Input style={{ marginTop: 8 }} placeholder="假设说明" value={note} onChange={(e) => setNote(e.target.value)} />
    <Select style={{ width: "100%", marginTop: 8 }} value={bench} onChange={setBench}
      options={(sc.indices.length ? sc.indices : ["csi300", "csi500", "csi1000"]).map((i) => ({ value: i, label: indexZh(i) }))} />
    <Button type="primary" block style={{ marginTop: 8 }} disabled={!config} onClick={run}>▶ 运行一轮</Button>
    {rs.runError && <div style={{ color: "#dc2626", marginTop: 8 }}>{rs.runError}</div>}
  </div>;
}
