import { create } from "zustand";
import type { LedgerRoundDto } from "@bindings/LedgerRoundDto";
import type { IterQueueDto } from "@bindings/IterQueueDto";
import type { RoundCardDto } from "@bindings/RoundCardDto";
import { api as realApi, type Api } from "../api/ipc";
import { friendlyError } from "../errors";

interface ResearchState {
  api: Api;
  ledger: LedgerRoundDto[];
  queue: IterQueueDto | null;
  card: RoundCardDto | null;
  error: string | null;
  load: () => Promise<void>;
  selectRound: (round: number) => Promise<void>;
}

export const useResearch = create<ResearchState>((set, get) => ({
  api: realApi, ledger: [], queue: null, card: null, error: null,
  load: async () => {
    try { set({ ledger: await get().api.iterLedger(), queue: await get().api.iterQueue() }); } catch { /* 静默 */ }
  },
  selectRound: async (round) => {
    set({ card: null, error: null });
    try { set({ card: await get().api.iterRoundCard(round) }); }
    catch (e) { set({ error: friendlyError(String(e)).title }); }
  },
}));
