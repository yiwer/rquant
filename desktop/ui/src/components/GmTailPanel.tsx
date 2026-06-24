import { App as AntApp, Badge, Button, Card, Col, Input, InputNumber, Popconfirm, Row, Select, Space, Switch, Typography } from "antd";
import { useCallback, useEffect, useState } from "react";
import { api } from "../api/ipc";
import type { GmTailConfig } from "@bindings/GmTailConfig";
import type { GmTailStatusDto } from "@bindings/GmTailStatusDto";

const RANKS = ["liquidity", "intraday", "range_pos", "vwap_gap"];

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <Typography.Text type="secondary" style={{ fontSize: 12 }}>{label}</Typography.Text>
      <div>{children}</div>
    </div>
  );
}

/** 尾盘取数(掘金 gm)驾驶舱面板:装/卸/手动跑计划任务 + 编辑漏斗配置。调 6 个 gm_tail_* 命令。 */
export default function GmTailPanel() {
  const { message } = AntApp.useApp();
  const [status, setStatus] = useState<GmTailStatusDto | null>(null);
  const [cfg, setCfg] = useState<GmTailConfig | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const s = await api.gmTailStatus();
      setStatus(s);
      setCfg(s.config);
      setErr(null);
    } catch (e) {
      setErr(String(e)); // 命令不可用(无后端/未编译)→ 降级显示,不崩驾驶舱
    }
  }, []);

  // 首屏取数:内联 async IIFE(await 在 setState 之前)→ 满足 react-hooks/set-state-in-effect
  useEffect(() => {
    void (async () => { await load(); })();
  }, [load]);

  const set = <K extends keyof GmTailConfig>(k: K, v: GmTailConfig[K]) =>
    setCfg((c) => (c ? { ...c, [k]: v } : c));

  const run = async (fn: () => Promise<unknown>, ok: string) => {
    setBusy(true);
    try {
      await fn();
      message.success(ok);
      await load();
    } catch (e) {
      message.error(String(e));
    } finally {
      setBusy(false);
    }
  };

  if (err && !status)
    return (
      <Card size="small" title="尾盘取数（掘金 gm）">
        <Typography.Text type="secondary" style={{ fontSize: 12 }}>状态不可用: {err}</Typography.Text>
      </Card>
    );
  if (!status || !cfg) return <Card size="small" title="尾盘取数（掘金 gm）" loading />;
  const st = status.schtask;

  return (
    <Card size="small" title="尾盘取数（掘金 gm）">
      <Space direction="vertical" size={8} style={{ width: "100%" }}>
        <div>
          <Badge
            status={status.installed ? "success" : "default"}
            text={status.installed ? "计划任务已安装" : "未安装"}
          />
          {st && (
            <Typography.Text type="secondary" style={{ fontSize: 12, marginLeft: 8 }}>
              {st.status ?? "?"} · 下次 {st.next_run ?? "?"} · 上次 {st.last_run ?? "?"}（{st.last_result ?? "?"}）
            </Typography.Text>
          )}
        </div>
        <Typography.Text type={status.token_present ? "secondary" : "danger"} style={{ fontSize: 12 }}>
          token {status.token_present ? "✓" : "✗ 未配置(data/gm/.token)"} · 15m {status.k15m_count} 只 · 最新快照 {status.last_snapshot ?? "—"}
        </Typography.Text>

        <Row gutter={8}>
          <Col span={8}>
            <Field label="触发时刻 HH:MM">
              <Input value={cfg.schedule_time} onChange={(e) => set("schedule_time", e.target.value)} placeholder="14:46" />
            </Field>
          </Col>
          <Col span={8}>
            <Field label="粗排键">
              <Select style={{ width: "100%" }} value={cfg.rank} options={RANKS.map((r) => ({ value: r, label: r }))} onChange={(v) => set("rank", v)} />
            </Field>
          </Col>
          <Col span={8}>
            <Field label="取前 N">
              <InputNumber style={{ width: "100%" }} min={1} max={5115} value={cfg.top} onChange={(v) => set("top", v ?? 300)} />
            </Field>
          </Col>
        </Row>
        <Row gutter={8} align="bottom">
          <Col span={8}>
            <Field label="成交额下限(元)">
              <InputNumber style={{ width: "100%" }} min={0} step={1e7} value={cfg.min_amount} onChange={(v) => set("min_amount", v ?? 0)} />
            </Field>
          </Col>
          <Col span={8}>
            <Field label="最低价">
              <InputNumber style={{ width: "100%" }} min={0} value={cfg.min_price} onChange={(v) => set("min_price", v ?? 0)} />
            </Field>
          </Col>
          <Col span={8}>
            <Field label="去涨停封板">
              <Switch checked={cfg.drop_limit_up} onChange={(v) => set("drop_limit_up", v)} />
            </Field>
          </Col>
        </Row>
        <Field label="日线候选集 pool（空=不用；相对仓库根或绝对路径）">
          <Input value={cfg.pool} onChange={(e) => set("pool", e.target.value)} placeholder="data/gm/daily_pool.txt" />
        </Field>

        <Space wrap>
          <Button type="primary" loading={busy} onClick={() => void run(() => api.gmTailInstall(cfg), "已安装/更新计划任务")}>
            安装/更新
          </Button>
          <Button loading={busy} onClick={() => void run(() => api.gmTailSetConfig(cfg), "配置已保存（改时刻需重新安装）")}>
            仅存配置
          </Button>
          <Button loading={busy} disabled={!status.installed} onClick={() => void run(() => api.gmTailRunNow(), "已触发一次运行")}>
            立即运行
          </Button>
          <Popconfirm title="卸载 rquant-gm-tail 计划任务?" okText="卸载" cancelText="取消" onConfirm={() => void run(() => api.gmTailRemove(), "已卸载")}>
            <Button danger loading={busy} disabled={!status.installed}>卸载</Button>
          </Popconfirm>
        </Space>

        {status.log_tail.length > 0 && (
          <pre style={{ fontSize: 11, whiteSpace: "pre-wrap", maxHeight: 140, overflow: "auto", background: "#fafafa", padding: 8, margin: 0 }}>
            {status.log_tail.join("\n")}
          </pre>
        )}
      </Space>
    </Card>
  );
}
