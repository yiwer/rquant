import { create } from "zustand";
import type { LedgerRoundDto } from "@bindings/LedgerRoundDto";
import type { IterQueueDto } from "@bindings/IterQueueDto";
import type { RoundCardDto } from "@bindings/RoundCardDto";
import { api as realApi, type Api } from "../api/ipc";
import { friendlyError } from "../errors";
import { trackTask } from "./tasks";

interface ResearchState {
  api: Api;
  ledger: LedgerRoundDto[];
  queue: IterQueueDto | null;
  card: RoundCardDto | null;
  error: string | null;
  runTaskId: string | null;
  runError: string | null;
  load: () => Promise<void>;
  selectRound: (round: number) => Promise<void>;
  runRound: (config: string, note: string, axis: string, top: number, benchmark: string, rebalance: number) => Promise<void>;
}

export const useResearch = create<ResearchState>((set, get) => ({
  api: realApi, ledger: [], queue: null, card: null, error: null,
  runTaskId: null, runError: null,
  load: async () => {
    try { set({ ledger: await get().api.iterLedger(), queue: await get().api.iterQueue() }); } catch { /* 静默 */ }
  },
  selectRound: async (round) => {
    set({ card: null, error: null });
    try { set({ card: await get().api.iterRoundCard(round) }); }
    catch (e) { set({ error: friendlyError(String(e)).title }); }
  },
  runRound: async (config, note, axis, top, benchmark, rebalance) => {
    set({ runTaskId: null, runError: null });
    try {
      const id = await get().api.iterRunRound(config, note, axis, top, benchmark, rebalance);
      set({ runTaskId: id });
      trackTask(id, {
        done: () => {
          void get().load();
        },
        failed: (info) => {
          set({ runError: friendlyError(info.error ?? "跑轮失败").title });
          void get().load();
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
