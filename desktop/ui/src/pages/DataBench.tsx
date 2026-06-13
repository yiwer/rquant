import { useEffect, useState } from "react";
import { App as AntApp, Button, Card, Col, Input, List, Row, Select, Space, Table, Tag, Typography } from "antd";
import type { CsvInfoDto } from "@bindings/CsvInfoDto";
import type { BarDto } from "@bindings/BarDto";
import type { UniverseInfoDto } from "@bindings/UniverseInfoDto";
import type { Overlay } from "../components/KlineChart";
import KlineChart from "../components/KlineChart";
import { api } from "../api/ipc";

export default function DataBench() {
  const { message } = AntApp.useApp();
  const [csvs, setCsvs] = useState<CsvInfoDto[]>([]);
  const [universes, setUniverses] = useState<UniverseInfoDto[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [bars, setBars] = useState<BarDto[]>([]);
  const [expr, setExpr] = useState("sma(close, 20)");
  const [overlays, setOverlays] = useState<Overlay[]>([]);
  const [fetchSyms, setFetchSyms] = useState("sh600030");
  const [fetchScale, setFetchScale] = useState(60);

  const refresh = () => {
    api.dataCsvList().then(setCsvs).catch(() => {});
    api.universeList().then(setUniverses).catch(() => {});
  };
  useEffect(refresh, []);

  const open = (path: string) => {
    setSelected(path);
    setOverlays([]);
    setBars([]);
    api.dataReadBars(path, 800).then(setBars).catch((e) => message.error(String(e)));
  };

  const addOverlay = async () => {
    if (!selected) return;
    try {
      const pts = await api.dataEvalFactor(selected, expr, 100, 800);
      setOverlays((o) => [...o.slice(-1), { name: expr, points: pts }]); // 至多 2 条
    } catch (e) {
      message.error(String(e));
    }
  };

  const startFetch = async () => {
    const symbols = fetchSyms.split(/[,\s]+/).filter(Boolean);
    if (!symbols.length) return;
    try {
      const id = await api.fetchBatch(symbols, fetchScale, 1023, "qfq");
      message.success(`拉取任务已启动(${id});完成后刷新清单`);
    } catch (e) {
      message.error(String(e));
    }
  };

  return (
    <Row gutter={12}>
      <Col span={8}>
        <Card size="small" title="行情 CSV(paper/ + .rquant-desktop/data/)" extra={<Typography.Link onClick={refresh}>刷新</Typography.Link>}
          style={{ marginBottom: 12 }}>
          <List
            size="small"
            dataSource={csvs}
            style={{ maxHeight: 320, overflow: "auto" }}
            renderItem={(c) => (
              <List.Item
                style={{ cursor: c.rows != null ? "pointer" : "not-allowed",
                  background: c.path === selected ? "rgba(22,119,255,.08)" : undefined }}
                onClick={() => c.rows != null && open(c.path)}
              >
                <List.Item.Meta
                  title={c.path}
                  description={c.rows != null ? `${c.rows} 根 · ${c.first_t} → ${c.last_t}` : "解析失败"}
                />
              </List.Item>
            )}
          />
        </Card>
        <Card size="small" title="批量拉取(新浪 qfq → .rquant-desktop/data/)" style={{ marginBottom: 12 }}>
          <Space.Compact block>
            <Input value={fetchSyms} onChange={(e) => setFetchSyms(e.target.value)} placeholder="sh600030, sz000333" />
            <Select value={fetchScale} onChange={setFetchScale} style={{ width: 110 }}
              options={[{ value: 15 }, { value: 60 }, { value: 240, label: "240(日线)" }]} />
            <Button type="primary" onClick={() => void startFetch()}>拉取</Button>
          </Space.Compact>
          <Typography.Text type="secondary" style={{ fontSize: 12 }}>串行+500ms 节流;进度见任务抽屉</Typography.Text>
        </Card>
        <Card size="small" title="universe 清单">
          <Table
            size="small"
            rowKey="path"
            pagination={false}
            dataSource={universes}
            columns={[
              { title: "清单", dataIndex: "name",
                render: (v: string, u) => (<>{v} {u.frozen && <Tag>deploy 只读</Tag>}</>) },
              { title: "成员", render: (_, u) => u.entries.length },
            ]}
            expandable={{
              expandedRowRender: (u) => (
                <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                  {u.entries.map((e) => e.symbol).join(" · ")}
                </Typography.Text>
              ),
            }}
          />
        </Card>
      </Col>
      <Col span={16}>
        <Card
          size="small"
          title={selected ? `K线 · ${selected}(末 800 根)` : "K线浏览器"}
          extra={
            <Space.Compact>
              <Input value={expr} onChange={(e) => setExpr(e.target.value)} style={{ width: 260 }}
                placeholder="DSL 表达式,如 sma(close,20)" onPressEnter={() => void addOverlay()} />
              <Button onClick={() => void addOverlay()} disabled={!selected}>叠加因子</Button>
            </Space.Compact>
          }
        >
          {bars.length ? (
            <KlineChart bars={bars} overlays={overlays} height={520} />
          ) : (
            <Typography.Text type="secondary">左侧选择 CSV 打开;因子叠加走引擎 DSL 同口径求值(NaN 断线=弃权)</Typography.Text>
          )}
        </Card>
      </Col>
    </Row>
  );
}
