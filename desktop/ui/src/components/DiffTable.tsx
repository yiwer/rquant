import { Card, Table, Tag } from "antd";
import type { DiffRowDto } from "@bindings/DiffRowDto";
import { actionZh } from "../labels";
import { stockName } from "../data/stocks";

const ACTION_COLOR: Record<string, string> = { Buy: "green", Sell: "red", Adjust: "orange", Hold: "default" };

export default function DiffTable({ rows, t, title }: { rows: DiffRowDto[]; t: string | null; title?: string }) {
  return (
    <Card size="small" title={title ?? `今日组合清单 diff${t ? ` @ ${t}` : ""}`}>
      <Table
        size="small"
        rowKey="symbol"
        pagination={false}
        dataSource={rows}
        locale={{ emptyText: "暂无持仓目标（持仓组合未运行）" }}
        columns={[
          { title: "标的", dataIndex: "symbol" },
          { title: "名称", dataIndex: "symbol", key: "name", render: (s: string) => stockName(s) },
          {
            title: "动作",
            dataIndex: "action",
            render: (a: string) => <Tag color={ACTION_COLOR[a] ?? "default"}>{actionZh(a)}</Tag>,
          },
          { title: "现权重", dataIndex: "from_w", render: (v: number) => v.toFixed(2) },
          { title: "目标权重", dataIndex: "to_w", render: (v: number) => v.toFixed(2) },
        ]}
      />
    </Card>
  );
}
