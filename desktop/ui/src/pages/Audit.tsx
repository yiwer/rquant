import { useEffect, useState } from "react";
import { Table, Select, Input, Drawer, Tag, Typography, Tabs } from "antd";
import { useAudit } from "../stores/audit";
import { auditKindZh } from "../labels";
import type { AuditRecordDto } from "@bindings/AuditRecordDto";
import { api } from "../api/ipc";

const STATUS_COLOR: Record<string, string> = {
  done: "green",
  failed: "red",
  cancelled: "default",
  running: "blue",
};

export default function Audit() {
  const st = useAudit();
  const [kind, setKind] = useState<string | undefined>();
  const [status, setStatus] = useState<string | undefined>();
  const [q, setQ] = useState("");
  const [sel, setSel] = useState<AuditRecordDto | null>(null);
  const [rawLog, setRawLog] = useState("");

  useEffect(() => {
    void st.load(kind, status);
  }, [kind, status]);

  const rows = st.records.filter(
    (r) =>
      !q ||
      JSON.stringify(r.params).includes(q) ||
      r.kind.includes(q) ||
      (r.error ?? "").includes(q),
  );

  return (
    <div>
      <div style={{ display: "flex", gap: 8, marginBottom: 8 }}>
        <Select
          allowClear
          placeholder="类型"
          style={{ width: 140 }}
          value={kind}
          onChange={setKind}
          options={[...new Set(st.records.map((r) => r.kind))].map((k) => ({
            value: k,
            label: auditKindZh(k),
          }))}
        />
        <Select
          allowClear
          placeholder="状态"
          style={{ width: 120 }}
          value={status}
          onChange={setStatus}
          options={["done", "failed", "cancelled"].map((s) => ({
            value: s,
            label: s,
          }))}
        />
        <Input
          placeholder="检索参数/错误"
          value={q}
          onChange={(e) => setQ(e.target.value)}
          style={{ width: 220 }}
          allowClear
        />
        <Typography.Link
          onClick={() => {
            void api.auditLogTail(400).then(setRawLog);
          }}
        >
          原始日志
        </Typography.Link>
      </div>
      <Table
        size="small"
        rowKey="id"
        dataSource={rows}
        pagination={{ pageSize: 20 }}
        onRow={(r) => ({ onClick: () => setSel(r) })}
        columns={[
          { title: "时间", dataIndex: "started_at", width: 160 },
          {
            title: "类型",
            dataIndex: "kind",
            render: auditKindZh,
            width: 110,
          },
          {
            title: "状态",
            dataIndex: "status",
            width: 90,
            render: (s: string) => (
              <Tag color={STATUS_COLOR[s] ?? "default"}>{s}</Tag>
            ),
          },
          {
            title: "耗时",
            dataIndex: "duration_ms",
            width: 90,
            render: (m: number) => `${(m / 1000).toFixed(1)}s`,
          },
          {
            title: "参数",
            dataIndex: "params",
            ellipsis: true,
            render: (p: unknown) => JSON.stringify(p),
          },
          {
            title: "错误",
            dataIndex: "error",
            ellipsis: true,
            render: (e: string | null) =>
              e ? <span style={{ color: "#dc2626" }}>{e}</span> : "",
          },
        ]}
      />
      <Drawer
        title={sel ? `${auditKindZh(sel.kind)} · ${sel.id}` : ""}
        open={!!sel}
        onClose={() => setSel(null)}
        width={560}
      >
        {sel && (
          <Tabs
            items={[
              {
                key: "detail",
                label: "详情",
                children: (
                  <>
                    <p>
                      <b>参数</b>
                    </p>
                    <pre style={{ whiteSpace: "pre-wrap" }}>
                      {JSON.stringify(sel.params, null, 2)}
                    </pre>
                    <p>
                      <b>阶段时序</b>
                    </p>
                    {sel.stages.map((s, i) => (
                      <div key={i}>
                        {(s.at_ms / 1000).toFixed(1)}s · {s.stage} {s.detail}
                      </div>
                    ))}
                    <p>
                      <b>触及文件</b>（桥层输入/产物，非逐股）
                    </p>
                    {sel.files.map((f, i) => (
                      <div key={i} style={{ fontSize: 12, opacity: 0.8 }}>
                        {f}
                      </div>
                    ))}
                    {sel.result_summary && (
                      <p>
                        <b>结果</b>：{sel.result_summary}
                      </p>
                    )}
                    {sel.error && (
                      <>
                        <p>
                          <b>完整错误</b>
                        </p>
                        <pre
                          style={{
                            whiteSpace: "pre-wrap",
                            color: "#dc2626",
                          }}
                        >
                          {sel.error}
                        </pre>
                      </>
                    )}
                  </>
                ),
              },
            ]}
          />
        )}
      </Drawer>
      {rawLog && (
        <Drawer
          title="原始日志（尾部）"
          open={!!rawLog}
          onClose={() => setRawLog("")}
          width={680}
        >
          <pre style={{ whiteSpace: "pre-wrap", fontSize: 12 }}>{rawLog}</pre>
        </Drawer>
      )}
    </div>
  );
}
