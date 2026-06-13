use crate::backtest::runner::BacktestConfig;
use crate::data::aux_table::AuxTable;
use crate::data::news::NewsRecord;
use crate::engine::soft::traverse_soft;
use crate::engine::traversal::traverse;
use crate::eval::llm::LlmEvaluator;
use crate::features::context::{build_context, SimState};
use crate::tree::schema::Stance;
use crate::Result;
use chrono::{NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;

/// 平仓回合记录。reason: tree/stop/tp/max_hold/end。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundTrip {
    pub entry_t: NaiveDateTime,
    pub exit_t: NaiveDateTime,
    pub entry_px: f64,
    pub exit_px: f64,
    pub max_abs_pos: f64,
    pub trip_return: f64,
    pub bars_held: usize,
    pub reason: String,
}

#[derive(Debug)]
struct OpenTrip {
    entry_t: NaiveDateTime,
    entry_px: f64,
    open_nav: f64,
    max_abs_pos: f64,
}

/// 模拟账户（spec §3 为记账权威）。
#[derive(Debug)]
pub struct SimAccount {
    pub pos: f64,
    pub entry_price: f64,
    pub bars_held: usize,
    pub nav: f64,
    pub peak_nav: f64,
    pub max_drawdown: f64,
    pub turnover: f64,
    pub last_increase_date: Option<NaiveDate>,
    /// 入场以来所见 high 最大值，空仓 NaN（弃权纪律同 entry_price）。
    pub max_price_since_entry: f64,
    /// 入场以来所见 low 最小值，空仓 NaN（弃权纪律同 entry_price）。
    pub min_price_since_entry: f64,
    /// 距最近一次平仓执行 bar 的 bar 数；平仓执行 bar 收盘记 1（镜像 bars_held 口径），
    /// 其后每执行 bar 单调 +1（不论持仓与否）；从未平仓 → NaN（弃权纪律同极值字段）。
    pub bars_since_exit: f64,
    /// 最近一次平仓回合的 trip_return（净值口径）；从未平仓 → NaN。
    pub last_trip_return: f64,
    trip: Option<OpenTrip>,
}

impl Default for SimAccount {
    fn default() -> Self {
        Self {
            pos: 0.0,
            entry_price: f64::NAN,
            bars_held: 0,
            nav: 1.0,
            peak_nav: 1.0,
            max_drawdown: 0.0,
            turnover: 0.0,
            last_increase_date: None,
            max_price_since_entry: f64::NAN,
            min_price_since_entry: f64::NAN,
            bars_since_exit: f64::NAN,
            last_trip_return: f64::NAN,
            trip: None,
        }
    }
}

const EPS: f64 = 1e-12;

impl SimAccount {
    fn close_trip(
        &mut self,
        exit_t: NaiveDateTime,
        exit_px: f64,
        reason: &str,
    ) -> Option<RoundTrip> {
        let trip = self.trip.take()?;
        Some(RoundTrip {
            entry_t: trip.entry_t,
            exit_t,
            entry_px: trip.entry_px,
            exit_px,
            max_abs_pos: trip.max_abs_pos,
            trip_return: self.nav / trip.open_nav - 1.0,
            bars_held: self.bars_held,
            reason: reason.to_string(),
        })
    }
}

/// 一步执行+记账：决策于上根 bar 收盘的 target，在本 bar（prev_close→open→close）执行。
/// 返回本步平掉的回合（翻向时为旧回合）。T+1：同自然日加过仓 → 减仓/翻向顺延（本步不交易）。
#[allow(clippy::too_many_arguments)]
pub fn sim_step(
    acc: &mut SimAccount,
    prev_close: f64,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    exec_t: NaiveDateTime,
    target: f64,
    rate: f64,
    reason: &str,
) -> Option<RoundTrip> {
    let mut target = target.clamp(-1.0, 1.0);
    let reduces = acc.pos.abs() > EPS
        && (target.abs() < acc.pos.abs() - EPS || target * acc.pos < -EPS);
    if reduces && acc.last_increase_date == Some(exec_t.date()) {
        target = acc.pos; // T+1 顺延
    }
    // 段1：旧仓 prev_close→open
    acc.nav *= 1.0 + acc.pos * (open / prev_close - 1.0);
    let delta = target - acc.pos;
    let mut closed = None;
    if delta.abs() > EPS {
        acc.nav *= 1.0 - rate * delta.abs();
        acc.turnover += delta.abs();
        let old = acc.pos;
        let flat_or_flip = old.abs() > EPS && (target.abs() <= EPS || target * old < -EPS);
        if flat_or_flip {
            closed = acc.close_trip(exec_t, open, reason);
            acc.entry_price = f64::NAN;
            acc.bars_held = 0;
        }
        if target.abs() > EPS {
            if old.abs() <= EPS || target * old < -EPS {
                // 自 flat 开仓 / 翻向开新
                acc.trip = Some(OpenTrip {
                    entry_t: exec_t,
                    entry_px: open,
                    open_nav: acc.nav,
                    max_abs_pos: target.abs(),
                });
                acc.entry_price = open;
                acc.bars_held = 0;
                acc.last_increase_date = Some(exec_t.date());
                acc.max_price_since_entry = f64::NAN;
                acc.min_price_since_entry = f64::NAN;
            } else if target.abs() > old.abs() + EPS {
                // 加仓：加权均价
                acc.entry_price = (acc.entry_price * old.abs()
                    + open * (target.abs() - old.abs()))
                    / target.abs();
                acc.last_increase_date = Some(exec_t.date());
            }
            // 部分减仓：entry 不变
        }
        acc.pos = target;
    }
    // 段2：新仓 open→close
    acc.nav *= 1.0 + acc.pos * (close / open - 1.0);
    if acc.pos.abs() > EPS {
        acc.bars_held += 1; // 开仓执行 bar 收盘即为 1（spec §3.5）
        if let Some(trip) = acc.trip.as_mut() {
            trip.max_abs_pos = trip.max_abs_pos.max(acc.pos.abs());
        }
    }
    // 持仓极值（含执行 bar 本身的 high/low）；空仓重置 NaN
    if acc.pos.abs() > EPS {
        if acc.max_price_since_entry.is_nan() {
            acc.max_price_since_entry = high;
            acc.min_price_since_entry = low;
        } else {
            acc.max_price_since_entry = acc.max_price_since_entry.max(high);
            acc.min_price_since_entry = acc.min_price_since_entry.min(low);
        }
    } else {
        acc.max_price_since_entry = f64::NAN;
        acc.min_price_since_entry = f64::NAN;
    }
    acc.peak_nav = acc.peak_nav.max(acc.nav);
    acc.max_drawdown = acc.max_drawdown.max(1.0 - acc.nav / acc.peak_nav);
    // 节流状态量：平仓事件（含翻向）重置计数并记账回合收益；其后每执行 bar 单调 +1。
    if let Some(rt) = &closed {
        acc.last_trip_return = rt.trip_return;
        acc.bars_since_exit = 1.0; // 平仓执行 bar 收盘记 1（镜像 bars_held 口径）
    } else if !acc.bars_since_exit.is_nan() {
        acc.bars_since_exit += 1.0;
    }
    closed
}

