import { useEffect, useState } from "react";
import { Button, Card, Col, Input, Row, Table } from "antd";
import { useVerdict } from "../stores/verdict";
import VerdictMatrix from "../components/VerdictMatrix";
export default function Verdict() {
  const st = useVerdict();
  const [sel, setSel] = useState<string[]>([]); const [name, setName] = useState("");
  useEffect(() => { void st.loadReports(); }, []);
  return (
    <Row gutter={12}>
      <Col span={9}>
        <Card size="small" title="选 optimize 报告(可多选)">
          <Table size="small" rowKey="path" pagination={false} dataSource={st.reports}
            rowSelection={{ selectedRowKeys: sel, onChange: (k) => setSel(k as string[]) }}
            columns={[{ title: "报告", dataIndex: "name", render: (n: string | null, r) => n ?? r.path },
                      { title: "组合", dataIndex: "n_combos" }, { title: "折", dataIndex: "folds" }]} />
          <Input style={{ marginTop: 8 }} placeholder="策略名(可空,默认首标的)" value={name} onChange={(e) => setName(e.target.value)} />
          <Button type="primary" block style={{ marginTop: 8 }} disabled={!sel.length}
            onClick={() => { void st.certify(sel, name); }}>运行认证</Button>
        </Card>
      </Col>
      <Col span={15}>{st.verdict ? <VerdictMatrix v={st.verdict} /> : <span style={{ opacity: .6 }}>选报告并运行认证</span>}</Col>
    </Row>
  );
}
