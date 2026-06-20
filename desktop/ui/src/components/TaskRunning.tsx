import { useEffect, useState } from "react";
import { Button, Progress, Space, Spin, Typography } from "antd";
import type { TaskInfoDto } from "@bindings/TaskInfoDto";
import { stageZh } from "../labels";

export default function TaskRunning({ info, startedAt, onCancel }: { info: TaskInfoDto; startedAt?: number; onCancel?: () => void }) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, []);
  const elapsed = startedAt ? Math.max(0, Math.round((now - startedAt) / 1000)) : null;
  const pct = info.progress.pct;
  const determinate = pct > 0 && pct < 1;
  return (
    <div style={{ textAlign: "center", padding: 40 }}>
      <Space direction="vertical" size="middle" style={{ width: 360, maxWidth: "100%" }}>
        {determinate ? <Progress percent={Math.round(pct * 100)} status="active" /> : <Spin />}
        <Typography.Text>
          {stageZh(info.progress.stage)}
          {info.progress.detail ? ` · ${info.progress.detail}` : ""}
          {elapsed != null ? ` · 已耗时 ${elapsed}s` : ""}
        </Typography.Text>
        <Typography.Text type="secondary" style={{ fontSize: 12 }}>
          横截面计算中,通常数十秒;切换页面不会中断,可在右上「任务」查看。
        </Typography.Text>
        {onCancel && <Button size="small" onClick={onCancel}>取消</Button>}
      </Space>
    </div>
  );
}
