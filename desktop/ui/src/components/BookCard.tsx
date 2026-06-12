import { Card, Statistic, Tag, Typography } from "antd";
import type { BookCardDto } from "@bindings/BookCardDto";
import { useNavigate } from "react-router-dom";

const STATUS_TAG: Record<string, { color: string; text: string }> = {
  ok: { color: "green", text: "正常" },
  empty: { color: "default", text: "未建账" },
  corrupt: { color: "red", text: "异常" },
};

export default function BookCard({ card }: { card: BookCardDto }) {
  const nav = useNavigate();
  const st = STATUS_TAG[card.status] ?? STATUS_TAG.empty;
  return (
    <Card
      size="small"
      title={card.title}
      extra={<Tag color={st.color}>{st.text}</Tag>}
      hoverable
      onClick={() => nav(`/cockpit/${card.book}`)}
      style={{ height: "100%" }}
    >
      {card.status === "ok" && card.kind === "single" && (
        <>
          <Statistic title="nav" value={card.nav ?? 0} precision={4} />
          <Typography.Text type="secondary">
            持仓 {card.pos} · 回撤 {((card.max_drawdown ?? 0) * 100).toFixed(2)}% · {card.state_time}
          </Typography.Text>
        </>
      )}
      {card.status === "ok" && card.kind === "portfolio" && (
        <Typography.Text>
          持仓 {card.holdings?.map(([s, w]) => `${s} ${w.toFixed(2)}`).join(" / ") || "(空)"}
        </Typography.Text>
      )}
      {card.status !== "ok" && <Typography.Text type="secondary">{card.advice}</Typography.Text>}
      {card.last_signal && (
        <div style={{ marginTop: 8 }}>
          <Typography.Text type="secondary" style={{ fontSize: 12 }}>
            最新信号 {card.last_signal.t}
            {card.last_signal.leaf ? ` · 叶 ${card.last_signal.leaf}` : ""}
            {card.last_signal.targets ? ` · 入选 ${card.last_signal.targets.length} 只` : ""}
            {card.last_signal.bars_replayed != null ? ` · 重放 ${card.last_signal.bars_replayed}` : ""}
          </Typography.Text>
        </div>
      )}
    </Card>
  );
}
