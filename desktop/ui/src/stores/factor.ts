import { create } from "zustand";
import type { FactorReportDto } from "@bindings/FactorReportDto";
import { api as realApi, type Api } from "../api/ipc";

interface FactorState {
  api: Api;
  report: FactorReportDto | null;
  error: string | null;
  setReport: (r: FactorReportDto | null) => void;
  setError: (e: string | null) => void;
}

export const useFactor = create<FactorState>((set) => ({
  api: realApi,
  report: null,
  error: null,
  setReport: (report) => set({ report }),
  setError: (error) => set({ error }),
}));
