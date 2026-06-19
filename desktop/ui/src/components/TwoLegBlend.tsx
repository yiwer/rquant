import { useEffect, useState } from "react";
import { App as AntApp, Select, Slider, Table } from "antd";
import { useScreen } from "../stores/screen";
import type { TwoLegDto } from "@bindings/TwoLegDto";
export default function TwoLegBlend({ runId }: { runId: string }) {
  const st = useScreen(); const { message } = AntApp.useApp();
  const [growth, setGrowth] = useState<string>(""); const [w, setW] = useState(0.8);
  const [d, setD] = useState<TwoLegDto | null>(null);
  useEffect(() => { void st.loadRuns(); }, []);
  useEffect(() => { if (!growth) return; (async () => { try { setD(await st.api.analyzeTwoleg(runId, growth, w)); } catch (e) { message.error(String(e)); } })(); }, [growth]);
  const pct = (v?: number | null) => (v == null ? "—" : `${(v*100).toFixed(0)}%`);
  return <div>
    <Select style={{ width: 320 }} placeholder="选成长腿 run" value={growth || undefined} onChange={setGrowth}
      options={st.runs.filter(r => r.id !== runId).map(r => ({ value: r.id, label: `${r.config} · ${r.created}` }))} />
    {d && <>
      <div style={{ margin: "8px 0" }}>价值腿权重 w={w.toFixed(1)}(最优 {d.best_w.toFixed(1)})<Slider min={0} max={1} step={0.1} value={w} onChange={setW} /></div>
      <Table size="small" pagination={false} rowKey="w" dataSource={d.rows}
        columns={[{ title: "w(价值)", dataIndex: "w", render: (x: number) => x.toFixed(1) },
          { title: "净总", dataIndex: "net_total", render: pct }, { title: "超额", dataIndex: "excess", render: pct },
          { title: "样本外超额", dataIndex: "oos_excess", render: pct }, { title: "夏普", dataIndex: "sharpe", render: (x: number) => x.toFixed(2) },
          { title: "最大回撤", dataIndex: "max_dd", render: pct }]} />
    </>}
  </div>;
}
