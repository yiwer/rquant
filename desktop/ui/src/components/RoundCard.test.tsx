import { render, screen } from "@testing-library/react";
import "@testing-library/jest-dom";
import { test, expect } from "vitest";
import RoundCard from "./RoundCard";
import type { RoundCardDto } from "@bindings/RoundCardDto";

const CARD: RoundCardDto = { round: 4, label: "value_pb", benchmark: "csi300", rebalance: 1, verdict: "PASS",
  gates: [{ name: "net-OOS 超额>0", pass: true, value: 0.64, threshold: 0, note: "金标准" }],
  tier2: [{ top: 50, rebalance: 1, net_excess: 0.64 }], flags: [], config_path: "examples/screen/iter/value_pb_base.yaml" };
test("round card shows verdict and gates", () => {
  render(<RoundCard card={CARD} />);
  expect(screen.getByText("PASS")).toBeInTheDocument();
  expect(screen.getByText("net-OOS 超额>0")).toBeInTheDocument();
});
