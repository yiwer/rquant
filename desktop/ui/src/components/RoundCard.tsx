import { Card, Table, Tag } from "antd";
import type { RoundCardDto } from "@bindings/RoundCardDto";
import type { RoundGateDto } from "@bindings/RoundGateDto";
import type { Tier2CellDto } from "@bindings/Tier2CellDto";

export default function RoundCard({ card }: { card: RoundCardDto }) {
  return (
    <Card size="small" title={`Round ${card.round} · ${card.label} [${card.benchmark}]`}
      extra={<Tag color={card.verdict === "PASS" ? "green" : "red"}>{card.verdict}</Tag>}>
      <Table<RoundGateDto> size="small" pagination={false} rowKey="name" title={() => "verdict 门槛"}
        columns={[
          { title: "门槛", dataIndex: "name" },
          { title: "", dataIndex: "pass", width: 40, render: (p: boolean) => <span style={{ color: p ? "#16a34a" : "#dc2626" }}>{p ? "✓" : "✗"}</span> },
          { title: "值", dataIndex: "value", render: (v: number | null) => (v == null ? "—" : v.toFixed(2)) },
        ]} dataSource={card.gates} />
      {card.tier2.length > 0 && (
        <Table<Tier2CellDto> size="small" pagination={false} rowKey={(r) => `${r.top}-${r.rebalance}`} style={{ marginTop: 8 }} title={() => "Tier-2 敏感扫(net超额)"}
          columns={[{ title: "Top", dataIndex: "top" }, { title: "调仓", dataIndex: "rebalance" }, { title: "net超额", dataIndex: "net_excess", render: (v: number) => `${(v * 100).toFixed(0)}%` }]}
          dataSource={card.tier2} />
      )}
    </Card>
  );
}
