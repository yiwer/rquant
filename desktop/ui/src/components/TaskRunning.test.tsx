import { test, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import TaskRunning from "./TaskRunning";

const info = (pct: number, stage = "选股") => ({
  id: "t1", kind: "screen_asof", status: "running",
  progress: { pct, stage, detail: "" }, error: null, result: null,
});

test("shows stage in Chinese and elapsed seconds", () => {
  render(<TaskRunning info={info(0.4) as any} startedAt={Date.now() - 5000} />);
  expect(screen.getByText(/横截面选股/)).toBeTruthy();
  expect(screen.getByText(/已耗时/)).toBeTruthy();
});

test("determinate bar when pct in (0,1)", () => {
  const { container } = render(<TaskRunning info={info(0.4) as any} startedAt={Date.now()} />);
  expect(container.querySelector(".ant-progress")).toBeTruthy();
});

test("indeterminate spinner when pct is 0 or 1 (no fabricated bar)", () => {
  const { container } = render(<TaskRunning info={info(0) as any} startedAt={Date.now()} />);
  expect(container.querySelector(".ant-spin")).toBeTruthy();
  expect(container.querySelector(".ant-progress")).toBeFalsy();
});
