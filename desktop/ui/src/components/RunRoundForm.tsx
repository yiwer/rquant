import { useState } from "react";
import { App as AntApp, Button, Input, Select } from "antd";
import { listen } from "@tauri-apps/api/event";
import { useScreen } from "../stores/screen";
import { useResearch } from "../stores/research";

export default function RunRoundForm() {
  const sc = useScreen();
  const rs = useResearch();
  const { message } = AntApp.useApp();
  const [config, setConfig] = useState("");
  const [note, setNote] = useState("");
  const [bench, setBench] = useState("csi300");
  async function run() {
    if (!config) { message.warning("请选择配置"); return; }
    try {
      const taskId = await rs.api.iterRunRound(config, note, "daily", 50, bench, 1);
      message.success("已开始跑轮(后台)");
      const un = await listen<{ id: string; status: string }>("task://progress", (e) => {
        if (e.payload.id !== taskId) return;
        if (e.payload.status === "done" || e.payload.status === "failed") { void rs.load(); void un(); }
      });
    } catch (e) { message.error(String(e)); }
  }
  return <div>
    <Select style={{ width: "100%" }} placeholder="配置" value={config || undefined} onChange={setConfig}
      options={sc.configs.map((c) => ({ value: c.path, label: c.name ?? c.path }))} />
    <Input style={{ marginTop: 8 }} placeholder="假设 note" value={note} onChange={(e) => setNote(e.target.value)} />
    <Select style={{ width: "100%", marginTop: 8 }} value={bench} onChange={setBench}
      options={(sc.indices.length ? sc.indices : ["csi300", "csi500", "csi1000"]).map((i) => ({ value: i, label: i.toUpperCase() }))} />
    <Button type="primary" block style={{ marginTop: 8 }} disabled={!config} onClick={run}>▶ 运行一轮</Button>
  </div>;
}
