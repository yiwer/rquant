import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import "@testing-library/jest-dom";
import { test, expect, vi } from "vitest";
import { App as AntApp } from "antd";

vi.mock("../api/ipc", () => {
  const status = {
    installed: true,
    schtask: { next_run: "6/22/2026 2:46:00 PM", last_run: null, last_result: null, status: "Ready" },
    config: { schedule_time: "14:46", rank: "liquidity", top: 300, pool: "", min_amount: 30000000, min_price: 2.0, drop_limit_up: false },
    token_present: true,
    k15m_count: 300,
    last_snapshot: "snapshot_20260621_1446.csv",
    log_tail: ["[2026-06-21 14:47] === done exit=0 ==="],
  };
  return {
    api: {
      gmTailStatus: vi.fn(async () => status),
      gmTailInstall: vi.fn(async () => status),
      gmTailSetConfig: vi.fn(async () => status.config),
      gmTailRemove: vi.fn(async () => status),
      gmTailRunNow: vi.fn(async () => undefined),
    },
  };
});

import GmTailPanel from "./GmTailPanel";
import { api } from "../api/ipc";

test("shows install status + counts and triggers install", async () => {
  render(
    <AntApp>
      <GmTailPanel />
    </AntApp>,
  );
  expect(await screen.findByText("计划任务已安装")).toBeInTheDocument();
  expect(screen.getByText(/15m 300 只/)).toBeInTheDocument();
  expect(screen.getByText(/snapshot_20260621_1446\.csv/)).toBeInTheDocument();

  fireEvent.click(screen.getByText("安装/更新"));
  await waitFor(() => expect(api.gmTailInstall).toHaveBeenCalled());
});

test("run-now button enabled when installed", async () => {
  render(
    <AntApp>
      <GmTailPanel />
    </AntApp>,
  );
  const btn = await screen.findByText("立即运行");
  fireEvent.click(btn);
  await waitFor(() => expect(api.gmTailRunNow).toHaveBeenCalled());
});
