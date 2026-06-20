import { useEffect, useRef, useState } from "react";
import { App as AntApp, Button, Card, Col, DatePicker, Row, Statistic, Table } from "antd";
import { listen } from "@tauri-apps/api/event";
import * as echarts from "echarts";
import { useDeploy } from "../stores/deploy";
import type { DeployMonthDto } from "@bindings/DeployMonthDto";
import type { DeployNavPointDto } from "@bindings/DeployNavPointDto";
import DiffTable from "../components/DiffTable";

/** Inline NAV vs 沪深300 chart — NavChart is coupled to JournalPointDto/portfolio shape
 *  and cannot accept DeployNavPointDto. Built inline following ScreenBacktestResult's
 *  ExcessChart pattern. Shows both book NAV and bench_nav as two separate lines. */
function NavVsBenchChart({ data }: { data: DeployNavPointDto[] }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!ref.current) return;
    const chart = echarts.init(ref.current);
    chart.setOption({
      tooltip: { trigger: "axis" },
      legend: { data: ["价值盘 NAV", "沪深300"], top: 4 },
      xAxis: { type: "category", data: data.map((p) => p.t) },
      yAxis: { type: "value", scale: true },
      series: [
        {
          name: "价值盘 NAV",
          type: "line",
          data: data.map((p) => p.nav),
          lineStyle: { color: "#16a34a" },
          showSymbol: false,
        },
        {
          name: "沪深300",
          type: "line",
          data: data.map((p) => p.bench_nav),
          lineStyle: { color: "#9ca3af", type: "dashed" },
          showSymbol: false,
        },
      ],
      grid: { left: 52, right: 16, top: 40, bottom: 24 },
    });
    const onResize = () => chart.resize();
    window.addEventListener("resize", onResize);
    return () => {
      window.removeEventListener("resize", onResize);
      chart.dispose();
    };
  }, [data]);
  return <div ref={ref} style={{ height: 260 }} />;
}

export default function Deploy() {
  const st = useDeploy();
  const { message } = AntApp.useApp();
  const [asOf, setAsOf] = useState("");
  const [running, setRunning] = useState(false);

  useEffect(() => { void st.load(); }, []);

  const pct = (v?: number | null) => (v == null ? "—" : `${(v * 100).toFixed(1)}%`);

  async function runMonth() {
    if (!asOf) { message.warning("请选月末日期"); return; }
    setRunning(true);
    try {
      const taskId = await st.api.deployRunMonth(asOf);
      const un = await listen<{ id: string; status: string; result: DeployMonthDto | null }>(
        "task://progress",
        (e) => {
          if (e.payload.id !== taskId) return;
          if (e.payload.status === "done") {
            st.setPreview(e.payload.result);
            setRunning(false);
            void un();
          } else if (e.payload.status === "failed") {
            message.error("跑本月失败");
            setRunning(false);
            void un();
          }
        }
      );
    } catch (e) {
      message.error(String(e));
      setRunning(false);
    }
  }

  async function confirm() {
    if (st.preview) {
      await st.commit(st.preview.as_of);
      message.success("已调仓落账");
    }
  }

  const b = st.book;
  const pv = st.preview;

  return (
    <Row gutter={12}>
      <Col span={9}>
        <Card size="small" title="价值选股盘(纸面 · 不下真单)">
          {b && b.status !== "empty" ? (
            <Row gutter={12}>
              <Col><Statistic title="NAV" value={b.nav?.toFixed(3) ?? "—"} /></Col>
              <Col><Statistic title="累计超额" value={pct(b.excess_total)} valueStyle={{ color: "#16a34a" }} /></Col>
              <Col><Statistic title="持仓" value={b.holdings.length} /></Col>
            </Row>
          ) : (
            <span style={{ opacity: 0.6 }}>未建仓——选月末日期跑首月</span>
          )}
          <div style={{ marginTop: 12 }}>
            <DatePicker
              onChange={(_, s) => setAsOf(((Array.isArray(s) ? s[0] : s) ?? "") as string)}
            />
            <Button
              type="primary"
              loading={running}
              style={{ marginLeft: 8 }}
              onClick={runMonth}
            >
              跑本月(预览)
            </Button>
          </div>
          {pv && (
            <Card
              size="small"
              title={`预览 ${pv.as_of}：拟 NAV ${pv.proj_nav.toFixed(3)} · 超额 ${pct(pv.proj_excess)} · 实现 ${pct(pv.realized_ret)}`}
              style={{ marginTop: 8 }}
            >
              <DiffTable rows={pv.diff} t={pv.as_of} />
              <Button
                type="primary"
                danger
                block
                style={{ marginTop: 8 }}
                onClick={confirm}
              >
                确认调仓(落账)
              </Button>
            </Card>
          )}
        </Card>
      </Col>
      <Col span={15}>
        <Card size="small" title="NAV vs 沪深300">
          {b && b.nav_history.length ? (
            <NavVsBenchChart data={b.nav_history} />
          ) : (
            <span style={{ opacity: 0.6 }}>暂无净值，跑首月后显示</span>
          )}
        </Card>
        <Card size="small" title="月度调仓" style={{ marginTop: 8 }}>
          <Table
            size="small"
            rowKey="as_of"
            pagination={false}
            dataSource={b?.months ?? []}
            columns={[
              { title: "日期", dataIndex: "as_of" },
              { title: "NAV", dataIndex: "nav", render: (v: number) => v.toFixed(3) },
              { title: "超额", dataIndex: "excess", render: pct },
              { title: "持仓", dataIndex: "n_holdings" },
              { title: "买", dataIndex: "n_buy" },
              { title: "卖", dataIndex: "n_sell" },
            ]}
          />
        </Card>
      </Col>
    </Row>
  );
}
