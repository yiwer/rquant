import { Alert, Button, Form, InputNumber, Popover, Select, Space, Switch, Tooltip, Typography } from "antd";
import { App as AntApp } from "antd";
import { QuestionCircleOutlined } from "@ant-design/icons";
import { useEffect, useState } from "react";
import type { TreeInfoDto } from "@bindings/TreeInfoDto";
import type { CsvInfoDto } from "@bindings/CsvInfoDto";
import type { BacktestConfigDto } from "@bindings/BacktestConfigDto";
import { api } from "../api/ipc";
import { modeZh, MODE_GLOSS, TERM } from "../labels";
import { friendlyError } from "../errors";

export default function BacktestConfigForm({ onStarted }: { onStarted: (taskId: string) => void }) {
  const { message } = AntApp.useApp();
  const [trees, setTrees] = useState<TreeInfoDto[]>([]);
  const [csvs, setCsvs] = useState<CsvInfoDto[]>([]);
  const [useFetch, setUseFetch] = useState(false);
  const [starting, setStarting] = useState(false);
  const [form] = Form.useForm();

  // 加载失败的树列表，用于在表单下方展示详情
  const failedTrees = trees.filter((t) => !t.name);

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
      const fe = friendlyError(String(e));
      message.error(fe.title);
      console.error("[backtest start]", fe.detail);
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
      <Form.Item name="tree_path" label="决策树" rules={[{ required: true }]} preserve={false}>
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
          <Form.Item name="symbol" rules={[{ required: useFetch }]} style={{ flex: 1 }} preserve={false}>
            <Select
              showSearch
              placeholder="sh600030"
              options={["sh600030", "sh600036", "sh600519", "sz000858"].map((s) => ({ value: s }))}
              popupMatchSelectWidth={false}
              optionFilterProp="value"
            />
          </Form.Item>
          <Form.Item name="scale" preserve={false} label="周期(分)">
            <Select options={[{ value: 15 }, { value: 60 }, { value: 240, label: "240(日线)" }]} />
          </Form.Item>
        </Space.Compact>
      ) : (
        <Form.Item name="primary_path" label="行情 CSV" rules={[{ required: !useFetch }]} preserve={false}>
          <Select
            showSearch
            options={csvs.map((c) => ({
              value: c.path,
              label: `${c.path}${c.rows != null ? ` (共${c.rows}根K线,至${c.last_t})` : " (解析失败)"}`,
              disabled: c.rows == null,
            }))}
          />
        </Form.Item>
      )}
      <Space wrap>
        <Form.Item
          name="mode"
          label={
            <Space size={4}>
              模式
              <Popover content={MODE_GLOSS} title="模式说明">
                <QuestionCircleOutlined style={{ color: "#8c8c8c", cursor: "pointer" }} />
              </Popover>
            </Space>
          }
        >
          <Select
            style={{ width: 130 }}
            options={[
              { value: "sim_hard", label: modeZh("sim_hard") },
              { value: "sim_soft", label: modeZh("sim_soft") },
              { value: "score_hard", label: modeZh("score_hard") },
              { value: "score_soft", label: modeZh("score_soft") },
            ]}
          />
        </Form.Item>
        <Form.Item name="cost_bps" label={`成本(${TERM.bps})`}>
          <InputNumber min={0} />
        </Form.Item>
        <Form.Item
          name="warmup"
          label={
            <Tooltip title="回测预热根数，非数据拉取参数">
              {TERM.warmup}
            </Tooltip>
          }
        >
          <InputNumber min={0} />
        </Form.Item>
        <Form.Item
          name="window"
          label={
            <Tooltip title="滚动回溯窗口根数，非数据拉取参数">
              {TERM.window}
            </Tooltip>
          }
        >
          <InputNumber min={10} />
        </Form.Item>
        <Form.Item name="initial_capital" label="初始资金(元)">
          <InputNumber min={1} step={10000} />
        </Form.Item>
      </Space>
      <Button type="primary" loading={starting} onClick={() => void submit()}>
        运行回测
      </Button>
      {failedTrees.length > 0 && (
        <Alert
          type="warning"
          style={{ marginTop: 8 }}
          message="部分决策树加载失败"
          description={
            <ul style={{ margin: 0, paddingLeft: 16 }}>
              {failedTrees.map((t) => (
                <li key={t.path}>
                  <Typography.Text code>{t.path}</Typography.Text>
                  {" — "}
                  {t.error ?? "解析失败，请检查 YAML"}
                </li>
              ))}
            </ul>
          }
        />
      )}
    </Form>
  );
}
