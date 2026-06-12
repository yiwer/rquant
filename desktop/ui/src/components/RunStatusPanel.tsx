import { Badge, Card, Typography } from "antd";
import type { RunlogStatusDto } from "@bindings/RunlogStatusDto";
import type { SchtaskDto } from "@bindings/SchtaskDto";

export default function RunStatusPanel({
  runlog,
  schtask,
  onOpenLog,
}: {
  runlog: RunlogStatusDto;
  schtask: SchtaskDto | null;
  onOpenLog: () => void;
}) {
  const status = runlog.ok == null ? "default" : runlog.ok ? "success" : "error";
  return (
    <Card size="small" title="运行状态">
      <Badge status={status as never} text={runlog.summary} />
      <div>
        <Typography.Text type="secondary" style={{ fontSize: 12 }}>
          {runlog.last_header ?? "暂无 run 记录"}
        </Typography.Text>
      </div>
      <div style={{ marginTop: 8 }}>
        <Typography.Text type="secondary" style={{ fontSize: 12 }}>
          schtask: {schtask ? `${schtask.status ?? "?"} · 下次 ${schtask.next_run ?? "?"}` : "未检测到 rquant-paper"}
        </Typography.Text>
      </div>
      <Typography.Link onClick={onOpenLog}>查看 run.log</Typography.Link>
    </Card>
  );
}
