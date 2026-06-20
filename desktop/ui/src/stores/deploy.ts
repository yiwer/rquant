import { create } from "zustand";
import type { DeployBookDto } from "@bindings/DeployBookDto";
import type { DeployMonthDto } from "@bindings/DeployMonthDto";
import { api as realApi, type Api } from "../api/ipc";
import { friendlyError } from "../errors";
import { trackTask } from "./tasks";
interface DeployState {
  api: Api; book: DeployBookDto | null; preview: DeployMonthDto | null; error: string | null;
  runTaskId: string | null; runError: string | null;
  commitTaskId: string | null; commitError: string | null;
  load: () => Promise<void>; setPreview: (p: DeployMonthDto | null) => void;
  commit: (asOf: string) => Promise<void>;
  runMonth: (asOf: string) => Promise<void>;
}
export const useDeploy = create<DeployState>((set, get) => ({
  api: realApi, book: null, preview: null, error: null,
  runTaskId: null, runError: null,
  commitTaskId: null, commitError: null,
  load: async () => { try { set({ book: await get().api.deployBookRead() }); } catch { /* 静默 */ } },
  setPreview: (preview) => set({ preview }),
  commit: async (asOf) => {
    set({ commitError: null });
    try {
      const id = await get().api.deployCommitMonth(asOf);
      set({ commitTaskId: id });
      trackTask(id, {
        done: () => { set({ preview: null, commitError: null }); void get().load(); },
        failed: (info) => set({ commitError: friendlyError(info.error ?? "落账失败").title }),
      });
    } catch (e) {
      set({ commitError: friendlyError(String(e)).title });
    }
  },
  runMonth: async (asOf) => {
    set({ runTaskId: null, runError: null });
    try {
      const id = await get().api.deployRunMonth(asOf);
      set({ runTaskId: id });
      trackTask(id, {
        done: (info) => {
          if (get().runTaskId === info.id) set({ preview: info.result as DeployMonthDto });
        },
        failed: (info) => {
          if (get().runTaskId === info.id) set({ runError: friendlyError(info.error ?? "跑本月失败").title });
        },
        cancelled: (info) => {
          if (get().runTaskId === info.id) set({ runError: "已取消" });
        },
      });
    } catch (e) {
      set({ runError: friendlyError(String(e)).title });
    }
  },
}));
