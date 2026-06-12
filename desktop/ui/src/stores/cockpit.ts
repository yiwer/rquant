import { create } from "zustand";
import type { OverviewDto } from "@bindings/OverviewDto";
import { api as realApi, type Api } from "../api/ipc";

interface CockpitState {
  api: Api;
  overview: OverviewDto | null;
  loading: boolean;
  error: string | null;
  load: () => Promise<void>;
}

export const useCockpit = create<CockpitState>((set, get) => ({
  api: realApi,
  overview: null,
  loading: false,
  error: null,
  load: async () => {
    set({ loading: true, error: null });
    try {
      set({ overview: await get().api.cockpitOverview(), loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },
}));
