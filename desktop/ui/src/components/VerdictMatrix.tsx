import { Card, Table, Tag } from "antd";
import type { VerdictDto } from "@bindings/VerdictDto";
import { GATE_STATUS_ZH } from "../labels";
export default function VerdictMatrix({ v }: { v: VerdictDto }) {
  const color = (s: string) => (s === "pass" ? "green" : s === "fail" ? "red" : "orange");
  return (
    <Card size="small" title={`认证:${v.strategy} · ${v.n_symbols} 标的`}
      extra={<Tag color={v.certified ? "green" : "red"}>{v.certified ? "已认证 ✓" : "未通过"}</Tag>}>
      <Table size="small" pagination={false} rowKey="gate" dataSource={v.gates}
        columns={[
          { title: "门槛", dataIndex: "gate" },
          { title: "状态", dataIndex: "status", render: (s: string) => <Tag color={color(s)}>{GATE_STATUS_ZH[s] ?? s}</Tag> },
          { title: "值", dataIndex: "value", render: (x: number) => x.toFixed(3) },
          { title: "阈值", dataIndex: "threshold", render: (x: number) => x.toFixed(3) },
          { title: "说明", dataIndex: "note", ellipsis: true },
        ]} />
    </Card>
  );
}
