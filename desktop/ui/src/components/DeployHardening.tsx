import { useEffect, useState } from "react";
import { App as AntApp, Statistic, Row, Col, Table } from "antd";
import { useScreen } from "../stores/screen";
import type { DeployDto } from "@bindings/DeployDto";
export default function DeployHardening({ runId }: { runId: string }) {
  const st = useScreen(); const { message } = AntApp.useApp();
  const [d, setD] = useState<DeployDto | null>(null);
  useEffect(() => { (async () => { try { setD(await st.api.analyzeDeploy(runId)); } catch (e) { message.error(String(e)); } })(); }, [runId]);
  if (!d) return <span style={{ opacity: .6 }}>计算中…</span>;
  const pct = (v: number) => `${(v*100).toFixed(1)}%`;
  const yi = (v: number) => `${(v/1e8).toFixed(2)} 亿`;
  return <div>
    <div style={{ opacity: .6, fontSize: 12, marginBottom: 8 }}>执行/容量基准:沪深300</div>
    <Row gutter={16}>
      <Col><Statistic title="即时执行超额" value={pct(d.lag0_excess)} /></Col>
      <Col><Statistic title="T+1 执行超额" value={pct(d.lag1_excess)} /></Col>
      <Col><Statistic title="执行拖累" value={pct(d.drag)} /></Col>
      <Col><Statistic title="持仓中位 ADV" value={yi(d.adv_median)} /></Col>
    </Row>
    <Table size="small" pagination={false} rowKey="adv_pct" style={{ marginTop: 8 }} dataSource={d.capacity}
      columns={[{ title: "%ADV", dataIndex: "adv_pct", render: (x: number) => `${(x*100).toFixed(0)}%` },
        { title: "最大容量(AUM)", dataIndex: "max_aum", render: yi }]} />
  </div>;
}
