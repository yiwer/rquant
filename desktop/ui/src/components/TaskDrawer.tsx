import { useCallback, useEffect, useState } from "react";
import { Badge, Button, Drawer, List, Progress, Typography } from "antd";
import { listen } from "@tauri-apps/api/event";
import type { TaskInfoDto } from "@bindings/TaskInfoDto";
import { api } from "../api/ipc";

const STATUS_BADGE: Record<string, string> = {
  running: "processing", done: "success", failed: "error", cancelled: "default",
};

export default function TaskDrawer() {
  const [open, setOpen] = useState(false);
  const [tasks, setTasks] = useState<TaskInfoDto[]>([]);

  const refresh = useCallback(() => void api.taskList().then(setTasks).catch(() => {}), []);

  useEffect(() => {
    refresh();
    // 后端双发:精确通道 + 固定通道 task://progress(T11);统一订阅固定通道全量刷新
    const un = listen("task://progress", refresh).catch(() => () => {});
    const timer = setInterval(refresh, 2000);
    return () => {
      void un.then((f) => f());
      clearInterval(timer);
    };
  }, [refresh]);

  const running = tasks.filter((t) => t.status === "running").length;

  return (
    <>
      <Badge count={running} size="small">
        <Button size="small" onClick={() => setOpen(true)}>任务</Button>
      </Badge>
      <Drawer title="任务" open={open} onClose={() => setOpen(false)} width={420}>
        <List
          dataSource={tasks}
          locale={{ emptyText: "暂无任务" }}
          renderItem={(t) => (
            <List.Item
              actions={t.status === "running" ? [<Typography.Link key="c" onClick={() => void api.taskCancel(t.id)}>取消</Typography.Link>] : []}
            >
              <List.Item.Meta
                title={<Badge status={(STATUS_BADGE[t.status] ?? "default") as never} text={`${t.kind} · ${t.id}`} />}
                description={
                  <>
                    <Progress percent={Math.round(t.progress.pct * 100)} size="small" />
                    <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                      {t.progress.stage} {t.progress.detail} {t.error ?? ""}
                    </Typography.Text>
                  </>
                }
              />
            </List.Item>
          )}
        />
      </Drawer>
    </>
  );
}
