import { create } from "zustand";
import type { FactorReportDto } from "@bindings/FactorReportDto";
import { api as realApi, type Api } from "../api/ipc";
import { friendlyError } from "../errors";
import { trackTask } from "./tasks";

interface FactorState {
  api: Api;
  report: FactorReportDto | null;
  error: string | null;
  runTaskId: string | null;
  runError: string | null;
  setReport: (r: FactorReportDto | null) => void;
  setError: (e: string | null) => void;
  runFactor: (factors: [string, string][], horizon: number, layers: number, sample: number) => Promise<void>;
}

export const useFactor = create<FactorState>((set, get) => ({
  api: realApi,
  report: null,
  error: null,
  runTaskId: null,
  runError: null,
  setReport: (report) => set({ report }),
  setError: (error) => set({ error }),
  runFactor: async (factors, horizon, layers, sample) => {
    set({ runTaskId: null, runError: null });
    try {
      const id = await get().api.factorRun(factors, horizon, layers, sample);
      set({ runTaskId: id });
      trackTask(id, {
        done: (info) => {
          set({ report: info.result as FactorReportDto });
        },
        failed: (info) => {
          set({ runError: friendlyError(info.error ?? "因子分析失败").title });
        },
        cancelled: () => {
          set({ runError: "已取消" });
        },
      });
    } catch (e) {
      set({ runError: friendlyError(String(e)).title });
    }
  },
}));
