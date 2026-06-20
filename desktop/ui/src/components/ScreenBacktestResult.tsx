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
  Tabs,
  Tooltip,
} from "antd";
import SectorAttrib from "./SectorAttrib";
import TwoLegBlend from "./TwoLegBlend";
import DeployHardening from "./DeployHardening";
import TaskRunning from "./TaskRunning";
import { QuestionCircleOutlined } from "@ant-design/icons";
import * as echarts from "echarts";
import { useScreen } from "../stores/screen";
import { useTaskInfo, useTaskStartedAt } from "../stores/tasks";
import { indexZh, regimeLabelZh, TERM_HELP } from "../labels";

const pct = (v?: number | null) =>
  v == null ? "—" : `${(v * 100).toFixed(2)}%`;

/** 一等术语标题 + 克制的 ? 角标悬浮解释。 */
function HelpTitle({ text, help }: { text: string; help: string }) {
  return (
    <span>
      {text}{" "}
      <Tooltip title={help}>
        <QuestionCircleOutlined style={{ opacity: 0.45, fontSize: 12, cursor: "help" }} />
      </Tooltip>
    </span>
  );
}

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
  return <div ref={ref} style={{ height: 220 }} />;
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
  const [selId, setSelId] = useState<string | null>(null);
  const [anaTab, setAnaTab] = useState("sector");

  const btInfo = useTaskInfo(st.btTaskId);
  const btStartedAt = useTaskStartedAt(st.btTaskId);
  const running = !!st.btTaskId && (!btInfo || (btInfo.status !== "done" && btInfo.status !== "failed" && btInfo.status !== "cancelled"));

  useEffect(() => {
    void st.loadConfigs();
    void st.loadRuns();
  }, []);

  // After backtest completes, fetch report and refresh runs list
  useEffect(() => {
    if (!st.btRunId) return;
    const rid = st.btRunId;
    void st.loadRuns().then(() => {
      setSelId(rid);
      void st.selectRun(rid);
    });
  }, [st.btRunId]);

  async function handleRunBacktest() {
    if (!config) {
      message.warning("请选择配置");
      return;
    }
    await st.runBacktest(config, from, to, top, reb, cost);
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
              addonBefore="数量"
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
              addonBefore="成本(基点)"
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
              onClick={handleRunBacktest}
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
        {st.btError && (
          <div style={{ color: "#dc2626", marginTop: 8, fontSize: 13 }}>{st.btError}</div>
        )}
      </Card>

      {running && btInfo ? (
        <TaskRunning info={btInfo} startedAt={btStartedAt} />
      ) : running && !btInfo ? (
        <div style={{ textAlign: "center", padding: 48, opacity: 0.6 }}>回测启动中…</div>
      ) : !rep ? (
        <span style={{ opacity: 0.6 }}>
          设置区间与参数后点「运行回测」，或在上方下拉里选择一次历史回测查看结果。
        </span>
      ) : (
        <>
          {/* Benchmark 切换 */}
          <Segmented
            value={st.benchmark}
            options={[
              ...st.indices.map((i) => ({
                value: i,
                label: indexZh(i),
              })),
              {
                value: "EW",
                label: (
                  <Tooltip title={TERM_HELP.ewRef}>
                    <span>等权基准（不可投）</span>
                  </Tooltip>
                ),
              },
            ]}
            onChange={(b) => {
              if (selId) void st.setBenchmark(selId, b as string);
            }}
          />

          {/* 一等口径：指数相对 */}
          <Card
            size="small"
            style={{ marginTop: 8, background: "rgba(59,130,246,.05)" }}
            title={
              <HelpTitle
                text={`指数相对（vs ${indexZh(st.benchmark)}）`}
                help={TERM_HELP.indexRel}
              />
            }
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
                  title={<HelpTitle text="样本外超额" help={TERM_HELP.oos} />}
                  value={pct(
                    ir?.per_regime.find((r) => r.label.includes("OOS"))?.excess
                  )}
                  valueStyle={{ color: "#16a34a" }}
                />
              </Col>
              <Col>
                <Statistic
                  title={<HelpTitle text="盈亏平衡(基点)" help={TERM_HELP.breakEven} />}
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
            绝对：净总 {pct(rep.net_total_return)} · 夏普{" "}
            {rep.abs_sharpe != null ? rep.abs_sharpe.toFixed(2) : "—"} · 回撤{" "}
            {pct(rep.max_drawdown)} · 换手 {rep.turnover.toFixed(2)}
          </div>

          {/* 累计超额曲线（独立成行，固定高度） */}
          {ir && ir.curve.length > 0 && (
            <Card
              size="small"
              title={`累计超额曲线（vs ${indexZh(st.benchmark)}）`}
              style={{ marginBottom: 8 }}
            >
              <ExcessChart data={ir.curve} />
            </Card>
          )}

          {/* 三联等高网格 */}
          <Row gutter={8} align="stretch">
            <Col span={8} style={{ display: "flex" }}>
              <Card size="small" title="分段切片（超额）" style={{ width: "100%" }}>
                {ir?.per_regime.map((r) => (
                  <div
                    key={r.label}
                    style={{ display: "flex", justifyContent: "space-between" }}
                  >
                    <span>{regimeLabelZh(r.label)}</span>
                    <span style={{ color: "#16a34a" }}>{pct(r.excess)}</span>
                  </div>
                ))}
              </Card>
            </Col>
            <Col span={8} style={{ display: "flex" }}>
              <Card size="small" title="标签归因" style={{ width: "100%" }}>
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
            <Col span={8} style={{ display: "flex" }}>
              <Card size="small" title="优质分分层" style={{ width: "100%" }}>
                <Table
                  size="small"
                  pagination={false}
                  rowKey="layer"
                  columns={[
                    { title: "层", dataIndex: "layer" },
                    {
                      title: "区间收益",
                      dataIndex: "mean_fwd_return",
                      render: (v: number) => pct(v),
                    },
                  ]}
                  dataSource={rep.quality_layers}
                />
              </Card>
            </Col>
          </Row>
          {selId && (
            <Card size="small" title="分析" style={{ marginTop: 8 }}>
              <Tabs activeKey={anaTab} onChange={setAnaTab} items={[
                { key: "sector", label: "行业归因", children: anaTab === "sector" ? <SectorAttrib runId={selId} /> : null },
                { key: "twoleg", label: "两腿组合", children: anaTab === "twoleg" ? <TwoLegBlend runId={selId} /> : null },
                { key: "deploy", label: "部署加固", children: anaTab === "deploy" ? <DeployHardening runId={selId} /> : null },
              ]} />
            </Card>
          )}
        </>
      )}
    </div>
  );
}
