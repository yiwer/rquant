import { useEffect, useRef, useState } from "react";
import {
  App as AntApp,
  Button,
  Card,
  Col,
  DatePicker,
  InputNumber,
  Row,
  Segmented,
  Select,
  Statistic,
  Table,
} from "antd";
import { listen } from "@tauri-apps/api/event";
import * as echarts from "echarts";
import { useScreen } from "../stores/screen";

const pct = (v?: number | null) =>
  v == null ? "—" : `${(v * 100).toFixed(1)}%`;

/** Inline excess-return line chart — avoids NavChart's JournalPointDto mismatch. */
function ExcessChart({ data }: { data: { t: string; excess: number }[] }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!ref.current) return;
    const chart = echarts.init(ref.current);
    chart.setOption({
      tooltip: { trigger: "axis" },
      xAxis: { type: "category", data: data.map((p) => p.t) },
      yAxis: { type: "value", scale: true },
      series: [
        {
          name: "累计超额",
          type: "line",
          data: data.map((p) => p.excess),
          lineStyle: { color: "#16a34a" },
          areaStyle: { color: "rgba(22,163,74,0.08)" },
        },
      ],
      grid: { left: 52, right: 16, top: 24, bottom: 24 },
    });
    const onResize = () => chart.resize();
    window.addEventListener("resize", onResize);
    return () => {
      window.removeEventListener("resize", onResize);
      chart.dispose();
    };
  }, [data]);
  return <div ref={ref} style={{ height: 240 }} />;
}

