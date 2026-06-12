import { Button, Form, InputNumber, Select, Space, Switch, Typography } from "antd";
import { App as AntApp } from "antd";
import { useEffect, useState } from "react";
import type { TreeInfoDto } from "@bindings/TreeInfoDto";
import type { CsvInfoDto } from "@bindings/CsvInfoDto";
import type { BacktestConfigDto } from "@bindings/BacktestConfigDto";
import { api } from "../api/ipc";

export default function BacktestConfigForm({ onStarted }: { onStarted: (taskId: string) => void }) {
  const { message } = AntApp.useApp();
  const [trees, setTrees] = useState<TreeInfoDto[]>([]);
  const [csvs, setCsvs] = useState<CsvInfoDto[]>([]);
  const [useFetch, setUseFetch] = useState(false);
  const [starting, setStarting] = useState(false);
  const [form] = Form.useForm();

  useEffect(() => {
    api.treeList().then(setTrees).catch(() => {});
    api.dataCsvList().then(setCsvs).catch(() => {});
  }, []);

  const submit = async () => {
    const v = await form.validateFields();
    const config: BacktestConfigDto = {
      tree_path: v.tree_path,
      primary_path: useFetch ? "" : v.primary_path,
      mode: v.mode,
      cost_bps: v.cost_bps,
      warmup: v.warmup,
      window: v.window,
      initial_capital: v.initial_capital,
      fetch: useFetch
        ? { symbol: v.symbol, scale: v.scale, datalen: 1023, adjust: "qfq" }
        : null,
    };
    setStarting(true);
    try {
      const taskId = await api.backtestRun(config);
      message.success(`回测已启动(任务 ${taskId})`);
      onStarted(taskId);
    } catch (e) {
      message.error(String(e));
    } finally {
      setStarting(false);
    }
  };

  return (
    <Form
      form={form}
      layout="vertical"
      size="small"
      initialValues={{ mode: "sim_hard", cost_bps: 10, warmup: 80, window: 100, initial_capital: 100000, scale: 60 }}
    >
      <Form.Item name="tree_path" label="决策树" rules={[{ required: true }]}>
        <Select
          showSearch
          options={trees.map((t) => ({
            value: t.path,
            label: `${t.name ?? "(加载失败)"} · ${t.path}${t.frozen ? " 🔒" : ""}`,
            disabled: !t.name,
          }))}
        />
      </Form.Item>
      <Form.Item label="数据来源">
        <Space>
          <Switch checked={useFetch} onChange={setUseFetch} checkedChildren="拉取" unCheckedChildren="本地CSV" />
          <Typography.Text type="secondary">{useFetch ? "新浪 qfq" : "工作区内 CSV"}</Typography.Text>
        </Space>
      </Form.Item>
      {useFetch ? (
        <Space.Compact block>
          <Form.Item name="symbol" rules={[{ required: useFetch }]} style={{ flex: 1 }}>
            <Select
              showSearch
              placeholder="sh600030"
              options={["sh600030", "sh600036", "sh600519", "sz000858"].map((s) => ({ value: s }))}
              popupMatchSelectWidth={false}
              // 允许自由输入
              mode={undefined}
              optionFilterProp="value"
            />
          </Form.Item>
          <Form.Item name="scale">
            <Select options={[{ value: 15 }, { value: 60 }, { value: 240, label: "240(日线)" }]} />
          </Form.Item>
        </Space.Compact>
      ) : (
        <Form.Item name="primary_path" label="行情 CSV" rules={[{ required: !useFetch }]}>
          <Select
            showSearch
            options={csvs.map((c) => ({
              value: c.path,
              label: `${c.path}${c.rows != null ? ` (${c.rows}根,至${c.last_t})` : " (解析失败)"}`,
              disabled: c.rows == null,
            }))}
          />
        </Form.Item>
      )}
      <Space wrap>
        <Form.Item name="mode" label="模式">
          <Select
            style={{ width: 130 }}
            options={[
              { value: "sim_hard", label: "sim·硬" },
              { value: "sim_soft", label: "sim·软" },
              { value: "score_hard", label: "打分·硬" },
              { value: "score_soft", label: "打分·软" },
            ]}
          />
        </Form.Item>
        <Form.Item name="cost_bps" label="成本bps">
          <InputNumber min={0} />
        </Form.Item>
        <Form.Item name="warmup" label="warmup">
          <InputNumber min={0} />
        </Form.Item>
        <Form.Item name="window" label="window">
          <InputNumber min={10} />
        </Form.Item>
        <Form.Item name="initial_capital" label="初始资金(元)">
          <InputNumber min={1} step={10000} />
        </Form.Item>
      </Space>
      <Button type="primary" loading={starting} onClick={() => void submit()}>
        运行回测
      </Button>
    </Form>
  );
}
