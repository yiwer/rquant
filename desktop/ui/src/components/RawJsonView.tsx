import { useEffect, useState } from "react";
import { Typography } from "antd";
import { api } from "../api/ipc";

export default function RawJsonView({ runId }: { runId: string }) {
  const [txt, setTxt] = useState("");
  useEffect(() => {
    let cancelled = false;
    setTxt("");
    api
      .runSummary(runId)
      .then((s) => {
        if (!cancelled) setTxt(JSON.stringify(s.raw ?? s, null, 2));
      })
      .catch((e) => {
        if (!cancelled) setTxt(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [runId]);
  return (
    <div>
      <Typography.Text type="secondary" style={{ fontSize: 12 }}>
        sim 模式显示摘要 DTO;score 模式显示 result.json 原样。完整文件在 .rquant-desktop/runs/
        {runId}/
      </Typography.Text>
      <pre style={{ fontSize: 12, maxHeight: 480, overflow: "auto" }}>{txt}</pre>
    </div>
  );
}
