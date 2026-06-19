import { useEffect, useState } from "react";
import { App as AntApp, Row, Col, Statistic } from "antd";
import { useScreen } from "../stores/screen";
import type { SectorAttribDto } from "@bindings/SectorAttribDto";
export default function SectorAttrib({ runId }: { runId: string }) {
  const st = useScreen(); const { message } = AntApp.useApp();
  const [d, setD] = useState<SectorAttribDto | null>(null);
  useEffect(() => { (async () => { try { setD(await st.api.analyzeSector(runId)); } catch (e) { message.error(String(e)); } })(); }, [runId]);
  if (!d) return <span style={{ opacity: .6 }}>计算中…</span>;
  const pct = (v: number) => `${(v*100).toFixed(1)}%`;
  return <Row gutter={16}>
    <Col><Statistic title="总超额" value={pct(d.excess_total)} /></Col>
    <Col><Statistic title="配置效应占比" value={pct(d.alloc_pct)} /></Col>
    <Col><Statistic title="选择效应占比" value={pct(d.select_pct)} /></Col>
  </Row>;
}
