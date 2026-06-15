import { useEffect, useState } from "react";
import { useParams, Link } from "react-router-dom";
import { Alert, Card, Descriptions, Spin, Tooltip, Typography } from "antd";
import type { BookDetailDto } from "@bindings/BookDetailDto";
import { api } from "../api/ipc";
import { snapshotFieldZh } from "../labels";
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
      <Card
        size="small"
        title={data.card.kind === "portfolio" ? "持仓成员数 (journal)" : "纸面净值 (journal)"}
        extra={<Typography.Text type="secondary" style={{ fontSize: 12 }}>自桌面端启用日积累</Typography.Text>}
        style={{ marginBottom: 12 }}
      >
        {data.journal.length ? (
          <NavChart points={data.journal} portfolio={data.card.kind === "portfolio"} />
        ) : (
          <Typography.Text type="secondary">journal 暂无数据——历史从桌面端启用日开始积累</Typography.Text>
        )}
      </Card>
      {s && (
        <Card
          size="small"
          title={
            <span>
              持仓快照{" "}
              <Typography.Text type="secondary" style={{ fontSize: 12, fontWeight: "normal" }}>
                只读 · 13 字段
              </Typography.Text>
            </span>
          }
        >
          <Descriptions size="small" column={3} bordered>
            {(["pos","entry_price","bars_held","nav","peak_nav","max_drawdown","turnover",
               "last_increase_date","max_price_since_entry","min_price_since_entry",
               "bars_since_exit","last_trip_return","trip"] as const).map((key) => {
              const value = (() => {
                switch (key) {
                  case "pos": return s.pos;
                  case "entry_price": return s.entry_price?.toFixed(2) ?? "—";
                  case "bars_held": return s.bars_held;
                  case "nav": return s.nav.toFixed(6);
                  case "peak_nav": return s.peak_nav.toFixed(6);
                  case "max_drawdown": return `${(s.max_drawdown * 100).toFixed(2)}%`;
                  case "turnover": return s.turnover.toFixed(4);
                  case "last_increase_date": return s.last_increase_date ?? "—";
                  case "max_price_since_entry": return s.max_price_since_entry?.toFixed(2) ?? "—";
                  case "min_price_since_entry": return s.min_price_since_entry?.toFixed(2) ?? "—";
                  case "bars_since_exit": return s.bars_since_exit ?? "—";
                  case "last_trip_return": return s.last_trip_return ?? "—";
                  case "trip": return s.trip ? JSON.stringify(s.trip) : "—";
                }
              })();
              return (
                <Descriptions.Item
                  key={key}
                  label={
                    <Tooltip title={<span style={{ fontFamily: "monospace" }}>{key}</span>}>
                      {snapshotFieldZh(key)}
                    </Tooltip>
                  }
                >
                  {value}
                </Descriptions.Item>
              );
            })}
          </Descriptions>
        </Card>
      )}
    </div>
  );
}
