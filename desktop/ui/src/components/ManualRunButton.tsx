import { App as AntApp, Button, Checkbox, Modal, Space } from "antd";
import { useState } from "react";
import { api } from "../api/ipc";

export default function ManualRunButton({ onStarted }: { onStarted: (taskId: string) => void }) {
  const { message, modal } = AntApp.useApp();
  const [open, setOpen] = useState(false);
  const [commit, setCommit] = useState(false);
  const [gateMsg, setGateMsg] = useState<string | null>(null);
  const [dryOnly, setDryOnly] = useState(false);

  const openDialog = async () => {
    const gate = await api.runGateNow();
    setDryOnly(gate.gate === "dry_only");
    setGateMsg(gate.message ?? null);
    setCommit(false);
    setOpen(true);
  };

  const start = async (confirmed: boolean) => {
    try {
      const id = await api.manualRun(["b1", "b2", "b3"], commit, confirmed);
      setOpen(false);
      message.success(`run 已启动(任务 ${id})`);
      onStarted(id);
    } catch (e) {
      const s = String(e);
      if (s.includes("CONFIRM:")) {
        modal.confirm({
          title: "确认在 schtask 窗口附近 commit?",
          content: s.replace(/^.*CONFIRM:/, ""),
          okText: "确认执行",
          onOk: () => start(true),
        });
      } else {
        message.error(s);
      }
    }
  };

  return (
    <>
      <Button type="primary" onClick={() => void openDialog()}>手动触发 run</Button>
      <Modal title="手动触发当日 run" open={open} onCancel={() => setOpen(false)} onOk={() => void start(false)} okText="运行">
        <Space direction="vertical">
          <span>账本:b1 + b2 + b3(参数与 deploy/paper_run.cmd 一致)</span>
          {gateMsg && <span style={{ color: dryOnly ? "#cf1322" : "#d48806" }}>{gateMsg}</span>}
          <Checkbox checked={commit} disabled={dryOnly} onChange={(e) => setCommit(e.target.checked)}>
            commit(落盘 state;不勾 = DRY RUN)
          </Checkbox>
        </Space>
      </Modal>
    </>
  );
}