/// 期末清算：仍持仓 → 按末收盘计成本平仓（reason="end"）。
pub fn finalize(
    acc: &mut SimAccount,
    last_t: NaiveDateTime,
    last_close: f64,
    rate: f64,
) -> Option<RoundTrip> {
    if acc.pos.abs() <= EPS {
        return None;
    }
    acc.nav *= 1.0 - rate * acc.pos.abs();
    acc.turnover += acc.pos.abs();
    let closed = acc.close_trip(last_t, last_close, "end");
    acc.pos = 0.0;
    acc.entry_price = f64::NAN;
    acc.bars_held = 0;
    acc.max_price_since_entry = f64::NAN;
    acc.min_price_since_entry = f64::NAN;
    acc.peak_nav = acc.peak_nav.max(acc.nav);
    acc.max_drawdown = acc.max_drawdown.max(1.0 - acc.nav / acc.peak_nav);
    // 节流状态量：期末清算同样更新（signal 模式不调 finalize，不受影响）。
    if let Some(rt) = &closed {
        acc.last_trip_return = rt.trip_return;
        acc.bars_since_exit = 1.0;
    }
    closed
}

/// 开仓回合快照（持久化用；OpenTrip 为私有故转换住本文件）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TripSnapshot {
    pub entry_t: NaiveDateTime,
    pub entry_px: f64,
    pub open_nav: f64,
    pub max_abs_pos: f64,
}

/// SimAccount 可序列化快照（entry_price 非有限(NaN/±Inf) ↔ None：serde_json 不允许非有限值）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountSnapshot {
    pub pos: f64,
    pub entry_price: Option<f64>,
    pub bars_held: usize,
    pub nav: f64,
    pub peak_nav: f64,
    pub max_drawdown: f64,
    pub turnover: f64,
    pub last_increase_date: Option<NaiveDate>,
    /// 入场以来最高 high（非有限 ↔ None；旧 state 文件缺字段 → None → 恢复为 NaN，
    /// 该回合内引用极值的条件弃权至下次开仓——树侧应保留固定止损兜底）。
    #[serde(default)]
    pub max_price_since_entry: Option<f64>,
    /// 入场以来最低 low（纪律同上）。
    #[serde(default)]
    pub min_price_since_entry: Option<f64>,
    /// 距最近一次平仓执行 bar 的 bar 数（非有限 ↔ None；旧 state 文件缺字段 → None → 恢复为 NaN，
    /// 引用该字段的冷却条件弃权至下次平仓事件——阻断分支形态天然安全）。
    #[serde(default)]
    pub bars_since_exit: Option<f64>,
    /// 最近一次平仓回合的 trip_return（纪律同上）。
    #[serde(default)]
    pub last_trip_return: Option<f64>,
    pub trip: Option<TripSnapshot>,
}

impl SimAccount {
    pub fn snapshot(&self) -> AccountSnapshot {
        AccountSnapshot {
            pos: self.pos,
            entry_price: if self.entry_price.is_finite() { Some(self.entry_price) } else { None },
            bars_held: self.bars_held,
            nav: self.nav,
            peak_nav: self.peak_nav,
            max_drawdown: self.max_drawdown,
            turnover: self.turnover,
            last_increase_date: self.last_increase_date,
            max_price_since_entry: if self.max_price_since_entry.is_finite() {
                Some(self.max_price_since_entry)
            } else {
                None
            },
            min_price_since_entry: if self.min_price_since_entry.is_finite() {
                Some(self.min_price_since_entry)
            } else {
                None
            },
            bars_since_exit: if self.bars_since_exit.is_finite() {
                Some(self.bars_since_exit)
            } else {
                None
            },
            last_trip_return: if self.last_trip_return.is_finite() {
                Some(self.last_trip_return)
            } else {
                None
            },
            trip: self.trip.as_ref().map(|t| TripSnapshot {
                entry_t: t.entry_t,
                entry_px: t.entry_px,
                open_nav: t.open_nav,
                max_abs_pos: t.max_abs_pos,
            }),
        }
    }

