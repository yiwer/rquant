import { create } from "zustand";
import type { OptimizeReportInfoDto } from "@bindings/OptimizeReportInfoDto";
import type { VerdictDto } from "@bindings/VerdictDto";
import { api as realApi, type Api } from "../api/ipc";
import { friendlyError } from "../errors";

interface VerdictState {
  api: Api;
  reports: OptimizeReportInfoDto[];
  verdict: VerdictDto | null;
  error: string | null;
  loadReports: () => Promise<void>;
  certify: (paths: string[], name: string) => Promise<void>;
}

export const useVerdict = create<VerdictState>((set, get) => ({
  api: realApi,
  reports: [],
  verdict: null,
  error: null,
  loadReports: async () => {
    try { set({ reports: await get().api.evalListReports() }); } catch { /* 静默 */ }
  },
  certify: async (paths, name) => {
    set({ verdict: null, error: null });
    try { set({ verdict: await get().api.evalCertify(paths, name) }); }
    catch (e) { set({ error: friendlyError(String(e)).title }); }
  },
}));
