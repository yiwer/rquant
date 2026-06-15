import { useEffect, useState } from "react";
import { Alert, Card, Col, Descriptions, Row, Slider, Table, Tag, Typography } from "antd";
import type { ReplayFrameDto } from "@bindings/ReplayFrameDto";
import type { FactorValueDto } from "@bindings/FactorValueDto";
import { api } from "../api/ipc";

const STANCE_COLOR: Record<string, string> = { Long: "green", Short: "red", Flat: "default" };

export default function ReplayView({ runId }: { runId: string }) {
  const [frames, setFrames] = useState<ReplayFrameDto[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [i, setI] = useState(0);
  const [factors, setFactors] = useState<FactorValueDto[]>([]);
  const [factorT, setFactorT] = useState<string | null>(null);
  const [factorsWarn, setFactorsWarn] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setFrames([]);
    setError(null);
    setI(0);
    api.runReplayFrames(runId)
      .then((f) => {
        if (cancelled) return;
        setFrames(f);
        setI(f.length ? f.length - 1 : 0);
        setFactorT(f.length ? f[f.length - 1].t : null);
      })
      .catch((e) => {
        if (cancelled) return;
        setError(String(e));
      });
    return () => { cancelled = true; };
  }, [runId]);

  const f = frames[i];

  useEffect(() => {
    let cancelled = false;
    setFactors([]);
    if (factorT) {
      api.runReplayFactors(runId, factorT).then((v) => {
        if (cancelled) return;
        setFactors(v);
        setFactorsWarn(false);
      }).catch(() => {
        if (!cancelled) setFactorsWarn(true);
      });
    }
    return () => { cancelled = true; };
  }, [runId, factorT]);

  if (error) return <Alert type="info" message={error} />;
  if (!frames.length) return <Typography.Text type="secondary">加载回放帧…</Typography.Text>;

  return (
    <div>
      <Slider
        min={0}
        max={frames.length - 1}
        value={i}
        onChange={setI}
        onChangeComplete={(v) => setFactorT(frames[v ?? 0]?.t ?? null)}
        tooltip={{ formatter: (v) => frames[v ?? 0]?.t }}
      />
      <Row gutter={12}>
        <Col span={14}>
          <Card size="small" title={`决策路径 @ ${f.t}`}
            extra={<Tag color={STANCE_COLOR[f.stance] ?? "default"}>{f.leaf} · {f.stance}</Tag>}>
            <Table
              size="small"
              rowKey={(r) => r.node_id}
              pagination={false}
              dataSource={f.path}
              columns={[
                { title: "节点", dataIndex: "node_id" },
                { title: "分支", dataIndex: "label" },
                { title: "置信", dataIndex: "confidence", render: (v: number) => v.toFixed(3) },
                { title: "依据", dataIndex: "rationale", ellipsis: true },
              ]}
            />
            {f.nav != null && (
              <Descriptions size="small" column={3} style={{ marginTop: 8 }}>
                <Descriptions.Item label="target">{f.target?.toFixed(2)}</Descriptions.Item>
                <Descriptions.Item label="pos">{f.pos?.toFixed(2)}</Descriptions.Item>
                <Descriptions.Item label="nav">{f.nav?.toFixed(6)}</Descriptions.Item>
              </Descriptions>
            )}
          </Card>
        </Col>
        <Col span={10}>
          <Card size="small" title={`因子值(现算)${factorT ? ` @ ${factorT}` : ""}`}>
            {factorsWarn && <Alert type="warning" message="因子值拉取失败" style={{ marginBottom: 8 }} />}
            <Table
              size="small"
              rowKey={(r) => r.name}
              pagination={false}
              dataSource={factors}
              locale={{ emptyText: "该树无 factors 块" }}
              columns={[
                { title: "因子", dataIndex: "name" },
                { title: "值", dataIndex: "value",
                  render: (v: number | null) => (v == null ? <Tag>缺失</Tag> : v.toFixed(6)) },
              ]}
            />
          </Card>
        </Col>
      </Row>
    </div>
  );
}
