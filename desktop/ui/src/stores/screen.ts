import { create } from "zustand";
import type { ScreenConfigDto } from "@bindings/ScreenConfigDto";
import type { ScreenResultDto } from "@bindings/ScreenResultDto";
import type { ScreenRunMetaDto } from "@bindings/ScreenRunMetaDto";
import type { ScreenBacktestReportDto } from "@bindings/ScreenBacktestReportDto";
import type { IndexRelativeDto } from "@bindings/IndexRelativeDto";
import { api as realApi, type Api } from "../api/ipc";
import { friendlyError } from "../errors";
import { trackTask } from "./tasks";

interface ScreenState {
  api: Api;
  configs: ScreenConfigDto[];
  indices: string[];
  asof: ScreenResultDto | null;
  runs: ScreenRunMetaDto[];
  report: ScreenBacktestReportDto | null;
  indexRel: IndexRelativeDto | null;
  benchmark: string;
  error: string | null;
  // asof task state
  asofTaskId: string | null;
  asofResult: ScreenResultDto | null;
  asofError: string | null;
  // backtest task state
  btTaskId: string | null;
  btRunId: string | null;
  btError: string | null;
  loadConfigs: () => Promise<void>;
  loadRuns: () => Promise<void>;
  selectRun: (id: string) => Promise<void>;
  setBenchmark: (id: string, b: string) => Promise<void>;
  runAsof: (config: string, asOf: string, top: number) => Promise<void>;
  runBacktest: (config: string, from: string, to: string, top: number, rebalance: number, costBps: number) => Promise<void>;
}

export const useScreen = create<ScreenState>((set, get) => ({
  api: realApi, configs: [], indices: [], asof: null, runs: [], report: null, indexRel: null,
  benchmark: "csi300", error: null,
  asofTaskId: null, asofResult: null, asofError: null,
  btTaskId: null, btRunId: null, btError: null,
  loadConfigs: async () => {
    try { set({ configs: await get().api.screenConfigsList(), indices: await get().api.indexList() }); }
    catch { /* 启动早期静默 */ }
  },
  loadRuns: async () => { try { set({ runs: await get().api.screenRunsList() }); } catch { /* 静默 */ } },
  selectRun: async (id) => {
    set({ report: null, indexRel: null, error: null });
    try {
      const report = await get().api.screenRunReport(id);
      const indexRel = await get().api.screenIndexRelative(id, get().benchmark);
      set({ report, indexRel });
    } catch (e) { set({ error: friendlyError(String(e)).title }); }
  },
  setBenchmark: async (id, b) => {
    set({ benchmark: b });
    try { set({ indexRel: await get().api.screenIndexRelative(id, b) }); }
    catch (e) { set({ error: friendlyError(String(e)).title }); }
  },
  runAsof: async (config, asOf, top) => {
    set({ asofTaskId: null, asofResult: null, asofError: null });
    try {
      const id = await get().api.screenAsof(config, asOf, top);
      set({ asofTaskId: id });
      trackTask(id, {
        done: (info) => {
          if (get().asofTaskId === info.id) {
            const result = (info.result as ScreenResultDto | null) ?? null;
            set({ asofResult: result });
          }
        },
        failed: (info) => {
          if (get().asofTaskId === info.id) set({ asofError: friendlyError(info.error ?? "选股失败").title });
        },
        cancelled: (info) => {
          if (get().asofTaskId === info.id) set({ asofError: "已取消" });
        },
      });
    } catch (e) {
      set({ asofError: friendlyError(String(e)).title });
    }
  },
  runBacktest: async (config, from, to, top, rebalance, costBps) => {
    set({ btTaskId: null, btRunId: null, btError: null });
    try {
      const id = await get().api.screenBacktestRun(config, from, to, top, rebalance, costBps);
      set({ btTaskId: id });
      trackTask(id, {
        done: (info) => {
          if (get().btTaskId === info.id) set({ btRunId: info.result as string });
        },
        failed: (info) => {
          if (get().btTaskId === info.id) set({ btError: friendlyError(info.error ?? "回测失败").title });
        },
        cancelled: (info) => {
          if (get().btTaskId === info.id) set({ btError: "已取消" });
        },
      });
    } catch (e) {
      set({ btError: friendlyError(String(e)).title });
    }
  },
}));
