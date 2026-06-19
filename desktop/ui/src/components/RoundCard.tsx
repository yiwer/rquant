import { Card, Table, Tag } from "antd";
import type { RoundCardDto } from "@bindings/RoundCardDto";
import type { RoundGateDto } from "@bindings/RoundGateDto";
import type { Tier2CellDto } from "@bindings/Tier2CellDto";
import { indexZh, verdictZh } from "../labels";

export default function RoundCard({ card }: { card: RoundCardDto }) {
  return (
    <Card size="small" title={`第${card.round}轮 · ${card.label} [${indexZh(card.benchmark)}]`}
      extra={<Tag color={card.verdict === "PASS" ? "green" : "red"}>{verdictZh(card.verdict)}</Tag>}>
      <Table<RoundGateDto> size="small" pagination={false} rowKey="name" title={() => "裁决门槛"}
        columns={[
          { title: "门槛", dataIndex: "name" },
          { title: "", dataIndex: "pass", width: 40, render: (p: boolean) => <span style={{ color: p ? "#16a34a" : "#dc2626" }}>{p ? "✓" : "✗"}</span> },
          { title: "值", dataIndex: "value", render: (v: number | null) => (v == null ? "—" : v.toFixed(2)) },
        ]} dataSource={card.gates} />
      {card.tier2.length > 0 && (
        <Table<Tier2CellDto> size="small" pagination={false} rowKey={(r) => `${r.top}-${r.rebalance}`} style={{ marginTop: 8 }} title={() => "敏感性扫描·二阶（净超额）"}
          columns={[{ title: "数量", dataIndex: "top" }, { title: "调仓", dataIndex: "rebalance" }, { title: "净超额", dataIndex: "net_excess", render: (v: number) => `${(v * 100).toFixed(2)}%` }]}
          dataSource={card.tier2} />
      )}
    </Card>
  );
}
