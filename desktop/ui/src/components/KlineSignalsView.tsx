import { useEffect, useState, useMemo } from "react";
import { Tooltip, Typography, Spin } from "antd";
import { InfoCircleOutlined } from "@ant-design/icons";
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
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setBars([]);
    setTrades([]);
    setLoading(true);

    api.dataReadBars(primaryPath, 2000).then((b) => {
      if (!cancelled) {
        setBars(b);
        setLoading(false);
      }
    }).catch(() => {
      if (!cancelled) setLoading(false);
    });

    if (isSim) {
      api.runTrades(runId).then((t) => {
        if (!cancelled) setTrades(t);
      }).catch(() => {});
    }

    return () => { cancelled = true; };
  }, [runId, primaryPath, isSim]);

  // All hooks must run unconditionally before any early return.
  const markers = useMemo<TradeMarker[]>(
    () => trades.flatMap((t) => [
      { t: t.entry_t, price: t.entry_px, kind: "entry" as const, label: "买" },
      { t: t.exit_t, price: t.exit_px, kind: "exit" as const, label: t.reason },
    ]),
    [trades],
  );

  if (loading) return <Spin />;

  if (!bars.length) return <Typography.Text type="secondary">行情 CSV 不可读({primaryPath})</Typography.Text>;

  const barTimes = new Set(bars.map((b) => b.t));
  const visibleCount = trades.filter((t) => barTimes.has(t.entry_t) || barTimes.has(t.exit_t)).length;

  return (
    <div>
      <KlineChart bars={bars} markers={markers} />
      <Typography.Text type="secondary" style={{ fontSize: 12 }}>
        {isSim ? `${visibleCount} 笔交易标注/${trades.length} 笔共计(箭头=入场,旗标=出场)` : "打分模式无交易标注"}
        {" ；显示末 2000 根 "}
        <Tooltip title="仅展示末2000根，完整数据已参与回测">
          <InfoCircleOutlined style={{ color: "#8c8c8c" }} />
        </Tooltip>
      </Typography.Text>
    </div>
  );
}
