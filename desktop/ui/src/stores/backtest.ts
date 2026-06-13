import { create } from "zustand";
import type { RunMetaDto } from "@bindings/RunMetaDto";
import type { RunSummaryDto } from "@bindings/RunSummaryDto";
import { api as realApi, type Api } from "../api/ipc";

interface BacktestState {
  api: Api;
  runs: RunMetaDto[];
  selectedId: string | null;
  summary: RunSummaryDto | null;
  selectError: string | null;
  compareIds: string[];
  loadRuns: () => Promise<void>;
  select: (id: string) => Promise<void>;
  toggleCompare: (id: string) => void;
  remove: (id: string) => Promise<void>;
}

export const useBacktest = create<BacktestState>((set, get) => ({
  api: realApi,
  runs: [],
  selectedId: null,
  summary: null,
  selectError: null,
  compareIds: [],
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
      set({ summary: null, selectError: String(e) });
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
}));
