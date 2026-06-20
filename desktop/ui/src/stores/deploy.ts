import { create } from "zustand";
import type { DeployBookDto } from "@bindings/DeployBookDto";
import type { DeployMonthDto } from "@bindings/DeployMonthDto";
import { api as realApi, type Api } from "../api/ipc";
import { friendlyError } from "../errors";
import { trackTask } from "./tasks";
interface DeployState {
  api: Api; book: DeployBookDto | null; preview: DeployMonthDto | null; error: string | null;
  runTaskId: string | null; runError: string | null;
  load: () => Promise<void>; setPreview: (p: DeployMonthDto | null) => void;
  commit: (asOf: string) => Promise<boolean>;
  runMonth: (asOf: string) => Promise<void>;
}
export const useDeploy = create<DeployState>((set, get) => ({
  api: realApi, book: null, preview: null, error: null,
  runTaskId: null, runError: null,
  load: async () => { try { set({ book: await get().api.deployBookRead() }); } catch { /* 静默 */ } },
  setPreview: (preview) => set({ preview }),
  commit: async (asOf) => {
    set({ error: null });
    try { await get().api.deployCommitMonth(asOf); set({ preview: null }); await get().load(); return true; }
    catch (e) { set({ error: friendlyError(String(e)).title }); return false; }
  },
  runMonth: async (asOf) => {
    set({ runTaskId: null, runError: null });
    try {
      const id = await get().api.deployRunMonth(asOf);
      set({ runTaskId: id });
      trackTask(id, {
        done: (info) => {
          set({ preview: info.result as DeployMonthDto });
        },
        failed: (info) => {
          set({ runError: friendlyError(info.error ?? "跑本月失败").title });
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
