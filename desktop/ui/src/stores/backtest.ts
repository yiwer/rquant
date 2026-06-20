import { create } from "zustand";
import type { RunMetaDto } from "@bindings/RunMetaDto";
import type { RunSummaryDto } from "@bindings/RunSummaryDto";
import type { BacktestConfigDto } from "@bindings/BacktestConfigDto";
import { api as realApi, type Api } from "../api/ipc";
import { friendlyError } from "../errors";
import { trackTask } from "./tasks";

interface BacktestState {
  api: Api;
  runs: RunMetaDto[];
  selectedId: string | null;
  summary: RunSummaryDto | null;
  selectError: string | null;
  compareIds: string[];
  runTaskId: string | null;
  runError: string | null;
  loadRuns: () => Promise<void>;
  select: (id: string) => Promise<void>;
  toggleCompare: (id: string) => void;
  remove: (id: string) => Promise<void>;
  backtestRun: (config: BacktestConfigDto) => Promise<void>;
}

export const useBacktest = create<BacktestState>((set, get) => ({
  api: realApi,
  runs: [],
  selectedId: null,
  summary: null,
  selectError: null,
  compareIds: [],
  runTaskId: null,
  runError: null,
  loadRuns: async () => {
    try {
      set({ runs: await get().api.runsList() });
    } catch {
      /* 启动早期 invoke 不可用时静默 */
    }
  },
  select: async (id) => {
    set({ selectedId: id, summary: null, selectError: null });
    try {
      set({ summary: await get().api.runSummary(id) });
    } catch (e) {
      const fe = friendlyError(String(e));
      console.error("[run select]", fe.detail);
      set({ summary: null, selectError: fe.title });
    }
  },
  toggleCompare: (id) =>
    set((s) => ({
      compareIds: s.compareIds.includes(id)
        ? s.compareIds.filter((x) => x !== id)
        : [...s.compareIds, id].slice(-2), // 至多两个,后选顶替
    })),
  remove: async (id) => {
    await get().api.runDelete(id);
    const s = get();
    set({
      runs: s.runs.filter((r) => r.id !== id),
      selectedId: s.selectedId === id ? null : s.selectedId,
      summary: s.selectedId === id ? null : s.summary,
      compareIds: s.compareIds.filter((x) => x !== id),
    });
  },
  backtestRun: async (config) => {
    set({ runTaskId: null, runError: null });
    try {
      const id = await get().api.backtestRun(config);
      set({ runTaskId: id });
      trackTask(id, {
        done: (info) => {
          const runId = info.result as string;
          void get().loadRuns().then(() => {
            if (runId) void get().select(runId);
          });
        },
        failed: (info) => {
          set({ runError: friendlyError(info.error ?? "回测失败").title });
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
