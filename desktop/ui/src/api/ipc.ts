import { invoke } from "@tauri-apps/api/core";
import type { OverviewDto } from "@bindings/OverviewDto";
import type { BookDetailDto } from "@bindings/BookDetailDto";
import type { GateDto } from "@bindings/GateDto";
import type { TaskInfoDto } from "@bindings/TaskInfoDto";

export const api = {
  cockpitOverview: () => invoke<OverviewDto>("cockpit_overview"),
  bookDetail: (book: string) => invoke<BookDetailDto>("book_detail", { book }),
  runlogTail: (lines: number) => invoke<string>("runlog_tail", { lines }),
  runGateNow: () => invoke<GateDto>("run_gate_now"),
  manualRun: (books: string[], commit: boolean, confirmed: boolean) =>
    invoke<string>("manual_run", { books, commit, confirmed }),
  taskList: () => invoke<TaskInfoDto[]>("task_list"),
  taskCancel: (id: string) => invoke<void>("task_cancel", { id }),
};
export type Api = typeof api;
