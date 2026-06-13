import { invoke } from "@tauri-apps/api/core";
import type { OverviewDto } from "@bindings/OverviewDto";
import type { BookDetailDto } from "@bindings/BookDetailDto";
import type { GateDto } from "@bindings/GateDto";
import type { TaskInfoDto } from "@bindings/TaskInfoDto";
import type { TreeInfoDto } from "@bindings/TreeInfoDto";
import type { BacktestConfigDto } from "@bindings/BacktestConfigDto";
import type { RunMetaDto } from "@bindings/RunMetaDto";
import type { RunSummaryDto } from "@bindings/RunSummaryDto";
import type { EquityPointDto } from "@bindings/EquityPointDto";
import type { TradeDto } from "@bindings/TradeDto";
import type { ReplayFrameDto } from "@bindings/ReplayFrameDto";
import type { FactorValueDto } from "@bindings/FactorValueDto";
import type { BarDto } from "@bindings/BarDto";
import type { FactorPointDto } from "@bindings/FactorPointDto";
import type { CsvInfoDto } from "@bindings/CsvInfoDto";
import type { UniverseInfoDto } from "@bindings/UniverseInfoDto";
import type { UniverseEntryDto } from "@bindings/UniverseEntryDto";

export const api = {
  cockpitOverview: () => invoke<OverviewDto>("cockpit_overview"),
  bookDetail: (book: string) => invoke<BookDetailDto>("book_detail", { book }),
  runlogTail: (lines: number) => invoke<string>("runlog_tail", { lines }),
  runGateNow: () => invoke<GateDto>("run_gate_now"),
  manualRun: (books: string[], commit: boolean, confirmed: boolean) =>
    invoke<string>("manual_run", { books, commit, confirmed }),
  taskList: () => invoke<TaskInfoDto[]>("task_list"),
  taskCancel: (id: string) => invoke<void>("task_cancel", { id }),
  treeList: () => invoke<TreeInfoDto[]>("tree_list"),
  backtestRun: (config: BacktestConfigDto) => invoke<string>("backtest_run", { config }),
  runsList: () => invoke<RunMetaDto[]>("runs_list"),
  runDelete: (id: string) => invoke<void>("run_delete", { id }),
  runSummary: (id: string) => invoke<RunSummaryDto>("run_summary", { id }),
  runEquity: (id: string) => invoke<EquityPointDto[]>("run_equity", { id }),
  runTrades: (id: string) => invoke<TradeDto[]>("run_trades", { id }),
  runReplayFrames: (id: string) => invoke<ReplayFrameDto[]>("run_replay_frames", { id }),
  runReplayFactors: (id: string, t: string) => invoke<FactorValueDto[]>("run_replay_factors", { id, t }),
  dataCsvList: () => invoke<CsvInfoDto[]>("data_csv_list"),
  dataReadBars: (path: string, tail: number) => invoke<BarDto[]>("data_read_bars", { path, tail }),
  dataEvalFactor: (path: string, expr: string, window: number, tail: number) =>
    invoke<FactorPointDto[]>("data_eval_factor", { path, expr, window, tail }),
  universeList: () => invoke<UniverseInfoDto[]>("universe_list"),
  universeWrite: (name: string, entries: UniverseEntryDto[]) => invoke<void>("universe_write", { name, entries }),
  fetchBatch: (symbols: string[], scale: number, datalen: number, adjust: string) =>
    invoke<string>("fetch_batch", { symbols, scale, datalen, adjust }),
};
export type Api = typeof api;
