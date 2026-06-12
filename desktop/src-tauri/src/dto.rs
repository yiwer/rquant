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
    pub bars_replayed: Option<u64>,
    /// portfolio:目标清单。
    pub targets: Option<Vec<(String, f64)>>,
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
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SnapshotDto {
    pub pos: f64,
    pub entry_price: Option<f64>,
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
