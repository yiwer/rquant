import { useEffect, useState } from "react";
import { useParams, Link } from "react-router-dom";
import { Alert, Card, Descriptions, Spin, Typography } from "antd";
import type { BookDetailDto } from "@bindings/BookDetailDto";
import { api } from "../api/ipc";
import NavChart from "../components/NavChart";

export default function BookDetail() {
  const { book = "" } = useParams();
  const [data, setData] = useState<BookDetailDto | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setData(null);
    setError(null);
    api.bookDetail(book).then(setData).catch((e) => setError(String(e)));
  }, [book]);

  if (error) return <Alert type="error" message={error} />;
  if (!data) return <Spin />;
  const s = data.snapshot;

  return (
    <div>
      <Typography.Title level={4}>
        <Link to="/cockpit">驾驶舱</Link> / {data.card.title}
      </Typography.Title>
      <Card size="small" title={data.card.kind === "portfolio" ? "持仓成员数(journal)" : "纸面净值(journal,自桌面端启用日积累)"} style={{ marginBottom: 12 }}>
        {data.journal.length ? (
          <NavChart points={data.journal} portfolio={data.card.kind === "portfolio"} />
        ) : (
          <Typography.Text type="secondary">journal 暂无数据——历史从桌面端启用日开始积累</Typography.Text>
        )}
      </Card>
      {s && (
        <Card size="small" title="AccountSnapshot(13 字段,只读)">
          <Descriptions size="small" column={3} bordered>
            <Descriptions.Item label="pos">{s.pos}</Descriptions.Item>
            <Descriptions.Item label="entry_price">{s.entry_price?.toFixed(2) ?? "—"}</Descriptions.Item>
            <Descriptions.Item label="bars_held">{s.bars_held}</Descriptions.Item>
            <Descriptions.Item label="nav">{s.nav.toFixed(6)}</Descriptions.Item>
            <Descriptions.Item label="peak_nav">{s.peak_nav.toFixed(6)}</Descriptions.Item>
            <Descriptions.Item label="max_drawdown">{(s.max_drawdown * 100).toFixed(2)}%</Descriptions.Item>
            <Descriptions.Item label="turnover">{s.turnover.toFixed(4)}</Descriptions.Item>
            <Descriptions.Item label="last_increase_date">{s.last_increase_date ?? "—"}</Descriptions.Item>
            <Descriptions.Item label="max_price_since_entry">{s.max_price_since_entry?.toFixed(2) ?? "—"}</Descriptions.Item>
            <Descriptions.Item label="min_price_since_entry">{s.min_price_since_entry?.toFixed(2) ?? "—"}</Descriptions.Item>
            <Descriptions.Item label="bars_since_exit">{s.bars_since_exit ?? "—"}</Descriptions.Item>
            <Descriptions.Item label="last_trip_return">{s.last_trip_return ?? "—"}</Descriptions.Item>
            <Descriptions.Item label="trip">{s.trip ? JSON.stringify(s.trip) : "—"}</Descriptions.Item>
          </Descriptions>
        </Card>
      )}
    </div>
  );
}
