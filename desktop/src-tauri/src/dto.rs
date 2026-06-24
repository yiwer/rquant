//! 前端 DTO——桥接层对外的唯一数据形态;全部派生 ts-rs 供 ui 生成 TS 类型。
//! 字段语义对照 spec §5.1;时间一律 ISO-8601 字符串(前端不解析 NaiveDateTime)。
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct BookCardDto {
    /// "b1" | "b2" | "b3"
    pub book: String,
    pub title: String,
    /// "single" | "portfolio"
    pub kind: String,
    /// "ok" | "empty"(state 未建) | "corrupt"(state 损坏/串树)
    pub status: String,
    /// status != ok 时的可操作建议。
    pub advice: Option<String>,
    /// 以下来自已 commit 的 state(empty/corrupt 时 None)。
    pub nav: Option<f64>,
    pub total_return: Option<f64>,
    pub max_drawdown: Option<f64>,
    pub pos: Option<f64>,
    pub state_time: Option<String>,
    /// 账本3:当前持仓清单(symbol, weight)。
    pub holdings: Option<Vec<(String, f64)>>,
    /// 最新信号(来自 sig_*.json;时间戳可能比 state 新——dry 残留,如实分开展示)。
    pub last_signal: Option<SignalBriefDto>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SignalBriefDto {
    pub t: String,
    /// single:目标仓位;portfolio:入选数。
    pub target: Option<f64>,
    pub current_pos: Option<f64>,
    pub delta: Option<f64>,
    pub reason: Option<String>,
    pub leaf: Option<String>,
    #[ts(type = "number | null")]
    pub bars_replayed: Option<u64>,
    /// portfolio:目标清单。
    pub targets: Option<Vec<(String, f64)>>,
    #[ts(type = "number | null")]
    pub n_fresh: Option<u64>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DiffRowDto {
    pub symbol: String,
    /// "Buy" | "Sell" | "Adjust" | "Hold"
    pub action: String,
    pub from_w: f64,
    pub to_w: f64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RunlogStatusDto {
    /// 最近一段 run 的头行(==== 日期 ====)。
    pub last_header: Option<String>,
    /// true=最近段含 committed/DRY 正常收尾;false=可疑(含 error 等);None=无日志。
    pub ok: Option<bool>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SchtaskDto {
    pub next_run: Option<String>,
    pub last_run: Option<String>,
    pub last_result: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct OverviewDto {
    pub cards: Vec<BookCardDto>,
    /// 账本3 今日清单 diff(来自 sig_portfolio.json trades)。
    pub diff: Vec<DiffRowDto>,
    pub diff_t: Option<String>,
    pub runlog: RunlogStatusDto,
    /// schtasks 查询失败/任务不存在 → None。
    pub schtask: Option<SchtaskDto>,
    /// gm 尾盘取数计划任务(rquant-gm-tail)状态;未装/查询失败 → None。
    pub gm_tail: Option<SchtaskDto>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SnapshotDto {
    pub pos: f64,
    pub entry_price: Option<f64>,
    #[ts(type = "number")]
    pub bars_held: u64,
    pub nav: f64,
    pub peak_nav: f64,
    pub max_drawdown: f64,
    pub turnover: f64,
    pub last_increase_date: Option<String>,
    pub max_price_since_entry: Option<f64>,
    pub min_price_since_entry: Option<f64>,
    pub bars_since_exit: Option<f64>,
    pub last_trip_return: Option<f64>,
    /// TripSnapshot 原样 JSON(UI 直接展示,不拆字段)。
    #[ts(type = "unknown")]
    pub trip: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct JournalPointDto {
    pub state_time: String,
    pub nav: Option<f64>,
    pub pos: Option<f64>,
    /// 账本3:成员数。
    pub members: Option<u32>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct BookDetailDto {
    pub card: BookCardDto,
    pub snapshot: Option<SnapshotDto>,
    pub journal: Vec<JournalPointDto>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TaskProgressDto {
    pub pct: f32,
    pub stage: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TaskInfoDto {
    pub id: String,
    pub kind: String,
    /// "running" | "done" | "failed" | "cancelled"
    pub status: String,
    pub progress: TaskProgressDto,
    pub error: Option<String>,
    /// 完成结果(JSON 任意形态,manual_run 放 run 摘要)。
    #[ts(type = "unknown")]
    pub result: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct GateDto {
    /// "allow" | "dry_only" | "warn"
    pub gate: String,
    pub message: Option<String>,
}

// ───────────────────────── M2: 回测中心 / 数据工作台 ─────────────────────────

/// 回测运行配置(留档 config.json 原文;Deserialize 供读回与重跑)。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BacktestConfigDto {
    /// 工作区相对路径(examples/.. 或 deploy/..)。
    pub tree_path: String,
    /// 主行情 CSV(工作区相对)。fetch 置时由任务先拉取生成。
    pub primary_path: String,
    /// "sim_hard" | "sim_soft" | "score_hard" | "score_soft"
    pub mode: String,
    pub cost_bps: f64,
    pub warmup: u32,
    pub window: u32,
    /// 展示层初始资金(元);默认 100000。引擎 nav 语义不感知此值。
    pub initial_capital: f64,
    /// 可选:运行前刷新行情。
    pub fetch: Option<FetchSpecDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FetchSpecDto {
    pub symbol: String,
    /// 分钟:15/60;日线:240。
    pub scale: u32,
    pub datalen: u32,
    /// "qfq" | "none"
    pub adjust: String,
}

/// 留档条目(meta.json;Deserialize 供列表读回)。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RunMetaDto {
    pub id: String,
    /// 同 BacktestConfigDto.mode。
    pub kind: String,
    /// 用户可改名;默认 "<树名> × <primary 文件名>"。
    pub name: String,
    pub tree_name: String,
    pub created: String,
    pub ok: bool,
    pub error: Option<String>,
}

/// 概览指标卡(sim 全量;score 仅 kind/tree_name + raw)。
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RunSummaryDto {
    pub meta: RunMetaDto,
    pub config: BacktestConfigDto,
    pub total_return: Option<f64>,
    pub max_drawdown: Option<f64>,
    pub n_round_trips: Option<u32>,
    pub win_rate: Option<f64>,
    pub avg_hold_bars: Option<f64>,
    pub turnover: Option<f64>,
    pub buy_and_hold: Option<f64>,
    pub sharpe: Option<f64>,
    /// 资金换算(严格口径):initial_capital×(1+total_return) / ×total_return。
    pub final_equity: Option<f64>,
    pub net_pnl: Option<f64>,
    /// score 模式:result.json 原样(UI 原始视图/简版概览用)。sim 模式 None。
    #[ts(type = "unknown")]
    pub raw: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct EquityPointDto {
    pub t: String,
    pub nav: f64,
    /// nav × initial_capital。
    pub equity: f64,
    pub pos: f64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TradeDto {
    pub entry_t: String,
    pub exit_t: String,
    pub entry_px: f64,
    pub exit_px: f64,
    pub max_abs_pos: f64,
    pub trip_return: f64,
    pub bars_held: u32,
    pub reason: String,
    /// 资金×trip_return——单利近似口径(UI 注明)。
    pub pnl_amount: f64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ReplayStepDto {
    pub node_id: String,
    pub label: String,
    pub confidence: f64,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ReplayFrameDto {
    pub t: String,
    pub leaf: String,
    pub stance: String,
    pub path: Vec<ReplayStepDto>,
    /// sim 模式由 SimStepRecord 对齐补充;score 模式 None。
    pub target: Option<f64>,
    pub pos: Option<f64>,
    pub nav: Option<f64>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct FactorValueDto {
    pub name: String,
    /// 非有限→None(NaN 弃权语义如实呈现)。
    pub value: Option<f64>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct BarDto {
    pub t: String,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct FactorPointDto {
    pub t: String,
    pub value: Option<f64>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct CsvInfoDto {
    /// 工作区相对路径。
    pub path: String,
    /// 解析失败→None(坏文件如实列出)。
    pub rows: Option<u32>,
    pub first_t: Option<String>,
    pub last_t: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UniverseEntryDto {
    pub symbol: String,
    pub primary: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct UniverseInfoDto {
    pub path: String,
    pub name: String,
    /// deploy/ 下=true(只读)。
    pub frozen: bool,
    pub entries: Vec<UniverseEntryDto>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TreeInfoDto {
    pub path: String,
    /// load 失败→None + error。
    pub name: Option<String>,
    pub frozen: bool,
    pub error: Option<String>,
}
