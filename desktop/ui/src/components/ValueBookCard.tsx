import { useEffect, useState } from "react";
import { Card, Statistic, Row, Col } from "antd";
import { useNavigate } from "react-router-dom";
import { api } from "../api/ipc";
import type { DeployBookDto } from "@bindings/DeployBookDto";
export default function ValueBookCard() {
  const nav = useNavigate(); const [d, setD] = useState<DeployBookDto | null>(null);
  useEffect(() => { api.deployBookRead().then(setD).catch(() => {}); }, []);
  const pct = (v?: number | null) => (v == null ? "—" : `${(v*100).toFixed(1)}%`);
  return (
    <Card size="small" title="价值选股盘(纸面)" hoverable onClick={() => nav("/deploy")}
      extra={d?.status === "empty" ? "未建仓" : "跟踪中"}>
      {d?.status === "empty" || !d ? <span style={{ opacity: .6 }}>去部署页跑首月建仓 →</span> : (
        <Row gutter={12}>
          <Col><Statistic title="NAV" value={d.nav?.toFixed(3) ?? "—"} /></Col>
          <Col><Statistic title="超额(沪深300)" value={pct(d.excess_total)} /></Col>
          <Col><Statistic title="持仓" value={d.holdings.length} /></Col>
          <Col><Statistic title="上次调仓" value={d.last_rebalance ?? "—"} /></Col>
        </Row>
      )}
    </Card>
  );
}
