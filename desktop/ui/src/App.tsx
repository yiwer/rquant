import { HashRouter, Routes, Route, Navigate, useNavigate, useLocation } from "react-router-dom";
import { Layout, Menu, Typography } from "antd";
import Cockpit from "./pages/Cockpit";
import BookDetail from "./pages/BookDetail";
import Backtest from "./pages/Backtest";
import DataBench from "./pages/DataBench";
import Screen from "./pages/Screen";
import Research from "./pages/Research";
import TaskDrawer from "./components/TaskDrawer";
import Verdict from "./pages/Verdict";
import Factor from "./pages/Factor";

export const MODULES = [
  { key: "cockpit", label: "驾驶舱" },
  { key: "backtest", label: "回测中心" },
  { key: "data", label: "数据工作台" },
  { key: "screen", label: "选股" },
  { key: "research", label: "研究" },
  { key: "verdict", label: "认证" },
  { key: "tree", label: "策略树" },
  { key: "factor", label: "因子工作台" },
  { key: "wfo", label: "调参/WFO" },
  { key: "portfolio", label: "组合回测" },
  { key: "archive", label: "档案馆" },
];

function Placeholder({ name }: { name: string }) {
  return <Typography.Text type="secondary">{name} —— M2+ 交付</Typography.Text>;
}

function Shell() {
  const nav = useNavigate();
  const loc = useLocation();
  const selected = MODULES.find((m) => loc.pathname.startsWith(`/${m.key}`))?.key ?? "cockpit";
  return (
    <Layout style={{ minHeight: "100vh" }}>
      <Layout.Sider theme="light" width={140}>
        <Menu
          mode="inline"
          selectedKeys={[selected]}
          items={MODULES.map((m) => ({ key: m.key, label: m.label }))}
          onClick={(e) => nav(`/${e.key}`)}
        />
      </Layout.Sider>
      <Layout.Content style={{ padding: 16 }}>
        <div style={{ display: "flex", justifyContent: "flex-end", marginBottom: 8 }}>
          <TaskDrawer />
        </div>
        <Routes>
          <Route path="/cockpit" element={<Cockpit />} />
          <Route path="/cockpit/:book" element={<BookDetail />} />
          <Route path="/backtest" element={<Backtest />} />
          <Route path="/data" element={<DataBench />} />
          <Route path="/screen" element={<Screen />} />
          <Route path="/research" element={<Research />} />
          <Route path="/verdict" element={<Verdict />} />
          <Route path="/factor" element={<Factor />} />
          {MODULES.filter((m) => m.key !== "cockpit" && m.key !== "backtest" && m.key !== "data" && m.key !== "screen" && m.key !== "research" && m.key !== "verdict" && m.key !== "factor").map((m) => (
            <Route key={m.key} path={`/${m.key}`} element={<Placeholder name={m.label} />} />
          ))}
          <Route path="*" element={<Navigate to="/cockpit" replace />} />
        </Routes>
      </Layout.Content>
    </Layout>
  );
}

export default function App() {
  return (
    <HashRouter>
      <Shell />
    </HashRouter>
  );
}
