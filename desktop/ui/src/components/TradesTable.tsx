import { useEffect, useState } from "react";
import { Table, Tooltip, Typography } from "antd";
import type { TradeDto } from "@bindings/TradeDto";
import { api } from "../api/ipc";

export default function TradesTable({ runId }: { runId: string }) {
  const [rows, setRows] = useState<TradeDto[]>([]);
  useEffect(() => {
    let cancelled = false;
    setRows([]);
    api.runTrades(runId)
      .then((r) => {
        if (!cancelled) setRows(r);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [runId]);
  return (
    <Table
      size="small"
      rowKey={(r) => `${r.entry_t}-${r.exit_t}`}
      dataSource={rows}
      pagination={{ pageSize: 20 }}
      locale={{ emptyText: "无交易（打分模式或全程无持仓）" }}
      columns={[
        { title: "入场", dataIndex: "entry_t" },
        { title: "出场", dataIndex: "exit_t" },
        { title: "入场价", dataIndex: "entry_px", render: (v: number) => v.toFixed(2) },
        { title: "出场价", dataIndex: "exit_px", render: (v: number) => v.toFixed(2) },
        { title: "持仓根数", dataIndex: "bars_held" },
        {
          title: "收益率",
          dataIndex: "trip_return",
          render: (v: number) => (
            <span style={{ color: v >= 0 ? "#3f8600" : "#cf1322" }}>
              {(v * 100).toFixed(2)}%
            </span>
          ),
        },
        {
          title: <Tooltip title="按简单收益率近似（资金×回合收益率）">盈亏额*</Tooltip>,
          dataIndex: "pnl_amount",
          render: (v: number) => v.toLocaleString("zh-CN", { maximumFractionDigits: 0 }),
        },
        {
          title: "原因",
          dataIndex: "reason",
          render: (v: string) => <Typography.Text type="secondary">{v}</Typography.Text>,
        },
      ]}
    />
  );
}
