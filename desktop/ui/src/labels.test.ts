import { describe, it, expect } from "vitest";
import { actionZh, modeZh, snapshotFieldZh, TERM, MODE_GLOSS } from "./labels";

describe("labels", () => {
  it("maps trade actions to Chinese", () => {
    expect(actionZh("Buy")).toBe("买入");
    expect(actionZh("Sell")).toBe("卖出");
    expect(actionZh("Adjust")).toBe("调整");
    expect(actionZh("Hold")).toBe("持有");
  });
  it("falls back to raw key for unknown action", () => {
    expect(actionZh("Weird")).toBe("Weird");
  });
  it("maps run modes to Chinese", () => {
    expect(modeZh("sim_hard")).toBe("模拟·硬");
    expect(modeZh("score_soft")).toBe("打分·软");
  });
  it("maps all 13 AccountSnapshot fields", () => {
    for (const k of ["pos","entry_price","bars_held","nav","peak_nav","max_drawdown","turnover","last_increase_date","max_price_since_entry","min_price_since_entry","bars_since_exit","last_trip_return","trip"]) {
      expect(snapshotFieldZh(k)).not.toBe(k); // every field has a zh label
    }
    expect(snapshotFieldZh("entry_price")).toBe("建仓价");
  });
  it("exposes glossary terms + mode gloss", () => {
    expect(TERM.bps).toBe("基点");
    expect(TERM.warmup).toBe("热身期");
    expect(MODE_GLOSS).toContain("模拟");
  });
});
