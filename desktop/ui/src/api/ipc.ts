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
  // 选股
  screenConfigsList: () => invoke<import("@bindings/ScreenConfigDto").ScreenConfigDto[]>("screen_configs_list"),
  indexList: () => invoke<string[]>("index_list"),
  screenAsof: (config: string, asOf: string, top: number) => invoke<string>("screen_asof", { config, asOf, top }),
  screenBacktestRun: (config: string, from: string, to: string, top: number, rebalance: number, costBps: number) =>
    invoke<string>("screen_backtest_run", { config, from, to, top, rebalance, costBps }),
  screenRunsList: () => invoke<import("@bindings/ScreenRunMetaDto").ScreenRunMetaDto[]>("screen_runs_list"),
  screenRunReport: (id: string) => invoke<import("@bindings/ScreenBacktestReportDto").ScreenBacktestReportDto>("screen_run_report", { id }),
  screenIndexRelative: (id: string, benchmark: string) => invoke<import("@bindings/IndexRelativeDto").IndexRelativeDto>("screen_index_relative", { id, benchmark }),
  // 迭代
  iterLedger: () => invoke<import("@bindings/LedgerRoundDto").LedgerRoundDto[]>("iter_ledger"),
  iterQueue: () => invoke<import("@bindings/IterQueueDto").IterQueueDto>("iter_queue"),
  iterRoundCard: (round: number) => invoke<import("@bindings/RoundCardDto").RoundCardDto>("iter_round_card", { round }),
  iterRunRound: (config: string, note: string, axis: string, top: number, benchmark: string, rebalance: number) =>
    invoke<string>("iter_run_round", { config, note, axis, top, benchmark, rebalance }),
  // 因子
  factorRun: (factors: [string, string][], horizon: number, layers: number, sample: number) =>
    invoke<string>("factor_run", { factors, horizon, layers, sample }),
  // 认证
  evalListReports: () => invoke<import("@bindings/OptimizeReportInfoDto").OptimizeReportInfoDto[]>("eval_list_reports"),
  evalCertify: (paths: string[], name: string) => invoke<import("@bindings/VerdictDto").VerdictDto>("eval_certify", { paths, name }),
  // 分析器
  analyzeSector: (runId: string) => invoke<import("@bindings/SectorAttribDto").SectorAttribDto>("analyze_sector", { runId }),
  analyzeTwoleg: (valueRunId: string, growthRunId: string, w: number) => invoke<import("@bindings/TwoLegDto").TwoLegDto>("analyze_twoleg", { valueRunId, growthRunId, w }),
  analyzeDeploy: (runId: string) => invoke<import("@bindings/DeployDto").DeployDto>("analyze_deploy", { runId }),
  // 部署
  deployBookRead: () => invoke<import("@bindings/DeployBookDto").DeployBookDto>("deploy_book_read"),
  deployRunMonth: (asOf: string) => invoke<string>("deploy_run_month", { asOf }),
  deployCommitMonth: (asOf: string) => invoke<string>("deploy_commit_month", { asOf }),
  // 审计
  auditList: (limit: number, kind?: string, status?: string) => invoke<import("@bindings/AuditRecordDto").AuditRecordDto[]>("audit_list", { limit, kind: kind ?? null, status: status ?? null }),
  auditLogTail: (lines: number) => invoke<string>("audit_log_tail", { lines }),
};
export type Api = typeof api;