export default function ScreenBacktestResult() {
  const st = useScreen();
  const { message } = AntApp.useApp();

  const [config, setConfig] = useState<string>("");
  const [from, setFrom] = useState("2018-01-01");
  const [to, setTo] = useState("2026-06-12");
  const [top, setTop] = useState(50);
  const [reb, setReb] = useState(1);
  const [cost, setCost] = useState(20);
  const [running, setRunning] = useState(false);
  const [selId, setSelId] = useState<string | null>(null);

  useEffect(() => {
    void st.loadConfigs();
    void st.loadRuns();
  }, []);

  async function runBacktest() {
    if (!config) {
      message.warning("请选择配置");
      return;
    }
    setRunning(true);
    try {
      const taskId = await st.api.screenBacktestRun(
        config,
        from,
        to,
        top,
        reb,
        cost
      );
      const un = await listen<{
        id: string;
        status: string;
        result: { run_id?: string } | null;
      }>("task://progress", (e) => {
        if (e.payload.id !== taskId) return;
        if (e.payload.status === "done") {
          setRunning(false);
          void un();
          void st.loadRuns().then(() => {
            const rid = e.payload.result?.run_id;
            if (rid) {
              setSelId(rid);
              void st.selectRun(rid);
            }
          });
        } else if (e.payload.status === "failed") {
          message.error("回测失败");
          setRunning(false);
          void un();
        }
      });
    } catch (e) {
      message.error(String(e));
      setRunning(false);
    }
  }

  const rep = st.report;
  const ir = st.indexRel;

  return (
    <div>
      {/* 参数 + 运行 */}
      <Card size="small" style={{ marginBottom: 8 }}>
        <Row gutter={8} align="middle">
          <Col flex="auto">
            <Select
              style={{ width: "100%" }}
              placeholder="配置"
              value={config || undefined}
              onChange={setConfig}
              options={st.configs.map((c) => ({
                value: c.path,
                label: c.name ?? c.path,
              }))}
            />
          </Col>
          <Col>
            <DatePicker
              placeholder="从"
              onChange={(_, s) =>
                setFrom(((Array.isArray(s) ? s[0] : s) ?? "") as string)
              }
            />
          </Col>
          <Col>
            <DatePicker
              placeholder="到"
              onChange={(_, s) =>
                setTo(((Array.isArray(s) ? s[0] : s) ?? "") as string)
              }
            />
          </Col>
          <Col>
            <InputNumber
              addonBefore="Top"
              min={1}
              value={top}
              onChange={(v) => setTop(v ?? 50)}
            />
          </Col>
          <Col>
            <InputNumber
              addonBefore="调仓"
              min={1}
              value={reb}
              onChange={(v) => setReb(v ?? 1)}
            />
          </Col>
          <Col>
            <InputNumber
              addonBefore="成本bps"
              min={0}
              value={cost}
              onChange={(v) => setCost(v ?? 20)}
            />
          </Col>
          <Col>
            <Button
              type="primary"
              loading={running}
              disabled={!config}
              onClick={runBacktest}
            >
              运行回测
            </Button>
          </Col>
        </Row>
        <div style={{ marginTop: 8 }}>
          <Select
            style={{ width: 360 }}
            placeholder="选择历史回测"
            allowClear
            value={selId ?? undefined}
            onChange={(id: string | undefined) => {
              if (id) {
                setSelId(id);
                void st.selectRun(id);
              } else {
                setSelId(null);
              }
            }}
            options={st.runs.map((r) => ({
              value: r.id,
              label: `${r.config} · ${r.from}~${r.to} · top${r.top} · ${r.created}`,
            }))}
          />
        </div>
      </Card>

      {!rep ? (
        <span style={{ opacity: 0.6 }}>运行或选择一次回测以查看结果</span>
      ) : (
        <>
          {/* Benchmark 切换 */}
          <Segmented
            value={st.benchmark}
            options={[
              ...st.indices.map((i) => ({
                value: i,
                label: i.toUpperCase(),
              })),
              { value: "EW", label: "等权·参考" },
            ]}
            onChange={(b) => {
              if (selId) void st.setBenchmark(selId, b as string);
            }}
          />

          {/* 一等口径：指数相对 */}
          <Card
            size="small"
            style={{ marginTop: 8, background: "rgba(59,130,246,.05)" }}
            title={`指数相对（vs ${st.benchmark.toUpperCase()}）`}
          >
            <Row gutter={16}>
              <Col>
                <Statistic
                  title="净超额(累计)"
                  value={pct(ir?.excess_cum)}
                  valueStyle={{ color: "#16a34a" }}
                />
              </Col>
              <Col>
                <Statistic
                  title="OOS超额"
                  value={pct(
                    ir?.per_regime.find((r) => r.label.includes("OOS"))?.excess
                  )}
                  valueStyle={{ color: "#16a34a" }}
                />
              </Col>
              <Col>
                <Statistic
                  title="盈亏平衡(bps)"
                  value={
                    rep.break_even != null
                      ? rep.break_even.toFixed(0)
                      : "—"
                  }
                />
              </Col>
            </Row>
          </Card>

          {/* 次行：绝对口径 */}
          <div style={{ opacity: 0.8, fontSize: 12, margin: "8px 0" }}>
            绝对：净总 {pct(rep.net_total_return)} · Sharpe{" "}
            {rep.abs_sharpe != null ? rep.abs_sharpe.toFixed(2) : "—"} · 回撤{" "}
            {pct(rep.max_drawdown)} · 换手 {rep.turnover.toFixed(1)}
          </div>

          {/* 累计超额曲线（独立成行） */}
          {ir && ir.curve.length > 0 && (
            <Card
              size="small"
              title={`累计超额曲线（vs ${st.benchmark.toUpperCase()}）`}
              style={{ marginBottom: 8 }}
            >
              <ExcessChart data={ir.curve} />
            </Card>
          )}

          {/* 三联等高网格 */}
          <Row gutter={8}>
            <Col span={8}>
              <Card size="small" title="regime 切片（超额）">
                {ir?.per_regime.map((r) => (
                  <div
                    key={r.label}
                    style={{ display: "flex", justifyContent: "space-between" }}
                  >
                    <span>{r.label}</span>
                    <span style={{ color: "#16a34a" }}>{pct(r.excess)}</span>
                  </div>
                ))}
              </Card>
            </Col>
            <Col span={8}>
              <Card size="small" title="标签归因">
                {rep.tag_attribution.map((t) => (
                  <div
                    key={t.tag}
                    style={{ display: "flex", justifyContent: "space-between" }}
                  >
                    <span>{t.tag}</span>
                    <span>{pct(t.mean_fwd_return)}</span>
                  </div>
                ))}
              </Card>
            </Col>
            <Col span={8}>
              <Card size="small" title="优质分分层">
                <Table
                  size="small"
                  pagination={false}
                  rowKey="layer"
                  columns={[
                    { title: "层", dataIndex: "layer" },
                    {
                      title: "年化",
                      dataIndex: "mean_fwd_return",
                      render: (v: number) => pct(v),
                    },
                  ]}
                  dataSource={rep.quality_layers}
                />
              </Card>
            </Col>
          </Row>
        </>
      )}
    </div>
  );
}
