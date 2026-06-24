import { useEffect, useState } from "react";
import { Card, Table, Button, Statistic, Row, Col, Empty, Tag, Space, message } from "antd";
import { api } from "../api/ipc";
import type { PaperStatusDto } from "@bindings/PaperStatusDto";
import StockDetailDrawer from "../components/StockDetailDrawer";

export default function PaperRidge() {
  const [s, setS] = useState<PaperStatusDto | null>(null);
  const [sel, setSel] = useState<string | null>(null);
  const load = () => api.paperRidgeStatus().then(setS).catch((e) => message.error(String(e)));
  useEffect(() => { load(); }, []);
  const act = async (fn: () => Promise<string>, name: string) => {
    try { await fn(); message.success(`${name} 已启动(见任务抽屉)`); } catch (e) { message.error(String(e)); }
  };
  const label = (code: string) => s?.names[code] ? `${code} ${s.names[code]}` : code;
  if (!s) return <Empty description="加载中…" />;
  if (!s.initialized) return (
    <Card title="纸面盘 · 去相关岭组合">
      <Empty description="尚未冻结权重">
        <Button type="primary" onClick={() => act(api.paperRidgeRetrain, "重训")}>重训权重(生成)</Button>
      </Empty>
    </Card>
  );
  return (
    <>
      <StockDetailDrawer symbol={sel} onClose={() => setSel(null)} />
      <Space direction="vertical" style={{ width: "100%" }} size="middle">
        <Card title={`纸面盘 · ${s.strategy}`} extra={
          <Space>
            <Button onClick={() => act(api.paperRidgeAdvance, "推进纸面册")}>推进纸面册</Button>
            <Button onClick={() => act(api.paperRidgeRetrain, "重训")}>重训权重</Button>
            <Button onClick={() => act(api.paperBlendRecompute, "重算对照")}>重算对照</Button>
          </Space>}>
          <Row gutter={16}>
            <Col><Statistic title="训练区间" value={`${s.train_lo}~${s.train_hi}`} /></Col>
            <Col><Statistic title="周数" value={Number(s.n_train_dates)} /></Col>
            <Col><Statistic title="delta" value={s.delta} precision={2} /></Col>
            <Col><Statistic title={`top${s.top_n} · 成本`} value={`${s.cost_bps}bp`} /></Col>
            <Col><Statistic title="累计净收益" value={s.cum_net} precision={4} /></Col>
            <Col><Statistic title="超额 vs csi300" value={s.cum_excess ?? "-"} precision={s.cum_excess == null ? undefined : 4} /></Col>
          </Row>
        </Card>
        <Card title="本周持仓 (open)">
          {s.open_picks.length
            ? s.open_picks.map((p) => (
                <Tag key={p} onClick={() => setSel(p)} style={{ cursor: "pointer" }}>
                  {label(p)}
                </Tag>
              ))
            : <Empty description="无持仓" />}
        </Card>
        <Card title="纸面册 (已结算)">
          <Table rowKey="date" size="small" pagination={false} dataSource={s.closed} columns={[
            { title: "日期", dataIndex: "date" },
            {
              title: "选股",
              dataIndex: "picks",
              render: (p: string[]) => (
                <>
                  {p.map((code, i) => (
                    <span key={code}>
                      {i > 0 && " "}
                      <a onClick={() => setSel(code)}>{label(code)}</a>
                    </span>
                  ))}
                </>
              ),
            },
            { title: "换手", dataIndex: "turnover", render: (v: number | null) => v?.toFixed(2) ?? "-" },
            { title: "毛", dataIndex: "gross_ret", render: (v: number | null) => v?.toFixed(4) ?? "-" },
            { title: "净", dataIndex: "net_ret", render: (v: number | null) => v?.toFixed(4) ?? "-" },
            { title: "NAV", dataIndex: "nav", render: (v: number) => v.toFixed(4) },
          ]} />
        </Card>
        {s.blend && (
          <Card title="岭值双引擎 6 折对照(回测)">
            <Table rowKey="oos" size="small" pagination={false} dataSource={s.blend.folds} columns={[
              { title: "OOS", dataIndex: "oos" },
              { title: "相关", dataIndex: "corr", render: (v: number) => v.toFixed(2) },
              { title: "Sh岭", dataIndex: "sh_ridge", render: (v: number) => v.toFixed(2) },
              { title: "Sh值", dataIndex: "sh_val", render: (v: number) => v.toFixed(2) },
              { title: "Sh合", dataIndex: "sh_blend", render: (v: number) => v.toFixed(2) },
              { title: "回撤合", dataIndex: "dd_blend", render: (v: number) => v.toFixed(2) },
              { title: "超额合", dataIndex: "ex_blend", render: (v: number) => v.toFixed(3) },
            ]} />
            <div style={{ marginTop: 8 }}>均值:相关 {s.blend.mean.corr.toFixed(2)} · Sharpe 岭/值/合 {s.blend.mean.sh_ridge.toFixed(2)}/{s.blend.mean.sh_val.toFixed(2)}/{s.blend.mean.sh_blend.toFixed(2)} · 回撤合 {s.blend.mean.dd_blend.toFixed(2)}</div>
          </Card>
        )}
      </Space>
    </>
  );
}
