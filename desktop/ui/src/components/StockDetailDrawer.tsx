import { useEffect, useState } from "react";
import { App as AntApp, Drawer, Table, Typography } from "antd";
import type { PaperStockDetailDto } from "@bindings/PaperStockDetailDto";
import type { BarDto } from "@bindings/BarDto";
import { api } from "../api/ipc";
import KlineChart from "./KlineChart";

export default function StockDetailDrawer({
  symbol,
  onClose,
}: {
  symbol: string | null;
  onClose: () => void;
}) {
  const { message } = AntApp.useApp();
  const [detail, setDetail] = useState<PaperStockDetailDto | null>(null);
  const [bars, setBars] = useState<BarDto[]>([]);

  useEffect(() => {
    if (!symbol) return;
    setDetail(null);
    setBars([]);
    api
      .paperStockDetail(symbol)
      .then(async (d) => {
        setDetail(d);
        const b = await api.dataReadBars(d.kday_path, 120);
        setBars(b);
      })
      .catch((e) => message.error(String(e)));
  }, [symbol, message]);

  const factorCols = [
    { title: "因子", dataIndex: "key" },
    {
      title: "值",
      dataIndex: "value",
      render: (v: number | null) => (v == null ? "-" : v.toFixed(4)),
    },
  ];

  return (
    <Drawer
      open={!!symbol}
      onClose={onClose}
      width={720}
      title={detail ? `${symbol} ${detail.name}` : symbol ?? ""}
    >
      {detail && (
        <>
          <KlineChart bars={bars} height={360} />
          <Typography.Text type="secondary" style={{ fontSize: 12 }}>
            as-of {detail.asof}
          </Typography.Text>
          <Table
            rowKey="key"
            size="small"
            dataSource={detail.factors}
            columns={factorCols}
            pagination={false}
            scroll={{ y: 400 }}
            style={{ marginTop: 8 }}
          />
        </>
      )}
    </Drawer>
  );
}
