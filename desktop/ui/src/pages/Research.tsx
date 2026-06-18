import { useEffect } from "react";
import { Card, Col, Row } from "antd";
import { useResearch } from "../stores/research";
import { useScreen } from "../stores/screen";
import LedgerTable from "../components/LedgerTable";
import RoundCard from "../components/RoundCard";
import RunRoundForm from "../components/RunRoundForm";

export default function Research() {
  const rs = useResearch();
  const sc = useScreen();
  useEffect(() => { void rs.load(); void sc.loadConfigs(); }, []);
  return (
    <Row gutter={12}>
      <Col span={7}>
        <Card size="small" title="跑一轮"><RunRoundForm /></Card>
        <Card size="small" title="待试角度" style={{ marginTop: 12 }}>{rs.queue?.queue.map((q) => <div key={q}>• {q}</div>)}</Card>
        <Card size="small" title="已证伪角度(不再试)" style={{ marginTop: 12 }}>{rs.queue?.falsified.map((q) => <div key={q} style={{ opacity: 0.6 }}>• {q}</div>)}</Card>
      </Col>
      <Col span={17}>
        <Card size="small" title="轮次台账"><LedgerTable rows={rs.ledger} onSelect={(r) => void rs.selectRound(r)} /></Card>
        {rs.card && <div style={{ marginTop: 12 }}><RoundCard card={rs.card} /></div>}
      </Col>
    </Row>
  );
}
