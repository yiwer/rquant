import { useEffect, useMemo, useState } from "react";
import { App as AntApp, Button, Card, Col, Input, List, Row, Select, Space, Table, Tag, Tooltip, Typography } from "antd";
import type { CsvInfoDto } from "@bindings/CsvInfoDto";
import type { BarDto } from "@bindings/BarDto";
import type { UniverseInfoDto } from "@bindings/UniverseInfoDto";
import type { Overlay } from "../components/KlineChart";
import KlineChart from "../components/KlineChart";
import { api } from "../api/ipc";
import { friendlyError } from "../errors";

// Extracts a display label from a CSV path.
// Strips leading p_/pd_ prefix and .csv suffix from the basename,
// then formats as "symbol · scale" if a scale segment is present.
// Example: "paper/p_sh600030_60.csv" → "sh600030 · 60m"
//          ".rquant-desktop/data/pd_sz000001.csv" → "sz000001"
function csvLabel(path: string): { primary: string; scale: string | null } {
  // Get the basename (last path segment, works with / and \)
  const basename = path.replace(/\\/g, "/").split("/").pop() ?? path;
  // Strip leading p_ or pd_ prefix and trailing .csv
  const stem = basename.replace(/^pd?_/, "").replace(/\.csv$/i, "");
  // If the stem contains an underscore after the symbol, the part after is the scale
  // Symbol pattern: (sh|sz|bj)\d{6}  — always 8 chars
  const symbolMatch = stem.match(/^((sh|sz|bj)\d{6})(?:_(.+))?$/);
  if (symbolMatch) {
    const symbol = symbolMatch[1];
    const scaleRaw = symbolMatch[3] ?? null;
    const scale = scaleRaw ? `${scaleRaw}m` : null;
    return { primary: symbol, scale };
  }
  // Fallback: show stem as-is
  return { primary: stem, scale: null };
}

const SYMBOL_RE = /^(sh|sz|bj)\d{6}$/;

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
  const [factorLoading, setFactorLoading] = useState(false);

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
    setFactorLoading(true);
    try {
      const pts = await api.dataEvalFactor(selected, expr, 100, 800);
      setOverlays((o) => [...o.slice(-1), { name: expr, points: pts }]); // 至多 2 条
    } catch (e) {
      const fe = friendlyError(String(e));
      message.error(fe.title);
    } finally {
      setFactorLoading(false);
    }
  };

  // Parse and validate the fetch symbols input
  const parsedSyms = useMemo(
    () => fetchSyms.split(/[,\s]+/).filter(Boolean),
    [fetchSyms],
  );
  const validSyms = useMemo(
    () => parsedSyms.filter((s) => SYMBOL_RE.test(s)),
    [parsedSyms],
  );
  const invalidSyms = useMemo(
    () => parsedSyms.filter((s) => !SYMBOL_RE.test(s)),
    [parsedSyms],
  );
  const hasValidSyms = validSyms.length > 0;

  const startFetch = async () => {
    if (!hasValidSyms) return;
    try {
      const id = await api.fetchBatch(validSyms, fetchScale, 1023, "qfq");
      message.success(`拉取任务已启动(${id});完成后刷新清单`);
    } catch (e) {
      const fe = friendlyError(String(e));
      message.error(fe.title);
    }
  };

  return (
    <Row gutter={12}>
      <Col span={8}>
        <Card size="small" title="行情数据库" extra={<Typography.Link onClick={refresh}>刷新</Typography.Link>}
          style={{ marginBottom: 12 }}>
          <List
            size="small"
            dataSource={csvs}
            style={{ maxHeight: 320, overflow: "auto" }}
            renderItem={(c) => {
              const { primary, scale } = csvLabel(c.path);
              const displayTitle = scale ? `${primary} · ${scale}` : primary;
              return (
                <List.Item
                  style={{ cursor: c.rows != null ? "pointer" : "not-allowed",
                    background: c.path === selected ? "rgba(22,119,255,.08)" : undefined }}
                  onClick={() => c.rows != null && open(c.path)}
                >
                  <List.Item.Meta
                    title={
                      <Tooltip title={c.path}>
                        <span>{displayTitle}</span>
                      </Tooltip>
                    }
                    description={
                      c.rows != null
                        ? <><Typography.Text type="secondary" style={{ fontSize: 11 }}>{c.path}</Typography.Text><br />{c.rows} 根 · {c.first_t} → {c.last_t}</>
                        : "解析失败"
                    }
                  />
                </List.Item>
              );
            }}
          />
        </Card>
        <Card size="small" title="批量拉取（新浪 qfq）" style={{ marginBottom: 12 }}>
          <Space.Compact block>
            <Input value={fetchSyms} onChange={(e) => setFetchSyms(e.target.value)} placeholder="sh600030, sz000333" />
            <Select value={fetchScale} onChange={setFetchScale} style={{ width: 110 }}
              options={[{ value: 15 }, { value: 60 }, { value: 240, label: "240(日线)" }]} />
            <Button type="primary" onClick={() => void startFetch()} disabled={!hasValidSyms}>拉取</Button>
          </Space.Compact>
          {/* Real-time preview of parsed symbols */}
          {parsedSyms.length > 0 && (
            <div style={{ marginTop: 4 }}>
              {validSyms.length > 0 && (
                <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                  将拉取: {validSyms.join(", ")}
                </Typography.Text>
              )}
              {invalidSyms.length > 0 && (
                <div>
                  <Typography.Text type="danger" style={{ fontSize: 12 }}>
                    格式有误（须为 sh/sz/bj + 6位数字）: {invalidSyms.join(", ")}
                  </Typography.Text>
                </div>
              )}
            </div>
          )}
          <Typography.Text type="secondary" style={{ fontSize: 12 }}>逐个拉取（节流）；进度见任务抽屉</Typography.Text>
        </Card>
        <Card size="small" title="universe 清单">
          <Table
            size="small"
            rowKey="path"
            pagination={false}
            dataSource={universes}
            columns={[
              { title: "清单", dataIndex: "name",
                render: (v: string, u) => (<>{v} {u.frozen && <Tag>内置</Tag>}</>) },
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
              <Button
                onClick={() => void addOverlay()}
                disabled={!selected || factorLoading}
                loading={factorLoading}
              >
                叠加因子
              </Button>
            </Space.Compact>
          }
        >
          {bars.length ? (
            <KlineChart bars={bars} overlays={overlays} height={520} />
          ) : (
            <Typography.Text type="secondary">左侧选择 CSV 打开；因子叠加走引擎 DSL 同口径求值（NaN 无法计算，显示断线）</Typography.Text>
          )}
        </Card>
      </Col>
    </Row>
  );
}
