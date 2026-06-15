import { describe, it, expect } from "vitest";
import { friendlyError } from "./errors";

describe("friendlyError", () => {
  it("maps tree parse errors", () => {
    const r = friendlyError("backtest runner failed: tree parse error at line 42");
    expect(r.title).toBe("策略树解析失败");
    expect(r.detail).toContain("line 42"); // 原文保留于 detail
  });
  it("maps file-not-found", () => {
    expect(friendlyError("No such file or directory: foo.csv").title).toBe("文件未找到或无法读取");
  });
  it("maps fetch/network errors", () => {
    expect(friendlyError("tencent request error: timeout").title).toBe("数据拉取失败（网络或数据源）");
  });
  it("maps csv format errors", () => {
    expect(friendlyError("csv: row too short (3 fields)").title).toBe("数据文件格式错误");
  });
  it("falls back to generic title, keeps raw detail", () => {
    const r = friendlyError("something totally unexpected");
    expect(r.title).toBe("操作失败");
    expect(r.detail).toBe("something totally unexpected");
  });
});
