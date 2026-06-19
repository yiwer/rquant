import { render, screen } from "@testing-library/react";
import "@testing-library/jest-dom";
import { test, expect, afterEach } from "vitest";
import { App as AntApp } from "antd";
import DeployHardening from "./DeployHardening";
import { useScreen } from "../stores/screen";
const real = useScreen.getState().api;
afterEach(() => useScreen.setState({ api: real }));
test("deploy hardening shows drag + capacity", async () => {
  useScreen.setState({ api: { ...real, analyzeDeploy: async () => ({ lag0_excess: 3.0, lag1_excess: 3.05, drag: 0.05, adv_median: 3.18e8, capacity: [{ adv_pct: 0.1, max_aum: 2.5e8 }] }) } });
  render(<AntApp><DeployHardening runId="scr-1" /></AntApp>);
  expect(await screen.findByText("执行拖累")).toBeInTheDocument();
});
