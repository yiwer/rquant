import { Card, Table, Tag } from "antd";
import type { DiffRowDto } from "@bindings/DiffRowDto";

const ACTION_COLOR: Record<string, string> = { Buy: "green", Sell: "red", Adjust: "orange", Hold: "default" };

export default function DiffTable({ rows, t }: { rows: DiffRowDto[]; t: string | null }) {
  return (
    <Card size="small" title={`今日组合清单 diff${t ? ` @ ${t}` : ""}`}>
      <Table
        size="small"
        rowKey="symbol"
        pagination={false}
        dataSource={rows}
        locale={{ emptyText: "暂无清单(等待账本3 run)" }}
        columns={[
          { title: "标的", dataIndex: "symbol" },
          {
            title: "动作",
            dataIndex: "action",
            render: (a: string) => <Tag color={ACTION_COLOR[a] ?? "default"}>{a}</Tag>,
          },
          { title: "现权重", dataIndex: "from_w", render: (v: number) => v.toFixed(2) },
          { title: "目标权重", dataIndex: "to_w", render: (v: number) => v.toFixed(2) },
        ]}
      />
    </Card>
  );
}
