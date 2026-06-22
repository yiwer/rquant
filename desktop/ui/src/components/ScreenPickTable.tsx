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
  // 引擎返回全 universe 行（按综合分排名，top-N 标 selected）。
  // 关 ST：显示引擎原始 top-N（selected），使「数量」即时生效。
  // 开 ST：从全量排名剔除 ST 后取前 top 名 —— 回补，使集中口径仍是 N 只非 ST。
  //   无横截面闸的配置（含已部署的价值净利双核）下，与引擎级「选股前剔除 ST」完全一致。
  const selected = result.rows.filter((r) => r.selected);
  const stRemoved = excludeSt ? selected.filter((r) => isST(r.symbol)).length : 0;
  const picks = excludeSt
    ? [...result.rows].sort((a, b) => a.rank - b.rank).filter((r) => !isST(r.symbol)).slice(0, result.top)
    : selected;
  return (
    <>
      <div style={{ marginBottom: 8, display: "flex", alignItems: "center", gap: 8 }}>
        <Switch size="small" checked={excludeSt} onChange={setExcludeSt} />
        <span style={{ fontSize: 12 }}>
          过滤 ST/*ST 高风险股 · 显示 {picks.length} 只
          {stRemoved > 0 ? `（剔除 ${stRemoved} 只 ST，回补至 top-${result.top}）` : ""}
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
