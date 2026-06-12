import { useEffect, useState } from "react";
import { Typography } from "antd";
import type { BarDto } from "@bindings/BarDto";
import type { TradeDto } from "@bindings/TradeDto";
import { api } from "../api/ipc";
import KlineChart, { type TradeMarker } from "./KlineChart";

export default function KlineSignalsView({
  runId,
  primaryPath,
  isSim,
}: {
  runId: string;
  primaryPath: string;
  isSim: boolean;
}) {
  const [bars, setBars] = useState<BarDto[]>([]);
  const [trades, setTrades] = useState<TradeDto[]>([]);

  useEffect(() => {
    let cancelled = false;
    setBars([]);
    setTrades([]);

    api.dataReadBars(primaryPath, 2000).then((b) => {
      if (!cancelled) setBars(b);
    }).catch(() => {});

    if (isSim) {
      api.runTrades(runId).then((t) => {
        if (!cancelled) setTrades(t);
      }).catch(() => {});
    }

    return () => { cancelled = true; };
  }, [runId, primaryPath, isSim]);

  if (!bars.length) return <Typography.Text type="secondary">行情 CSV 不可读({primaryPath})</Typography.Text>;

  const markers: TradeMarker[] = trades.flatMap((t) => [
    { t: t.entry_t, price: t.entry_px, kind: "entry" as const, label: "买" },
    { t: t.exit_t, price: t.exit_px, kind: "exit" as const, label: t.reason },
  ]);

  return (
    <div>
      <KlineChart bars={bars} markers={markers} />
      <Typography.Text type="secondary" style={{ fontSize: 12 }}>
        {isSim ? `${trades.length} 笔交易标注(箭头=入场,旗标=出场)` : "打分模式无交易标注"}
        ;显示末 2000 根
      </Typography.Text>
    </div>
  );
}
