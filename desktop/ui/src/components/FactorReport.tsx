import { useState } from "react";
import { Card, Table } from "antd";
import type { FactorReportDto } from "@bindings/FactorReportDto";
import type { FactorStatsDto } from "@bindings/FactorStatsDto";
export default function FactorReport({ report }: { report: FactorReportDto }) {
  const [sel, setSel] = useState(0);
  const f = report.factors[sel];
  const fx = (v?: number | null) => (v == null ? "—" : v.toFixed(3));
  return (
    <div>
      <Card size="small" title={`因子 IC(${report.n_symbols} 标的 · horizon ${report.horizon} · ${report.layers_q} 层)`}>
        <Table<FactorStatsDto> size="small" rowKey="name" pagination={false} dataSource={report.factors}
          onRow={(_, i) => ({ onClick: () => setSel(i ?? 0), style: { cursor: "pointer" } })}
          columns={[{ title: "因子", dataIndex: "name" }, { title: "表达式", dataIndex: "expr", ellipsis: true },
            { title: "IC 均值", dataIndex: "ic_mean", render: fx }, { title: "ICIR", dataIndex: "icir", render: fx },
            { title: "RankIC", dataIndex: "rank_ic_mean", render: fx }, { title: "RankICIR", dataIndex: "rank_icir", render: fx },
            { title: "IC t 值", dataIndex: "ic_t", render: fx }]} />
      </Card>
      {f && (<Card size="small" title={`${f.name} · IC 衰减 / 分层收益`} style={{ marginTop: 8 }}>
        <div style={{ fontSize: 12 }}>IC 衰减:{f.ic_decay.map((d) => `${d.horizon}→${fx(d.rank_ic)}`).join("  ")}</div>
        {f.layers && <div style={{ fontSize: 12, marginTop: 6 }}>分层年化:{f.layers.ann_returns.map((r, i) => `Q${i+1} ${r == null ? "—" : (r*100).toFixed(1)+"%"}`).join("  ")} · 单调性 {fx(f.layers.monotonicity)} · 多空价差 {(f.layers.spread_total*100).toFixed(1)}%</div>}
      </Card>)}
    </div>
  );
}
