import { Table, Tag } from "antd";
import type { ScreenResultDto } from "@bindings/ScreenResultDto";
import type { ScreenPickDto } from "@bindings/ScreenPickDto";

export default function ScreenPickTable({ result }: { result: ScreenResultDto }) {
  const cols = [
    { title: "#", dataIndex: "rank", width: 50 },
    { title: "代码", dataIndex: "symbol", width: 90 },
    { title: "综合分", dataIndex: "combined_score", width: 90, sorter: (a: ScreenPickDto, b: ScreenPickDto) => a.combined_score - b.combined_score, defaultSortOrder: "descend" as const, render: (v: number) => v.toFixed(2) },
    { title: "质量分", dataIndex: "quality_score", width: 80, render: (v: number) => v.toFixed(2) },
    { title: "投机分", dataIndex: "speculative_score", width: 80, render: (v: number) => v.toFixed(2) },
    { title: "标签", dataIndex: "tags", render: (t: string[]) => t.map((x) => <Tag key={x}>{x}</Tag>) },
  ];
  return (
    <Table<ScreenPickDto>
      size="small" rowKey="symbol" columns={cols} dataSource={result.rows}
      pagination={{ pageSize: 50 }}
      expandable={{ expandedRowRender: (r) => (
        <Table size="small" rowKey="tree" pagination={false}
          columns={[{ title: "树", dataIndex: "tree" }, { title: "命中叶子", dataIndex: "leaf" }, { title: "打分", dataIndex: "score", render: (v: number) => v.toFixed(3) }]}
          dataSource={r.reasons} />
      ) }}
    />
  );
}
