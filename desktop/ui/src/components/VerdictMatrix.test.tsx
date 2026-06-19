import { render, screen } from "@testing-library/react";
import "@testing-library/jest-dom";
import { test, expect } from "vitest";
import VerdictMatrix from "./VerdictMatrix";
import type { VerdictDto } from "@bindings/VerdictDto";
const V: VerdictDto = { strategy: "树4", n_symbols: 10, certified: false,
  gates: [{ gate: "T1_os_breadth", status: "fail", value: 0.4, threshold: 0.6, note: "样本外正占比不足" }], failed_gates: ["T1_os_breadth"] };
test("verdict matrix shows gate + status zh", () => {
  render(<VerdictMatrix v={V} />);
  expect(screen.getByText("未通过")).toBeInTheDocument();
  expect(screen.getByText("T1_os_breadth")).toBeInTheDocument();
  expect(screen.getByText("未过")).toBeInTheDocument();
});
