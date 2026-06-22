import { useState } from "react";
import { Switch, Table, Tag } from "antd";
import type { ScreenResultDto } from "@bindings/ScreenResultDto";
import type { ScreenPickDto } from "@bindings/ScreenPickDto";
import { stockName, isST } from "../data/stocks";

export default function ScreenPickTable({ result }: { result: ScreenResultDto }) {
  const [excludeSt, setExcludeSt] = useState(true); // 默认过滤 ST/*ST 高风险股
  const cols = [
    { title: "#", dataIndex: "rank", width: 50 },
    { title: "代码", dataIndex: "symbol", key: "symbol", width: 90 },
    { title: "名称", dataIndex: "symbol", key: "name", width: 110, render: (s: string) => stockName(s) },
    { title: "综合分", dataIndex: "combined_score", width: 90, sorter: (a: ScreenPickDto, b: ScreenPickDto) => a.combined_score - b.combined_score, defaultSortOrder: "descend" as const, render: (v: number) => v.toFixed(2) },
    { title: "质量分", dataIndex: "quality_score", width: 80, render: (v: number) => v.toFixed(2) },
    { title: "投机分", dataIndex: "speculative_score", width: 80, render: (v: number) => v.toFixed(2) },
    { title: "标签", dataIndex: "tags", render: (t: string[]) => t.map((x) => <Tag key={x}>{x}</Tag>) },
  ];
  // 选股榜 = 当日入选(top-N)标的：只显示 selected（使「数量」即时生效，引擎返回全 universe 行仅标记 top-N）。
  // ST 开关：默认过滤掉入选中的 ST/*ST 高风险股（投资不选 ST）；关掉则显示引擎原始 top-N。
  let picks = result.rows.filter((r) => r.selected);
  const stRemoved = excludeSt ? picks.filter((r) => isST(r.symbol)).length : 0;
  if (excludeSt) { picks = picks.filter((r) => !isST(r.symbol)); }
  return (
    <>
      <div style={{ marginBottom: 8, display: "flex", alignItems: "center", gap: 8 }}>
        <Switch size="small" checked={excludeSt} onChange={setExcludeSt} />
        <span style={{ fontSize: 12 }}>
          过滤 ST/*ST 高风险股 · 显示 {picks.length} 只{stRemoved > 0 ? `（已剔除 ${stRemoved} 只 ST）` : ""}
        </span>
      </div>
      <Table<ScreenPickDto>
        size="small" rowKey="symbol" columns={cols} dataSource={picks}
        pagination={{ pageSize: 50, hideOnSinglePage: true }}
        expandable={{ expandedRowRender: (r) => (
          <Table size="small" rowKey="tree" pagination={false}
            columns={[{ title: "树", dataIndex: "tree" }, { title: "命中叶子", dataIndex: "leaf" }, { title: "打分", dataIndex: "score", render: (v: number) => v.toFixed(3) }]}
            dataSource={r.reasons} />
        ) }}
      />
    </>
  );
}
