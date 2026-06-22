import { Table, Tag } from "antd";
import type { ScreenResultDto } from "@bindings/ScreenResultDto";
import type { ScreenPickDto } from "@bindings/ScreenPickDto";
import stockNamesJson from "../data/stockNames.json";

// symbol(sh600000) → 中文名称。静态映射(akshare stock_info_a_code_name 生成)，5500+ A股。
const stockNames = stockNamesJson as Record<string, string>;

export default function ScreenPickTable({ result }: { result: ScreenResultDto }) {
  const cols = [
    { title: "#", dataIndex: "rank", width: 50 },
    { title: "代码", dataIndex: "symbol", key: "symbol", width: 90 },
    { title: "名称", dataIndex: "symbol", key: "name", width: 110, render: (s: string) => stockNames[s] ?? "—" },
    { title: "综合分", dataIndex: "combined_score", width: 90, sorter: (a: ScreenPickDto, b: ScreenPickDto) => a.combined_score - b.combined_score, defaultSortOrder: "descend" as const, render: (v: number) => v.toFixed(2) },
    { title: "质量分", dataIndex: "quality_score", width: 80, render: (v: number) => v.toFixed(2) },
    { title: "投机分", dataIndex: "speculative_score", width: 80, render: (v: number) => v.toFixed(2) },
    { title: "标签", dataIndex: "tags", render: (t: string[]) => t.map((x) => <Tag key={x}>{x}</Tag>) },
  ];
  // 选股榜 = 当日入选(top-N)标的：只显示 selected，使「数量」即时生效（与 CLI print 的 .filter(selected) 一致）。
  // 引擎返回全 universe 行(selected 标记 top-N)；若全表展示则「数量」只改隐藏标记、视觉不变=本次修复点。
  const picks = result.rows.filter((r) => r.selected);
  return (
    <Table<ScreenPickDto>
      size="small" rowKey="symbol" columns={cols} dataSource={picks}
      pagination={{ pageSize: 50, hideOnSinglePage: true }}
      expandable={{ expandedRowRender: (r) => (
        <Table size="small" rowKey="tree" pagination={false}
          columns={[{ title: "树", dataIndex: "tree" }, { title: "命中叶子", dataIndex: "leaf" }, { title: "打分", dataIndex: "score", render: (v: number) => v.toFixed(3) }]}
          dataSource={r.reasons} />
      ) }}
    />
  );
}
