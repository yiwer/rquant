import { render, screen, waitFor } from "@testing-library/react";
import "@testing-library/jest-dom";
import { vi } from "vitest";
import type { ReplayFrameDto } from "@bindings/ReplayFrameDto";

const FRAMES: ReplayFrameDto[] = [
  { t: "2026-01-05T11:00:00", leaf: "l", stance: "Long",
    path: [{ node_id: "r", label: "up", confidence: 1, rationale: "close>sma" }],
    target: 1, pos: 0, nav: 1.0 },
  { t: "2026-01-05T12:00:00", leaf: "f", stance: "Flat",
    path: [{ node_id: "r", label: "default", confidence: 1, rationale: "" }],
    target: 0, pos: 1, nav: 1.01 },
];

vi.mock("../api/ipc", () => ({
  api: {
    runReplayFrames: async () => FRAMES,
    runReplayFactors: async () => [{ name: "ma", value: 10.2 }],
  },
}));

import ReplayView from "./ReplayView";

test("replay shows latest frame path and factors", async () => {
  render(<ReplayView runId="20260612-210000-0a1b-01" />);
  await waitFor(() => expect(screen.getByText(/决策路径 @ 2026-01-05T12:00:00/)).toBeInTheDocument());
  expect(screen.getByText("default")).toBeInTheDocument();
  await waitFor(() => expect(screen.getByText("ma")).toBeInTheDocument());
  expect(screen.getByText("10.200000")).toBeInTheDocument();
});
