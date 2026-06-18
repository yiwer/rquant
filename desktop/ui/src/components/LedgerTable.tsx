import { Table, Tag } from "antd";
import type { LedgerRoundDto } from "@bindings/LedgerRoundDto";
import { VERDICT_ZH } from "../labels";

const pct = (v?: number | null) => (v == null ? "—" : `${(v * 100).toFixed(0)}%`);

export default function LedgerTable({ rows, onSelect }: { rows: LedgerRoundDto[]; onSelect: (r: number) => void }) {
  return <Table<LedgerRoundDto> size="small" rowKey="round" dataSource={rows} pagination={false}
    onRow={(r) => ({ onClick: () => onSelect(r.round), style: { cursor: "pointer" } })}
    columns={[
      { title: "#", dataIndex: "round", width: 48 },
      { title: "label", dataIndex: "label" },
      { title: "假设", dataIndex: "note", ellipsis: true },
      { title: "net超额", dataIndex: "net_ex", render: (v: number | null) => pct(v) },
      { title: "OOS超额", dataIndex: "net_oos_ex", render: (v: number | null) => pct(v) },
      { title: "Sharpe", dataIndex: "net_sharpe", render: (v: number | null) => (v == null ? "—" : v.toFixed(2)) },
      { title: "裁决", dataIndex: "verdict", render: (v: string) => <Tag color={v === "PASS" ? "green" : "red"}>{VERDICT_ZH[v] ?? v}</Tag> },
    ]} />;
}