    pub fn restore(s: &AccountSnapshot) -> SimAccount {
        SimAccount {
            pos: s.pos,
            entry_price: s.entry_price.unwrap_or(f64::NAN),
            bars_held: s.bars_held,
            nav: s.nav,
            peak_nav: s.peak_nav,
            max_drawdown: s.max_drawdown,
            turnover: s.turnover,
            last_increase_date: s.last_increase_date,
            max_price_since_entry: s.max_price_since_entry.unwrap_or(f64::NAN),
            min_price_since_entry: s.min_price_since_entry.unwrap_or(f64::NAN),
            bars_since_exit: s.bars_since_exit.unwrap_or(f64::NAN),
            last_trip_return: s.last_trip_return.unwrap_or(f64::NAN),
            trip: s.trip.as_ref().map(|t| OpenTrip {
                entry_t: t.entry_t,
                entry_px: t.entry_px,
                open_nav: t.open_nav,
                max_abs_pos: t.max_abs_pos,
            }),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SimReport / SimStepRecord / run_sim / print_sim_summary
// ─────────────────────────────────────────────────────────────────────────────

/// SimReport: 整个回测期的汇总结果，含回合列表。
#[derive(Debug, Serialize, Deserialize)]
pub struct SimReport {
    pub tree_name: String,
    pub cost_bps: f64,
    pub total_return: f64,
    pub max_drawdown: f64,
    pub n_round_trips: usize,
    pub win_rate: f64,
    pub avg_hold_bars: f64,
    pub turnover: f64,
    pub buy_and_hold: f64,
    pub trades: Vec<RoundTrip>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub risk: Option<crate::report::risk::RiskMetrics>,
}

/// 逐 bar 决策记录（traces JSONL 行）。
#[derive(Debug, Serialize, Deserialize)]
pub struct SimStepRecord {
    pub t: NaiveDateTime,
    pub target: f64,
    pub pos: f64,
    pub nav: f64,
}

/// stance × weight → target 仓位方向。
fn stance_dir(stance: Stance) -> f64 {
    match stance {
        Stance::Long => 1.0,
        Stance::Short => -1.0,
        Stance::Flat => 0.0,
    }
}

/// 端到端顺序模拟：加载→逐 bar 遍历（无并发）→风控→sim_step→指标→写报告。
pub async fn run_sim(cfg: &BacktestConfig, llm: &LlmEvaluator, soft: bool) -> Result<SimReport> {
    // ── 加载（mirror run_soft 的加载段） ────────────────────────────────────
    let tree = crate::tree::loader::load_tree_file(&cfg.tree_path)?;
    let primary = crate::data::reader::read_bars_csv(&cfg.primary_path)?;
    let context = crate::data::reader::read_bars_csv(&cfg.context_path)?;
    let news: Vec<NewsRecord> = match &cfg.news_path {
        Some(p) => crate::data::news::read_news_csv(p)?,
        None => Vec::new(),
    };
    let holidays = match &cfg.holidays_path {
        Some(p) => crate::data::calendar::read_holidays(p)?,
        None => std::collections::HashSet::new(),
    };
    let gaps = crate::backtest::gaps::detect_gaps(
        &primary,
        &crate::data::calendar::AShareCalendar::new(holidays),
    );
    if !gaps.is_empty() {
        eprintln!(
            "[rquant] data gaps on primary: {} missing trading day(s), {} partial day(s)",
            gaps.missing_trading_days.len(),
            gaps.partial_days.len()
        );
        if cfg.holidays_path.is_none() {
            eprintln!("  note: no --holidays provided; A-share holidays may be reported as missing trading days");
        }
    }
    let mut aux_tables: BTreeMap<String, AuxTable> = BTreeMap::new();
    for (name, p) in &cfg.aux_paths {
        aux_tables.insert(name.clone(), crate::data::aux_table::read_aux_csv(p)?);
    }

    // ── 参数 ────────────────────────────────────────────────────────────────
    let rate = cfg.cost_bps / 2.0 / 10_000.0;
    let start = cfg.warmup.min(primary.len());

    // 决策轨迹写入器(硬模式专属;软遍历无单一路径)
    let mut decision_w = match (&cfg.decision_traces_path, soft) {
        (Some(p), false) => Some(std::io::BufWriter::new(std::fs::File::create(p)?)),
        _ => None,
    };

    // ── 主循环（顺序，无 buffered） ──────────────────────────────────────────
    let mut acc = SimAccount::default();
    let mut trips: Vec<RoundTrip> = Vec::new();
    let mut step_records: Vec<SimStepRecord> = Vec::new();

    let loop_end = primary.len().saturating_sub(1);
    for i in start..loop_end {
        let close_i = primary[i].close;
        let open_next = primary[i + 1].open;
        let close_next = primary[i + 1].close;
        let t_next = primary[i + 1].time;

        // 构建 Context（time ≤ primary[i].time 闸门）
        let mut ctx = build_context(
            &primary,
            &context,
            &news,
            &aux_tables,
            primary[i].time,
            cfg.window,
        );

        // 注入 SimState（spec §3.1）
        let unreal_pnl = if acc.pos.abs() > EPS {
            (close_i / acc.entry_price - 1.0) * acc.pos.signum()
        } else {
            0.0
        };
        ctx.sim = SimState {
            pos: acc.pos,
            entry_price: acc.entry_price,
            bars_held: acc.bars_held,
            unreal_pnl,
            max_price_since_entry: acc.max_price_since_entry,
            min_price_since_entry: acc.min_price_since_entry,
            bars_since_exit: acc.bars_since_exit,
            last_trip_return: acc.last_trip_return,
        };

        // 风控覆盖（spec §3.2）：pos≠0 时按 stop→tp→max_hold 顺序检查
        let (target, reason, bar_trace): (f64, &str, Option<crate::engine::trace::Trace>) =
            if acc.pos.abs() > EPS {
                if let Some(risk) = &tree.risk {
                    if risk.stop_loss.is_some_and(|sl| unreal_pnl <= -sl) {
                        (0.0, "stop", None)
                    } else if risk.take_profit.is_some_and(|tp| unreal_pnl >= tp) {
                        (0.0, "tp", None)
                    } else if risk.max_hold_bars.is_some_and(|mh| acc.bars_held >= mh) {
                        (0.0, "max_hold", None)
                    } else {
                        // 未触发风控 → 树目标
                        tree_target(&tree, &ctx, llm, soft).await?
                    }
                } else {
                    tree_target(&tree, &ctx, llm, soft).await?
                }
            } else {
                tree_target(&tree, &ctx, llm, soft).await?
            };
        // 决策轨迹(硬模式专属;软遍历无单一路径)
        if let (Some(w), Some(tr)) = (decision_w.as_mut(), bar_trace.as_ref()) {
            serde_json::to_writer(&mut *w, tr)?;
            writeln!(w)?;
        }

        // 执行 sim_step
        if let Some(rt) = sim_step(
            &mut acc,
            close_i,
            open_next,
            primary[i + 1].high,
            primary[i + 1].low,
            close_next,
            t_next,
            target,
            rate,
            reason,
        ) {
            trips.push(rt);
        }

        // 记录 step（决策时点 primary[i].time，执行后的账户状态）
        step_records.push(SimStepRecord {
            t: primary[i].time,
            target,
            pos: acc.pos,
            nav: acc.nav,
        });
    }

    // 决策轨迹缓冲区刷盘
    if let Some(mut w) = decision_w {
        w.flush()?;
    }

    // 期末清算
    if let Some(last_bar) = primary.last()
        && let Some(rt) = finalize(&mut acc, last_bar.time, last_bar.close, rate)
    {
        trips.push(rt);
    }

    // ── 指标（spec §3.7） ────────────────────────────────────────────────────
    let n_trips = trips.len();
    let win_rate = if n_trips == 0 {
        0.0
    } else {
        trips.iter().filter(|t| t.trip_return > 0.0).count() as f64 / n_trips as f64
    };
    let avg_hold_bars = if n_trips == 0 {
        0.0
    } else {
        trips.iter().map(|t| t.bars_held as f64).sum::<f64>() / n_trips as f64
    };
    // buy_and_hold：首个执行 bar 开盘 → 末 bar 收盘（start+1 < len 时有效）
    let buy_and_hold = if start + 1 < primary.len() {
        primary.last().unwrap().close / primary[start + 1].open - 1.0
    } else {
        0.0
    };
    let total_return = acc.nav - 1.0;
    let max_drawdown = acc.max_drawdown;
    let turnover = acc.turnover;

    // 风险指标：从 step_records 的 (t, nav) 序列计算
    let nav_series: Vec<(chrono::NaiveDateTime, f64)> =
        step_records.iter().map(|r| (r.t, r.nav)).collect();
    let risk = crate::report::risk::risk_metrics(&nav_series, max_drawdown);

    let report = SimReport {
        tree_name: tree.meta.name.clone(),
        cost_bps: cfg.cost_bps,
        total_return,
        max_drawdown,
        n_round_trips: n_trips,
        win_rate,
        avg_hold_bars,
        turnover,
        buy_and_hold,
        trades: trips,
        risk,
    };

    // ── 写输出 ───────────────────────────────────────────────────────────────
    let json = serde_json::to_string_pretty(&report)?;
    std::fs::write(&cfg.out_path, json)?;

    if let Some(tp) = &cfg.traces_path {
        let mut f = std::fs::File::create(tp)?;
        for rec in &step_records {
            let line = serde_json::to_string(rec)?;
            writeln!(f, "{line}")?;
        }
    }

    Ok(report)
}

/// 从树取目标仓位：硬模式 = traverse → leaf stance×weight；软模式 = traverse_soft → Σ p·w·dir。
/// 硬模式额外返回完整 Trace（供调用方写决策轨迹）；软模式返回 None。
async fn tree_target(
    tree: &crate::tree::loader::Tree,
    ctx: &crate::features::context::Context,
    llm: &LlmEvaluator,
    soft: bool,
) -> Result<(f64, &'static str, Option<crate::engine::trace::Trace>)> {
    if soft {
        let soft_trace = traverse_soft(tree, ctx, llm).await?;
        let mut e = 0.0_f64;
        for (leaf_id, &p) in &soft_trace.leaf_probs {
            if let Some(leaf) = tree.leaves.get(leaf_id) {
                e += p * leaf.weight_at(ctx) * stance_dir(leaf.stance);
            }
        }
        Ok((e, "tree", None))
    } else {
        let trace = traverse(tree, ctx, llm).await?;
        let target = tree.leaves.get(&trace.leaf).map_or(0.0, |l| {
            stance_dir(l.stance) * l.weight_at(ctx)
        });
        Ok((target, "tree", Some(trace)))
    }
}

/// 打印 SimReport 摘要（中文标签，参照 print_soft_summary 风格）。
pub fn print_sim_summary(report: &SimReport) {
    println!("=== rquant SIM 回测: {} ===", report.tree_name);
    println!("cost_bps={}", report.cost_bps);
    println!("总收益率  : {:.4}", report.total_return);
    println!("最大回撤  : {:.4}", report.max_drawdown);
    println!("平仓回合数: {}", report.n_round_trips);
    println!("胜率      : {:.1}%", report.win_rate * 100.0);
    println!("平均持仓期: {:.1} bars", report.avg_hold_bars);
    println!("换手率    : {:.4}", report.turnover);
    println!("买入持有  : {:.4}", report.buy_and_hold);
    if let Some(r) = &report.risk {
        let fmt_opt = |v: Option<f64>| v.map_or("—".to_string(), |x| format!("{:.2}", x));
        let fmt_var = |v: f64| format!("{:+.4}", v);
        println!("年化收益  : {}", fmt_opt(r.ann_return));
        println!("年化波动  : {}", fmt_opt(r.ann_vol));
        println!("Sharpe    : {}", fmt_opt(r.sharpe));
        println!("Sortino   : {}", fmt_opt(r.sortino));
        println!("Calmar    : {}", fmt_opt(r.calmar));
        println!("VaR95     : {}", fmt_var(r.var95));
        println!("CVaR95    : {}", fmt_var(r.cvar95));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
    }

    #[test]
    fn golden_walk_enter_hold_exit() {
        // 注意：执行时间须跨自然日（入场日 T+1 禁止当日平仓——纯记账路径用三天展开）
        // bars: b0 c=10 | b1 o=10 c=10.2 | b2 o=10.4 c=10.6 | b3 o=10.8 c=10.6
        // rate=0.001。i0: target 1 → exec b1；i1: hold → b2 无交易；i2: target 0 → exec b3 平仓。
        let mut acc = SimAccount::default();
        let rt1 = sim_step(
            &mut acc,
            10.0,
            10.0,
            10.2,
            10.0,
            10.2,
            t("2024-01-02 10:00:00"),
            1.0,
            0.001,
            "tree",
        );
        assert!(rt1.is_none());
        assert_relative_eq!(acc.nav, 0.999 * (10.2 / 10.0), epsilon = 1e-12);
        assert_relative_eq!(acc.entry_price, 10.0);
        assert_eq!(acc.bars_held, 1);
        let rt2 = sim_step(
            &mut acc,
            10.2,
            10.4,
            10.6,
            10.4,
            10.6,
            t("2024-01-03 10:00:00"),
            1.0,
            0.001,
            "tree",
        );
        assert!(rt2.is_none());
        assert_relative_eq!(acc.nav, 0.999 * (10.6 / 10.0), epsilon = 1e-12); // 连续持仓 = 链式收益
        assert_eq!(acc.bars_held, 2);
        let rt3 = sim_step(
            &mut acc,
            10.6,
            10.8,
            10.8,
            10.6,
            10.6,
            t("2024-01-04 10:00:00"),
            0.0,
            0.001,
            "tree",
        )
        .unwrap();
        // 平仓后 nav = 0.999*(10.8/10.0)*0.999；段2 pos=0 不变
        assert_relative_eq!(
            acc.nav,
            0.999 * (10.8 / 10.0) * 0.999,
            epsilon = 1e-12
        );
        assert_eq!(acc.pos, 0.0);
        assert!(acc.entry_price.is_nan());
        assert_eq!(rt3.exit_px, 10.8);
        assert_eq!(rt3.bars_held, 2);
        assert_eq!(rt3.reason, "tree");
        // trip_return 以回合 open_nav（入场成本后、入场 bar 段2 前）为基：
        // open_nav = 0.999；close 时 nav = 0.999×(10.8/10)×0.999 → trip_return = (10.8/10)×0.999 − 1
        assert_relative_eq!(
            rt3.trip_return,
            (10.8 / 10.0) * 0.999 - 1.0,
            epsilon = 1e-12
        );
        assert_relative_eq!(acc.turnover, 2.0);
    }

    #[test]
    fn t1_defers_same_day_reduction() {
        let mut acc = SimAccount::default();
        // 同一自然日：开仓后立刻请求平仓 → 顺延；次日可平
        sim_step(
            &mut acc,
            10.0,
            10.0,
            10.0,
            10.0,
            10.0,
            t("2024-01-02 10:00:00"),
            1.0,
            0.0,
            "tree",
        );
        let r = sim_step(
            &mut acc,
            10.0,
            10.0,
            10.0,
            10.0,
            10.0,
            t("2024-01-02 10:15:00"),
            0.0,
            0.0,
            "tree",
        );
        assert!(r.is_none());
        assert_eq!(acc.pos, 1.0); // 被顺延
        let r2 = sim_step(
            &mut acc,
            10.0,
            10.0,
            10.0,
            10.0,
            10.0,
            t("2024-01-03 09:45:00"),
            0.0,
            0.0,
            "tree",
        );
        assert!(r2.is_some());
        assert_eq!(acc.pos, 0.0);
    }

    #[test]
    fn flip_closes_old_and_opens_new() {
        let mut acc = SimAccount::default();
        sim_step(
            &mut acc,
            10.0,
            10.0,
            10.0,
            10.0,
            10.0,
            t("2024-01-02 10:00:00"),
            1.0,
            0.0,
            "tree",
        );
        let closed = sim_step(
            &mut acc,
            10.0,
            10.0,
            10.0,
            10.0,
            10.0,
            t("2024-01-03 10:00:00"),
            -0.5,
            0.001,
            "tree",
        )
        .unwrap();
        assert_eq!(closed.exit_px, 10.0);
        assert_eq!(acc.pos, -0.5);
        assert_relative_eq!(acc.entry_price, 10.0);
        assert_eq!(acc.bars_held, 1); // 新回合从 1 起
        assert_relative_eq!(acc.turnover, 1.0 + 1.5); // |Δ|=1.5 一次计
    }

    #[test]
    fn add_position_weighted_entry_and_partial_reduce_keeps_entry() {
        let mut acc = SimAccount::default();
        sim_step(
            &mut acc,
            10.0,
            10.0,
            10.0,
            10.0,
            10.0,
            t("2024-01-02 10:00:00"),
            0.5,
            0.0,
            "tree",
        );
        sim_step(
            &mut acc,
            10.0,
            12.0,
            12.0,
            12.0,
            12.0,
            t("2024-01-03 10:00:00"),
            1.0,
            0.0,
            "tree",
        );
        assert_relative_eq!(acc.entry_price, (10.0 * 0.5 + 12.0 * 0.5) / 1.0); // 11.0
        sim_step(
            &mut acc,
            12.0,
            12.0,
            12.0,
            12.0,
            12.0,
            t("2024-01-04 10:00:00"),
            0.4,
            0.0,
            "tree",
        );
        assert_relative_eq!(acc.entry_price, 11.0); // 部分减仓 entry 不变
        assert_eq!(acc.pos, 0.4);
        // 部分减仓不重置极值：max 来自加仓 bar（12），min 来自入场 bar（10）
        assert_relative_eq!(acc.max_price_since_entry, 12.0);
        assert_relative_eq!(acc.min_price_since_entry, 10.0);
    }

    #[test]
    fn finalize_liquidates_with_cost() {
        let mut acc = SimAccount::default();
        sim_step(
            &mut acc,
            10.0,
            10.0,
            11.0,
            10.0,
            11.0,
            t("2024-01-02 10:00:00"),
            1.0,
            0.001,
            "tree",
        );
        let nav_before = acc.nav;
        let rt = finalize(&mut acc, t("2024-01-02 10:15:00"), 11.0, 0.001).unwrap();
        assert_relative_eq!(acc.nav, nav_before * 0.999, epsilon = 1e-12);
        assert_eq!(rt.reason, "end");
        assert_eq!(acc.pos, 0.0);
    }

    // ─────────────────────────────────────────────────────────────────────
    // Integration tests for run_sim (tokio async)
    // ─────────────────────────────────────────────────────────────────────

    /// 构建 6 根上升 bar 的 CSV 并返回 NamedTempFile（保持其生命周期）。
    /// Bars span 3 calendar days so T+1 constraint is exercised.
    fn write_rising_bars_csv() -> tempfile::NamedTempFile {
        use std::io::Write as _;
        // 6 bars across 3 days: 2 per day
        // day1: 2024-01-02, day2: 2024-01-03, day3: 2024-01-04
        let csv = "\
time,open,high,low,close,volume
2024-01-02 09:45:00,10.0,10.1,9.9,10.0,1000
2024-01-02 10:00:00,10.0,10.2,9.9,10.1,1000
2024-01-03 09:45:00,10.1,10.3,10.0,10.2,1000
2024-01-03 10:00:00,10.2,10.4,10.1,10.3,1000
2024-01-04 09:45:00,10.3,10.5,10.2,10.4,1000
2024-01-04 10:00:00,10.4,10.6,10.3,10.5,1000
";
        let mut f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        write!(f, "{csv}").unwrap();
        f.flush().unwrap();
        f
    }

    /// Tree: pos==0 and close>0 → long; pos>0 and bars_held>=2 → flat; pos>0 → long; default flat
    const ENTER_HOLD_EXIT_TREE: &str = r#"
meta: { name: enter_hold_exit, forward_window: 1, stances: [long, flat] }
root: gate
nodes:
  gate:
    type: quant
    branches:
      - when: "pos == 0 and close > 0"
        goto: leaf_long
        label: enter
      - when: "pos > 0 and bars_held >= 2"
        goto: leaf_flat
        label: exit
      - when: "pos > 0"
        goto: leaf_long
        label: hold
    default: { goto: leaf_flat, label: idle }
leaves:
  leaf_long: { stance: long }
  leaf_flat: { stance: flat }
"#;

    /// Tree with stop_loss=0.01 to test risk overlay on falling data.
    const STOP_LOSS_TREE: &str = r#"
meta: { name: stop_tree, forward_window: 1, stances: [long, flat] }
risk: { stop_loss: 0.01 }
root: gate
nodes:
  gate:
    type: quant
    branches:
      - when: "pos == 0 and close > 0"
        goto: leaf_long
        label: enter
      - when: "pos > 0"
        goto: leaf_long
        label: hold
    default: { goto: leaf_flat, label: idle }
leaves:
  leaf_long: { stance: long }
  leaf_flat: { stance: flat }
"#;

    /// Falling bars CSV: enters long then price drops triggering stop loss.
    fn write_falling_bars_csv() -> tempfile::NamedTempFile {
        use std::io::Write as _;
        // bar0: enter decision (close=10.0)
        // bar1: exec enter at open=10.0 close=10.0 (day2 → T+1 protection records increase date)
        // bar2: after 1 bar held, price drops 2% → unreal_pnl=-0.02 < -0.01=stop_loss → exit
        // bar3: exec exit
        let csv = "\
time,open,high,low,close,volume
2024-01-02 09:45:00,10.0,10.1,9.9,10.0,1000
2024-01-02 10:00:00,10.0,10.1,9.9,10.0,1000
2024-01-03 09:45:00,10.0,10.1,9.8,9.8,1000
2024-01-03 10:00:00,9.8,9.9,9.7,9.75,1000
";
        let mut f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        write!(f, "{csv}").unwrap();
        f.flush().unwrap();
        f
    }

    fn make_cfg(
        tree_f: &tempfile::NamedTempFile,
        primary_f: &tempfile::NamedTempFile,
        out_f: &tempfile::NamedTempFile,
        traces_f: Option<&tempfile::NamedTempFile>,
    ) -> BacktestConfig {
        BacktestConfig {
            tree_path: tree_f.path().to_path_buf(),
            primary_path: primary_f.path().to_path_buf(),
            context_path: primary_f.path().to_path_buf(), // reuse primary as context
            news_path: None,
            out_path: out_f.path().to_path_buf(),
            traces_path: traces_f.map(|f| f.path().to_path_buf()),
            cost_bps: 5.0,
            warmup: 0,
            window: 10,
            concurrency: 1,
            holidays_path: None,
            folds: 1,
            aux_paths: Vec::new(),
            decision_traces_path: None,
        }
    }

    /// Write a tree YAML string to a tempfile.
    fn write_tree_yaml(src: &str) -> tempfile::NamedTempFile {
        use std::io::Write as _;
        let mut f = tempfile::Builder::new().suffix(".yaml").tempfile().unwrap();
        write!(f, "{src}").unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn extremes_track_high_low_since_entry() {
        let mut acc = SimAccount::default();
        assert!(acc.max_price_since_entry.is_nan());
        // 入场执行 bar：high=10.5 low=9.9
        sim_step(&mut acc, 10.0, 10.0, 10.5, 9.9, 10.2, t("2024-01-02 10:00:00"), 1.0, 0.0, "tree");
        assert_relative_eq!(acc.max_price_since_entry, 10.5);
        assert_relative_eq!(acc.min_price_since_entry, 9.9);
        // 持仓 bar：high=11.0 创新高，low=10.2 不创新低
        sim_step(&mut acc, 10.2, 10.4, 11.0, 10.2, 10.8, t("2024-01-03 10:00:00"), 1.0, 0.0, "tree");
        assert_relative_eq!(acc.max_price_since_entry, 11.0);
        assert_relative_eq!(acc.min_price_since_entry, 9.9);
        // 平仓 → 极值重置 NaN
        sim_step(&mut acc, 10.8, 10.9, 10.9, 10.5, 10.6, t("2024-01-04 10:00:00"), 0.0, 0.0, "tree");
        assert!(acc.max_price_since_entry.is_nan());
        assert!(acc.min_price_since_entry.is_nan());
    }

    #[test]
    fn extremes_reset_on_flip() {
        let mut acc = SimAccount::default();
        sim_step(&mut acc, 10.0, 10.0, 10.5, 9.9, 10.0, t("2024-01-02 10:00:00"), 1.0, 0.0, "tree");
        // 翻向：新回合极值只来自当前执行 bar（10.2/9.8），不继承旧回合的 10.5/9.9
        sim_step(&mut acc, 10.0, 10.0, 10.2, 9.8, 10.0, t("2024-01-03 10:00:00"), -1.0, 0.0, "tree");
        assert_relative_eq!(acc.max_price_since_entry, 10.2);
        assert_relative_eq!(acc.min_price_since_entry, 9.8);
    }

    #[tokio::test]
    async fn run_sim_enter_hold_exit_hard() {
        let tree_f = write_tree_yaml(ENTER_HOLD_EXIT_TREE);
        let bars_f = write_rising_bars_csv();
        let out_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        let traces_f = tempfile::Builder::new().suffix(".jsonl").tempfile().unwrap();

        let cfg = make_cfg(&tree_f, &bars_f, &out_f, Some(&traces_f));
        let report = run_sim(&cfg, &LlmEvaluator::Disabled, false)
            .await
            .expect("run_sim should succeed");

        assert!(
            report.n_round_trips >= 1,
            "expected at least 1 round trip, got {}",
            report.n_round_trips
        );
        assert!(
            report.total_return.is_finite(),
            "total_return must be finite, got {}",
            report.total_return
        );

        // Verify traces file was written with correct line count (= decision count)
        let traces_content = std::fs::read_to_string(traces_f.path()).unwrap();
        let trace_lines = traces_content.lines().filter(|l| !l.trim().is_empty()).count();
        // Decision count = loop iterations = loop_end - start = (len-1) - 0 = 5
        // (6 bars → len=6 → loop_end=5 → indices 0..5 → 5 decisions)
        assert!(
            trace_lines > 0,
            "traces file should have at least one line"
        );
        // Each decision step produces exactly one record
        // primary.len()-1 - warmup = 5 - 0 = 5
        assert_eq!(
            trace_lines, 5,
            "traces line count should equal decision count (5), got {trace_lines}"
        );
    }

    #[test]
    fn sim_report_compat_old_json_without_risk() {
        // 旧 JSON 无 risk 字段 → 反序列化成功且 risk == None
        let json = r#"{
            "tree_name": "t", "cost_bps": 5.0, "total_return": 0.1,
            "max_drawdown": 0.05, "n_round_trips": 2, "win_rate": 0.5,
            "avg_hold_bars": 3.0, "turnover": 4.0, "buy_and_hold": 0.02,
            "trades": []
        }"#;
        let report: SimReport = serde_json::from_str(json).unwrap();
        assert!(report.risk.is_none(), "old JSON without risk should deserialize to risk=None");
    }

    #[tokio::test]
    async fn run_sim_stop_loss_fires() {
        let tree_f = write_tree_yaml(STOP_LOSS_TREE);
        let bars_f = write_falling_bars_csv();
        let out_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();

        let cfg = make_cfg(&tree_f, &bars_f, &out_f, None);
        let report = run_sim(&cfg, &LlmEvaluator::Disabled, false)
            .await
            .expect("run_sim should succeed");

        // At least the stop-triggered trip should have reason "stop"
        assert!(
            !report.trades.is_empty(),
            "expected at least one trade, got 0"
        );
        let first_reason = &report.trades[0].reason;
        assert_eq!(
            first_reason, "stop",
            "first trip reason should be 'stop', got '{first_reason}'"
        );
    }

    /// 节流状态量：平仓回合记账与逐 bar 计数（含翻向与从未平仓 NaN）。
    #[test]
    fn throttle_state_tracks_exits() {
        let mut acc = SimAccount::default();
        assert!(acc.bars_since_exit.is_nan() && acc.last_trip_return.is_nan());
        // 开仓（执行 bar1）：仍无平仓事件
        sim_step(&mut acc, 10.0, 10.0, 10.2, 9.9, 10.1, t("2024-01-02 10:00:00"), 1.0, 0.0, "tree");
        assert!(acc.bars_since_exit.is_nan());
        // 平仓（执行 bar2，开 10.1 → 平在开盘）：bars_since_exit=1，last_trip_return 记账
        sim_step(&mut acc, 10.1, 10.1, 10.3, 10.0, 10.2, t("2024-01-03 10:00:00"), 0.0, 0.0, "tree");
        assert!((acc.bars_since_exit - 1.0).abs() < 1e-12);
        let r1 = acc.last_trip_return;
        assert!((r1 - (10.1 / 10.0 - 1.0)).abs() < 1e-12); // 零成本：入 10.0 出 10.1
        // 空仓再走一根：+1
        sim_step(&mut acc, 10.2, 10.2, 10.4, 10.1, 10.3, t("2024-01-04 10:00:00"), 0.0, 0.0, "tree");
        assert!((acc.bars_since_exit - 2.0).abs() < 1e-12);
        // 再开仓后计数继续单调（不重置）
        sim_step(&mut acc, 10.3, 10.3, 10.5, 10.2, 10.4, t("2024-01-05 10:00:00"), 1.0, 0.0, "tree");
        assert!((acc.bars_since_exit - 3.0).abs() < 1e-12);
        assert!((acc.last_trip_return - r1).abs() < 1e-12); // 未再平仓，不变
    }

    #[test]
    fn account_snapshot_roundtrip_preserves_everything() {
        // 持仓中账户（含 open trip）：执行 bar high 10.6 / low 9.9 → 极值入账
        let mut acc = SimAccount::default();
        sim_step(&mut acc, 10.0, 10.0, 10.6, 9.9, 10.5, t("2024-01-02 10:00:00"), 0.7, 0.001, "tree");
        let snap = acc.snapshot();
        assert_eq!(snap.entry_price, Some(10.0));
        assert_eq!(snap.max_price_since_entry, Some(10.6));
        assert_eq!(snap.min_price_since_entry, Some(9.9));
        // 首次开仓，还没平仓 → bars_since_exit / last_trip_return 均 NaN → None
        assert!(snap.bars_since_exit.is_none(), "bars_since_exit should be None before first exit");
        assert!(snap.last_trip_return.is_none(), "last_trip_return should be None before first exit");
        let json = serde_json::to_string(&snap).unwrap(); // NaN 不出现 → 序列化成功
        let back: AccountSnapshot = serde_json::from_str(&json).unwrap();
        let acc2 = SimAccount::restore(&back);
        assert!((acc2.max_price_since_entry - 10.6).abs() < 1e-15);
        assert!((acc2.min_price_since_entry - 9.9).abs() < 1e-15);
        // 恢复后继续走一步（平仓），与原账户走同一步结果一致
        let mut a1 = acc;
        let mut a2 = acc2;
        let r1 = sim_step(&mut a1, 10.5, 10.6, 10.7, 10.3, 10.4, t("2024-01-03 10:00:00"), 0.0, 0.001, "tree");
        let r2 = sim_step(&mut a2, 10.5, 10.6, 10.7, 10.3, 10.4, t("2024-01-03 10:00:00"), 0.0, 0.001, "tree");
        assert_eq!(r1.is_some(), r2.is_some());
        assert!((a1.nav - a2.nav).abs() < 1e-15 && (a1.pos - a2.pos).abs() < 1e-15);
        assert_eq!(a1.bars_held, a2.bars_held);
        assert!((a1.turnover - a2.turnover).abs() < 1e-15);
        assert!((a1.peak_nav - a2.peak_nav).abs() < 1e-15);
        assert!((a1.max_drawdown - a2.max_drawdown).abs() < 1e-15);
        assert_eq!(a1.last_increase_date, a2.last_increase_date);
        // 平仓后两侧极值同步重置 NaN
        assert!(a1.max_price_since_entry.is_nan() && a2.max_price_since_entry.is_nan());
        assert!(a1.min_price_since_entry.is_nan() && a2.min_price_since_entry.is_nan());
        // 平仓后 bars_since_exit=1、last_trip_return 有值，且两侧一致
        // bars_since_exit 记录平仓执行 bar 收盘为 1（spec §3.6）
        assert!((a1.bars_since_exit - 1.0).abs() < 1e-12, "a1.bars_since_exit should be 1.0, got {}", a1.bars_since_exit);
        assert!((a2.bars_since_exit - 1.0).abs() < 1e-12, "a2.bars_since_exit should be 1.0, got {}", a2.bars_since_exit);
        // last_trip_return = nav / open_nav - 1；入场 10.0、出场 10.6、成本各 0.1%
        // = (1.0*(1-0.07%)*(1+0.7*5%)*(1+0.7*0.666%)*(1-0.07%)) / (1.0*(1-0.07%)) - 1
        // ≈ (1.0*0.9993*1.035*1.00666...*0.9993) / 0.9993 - 1 ≈ 0.041171
        let expected_last_trip_return = 0.04117067;  // entry 10.0 → exit 10.6，成本各 bp10
        assert!(a1.last_trip_return.is_finite(), "a1.last_trip_return should be finite");
        assert!((a1.last_trip_return - expected_last_trip_return).abs() < 1e-6, "a1.last_trip_return expected {}, got {}", expected_last_trip_return, a1.last_trip_return);
        assert!((a1.last_trip_return - a2.last_trip_return).abs() < 1e-15, "a1 and a2 last_trip_return should match");
        // 快照再往返：bars_since_exit/last_trip_return 有实算值，Some 存入 snapshot
        let snap2 = a1.snapshot();
        assert!(snap2.bars_since_exit.is_some(), "bars_since_exit should be Some after exit");
        assert!(snap2.last_trip_return.is_some(), "last_trip_return should be Some after exit");
        let back2: AccountSnapshot = serde_json::from_str(&serde_json::to_string(&snap2).unwrap()).unwrap();
        let a3 = SimAccount::restore(&back2);
        // 快照往返后 bars_since_exit 恢复精确值 = 1.0
        assert!((a3.bars_since_exit - 1.0).abs() < 1e-12, "a3.bars_since_exit should be 1.0 after roundtrip, got {}", a3.bars_since_exit);
        assert!((a3.bars_since_exit - a1.bars_since_exit).abs() < 1e-15, "a3 and a1 bars_since_exit should match after roundtrip");
        // 快照往返后 last_trip_return 恢复精确值 ≈ 0.041171
        let expected_trip_return = 0.04117067;
        assert!((a3.last_trip_return - expected_trip_return).abs() < 1e-6, "a3.last_trip_return expected {}, got {}", expected_trip_return, a3.last_trip_return);
        assert!((a3.last_trip_return - a1.last_trip_return).abs() < 1e-15, "a3 and a1 last_trip_return should match after roundtrip");
        // 空仓账户（全新）：所有 NaN/None 字段
        let flat = SimAccount::default();
        let s = flat.snapshot();
        assert!(s.entry_price.is_none() && s.trip.is_none());
        assert!(s.max_price_since_entry.is_none() && s.min_price_since_entry.is_none());
        assert!(s.bars_since_exit.is_none() && s.last_trip_return.is_none());
        assert!(SimAccount::restore(&s).entry_price.is_nan());
        assert!(SimAccount::restore(&s).max_price_since_entry.is_nan());
        assert!(SimAccount::restore(&s).bars_since_exit.is_nan());
        assert!(SimAccount::restore(&s).last_trip_return.is_nan());
    }

    /// Chandelier 式跟踪止损树：回撤超 2% 即离场。
    const CHANDELIER_TREE: &str = r#"
meta: { name: chandelier, forward_window: 1, stances: [long, flat] }
root: gate
nodes:
  gate:
    type: quant
    branches:
      - when: "pos == 0 and close > 0"
        goto: leaf_long
        label: enter
      - when: "pos > 0 and close < max_price_since_entry * 0.98"
        goto: leaf_flat
        label: chandelier_exit
      - when: "pos > 0"
        goto: leaf_long
        label: hold
    default: { goto: leaf_flat, label: idle }
leaves:
  leaf_long: { stance: long }
  leaf_flat: { stance: flat }
"#;

    /// 冲高后回撤：b1 执行入场（high 10.6），b2 收 10.3 < 10.6*0.98=10.388 → 决策离场，b3 执行。
    fn write_chandelier_bars_csv() -> tempfile::NamedTempFile {
        use std::io::Write as _;
        let csv = "\
time,open,high,low,close,volume
2024-01-02 09:45:00,10.0,10.1,9.9,10.0,1000
2024-01-02 10:00:00,10.0,10.6,9.9,10.5,1000
2024-01-03 09:45:00,10.5,10.55,10.2,10.3,1000
2024-01-03 10:00:00,10.3,10.35,10.1,10.2,1000
";
        let mut f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        write!(f, "{csv}").unwrap();
        f.flush().unwrap();
        f
    }

    #[tokio::test]
    async fn run_sim_chandelier_exit_fires() {
        let tree_f = write_tree_yaml(CHANDELIER_TREE);
        let bars_f = write_chandelier_bars_csv();
        let out_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        let cfg = make_cfg(&tree_f, &bars_f, &out_f, None);
        let report = run_sim(&cfg, &LlmEvaluator::Disabled, false).await.unwrap();
        assert_eq!(report.n_round_trips, 1);
        // 树内 chandelier 分支驱动的离场，reason 是 "tree"（风控块离场才是 stop/tp）
        assert_eq!(report.trades[0].reason, "tree");
        assert_relative_eq!(report.trades[0].exit_px, 10.3); // b3 开盘执行
    }

    /// Turtle 式金字塔：首仓 0.5，浮盈 1% 加到满仓；hold 用 weight:"pos" 维持现仓。
    const PYRAMID_TREE: &str = r#"
meta: { name: pyramid, forward_window: 1, stances: [long, flat] }
root: gate
nodes:
  gate:
    type: quant
    branches:
      - when: "pos == 0 and close > 0"
        goto: leaf_enter
        label: enter
      - when: "pos > 0 and pos < 1 and close > entry_price * 1.01"
        goto: leaf_add
        label: add_unit
      - when: "pos > 0"
        goto: leaf_hold
        label: hold
    default: { goto: leaf_flat, label: idle }
leaves:
  leaf_enter: { stance: long, weight: 0.5 }
  leaf_add:   { stance: long, weight: "min(1, pos + 0.5)" }
  leaf_hold:  { stance: long, weight: "pos" }
  leaf_flat:  { stance: flat }
"#;

    /// 5 bar 跨 5 日：b0 决策入场→b1 执行 0.5；b1 持平→hold；b2 涨 2%→加仓→b3 执行 1.0。
    fn write_pyramid_bars_csv() -> tempfile::NamedTempFile {
        use std::io::Write as _;
        let csv = "\
time,open,high,low,close,volume
2024-01-02 10:00:00,10.0,10.1,9.9,10.0,1000
2024-01-03 10:00:00,10.0,10.1,9.9,10.0,1000
2024-01-04 10:00:00,10.2,10.3,10.1,10.2,1000
2024-01-05 10:00:00,10.2,10.4,10.1,10.3,1000
2024-01-08 10:00:00,10.3,10.5,10.2,10.4,1000
";
        let mut f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        write!(f, "{csv}").unwrap();
        f.flush().unwrap();
        f
    }

    #[tokio::test]
    async fn run_sim_pyramid_adds_units() {
        let tree_f = write_tree_yaml(PYRAMID_TREE);
        let bars_f = write_pyramid_bars_csv();
        let out_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        let traces_f = tempfile::Builder::new().suffix(".jsonl").tempfile().unwrap();
        let cfg = make_cfg(&tree_f, &bars_f, &out_f, Some(&traces_f));
        let report = run_sim(&cfg, &LlmEvaluator::Disabled, false).await.unwrap();
        // 4 个决策点 target 阶梯：入场 0.5 → 维持 0.5 → 加仓 1.0 → 维持 1.0
        let targets: Vec<f64> = std::fs::read_to_string(traces_f.path()).unwrap()
            .lines().filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str::<SimStepRecord>(l).unwrap().target)
            .collect();
        assert_eq!(targets.len(), 4);
        assert!((targets[0] - 0.5).abs() < 1e-9, "enter 0.5, got {}", targets[0]);
        assert!((targets[1] - 0.5).abs() < 1e-9, "hold 0.5, got {}", targets[1]);
        assert!((targets[2] - 1.0).abs() < 1e-9, "add to 1.0, got {}", targets[2]);
        assert!((targets[3] - 1.0).abs() < 1e-9, "hold 1.0, got {}", targets[3]);
        // 期末清算一个回合；回合记录首次入场价 10.0，高水位仓位 1.0（加满）
        assert_eq!(report.n_round_trips, 1);
        assert_relative_eq!(report.trades[0].entry_px, 10.0);
        assert_relative_eq!(report.trades[0].max_abs_pos, 1.0);
    }

    #[tokio::test]
    async fn decision_traces_written_when_path_set_and_report_unchanged() {
        // 两跑:None vs Some——SimReport serde 串必须完全相等(行为零变锁);
        // Some 跑的文件每行可反序列化为 Trace 且 path 非空。
        let tree_f = write_tree_yaml(ENTER_HOLD_EXIT_TREE);
        let bars_f = write_rising_bars_csv();

        let out_f1 = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        let mut cfg1 = make_cfg(&tree_f, &bars_f, &out_f1, None);
        cfg1.decision_traces_path = None;
        let r1 = run_sim(&cfg1, &LlmEvaluator::Disabled, false).await.unwrap();

        let dt = tempfile::Builder::new().suffix(".jsonl").tempfile().unwrap();
        let out_f2 = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        let mut cfg2 = make_cfg(&tree_f, &bars_f, &out_f2, None);
        cfg2.decision_traces_path = Some(dt.path().to_path_buf());
        let r2 = run_sim(&cfg2, &LlmEvaluator::Disabled, false).await.unwrap();

        assert_eq!(
            serde_json::to_string(&r1).unwrap(),
            serde_json::to_string(&r2).unwrap(),
            "report must be identical regardless of decision trace emission"
        );
        let txt = std::fs::read_to_string(dt.path()).unwrap();
        let lines: Vec<_> = txt.lines().collect();
        assert!(!lines.is_empty());
        for l in &lines {
            let tr: crate::engine::trace::Trace = serde_json::from_str(l).unwrap();
            assert!(!tr.path.is_empty(), "trace path must be recorded");
        }
    }

    #[tokio::test]
    async fn decision_traces_skip_risk_override_bars() {
        // 风控覆盖 bar 不遍历树 → 无 Trace 行;文件行数必须少于决策 bar 总数。
        // 构造方式照抄 run_sim_stop_loss_fires(STOP_LOSS_TREE + 下跌 bars),
        // 仅多设 decision_traces_path。
        // 断言:1) run 正常完成且 trades 含 reason=="stop"
        //      2) decision jsonl 行数 < SimStepRecord 总数 —— 用一个保守断言:
        //         行数 < (traces_path 也设上,数其行数)
        let tree_f = write_tree_yaml(STOP_LOSS_TREE);
        let bars_f = write_falling_bars_csv();

        let dt = tempfile::Builder::new().suffix(".jsonl").tempfile().unwrap();
        let tr = tempfile::Builder::new().suffix(".jsonl").tempfile().unwrap();
        let out_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        let mut cfg = make_cfg(&tree_f, &bars_f, &out_f, Some(&tr));
        cfg.decision_traces_path = Some(dt.path().to_path_buf());
        let report = run_sim(&cfg, &LlmEvaluator::Disabled, false).await.unwrap();

        // 1) run 正常完成且 trades 含 reason=="stop"
        assert!(
            !report.trades.is_empty(),
            "expected at least one trade, got 0"
        );
        let first_reason = &report.trades[0].reason;
        assert_eq!(
            first_reason, "stop",
            "first trip reason should be 'stop', got '{first_reason}'"
        );

        // 2) decision jsonl 行数 < SimStepRecord 总数
        let d_lines = std::fs::read_to_string(dt.path()).unwrap().lines().count();
        let s_lines = std::fs::read_to_string(tr.path()).unwrap().lines().count();
        assert!(d_lines > 0, "decision traces should have at least one line");
        assert!(
            d_lines < s_lines,
            "risk-override bars must be absent from decision traces ({} vs {})",
            d_lines,
            s_lines
        );
    }
}
