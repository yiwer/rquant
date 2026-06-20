import { create } from "zustand";
import type { DeployBookDto } from "@bindings/DeployBookDto";
import type { DeployMonthDto } from "@bindings/DeployMonthDto";
import { api as realApi, type Api } from "../api/ipc";
import { friendlyError } from "../errors";
interface DeployState {
  api: Api; book: DeployBookDto | null; preview: DeployMonthDto | null; error: string | null;
  load: () => Promise<void>; setPreview: (p: DeployMonthDto | null) => void;
  commit: (asOf: string) => Promise<boolean>;
}
export const useDeploy = create<DeployState>((set, get) => ({
  api: realApi, book: null, preview: null, error: null,
  load: async () => { try { set({ book: await get().api.deployBookRead() }); } catch { /* 静默 */ } },
  setPreview: (preview) => set({ preview }),
  commit: async (asOf) => {
    set({ error: null });
    try { await get().api.deployCommitMonth(asOf); set({ preview: null }); await get().load(); return true; }
    catch (e) { set({ error: friendlyError(String(e)).title }); return false; }
  },
}));
