import { create } from "zustand";
import type { AuditRecordDto } from "@bindings/AuditRecordDto";
import { api as realApi, type Api } from "../api/ipc";

interface AuditState {
  api: Api;
  records: AuditRecordDto[];
  error: string | null;
  load: (kind?: string, status?: string) => Promise<void>;
}

export const useAudit = create<AuditState>((set, get) => ({
  api: realApi,
  records: [],
  error: null,
  load: async (kind, status) => {
    try {
      set({ records: await get().api.auditList(200, kind, status), error: null });
    } catch (e) {
      set({ error: String(e) });
    }
  },
}));
