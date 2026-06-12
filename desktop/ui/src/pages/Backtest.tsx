import { useEffect } from "react";
import { Card, Col, Row, Tabs, Typography } from "antd";
import { useBacktest } from "../stores/backtest";
import BacktestConfigForm from "../components/BacktestConfigForm";
import RunHistoryList from "../components/RunHistoryList";

export default function Backtest() {
  const st = useBacktest();

  useEffect(() => {
    void st.loadRuns();
    // 任务完成后列表会过时——简单轮询(驾驶舱模式一致,8s 足够)
    const timer = setInterval(() => void st.loadRuns(), 8000);
    return () => clearInterval(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <Row gutter={12}>
      <Col span={7}>
        <Card size="small" title="回测配置" style={{ marginBottom: 12 }}>
          <BacktestConfigForm onStarted={() => void st.loadRuns()} />
        </Card>
        <Card size="small" title={`历史留档(${st.runs.length})`}>
          <RunHistoryList
            runs={st.runs}
            selectedId={st.selectedId}
            compareIds={st.compareIds}
            onSelect={(id) => void st.select(id)}
            onToggleCompare={st.toggleCompare}
            onDelete={(id) => void st.remove(id)}
          />
        </Card>
      </Col>
      <Col span={17}>
        {st.selectedId == null ? (
          <Typography.Text type="secondary">从左侧选择一次留档查看结果</Typography.Text>
        ) : (
          <Tabs
            items={[
              { key: "overview", label: "概览", children: <Typography.Text type="secondary">U2 交付</Typography.Text> },
              { key: "kline", label: "K线信号", children: <Typography.Text type="secondary">U3 交付</Typography.Text> },
              { key: "trades", label: "交易明细", children: <Typography.Text type="secondary">U2 交付</Typography.Text> },
              { key: "replay", label: "决策回放", children: <Typography.Text type="secondary">U4 交付</Typography.Text> },
              { key: "raw", label: "原始", children: <Typography.Text type="secondary">U2 交付</Typography.Text> },
            ]}
          />
        )}
      </Col>
    </Row>
  );
}
