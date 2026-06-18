import { create } from "zustand";
import type { ScreenConfigDto } from "@bindings/ScreenConfigDto";
import type { ScreenResultDto } from "@bindings/ScreenResultDto";
import type { ScreenRunMetaDto } from "@bindings/ScreenRunMetaDto";
import type { ScreenBacktestReportDto } from "@bindings/ScreenBacktestReportDto";
import type { IndexRelativeDto } from "@bindings/IndexRelativeDto";
import { api as realApi, type Api } from "../api/ipc";
import { friendlyError } from "../errors";

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
  loadConfigs: () => Promise<void>;
  loadRuns: () => Promise<void>;
  selectRun: (id: string) => Promise<void>;
  setBenchmark: (id: string, b: string) => Promise<void>;
}

export const useScreen = create<ScreenState>((set, get) => ({
  api: realApi, configs: [], indices: [], asof: null, runs: [], report: null, indexRel: null,
  benchmark: "csi300", error: null,
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
}));
