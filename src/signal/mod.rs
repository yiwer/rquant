//! F-9 signal：单标的增量纸交易重放引擎。
//!
//! 核心语义（spec §2）：
//! - 决策于 bar i 收盘、执行于 bar i+1 开盘（与 sim 同口径）。
//! - **可记账决策** = i ≤ len−2；**悬挂决策** = i = len−1，输出今日信号，不记账。
//! - state.last_time = 已记账的最后**决策 bar** 时间。
//! - state 永远落后一根 bar → 增量 ≡ 全量天然成立。
//! - 不调用 finalize（持仓滚动，无期末清算）。

use crate::backtest::sim::{AccountSnapshot, SimAccount, sim_step};
use crate::data::aux_table::AuxTable;
use crate::data::news::NewsRecord;
use crate::engine::soft::traverse_soft;
use crate::engine::traversal::traverse;
use crate::eval::llm::LlmEvaluator;
use crate::features::context::{build_context, SimState};
use crate::tree::schema::Stance;
use crate::{Error, Result};
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// ──────────────────────────────────────────────────────────────────────────────
// 常量
// ──────────────────────────────────────────────────────────────────────────────
const EPS: f64 = 1e-12;
const STATE_VERSION: u32 = 1;

// ──────────────────────────────────────────────────────────────────────────────
// 公开类型
// ──────────────────────────────────────────────────────────────────────────────

/// 纸交易持久化状态（JSON落盘，人可读）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperState {
    /// 协议版本 = 1。
    pub version: u32,
    /// 树名（防串树）。
    pub tree_name: String,
    /// 已记账最后**决策 bar** 时间（落后最新 bar 一根；悬挂决策不记账）。
    pub last_time: Option<NaiveDateTime>,
    /// 账户快照（持仓/均价/bars_held）。
    pub account: AccountSnapshot,
}

/// 回放统计摘要（随信号一起输出）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperStats {
    /// 净值（权益 / 初始资金）。
    pub nav: f64,
    /// 总收益率（nav - 1）。
    pub total_return: f64,
    /// 最大回撤。
    pub max_drawdown: f64,
    /// 本次运行真正重放（记账）的 bar 数。
    pub bars_replayed: usize,
}

/// 单标的信号输出。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SingleSignal {
    /// 悬挂 bar 时间（今日决策）。
    pub t: NaiveDateTime,
    /// 树或风控建议的目标仓位。
    pub target: f64,
    /// 当前实际持仓（重放后账户）。
    pub current_pos: f64,
    /// 建议调仓量：target − current_pos。
    pub delta: f64,
    /// 触发原因：tree / stop / tp / max_hold。
    pub reason: String,
    /// 硬遍历叶 id；soft 模式 → None。
    pub leaf: Option<String>,
    /// 回放统计。
    pub paper: PaperStats,
}

/// 单标的信号运行配置。
#[derive(Debug, Clone)]
pub struct SignalSingleConfig {
    /// 决策树 YAML 文件路径。
    pub tree_path: PathBuf,
    /// 主行情 CSV 路径（K 线数据）。
    pub primary_path: PathBuf,
    /// 辅助行情 CSV 路径（Context 计算）。
    pub context_path: PathBuf,
    /// 新闻 CSV 路径（可选，供 LLM 节点）。
    pub news_path: Option<PathBuf>,
    /// 辅助表路径列表。
    pub aux_paths: Vec<(String, PathBuf)>,
    /// 特征工程窗口大小（单位：bars）。
    pub window: usize,
    /// 预热 bar 数（单位：bars，预热期不生成决策）。
    pub warmup: usize,
    /// 交易成本（单位：bp，万分比）。
    pub cost_bps: f64,
    /// 是否使用 soft 遍历（概率）。
    pub soft: bool,
    /// Paper state JSON 路径，读写均经 read/write_paper_state。
    pub state_path: PathBuf,
}

// ──────────────────────────────────────────────────────────────────────────────
// State IO
// ──────────────────────────────────────────────────────────────────────────────

/// 读取 paper state 文件。
/// - 不存在 → `Ok(None)`
/// - 空/损坏文件 → `Err`（含 "corrupt"）
/// - version ≠ 1 → `Err`
/// - tree_name 不符 → `Err`（防串树）
pub fn read_paper_state(path: &Path, tree_name: &str) -> Result<Option<PaperState>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(path)?;
    let st: PaperState = serde_json::from_str(&raw).map_err(|e| {
        Error::Data(format!(
            "signal state corrupt: {e}（如需重建请删除该文件）"
        ))
    })?;
    if st.version != STATE_VERSION {
        return Err(Error::Data(format!(
            "signal state version {} unsupported (expected {})（请删除 state 文件重建）",
            st.version, STATE_VERSION
        )));
    }
    if st.tree_name != tree_name {
        return Err(Error::Data(format!(
            "signal state tree_name '{}' does not match requested tree '{tree_name}'（state 与 --tree 不匹配：换 state 文件或删除重建）",
            st.tree_name
        )));
    }
    Ok(Some(st))
}

/// 原子落盘：同目录写 .json.tmp 再 rename 替换（Windows MoveFileEx 替换语义，std 文档保证）。
/// spec §7——半写状态文件不可能被 read_paper_state 观测为 corrupt。
fn write_json_atomic(path: &Path, json: &str) -> Result<()> {
    // 调用方须传 .json 路径;with_extension 替换的是最后一段扩展名。
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// 将 paper state 写入文件（JSON pretty，人可读）。
pub fn write_paper_state(path: &Path, st: &PaperState) -> Result<()> {
    let json = serde_json::to_string_pretty(st)?;
    write_json_atomic(path, &json)?;
    Ok(())
}

// ──────────────────────────────────────────────────────────────────────────────
// 内部助手
// ──────────────────────────────────────────────────────────────────────────────

/// stance × weight → 目标仓位方向。
fn stance_dir(stance: Stance) -> f64 {
    match stance {
        Stance::Long => 1.0,
        Stance::Short => -1.0,
        Stance::Flat => 0.0,
    }
}

/// 从树取目标仓位，返回 `(target, reason, leaf_id)`。
/// - hard：traverse → leaf stance×weight，reason="tree"，leaf=Some(id)。
/// - soft：traverse_soft → Σ p·w·dir，reason="tree"，leaf=None。
async fn compute_tree_target(
    tree: &crate::tree::loader::Tree,
    ctx: &crate::features::context::Context,
    llm: &LlmEvaluator,
    soft: bool,
) -> Result<(f64, &'static str, Option<String>)> {
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
        Ok((target, "tree", Some(trace.leaf)))
    }
}

/// 风控覆盖（stop→tp→max_hold）优先，否则树目标。重放与悬挂决策共用，保证两径同口径。
/// 返回 `(target, reason, leaf_id)`；风控触发时 leaf 为 None。
async fn resolve_target(
    acc: &SimAccount,
    tree: &crate::tree::loader::Tree,
    ctx: &crate::features::context::Context,
    llm: &LlmEvaluator,
    unreal_pnl: f64,
    soft: bool,
) -> Result<(f64, &'static str, Option<String>)> {
    if acc.pos.abs() > EPS {
        if let Some(risk) = &tree.risk {
            if risk.stop_loss.is_some_and(|sl| unreal_pnl <= -sl) {
                Ok((0.0, "stop", None))
            } else if risk.take_profit.is_some_and(|tp| unreal_pnl >= tp) {
                Ok((0.0, "tp", None))
            } else if risk.max_hold_bars.is_some_and(|mh| acc.bars_held >= mh) {
                Ok((0.0, "max_hold", None))
            } else {
                compute_tree_target(tree, ctx, llm, soft).await
            }
        } else {
            compute_tree_target(tree, ctx, llm, soft).await
        }
    } else {
        compute_tree_target(tree, ctx, llm, soft).await
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// 主函数
// ──────────────────────────────────────────────────────────────────────────────

/// 单标的增量重放引擎。
///
/// 返回 `(信号, 更新后 state)`；落盘由调用方按 --commit 决定。
pub async fn run_signal_single(
    cfg: &SignalSingleConfig,
    llm: &LlmEvaluator,
) -> Result<(SingleSignal, PaperState)> {
    // ── 1. 加载树 + state ────────────────────────────────────────────────────
    let tree = crate::tree::loader::load_tree_file(&cfg.tree_path)?;
    let tree_name = tree.meta.name.clone();

    let state_opt = read_paper_state(&cfg.state_path, &tree_name)?;
    let (mut acc, last_time) = match state_opt {
        Some(ref st) => (SimAccount::restore(&st.account), st.last_time),
        None => (SimAccount::default(), None),
    };

    // ── 2. 加载行情数据 ──────────────────────────────────────────────────────
    let primary = crate::data::reader::read_bars_csv(&cfg.primary_path)?;
    let context = crate::data::reader::read_bars_csv(&cfg.context_path)?;
    let news: Vec<NewsRecord> = match &cfg.news_path {
        Some(p) => crate::data::news::read_news_csv(p)?,
        None => Vec::new(),
    };
    let mut aux_tables: BTreeMap<String, AuxTable> = BTreeMap::new();
    for (name, p) in &cfg.aux_paths {
        aux_tables.insert(name.clone(), crate::data::aux_table::read_aux_csv(p)?);
    }

    let len = primary.len();
    if len < cfg.warmup + 1 {
        return Err(Error::Data("not enough bars".to_string()));
    }

    // ── 3. 参数 ──────────────────────────────────────────────────────────────
    let rate = cfg.cost_bps / 2.0 / 10_000.0;

    // ── 4. 重放：warmup..len-1，跳过 time_i <= last_time ──────────────────
    let mut bars_replayed: usize = 0;
    let mut new_last_time: Option<NaiveDateTime> = last_time;

    for i in cfg.warmup..len - 1 {
        let time_i = primary[i].time;
        // 跳过已记账的决策 bar
        if let Some(lt) = last_time
            && time_i <= lt
        {
            continue;
        }

        let close_i = primary[i].close;
        let open_next = primary[i + 1].open;
        let high_next = primary[i + 1].high;
        let low_next = primary[i + 1].low;
        let close_next = primary[i + 1].close;
        let t_next = primary[i + 1].time;

        // 构建 Context（time ≤ primary[i].time 闸门）
        let mut ctx = build_context(
            &primary,
            &context,
            &news,
            &aux_tables,
            time_i,
            cfg.window,
        );

        // 注入 SimState（与 run_sim 逐字同口径）
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
        let (target, reason, _) = resolve_target(&acc, &tree, &ctx, llm, unreal_pnl, cfg.soft).await?;

        // 执行 sim_step（哪怕 delta≈0 也记账）
        let _ = sim_step(
            &mut acc,
            close_i,
            open_next,
            high_next,
            low_next,
            close_next,
            t_next,
            target,
            rate,
            reason,
        );

        bars_replayed += 1;
        new_last_time = Some(time_i);
    }

    // ── 5. 悬挂决策（i = len−1，不记账）──────────────────────────────────
    let hang_i = len - 1;
    let close_hang = primary[hang_i].close;
    let time_hang = primary[hang_i].time;

    let mut ctx_hang = build_context(
        &primary,
        &context,
        &news,
        &aux_tables,
        time_hang,
        cfg.window,
    );

    let unreal_pnl = if acc.pos.abs() > EPS {
        (close_hang / acc.entry_price - 1.0) * acc.pos.signum()
    } else {
        0.0
    };
    ctx_hang.sim = SimState {
        pos: acc.pos,
        entry_price: acc.entry_price,
        bars_held: acc.bars_held,
        unreal_pnl,
        max_price_since_entry: acc.max_price_since_entry,
        min_price_since_entry: acc.min_price_since_entry,
        bars_since_exit: acc.bars_since_exit,
        last_trip_return: acc.last_trip_return,
    };

    // 悬挂决策：风控覆盖优先，否则树目标（保留 leaf trace）
    let (hang_target, hang_reason, hang_leaf) =
        resolve_target(&acc, &tree, &ctx_hang, llm, unreal_pnl, cfg.soft).await?;

    // ── 6. 组装输出 ──────────────────────────────────────────────────────────
    let paper = PaperStats {
        nav: acc.nav,
        total_return: acc.nav - 1.0,
        max_drawdown: acc.max_drawdown,
        bars_replayed,
    };

    let signal = SingleSignal {
        t: time_hang,
        target: hang_target,
        current_pos: acc.pos,
        delta: hang_target - acc.pos,
        reason: hang_reason.to_string(),
        leaf: hang_leaf,
        paper,
    };

    let new_state = PaperState {
        version: STATE_VERSION,
        tree_name,
        last_time: new_last_time,
        account: acc.snapshot(),
    };

    Ok((signal, new_state))
}

// ──────────────────────────────────────────────────────────────────────────────
// 组合清单引擎
// ──────────────────────────────────────────────────────────────────────────────

/// 组合持仓持久化状态（JSON 落盘，人可读）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoldingsState {
    /// 协议版本 = 1。
    pub version: u32,
    /// 树名（防串树）。
    pub tree_name: String,
    /// 最后一次信号生成的时间（目标持仓的时间点）。
    pub last_time: Option<NaiveDateTime>,
    /// 当前目标持仓：symbol → weight（合计 ≤ 1.0）。
    pub holdings: BTreeMap<String, f64>,
}

/// 交易指令动作。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TradeAction {
    /// 买入新头寸。
    Buy,
    /// 卖出全部头寸。
    Sell,
    /// 调整现有头寸。
    Adjust,
    /// 保持不变。
    Hold,
}

/// 单笔交易指令。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeInstr {
    /// 标的代码。
    pub symbol: String,
    /// 交易动作。
    pub action: TradeAction,
    /// 原权重（当前持仓）。
    pub from_w: f64,
    /// 目标权重。
    pub to_w: f64,
}

/// 组合信号输出（目标组成 + 交易清单）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioSignal {
    /// 信号生成时间。
    pub t: NaiveDateTime,
    /// 本轮新鲜标的数（至少有一根当期 bar）。
    pub n_fresh: usize,
    /// 入选目标 (symbol, weight)，按 select_top 顺序：score 降序、并列 symbol 升序。
    pub targets: Vec<(String, f64)>,
    /// 交易清单（按 symbol 字典序）。
    pub trades: Vec<TradeInstr>,
}

/// 组合信号运行配置。
#[derive(Debug, Clone)]
pub struct SignalPortfolioConfig {
    /// 决策树 YAML 文件路径。
    pub tree_path: PathBuf,
    /// Universe CSV 路径（symbol,primary[,context]，按 symbol 字典序）。
    pub universe_path: PathBuf,
    /// 入选数量（top-N）。
    pub top: usize,
    /// 特征工程窗口大小（单位：bars）。
    pub window: usize,
    /// 预热 bar 数（单位 bars）。组合模式不参与打分（横截面仅用 t_last），保留与单标的配置一致性。
    pub warmup: usize,
    /// 交易成本（单位：bp，万分比；清单不记账，保留一致性）。
    pub cost_bps: f64,
    /// 是否使用 soft 遍历（概率）。
    pub soft: bool,
    /// 辅助表路径列表。
    pub aux_paths: Vec<(String, PathBuf)>,
    /// Holdings state JSON 路径，读写均经 read/write_holdings_state。
    pub state_path: PathBuf,
}

// ──────────────────────────────────────────────────────────────────────────────
// Holdings State IO
// ──────────────────────────────────────────────────────────────────────────────

/// 读取 holdings state 文件。
/// - 不存在 → `Ok(None)`
/// - 空/损坏文件 → `Err`（含 "corrupt"）
/// - version ≠ 1 → `Err`
/// - tree_name 不符 → `Err`（防串树）
pub fn read_holdings_state(path: &Path, tree_name: &str) -> Result<Option<HoldingsState>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(path)?;
    let st: HoldingsState = serde_json::from_str(&raw).map_err(|e| {
        Error::Data(format!(
            "portfolio state corrupt: {e}（如需重建请删除该文件）"
        ))
    })?;
    if st.version != STATE_VERSION {
        return Err(Error::Data(format!(
            "portfolio state version {} unsupported (expected {})（请删除 state 文件重建）",
            st.version, STATE_VERSION
        )));
    }
    if st.tree_name != tree_name {
        return Err(Error::Data(format!(
            "portfolio state tree_name '{}' does not match requested tree '{tree_name}'（state 与 --tree 不匹配：换 state 文件或删除重建）",
            st.tree_name
        )));
    }
    Ok(Some(st))
}

/// 将 holdings state 写入文件（JSON pretty，人可读）。
pub fn write_holdings_state(path: &Path, st: &HoldingsState) -> Result<()> {
    let json = serde_json::to_string_pretty(st)?;
    write_json_atomic(path, &json)?;
    Ok(())
}

// ──────────────────────────────────────────────────────────────────────────────
// 组合信号主函数
// ──────────────────────────────────────────────────────────────────────────────

/// 组合信号生成引擎。
///
/// 返回 `(信号, 更新后 state)`；落盘由调用方按 --commit 决定。
pub async fn run_signal_portfolio(
    cfg: &SignalPortfolioConfig,
    llm: &LlmEvaluator,
) -> Result<(PortfolioSignal, HoldingsState)> {
    // ── 1. 加载树 + state ────────────────────────────────────────────────────
    let tree = crate::tree::loader::load_tree_file(&cfg.tree_path)?;
    let tree_name = tree.meta.name.clone();

    let state_opt = read_holdings_state(&cfg.state_path, &tree_name)?;
    let old_holdings = match state_opt {
        Some(ref st) => st.holdings.clone(),
        None => BTreeMap::new(),
    };

    // ── 2. 加载 universe + 行情数据 ──────────────────────────────────────────
    let universe = crate::data::universe::read_universe_csv(&cfg.universe_path)?;

    let mut aux_tables: BTreeMap<String, AuxTable> = BTreeMap::new();
    for (name, p) in &cfg.aux_paths {
        aux_tables.insert(name.clone(), crate::data::aux_table::read_aux_csv(p)?);
    }

    // 逐标的加载 bars（primary + context 均加载）
    let mut primaries: Vec<Vec<crate::data::bar::Bar>> = Vec::with_capacity(universe.len());
    let mut contexts: Vec<Vec<crate::data::bar::Bar>> = Vec::with_capacity(universe.len());
    for entry in &universe {
        primaries.push(crate::data::reader::read_bars_csv(&entry.primary)?);
        contexts.push(crate::data::reader::read_bars_csv(&entry.context)?);
    }

    // ── 3. 时间线 ────────────────────────────────────────────────────────────
    let timeline = crate::backtest::portfolio::build_timeline(&primaries);
    if timeline.is_empty() {
        return Err(Error::Data("empty timeline".into()));
    }
    let t_last = *timeline.last().unwrap();

    // ── 4. 逐标的打分 ───────────────────────────────────────────────────────
    let mut scores: Vec<(String, f64)> = Vec::new();
    for (i, entry) in universe.iter().enumerate() {
        if let Some(s) = crate::backtest::portfolio::score_symbol(
            &primaries[i],
            &contexts[i],
            &aux_tables,
            &tree,
            llm,
            cfg.soft,
            t_last,
            cfg.window,
        )
        .await?
        {
            scores.push((entry.symbol.clone(), s));
        }
    }
    let n_fresh = scores.len();

    // ── 5. select_top → 等权目标 ────────────────────────────────────────────
    let selected = crate::backtest::portfolio::select_top(&scores, cfg.top);
    let n_selected = selected.len();

    let targets: Vec<(String, f64)> = if n_selected > 0 {
        let eq_weight = 1.0 / n_selected as f64;
        selected
            .iter()
            .map(|(symbol, _)| (symbol.clone(), eq_weight))
            .collect()
    } else {
        Vec::new()
    };
    let targets_map: BTreeMap<String, f64> = targets.iter().cloned().collect();

    // ── 6. 生成交易清单 ──────────────────────────────────────────────────────
    // 并集 = old_holdings keys ∪ targets keys，遍历字典序
    let mut all_symbols: Vec<String> = old_holdings
        .keys()
        .chain(targets_map.keys())
        .cloned()
        .collect();
    all_symbols.sort();
    all_symbols.dedup();

    let mut trades: Vec<TradeInstr> = Vec::new();
    for symbol in &all_symbols {
        let from_w = old_holdings.get(symbol).copied().unwrap_or(0.0);
        let to_w = targets_map.get(symbol).copied().unwrap_or(0.0);

        let action = if (from_w - to_w).abs() < EPS {
            TradeAction::Hold
        } else if from_w < EPS && to_w > EPS {
            TradeAction::Buy
        } else if from_w > EPS && to_w < EPS {
            TradeAction::Sell
        } else {
            TradeAction::Adjust
        };

        trades.push(TradeInstr {
            symbol: symbol.clone(),
            action,
            from_w,
            to_w,
        });
    }

    // ── 7. 新鲜度检查 ────────────────────────────────────────────────────────
    if n_fresh < universe.len() {
        eprintln!(
            "[rquant portfolio] freshness: {n_fresh}/{} symbols have current bars",
            universe.len()
        );
    }

    // ── 8. 组装输出 ──────────────────────────────────────────────────────────
    let signal = PortfolioSignal {
        t: t_last,
        n_fresh,
        targets,
        trades,
    };

    let new_state = HoldingsState {
        version: STATE_VERSION,
        tree_name,
        last_time: Some(t_last),
        holdings: targets_map,
    };

    Ok((signal, new_state))
}

// ──────────────────────────────────────────────────────────────────────────────
// Print 函数
// ──────────────────────────────────────────────────────────────────────────────

/// 打印单标的信号摘要（中文标签，参照 print_sim_summary 风格）。
pub fn print_single_signal(sig: &SingleSignal) {
    println!("=== rquant SIGNAL (single) @ {} ===", sig.t);
    let leaf_suffix = match &sig.leaf {
        Some(id) => format!(" (叶 {})", id),
        None => String::new(),
    };
    println!(
        "目标仓位: {:.2}   当前: {:.2}   Δ: {:+.2}",
        sig.target, sig.current_pos, sig.delta
    );
    println!("reason: {}{}", sig.reason, leaf_suffix);
    println!(
        "纸面账户: nav {:.4}  总收益 {:+.2}%  回撤 {:.2}%  本次重放 {} bar",
        sig.paper.nav,
        sig.paper.total_return * 100.0,
        sig.paper.max_drawdown * 100.0,
        sig.paper.bars_replayed,
    );
}

/// 打印组合信号摘要（中文标签，参照 print_sim_summary 风格）。
pub fn print_portfolio_signal(sig: &PortfolioSignal) {
    println!("=== rquant SIGNAL (portfolio) @ {} ===", sig.t);
    println!("目标组合（{}只）:", sig.targets.len());
    for (symbol, weight) in &sig.targets {
        println!("  {}  {:.4}", symbol, weight);
    }
    println!("交易清单（{}条）:", sig.trades.len());
    for instr in &sig.trades {
        let action_str = match instr.action {
            TradeAction::Buy    => "BUY   ",
            TradeAction::Sell   => "SELL  ",
            TradeAction::Adjust => "ADJUST",
            TradeAction::Hold   => "HOLD  ",
        };
        println!("  {}  {}  {:.2} → {:.2}", action_str, instr.symbol, instr.from_w, instr.to_w);
    }
    println!("新鲜标的数: {}", sig.n_fresh);
}

// ──────────────────────────────────────────────────────────────────────────────
// 测试
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::llm::LlmEvaluator;
    use chrono::NaiveDate;
    use std::io::Write;

    // ── 测试辅助 ──────────────────────────────────────────────────────────────

    fn write_file(content: &str, suffix: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::Builder::new().suffix(suffix).tempfile().unwrap();
        write!(f, "{content}").unwrap();
        f.flush().unwrap();
        f
    }

    /// 合成 32 bar 数据（4 天 × 8 bar/天），稳定上行 10.0→13.1。
    fn gen_primary_csv() -> String {
        let mut s = String::from("time,open,high,low,close,volume\n");
        let mut idx = 0usize;
        for day in 0..4usize {
            for k in 0..8usize {
                let price = 10.0 + 0.1 * idx as f64;
                // 09:45 起每 15 分钟一根，至 11:30
                let hour = 9 + (45 + k * 15) / 60;
                let minute = (45 + k * 15) % 60;
                s.push_str(&format!(
                    "2024-01-{:02} {:02}:{:02}:00,{p},{p},{p},{p},1000\n",
                    2 + day,
                    hour,
                    minute,
                    p = price
                ));
                idx += 1;
            }
        }
        s
    }

    fn gen_context_csv() -> String {
        String::from(
            "time,open,high,low,close,volume\n\
             2024-01-02 10:30:00,10.0,10.0,10.0,10.0,1\n\
             2024-01-02 11:30:00,10.1,10.1,10.1,10.1,1\n\
             2024-01-03 10:30:00,10.2,10.2,10.2,10.2,1\n\
             2024-01-04 10:30:00,10.3,10.3,10.3,10.3,1\n\
             2024-01-05 10:30:00,10.4,10.4,10.4,10.4,1\n",
        )
    }

    /// 入/出/持有树（pos 条件）——与 sim e2e 同形态。
    /// 入场：pos==0，上行趋势（sma 条件简化成 close > 0 恒成立）。
    /// 持有：pos>0 且 bars_held < 5。
    /// 出场：pos>0 且 bars_held >= 5。
    fn enter_hold_exit_tree() -> String {
        r#"
meta: { name: test_signal, forward_window: 1, stances: [long, flat] }
root: gate
nodes:
  gate:
    type: quant
    branches:
      - when: "pos == 0 and close > 0"
        goto: leaf_long
        label: enter
      - when: "pos > 0 and bars_held >= 5"
        goto: leaf_flat
        label: exit
      - when: "pos > 0"
        goto: leaf_long
        label: hold
    default: { goto: leaf_flat, label: idle }
leaves:
  leaf_long: { stance: long }
  leaf_flat: { stance: flat }
"#
        .to_string()
    }

    /// 带止损的树（与 enter_hold_exit_tree 同，但加 risk.stop_loss=0.05）。
    fn enter_hold_stop_tree() -> String {
        r#"
meta: { name: test_signal_stop, forward_window: 1, stances: [long, flat] }
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
risk:
  stop_loss: 0.05
"#
        .to_string()
    }

    /// 吊灯跟踪止损树：出场条件依赖 max_price_since_entry（极值状态量）。
    fn chandelier_signal_tree() -> String {
        r#"
meta: { name: chand_sig, forward_window: 1, stances: [long, flat] }
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
"#
        .to_string()
    }

    /// 吊灯数据：20 根、一天一根（规避 T+1 同日限制）。先冲高（峰 = bar7 high 11.5）
    /// 后阴跌；入场执行 bar6，吊灯线 11.5×0.98=11.27，close 首次跌破在 i=10（11.25）。
    /// 峰值在切分点 k=10 之前、触线在其后——极值若没进 AccountSnapshot，B2 重播会以
    /// 重置后的低峰值（≈bar10 high 11.30）推迟出场 → 状态分叉，本测试即变红。
    fn gen_chandelier_csv() -> String {
        let mut s = String::from("time,open,high,low,close,volume\n");
        for i in 0..20usize {
            let close = if i <= 7 {
                10.0 + 0.2 * i as f64
            } else {
                11.35 - 0.05 * (i - 8) as f64
            };
            let (open, high, low) = if i <= 7 {
                (close - 0.1, close + 0.1, close - 0.2)
            } else {
                (close + 0.03, close + 0.05, close - 0.05)
            };
            s.push_str(&format!(
                "2024-01-{:02} 10:00:00,{open:.2},{high:.2},{low:.2},{close:.2},1000\n",
                2 + i
            ));
        }
        s
    }

    /// state 持久化要求 f64 经 JSON 位级精确往返（serde_json 默认解析为
    /// 尽力而为精度，差 1 ulp 即破坏 split==full——靠 float_roundtrip feature 保证；
    /// 本测试是该 feature 被误删时的回归锁）。
    #[test]
    fn paper_state_f64_json_roundtrip_is_exact() {
        for &x in &[1.0265135135135137_f64, 1.0265135135135135, 0.9793636669920975] {
            let s = serde_json::to_string_pretty(&x).unwrap();
            let y: f64 = serde_json::from_str(&s).unwrap();
            assert_eq!(x.to_bits(), y.to_bits(), "printed as {s}");
        }
    }

    /// 极值经 state 往返的黄金不变量（schema-hardening × F-9 接缝回归锁）。
    #[tokio::test]
    async fn golden_invariant_with_position_extremes() {
        let tree_f = write_file(&chandelier_signal_tree(), ".yaml");
        let full_csv = gen_chandelier_csv();
        let lines: Vec<&str> = full_csv.lines().collect(); // [0]=header + 20 行
        let full_f = write_file(&full_csv, ".csv");
        let llm = LlmEvaluator::Disabled;

        // A：一次性全量 fresh
        let tmp = tempfile::tempdir().unwrap();
        let state_a_path = tmp.path().join("state_a.json");
        let cfg_a = make_cfg(tree_f.path(), full_f.path(), full_f.path(), &state_a_path);
        let (_sig_a, state_a) = run_signal_single(&cfg_a, &llm).await.unwrap();
        // 非空转 sanity：吊灯出场 + 再入场确实发生（入1.0 + 出1.0 + 再入1.0 → turnover ≥ 2.5）
        assert!(
            state_a.account.turnover > 2.5,
            "chandelier exit should have fired, turnover={}",
            state_a.account.turnover
        );

        // B：前 k bar fresh commit → 全量续跑，k 取峰后持仓中(10)与首次出场后(14)
        for k in [10usize, 14] {
            let prefix = format!("{}\n", lines[..=k].join("\n"));
            let prefix_f = write_file(&prefix, ".csv");
            let state_b_path = tmp.path().join(format!("state_b_{k}.json"));
            let cfg_b1 = make_cfg(tree_f.path(), prefix_f.path(), prefix_f.path(), &state_b_path);
            let (_s1, state_b1) = run_signal_single(&cfg_b1, &llm).await.unwrap();
            write_paper_state(&state_b_path, &state_b1).unwrap();
            let cfg_b2 = make_cfg(tree_f.path(), full_f.path(), full_f.path(), &state_b_path);
            let (_s2, state_b2) = run_signal_single(&cfg_b2, &llm).await.unwrap();
            assert_eq!(
                serde_json::to_value(&state_a).unwrap(),
                serde_json::to_value(&state_b2).unwrap(),
                "split==full violated at k={k}（极值未随 state 往返？）"
            );
        }

        // 幂等：以 state_a 再跑全量 → 零重放
        write_paper_state(&state_a_path, &state_a).unwrap();
        let (sig_again, state_again) = run_signal_single(&cfg_a, &llm).await.unwrap();
        assert_eq!(sig_again.paper.bars_replayed, 0);
        assert_eq!(
            serde_json::to_value(&state_a).unwrap(),
            serde_json::to_value(&state_again).unwrap()
        );
    }

    /// 冷却树：离场后 3 根 bar 内不再入场（阻断分支形态——冷却写法纪律）。
    fn cooldown_signal_tree() -> String {
        r#"
meta: { name: cooldown_sig, forward_window: 1, stances: [long, flat] }
root: gate
nodes:
  gate:
    type: quant
    branches:
      - when: "pos > 0 and bars_held >= 2"
        goto: leaf_flat
        label: exit_after_2
      - when: "pos > 0"
        goto: leaf_long
        label: hold
      - when: "bars_since_exit < 3"
        goto: leaf_flat
        label: cooldown_block
      - when: "close > 0"
        goto: leaf_long
        label: enter
    default: { goto: leaf_flat, label: idle }
leaves:
  leaf_long: { stance: long }
  leaf_flat: { stance: flat }
"#
        .to_string()
    }

    /// 节流状态量经 state 往返的黄金不变量（playbook 第三例）。
    ///
    /// 数据 16 根一天一根（close > 0 恒成立），warmup=5：
    ///
    /// 节奏推演（bars 索引 0-15；决策在 i，执行在 bar[i+1]）：
    ///   i=5: pos==0,close>0 → enter → exec bar6; bars_held=1, bars_since_exit=NaN
    ///   i=6: pos>0,bars_held<2 → hold → exec bar7; bars_held=2
    ///   i=7: pos>0,bars_held>=2 → exit → exec bar8; bars_since_exit=1
    ///   i=8: bars_since_exit=1 < 3 → cooldown → exec bar9; bars_since_exit=2
    ///   i=9: bars_since_exit=2 < 3 → cooldown → exec bar10; bars_since_exit=3
    ///   i=10: bars_since_exit=3 ≥ 3 → enter → exec bar11; bars_held=1
    ///   i=11: hold → exec bar12; bars_held=2
    ///   i=12: exit → exec bar13; bars_since_exit=1
    ///   i=13: cooldown(1<3) → exec bar14; bars_since_exit=2
    ///   i=14: 悬挂决策（len-1=15），不记账
    ///
    /// k=9（bars 0-8，9行数据）: B1 末态 bars_since_exit=1（平仓刚发生）。
    ///   咬合点：B2 首个重播决策 i=8 即读 bars_since_exit，字段未入 state 则 NaN → cooldown 落空 → 提前入场 → 分叉。
    /// k=10（bars 0-9，10行数据）: B1 末态 bars_since_exit=2（冷却期中段）。
    ///   咬合点：B2 首个重播决策 i=9 即读 bars_since_exit，字段未入 state 则 NaN → cooldown 落空 → 提前入场 → 分叉。
    /// k=13（bars 0-12，13行数据）: B1 末态 bars_since_exit=1（第二轮平仓后）。
    ///   持仓期中段切分（非字段丢失咬合点：B2 首决策 i=12 走 exit 分支不读值，且重播自身在 i=12 重置 bse）— split==full 正确性覆盖点。
    #[tokio::test]
    async fn golden_invariant_with_throttle_state() {
        // 16 根一天一根，close 单调递增（close>0 恒真，入场条件自然满足）
        let full_csv: String = {
            let mut s = String::from("time,open,high,low,close,volume\n");
            for i in 0..16usize {
                let p = 10.0 + 0.1 * i as f64;
                s.push_str(&format!(
                    "2024-01-{:02} 10:00:00,{p:.2},{p:.2},{p:.2},{p:.2},1000\n",
                    2 + i
                ));
            }
            s
        };
        let lines: Vec<&str> = full_csv.lines().collect(); // [0]=header + 16 data rows

        let tree_f = write_file(&cooldown_signal_tree(), ".yaml");
        let full_f = write_file(&full_csv, ".csv");
        let llm = LlmEvaluator::Disabled;

        // A：一次性全量 fresh
        let tmp = tempfile::tempdir().unwrap();
        let state_a_path = tmp.path().join("state_a.json");
        let cfg_a = make_cfg(tree_f.path(), full_f.path(), full_f.path(), &state_a_path);
        let (_sig_a, state_a) = run_signal_single(&cfg_a, &llm).await.unwrap();
        // 非空转 sanity：至少有两次完整入出场（入1+出1+入2+出2=4次交易，turnover>2.5）
        assert!(
            state_a.account.turnover > 2.5,
            "cooldown tree must fire multiple round trips, turnover={}",
            state_a.account.turnover
        );

        // B：前 k bar fresh commit → 全量续跑；split==full 断言
        // k=9: bars 0-8，咬合点：B2 首个重播决策 i=8 即读 bars_since_exit，字段未入 state 则 NaN→冷却落空→提前入场→分叉
        // k=10: bars 0-9，咬合点：B2 首个重播决策 i=9 即读 bars_since_exit，字段未入 state 则 NaN→冷却落空→提前入场→分叉
        // k=13: bars 0-12，持仓期中段切分（非字段丢失咬合点：B2 首决策走 exit 分支不读值，且重播自身在 i=12 重置 bse）— split==full 正确性覆盖点
        for k in [9usize, 10, 13] {
            let prefix = format!("{}\n", lines[..=k].join("\n"));
            let prefix_f = write_file(&prefix, ".csv");
            let state_b_path = tmp.path().join(format!("state_b_{k}.json"));
            let cfg_b1 = make_cfg(tree_f.path(), prefix_f.path(), prefix_f.path(), &state_b_path);
            let (_s1, state_b1) = run_signal_single(&cfg_b1, &llm).await.unwrap();
            write_paper_state(&state_b_path, &state_b1).unwrap();
            let cfg_b2 = make_cfg(tree_f.path(), full_f.path(), full_f.path(), &state_b_path);
            let (_s2, state_b2) = run_signal_single(&cfg_b2, &llm).await.unwrap();
            assert_eq!(
                serde_json::to_value(&state_a).unwrap(),
                serde_json::to_value(&state_b2).unwrap(),
                "split==full violated at k={k}（bars_since_exit/last_trip_return 未随 state 往返？）"
            );
        }

        // 幂等：以 state_a 再跑全量 → 零重放，state 不变
        write_paper_state(&state_a_path, &state_a).unwrap();
        let (sig_again, state_again) = run_signal_single(&cfg_a, &llm).await.unwrap();
        assert_eq!(sig_again.paper.bars_replayed, 0);
        assert_eq!(
            serde_json::to_value(&state_a).unwrap(),
            serde_json::to_value(&state_again).unwrap()
        );
    }

    fn make_cfg(
        tree_path: &Path,
        primary_path: &Path,
        context_path: &Path,
        state_path: &Path,
    ) -> SignalSingleConfig {
        SignalSingleConfig {
            tree_path: tree_path.to_path_buf(),
            primary_path: primary_path.to_path_buf(),
            context_path: context_path.to_path_buf(),
            news_path: None,
            aux_paths: vec![],
            window: 100,
            warmup: 5,
            cost_bps: 10.0,
            soft: false,
            state_path: state_path.to_path_buf(),
        }
    }

    // ── Step 2: 黄金不变量测试 ────────────────────────────────────────────────

    /// 全量 == 两段增量（split==full invariant）。
    /// k 切分点 1：warmup+3；k 切分点 2：len-5。
    #[tokio::test]
    async fn golden_invariant_split_equals_full() {
        let tree_f = write_file(&enter_hold_exit_tree(), ".yaml");
        let primary_csv = gen_primary_csv();
        let context_csv = gen_context_csv();
        let primary_f = write_file(&primary_csv, ".csv");
        let context_f = write_file(&context_csv, ".csv");

        // 全量 primary 的 bars
        let all_bars: Vec<&str> = primary_csv.lines().collect();
        // header + all data
        let total_data_lines = all_bars.len() - 1; // 不含 header
        // warmup=5, len=total_data_lines
        let len = total_data_lines;
        let warmup = 5;

        for k in [warmup + 3, len - 5] {
            let tmp_dir = tempfile::tempdir().unwrap();
            // ── A：全量 fresh ────────────────────────────────────────────────
            let state_a_path = tmp_dir.path().join("state_a.json");
            let cfg_a = make_cfg(
                tree_f.path(),
                primary_f.path(),
                context_f.path(),
                &state_a_path,
            );
            let (_sig_a, state_a) = run_signal_single(&cfg_a, &LlmEvaluator::Disabled)
                .await
                .unwrap();

            // ── B：前 k bar（header + k 行数据）→ state_b1 ──────────────────
            let partial_lines: Vec<&str> = std::iter::once(all_bars[0])
                .chain(all_bars[1..=k].iter().copied())
                .collect();
            let partial_csv = partial_lines.join("\n") + "\n";
            let partial_f = write_file(&partial_csv, ".csv");

            let state_b1_path = tmp_dir.path().join("state_b1.json");
            let cfg_b1 = make_cfg(
                tree_f.path(),
                partial_f.path(),
                context_f.path(),
                &state_b1_path,
            );
            let (_sig_b1, state_b1) = run_signal_single(&cfg_b1, &LlmEvaluator::Disabled)
                .await
                .unwrap();

            // ── B2：从 state_b1 全量跑 ──────────────────────────────────────
            let state_b2_path = tmp_dir.path().join("state_b2.json");
            write_paper_state(&state_b2_path, &state_b1).unwrap();

            let cfg_b2 = make_cfg(
                tree_f.path(),
                primary_f.path(),
                context_f.path(),
                &state_b2_path,
            );
            let (_sig_b2, state_b2) = run_signal_single(&cfg_b2, &LlmEvaluator::Disabled)
                .await
                .unwrap();

            // ── 断言 state_a == state_b2（逐字段，serde_json::Value 相等）──
            let val_a = serde_json::to_value(&state_a).unwrap();
            let val_b2 = serde_json::to_value(&state_b2).unwrap();
            assert_eq!(
                val_a, val_b2,
                "split==full invariant FAILED at k={k}: state_a != state_b2\nstate_a={val_a}\nstate_b2={val_b2}"
            );
        }
    }

    /// 幂等性：以 state_a 再跑全量 → bars_replayed==0，state 不变，信号同 t 同 target。
    #[tokio::test]
    async fn idempotent_replay() {
        let tree_f = write_file(&enter_hold_exit_tree(), ".yaml");
        let primary_f = write_file(&gen_primary_csv(), ".csv");
        let context_f = write_file(&gen_context_csv(), ".csv");

        let tmp_dir = tempfile::tempdir().unwrap();
        let state_path = tmp_dir.path().join("state.json");

        // 第一次：全量 fresh
        let cfg = make_cfg(tree_f.path(), primary_f.path(), context_f.path(), &state_path);
        let (sig1, state1) = run_signal_single(&cfg, &LlmEvaluator::Disabled)
            .await
            .unwrap();

        // 保存 state1，再跑一次
        write_paper_state(&state_path, &state1).unwrap();
        let (sig2, state2) = run_signal_single(&cfg, &LlmEvaluator::Disabled)
            .await
            .unwrap();

        assert_eq!(
            sig2.paper.bars_replayed, 0,
            "idempotent: second run must replay 0 bars"
        );
        assert_eq!(
            sig1.t, sig2.t,
            "idempotent: signal time must not change"
        );
        assert_eq!(
            sig1.target, sig2.target,
            "idempotent: signal target must not change"
        );

        let val1 = serde_json::to_value(&state1).unwrap();
        let val2 = serde_json::to_value(&state2).unwrap();
        assert_eq!(val1, val2, "idempotent: state must not change on second run");
    }

    // ── Step 3: 悬挂风控测试 ─────────────────────────────────────────────────

    /// 入场后末 bar 大幅浮亏（> stop_loss）→ 悬挂 reason=="stop"，target==0.0。
    /// 并验证 state.account 与重放后（未记账悬挂）一致。
    #[tokio::test]
    async fn pending_decision_stop_loss_fires() {
        // 构造 primary：稳定上行入场，末 bar 大幅下跌（超 5% 止损）。
        // warmup=5, 前 10 bar 上行（入场），bar 10 大幅下跌。
        // 12 bars total: warmup=5, bar5=entry-decision, bar 6..=10（5 根）=hold, bar11=hang(crash)
        // Use clean per-minute times spread across 3 days.
        let mut rows = vec!["time,open,high,low,close,volume".to_string()];
        // bar 0..5: 上行预热（day 1）
        for i in 0..6usize {
            let p = 10.0 + 0.1 * i as f64;
            rows.push(format!("2024-01-02 10:{:02}:00,{p},{p},{p},{p},1000", i));
        }
        // bar 6..10: 继续上行，会入场（day 2）
        for i in 0..5usize {
            let p = 10.6 + 0.1 * i as f64; // bar6=10.6..bar10=11.0
            rows.push(format!("2024-01-03 10:{:02}:00,{p},{p},{p},{p},1000", i));
        }
        // bar 11 (悬挂决策 bar, day 3)：大幅下跌。
        // 入场于 bar5 决策，执行于 bar6.open=10.6，entry_price=10.6。
        // crash close = 10.6 * (1 - 0.10) = 9.54，unreal_pnl = -0.10 < -stop_loss=0.05。
        let entry_px = 10.6_f64;
        let crash_close = entry_px * (1.0 - 0.10);
        rows.push(format!(
            "2024-01-04 10:00:00,{crash_close},{crash_close},{crash_close},{crash_close},1000"
        ));
        let primary_csv = rows.join("\n") + "\n";

        let context_csv = String::from(
            "time,open,high,low,close,volume\n\
             2024-01-02 10:30:00,10.0,10.0,10.0,10.0,1\n\
             2024-01-03 10:30:00,10.2,10.2,10.2,10.2,1\n\
             2024-01-04 10:30:00,10.3,10.3,10.3,10.3,1\n",
        );

        let tree_f = write_file(&enter_hold_stop_tree(), ".yaml");
        let primary_f = write_file(&primary_csv, ".csv");
        let context_f = write_file(&context_csv, ".csv");
        let tmp_dir = tempfile::tempdir().unwrap();
        let state_path = tmp_dir.path().join("state.json");

        let cfg = SignalSingleConfig {
            tree_path: tree_f.path().to_path_buf(),
            primary_path: primary_f.path().to_path_buf(),
            context_path: context_f.path().to_path_buf(),
            news_path: None,
            aux_paths: vec![],
            window: 100,
            warmup: 5,
            cost_bps: 10.0,
            soft: false,
            state_path,
        };

        let (sig, state) = run_signal_single(&cfg, &LlmEvaluator::Disabled)
            .await
            .unwrap();

        // 悬挂决策应触发止损
        assert_eq!(sig.reason, "stop", "pending decision: reason must be 'stop' on crash bar");
        assert_eq!(sig.target, 0.0, "pending decision: target must be 0.0 on stop");

        // 悬挂决策未记账：state.last_time == time_{len-2}（最后可记账决策 bar）
        // 不要断言 last_time 具体值——只要验证 account 与重放后一致（悬挂未改变 acc）
        let acc_after = &state.account;
        // 重放后账户应仍持仓（悬挂未执行）
        assert!(
            acc_after.pos.abs() > EPS,
            "pending decision must NOT modify account: pos should still be nonzero"
        );
    }

    // ── Step 3b: state 坏文件校验 ─────────────────────────────────────────────

    #[test]
    fn corrupt_state_returns_err() {
        let f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        std::fs::write(f.path(), b"not json at all{{{").unwrap();
        let err = read_paper_state(f.path(), "any_tree").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("corrupt"),
            "corrupt state must mention 'corrupt', got: {msg}"
        );

        // 空文件应被拒绝（不静默返回 None）
        let f_empty = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        std::fs::write(f_empty.path(), "").unwrap();
        let err_empty = read_paper_state(f_empty.path(), "any_tree").unwrap_err();
        let msg_empty = err_empty.to_string();
        assert!(
            msg_empty.contains("corrupt"),
            "empty state file must be treated as corrupt, got: {msg_empty}"
        );
    }

    #[test]
    fn version_mismatch_returns_err() {
        let st = PaperState {
            version: 999,
            tree_name: "t".to_string(),
            last_time: None,
            account: SimAccount::default().snapshot(),
        };
        let f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        write_paper_state(f.path(), &st).unwrap();
        let err = read_paper_state(f.path(), "t").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("version") || msg.contains("999"),
            "version mismatch must mention version, got: {msg}"
        );
    }

    #[test]
    fn tree_name_mismatch_returns_err() {
        let st = PaperState {
            version: STATE_VERSION,
            tree_name: "tree_a".to_string(),
            last_time: None,
            account: SimAccount::default().snapshot(),
        };
        let f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        write_paper_state(f.path(), &st).unwrap();
        let err = read_paper_state(f.path(), "tree_b").unwrap_err();
        assert!(
            err.to_string().contains("tree_a") || err.to_string().contains("tree_b"),
            "tree_name mismatch must mention names, got: {err}"
        );
    }

    #[test]
    fn missing_state_file_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nonexistent.json");
        let result = read_paper_state(&path, "any").unwrap();
        assert!(result.is_none(), "missing file must return None");
    }

    // ── Step 1: Portfolio 类型 + IO ────────────────────────────────────────────

    #[test]
    fn holdings_state_io_roundtrip() {
        let st = HoldingsState {
            version: 1,
            tree_name: "test_tree".to_string(),
            last_time: Some(NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(10, 0, 0).unwrap()),
            holdings: BTreeMap::from([
                ("A".to_string(), 0.5),
                ("B".to_string(), 0.5),
            ]),
        };
        let f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        write_holdings_state(f.path(), &st).unwrap();
        let loaded = read_holdings_state(f.path(), "test_tree")
            .unwrap()
            .unwrap();
        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.tree_name, "test_tree");
        assert_eq!(loaded.holdings.get("A"), Some(&0.5));
        assert_eq!(loaded.holdings.get("B"), Some(&0.5));
    }

    #[test]
    fn holdings_state_version_mismatch() {
        let st = HoldingsState {
            version: 2,
            tree_name: "test_tree".to_string(),
            last_time: None,
            holdings: BTreeMap::new(),
        };
        let f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        write_holdings_state(f.path(), &st).unwrap();
        let err = read_holdings_state(f.path(), "test_tree").unwrap_err();
        assert!(
            err.to_string().contains("version"),
            "version mismatch must mention 'version', got: {err}"
        );
    }

    #[test]
    fn holdings_state_tree_name_mismatch() {
        let st = HoldingsState {
            version: 1,
            tree_name: "tree_a".to_string(),
            last_time: None,
            holdings: BTreeMap::new(),
        };
        let f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        write_holdings_state(f.path(), &st).unwrap();
        let err = read_holdings_state(f.path(), "tree_b").unwrap_err();
        assert!(
            err.to_string().contains("tree_a") || err.to_string().contains("tree_b"),
            "tree_name mismatch must mention names, got: {err}"
        );
    }

    #[test]
    fn missing_holdings_state_file_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nonexistent.json");
        let result = read_holdings_state(&path, "any").unwrap();
        assert!(result.is_none(), "missing file must return None");
    }

    // ── Step 2: Portfolio 信号生成 ──────────────────────────────────────────

    /// 生成四叉树：根据 close 值路由到四条互不相交的路径。
    /// 阈值：10.15 / 10.25 / 10.35（三个分支，依次比较）。
    /// - close < 10.15 → leaf_a（long, weight=0.9, score=0.9）
    /// - 10.15 ≤ close < 10.25 → leaf_c（long, weight=0.8, score=0.8）
    /// - 10.25 ≤ close < 10.35 → leaf_d（long, weight=0.7, score=0.7）
    /// - close ≥ 10.35 → leaf_b（**flat**, score=0 → select_top 的 score>0 过滤掉）
    ///
    /// 注意：meta 必须声明 stances: [long, flat] 才能在叶子里用 flat。
    fn four_way_tree() -> String {
        r#"
meta: { name: portfolio_test, forward_window: 1, stances: [long, flat] }
root: router
nodes:
  router:
    type: quant
    branches:
      - when: "close < 10.15"
        goto: leaf_a
        label: score_a
      - when: "close < 10.25"
        goto: leaf_c
        label: score_c
      - when: "close < 10.35"
        goto: leaf_d
        label: score_d
    default: { goto: leaf_b, label: score_b }
leaves:
  leaf_a: { stance: long, weight: 0.9 }
  leaf_b: { stance: flat }
  leaf_c: { stance: long, weight: 0.8 }
  leaf_d: { stance: long, weight: 0.7 }
"#
        .to_string()
    }

    /// 生成一致权重树（所有标的 score = 0.5）。
    fn uniform_tree() -> String {
        r#"
meta: { name: portfolio_test_uniform, forward_window: 1, stances: [long] }
root: router
nodes:
  router:
    type: quant
    branches: []
    default: { goto: leaf_long, label: uniform }
leaves:
  leaf_long: { stance: long, weight: 0.5 }
"#
        .to_string()
    }

    fn gen_bars_csv(start_day: u32, n_bars: usize) -> String {
        let mut s = String::from("time,open,high,low,close,volume\n");
        for i in 0..n_bars {
            let price = 10.0 + i as f64 * 0.1;
            let hour = 9 + (45 + i * 15) / 60;
            let minute = (45 + i * 15) % 60;
            s.push_str(&format!(
                "2024-01-{:02} {:02}:{:02}:00,{p},{p},{p},{p},1000\n",
                start_day,
                hour,
                minute,
                p = price
            ));
        }
        s
    }


    /// 四象限：旧持仓 {A:0.5, B:0.5} → 新目标 {A:1/3, C:1/3, D:1/3}（top=3）。
    /// 预期 trades：A=Adjust(0.5→1/3)、B=Sell(0.5→0)、C=Buy(0→1/3)、D=Buy(0→1/3)。
    ///
    /// 路由演算表（末收盘→命中条件→叶→得分）：
    /// - A：末收盘 10.05（< 10.15 ✓，余量 0.10）→ leaf_a → score=0.9（long 0.9）
    /// - B：末收盘 10.50（≥ 10.35，余量 0.15）→ leaf_b → score=0.0（flat）→ select_top score>0 过滤
    /// - C：末收盘 10.20（10.15 ≤ 10.20 < 10.25，余量 0.05/0.05）→ leaf_c → score=0.8（long 0.8）
    /// - D：末收盘 10.30（10.25 ≤ 10.30 < 10.35，余量 0.05/0.05）→ leaf_d → score=0.7（long 0.7）
    /// select_top(top=3) 过滤 B（score=0），余下 A/C/D 三者得分均为正，等权 1/3。
    #[tokio::test]
    async fn portfolio_four_quadrants() {
        let tree_f = write_file(&four_way_tree(), ".yaml");

        // A：末收盘 10.05 < 10.15 → leaf_a（score=0.9）。余量 0.10，远离阈值。
        // 固定末 bar 10.05；前 7 条是占位行情（路由仅看末 bar close）。
        let bars_a = {
            let mut s = String::from("time,open,high,low,close,volume\n");
            // 末收盘 10.05，前 7 条从 10.40 降到 10.10（随意，只有末 bar 参与路由）
            for i in 0..7usize {
                let p = 10.40 - 0.05 * i as f64;
                s.push_str(&format!(
                    "2024-01-02 {:02}:00:00,{p},{p},{p},{p},1000\n",
                    9 + i
                ));
            }
            // 末 bar：10.05（< 10.15，余量 0.10）
            s.push_str("2024-01-02 16:00:00,10.05,10.05,10.05,10.05,1000\n");
            s
        };

        // B：末收盘 10.50 ≥ 10.35 → leaf_b（flat, score=0）→ 被 select_top 过滤，余量 0.15。
        let bars_b = {
            let mut s = String::from("time,open,high,low,close,volume\n");
            for i in 0..7usize {
                let p = 10.10 + 0.05 * i as f64;
                s.push_str(&format!(
                    "2024-01-02 {:02}:00:00,{p},{p},{p},{p},1000\n",
                    9 + i
                ));
            }
            // 末 bar：10.50（> 10.35，余量 0.15）
            s.push_str("2024-01-02 16:00:00,10.50,10.50,10.50,10.50,1000\n");
            s
        };

        // C：末收盘 10.20（10.15 ≤ 10.20 < 10.25）→ leaf_c（score=0.8），余量 0.05/0.05。
        let bars_c = {
            let mut s = String::from("time,open,high,low,close,volume\n");
            for i in 0..7usize {
                let p = 10.50 - 0.05 * i as f64;
                s.push_str(&format!(
                    "2024-01-02 {:02}:00:00,{p},{p},{p},{p},1000\n",
                    9 + i
                ));
            }
            // 末 bar：10.20（10.15 ≤ 10.20 < 10.25）
            s.push_str("2024-01-02 16:00:00,10.20,10.20,10.20,10.20,1000\n");
            s
        };

        // D：末收盘 10.30（10.25 ≤ 10.30 < 10.35）→ leaf_d（score=0.7），余量 0.05/0.05。
        let bars_d = {
            let mut s = String::from("time,open,high,low,close,volume\n");
            for i in 0..7usize {
                let p = 10.60 - 0.05 * i as f64;
                s.push_str(&format!(
                    "2024-01-02 {:02}:00:00,{p},{p},{p},{p},1000\n",
                    9 + i
                ));
            }
            // 末 bar：10.30（10.25 ≤ 10.30 < 10.35）
            s.push_str("2024-01-02 16:00:00,10.30,10.30,10.30,10.30,1000\n");
            s
        };

        let tmp_dir = tempfile::tempdir().unwrap();
        let f_a = write_file(&bars_a, ".csv");
        let f_b = write_file(&bars_b, ".csv");
        let f_c = write_file(&bars_c, ".csv");
        let f_d = write_file(&bars_d, ".csv");

        let mut universe_content = String::from("symbol,primary\n");
        universe_content.push_str(&format!("A,{}\n", f_a.path().to_string_lossy()));
        universe_content.push_str(&format!("B,{}\n", f_b.path().to_string_lossy()));
        universe_content.push_str(&format!("C,{}\n", f_c.path().to_string_lossy()));
        universe_content.push_str(&format!("D,{}\n", f_d.path().to_string_lossy()));
        let universe_f = write_file(&universe_content, ".csv");

        let state_path = tmp_dir.path().join("state.json");

        // 第一轮：初始化，应生成 {A, C, D} 为目标（top=3）
        let cfg1 = SignalPortfolioConfig {
            tree_path: tree_f.path().to_path_buf(),
            universe_path: universe_f.path().to_path_buf(),
            top: 3,
            window: 100,
            warmup: 0,
            cost_bps: 0.0,
            soft: false,
            aux_paths: vec![],
            state_path: state_path.clone(),
        };

        let (sig1, _state1) = run_signal_portfolio(&cfg1, &LlmEvaluator::Disabled)
            .await
            .unwrap();

        // 验证初始信号
        // 得分：A=0.9、C=0.8、D=0.7（long，均为正）；B=0.0（flat，被 select_top score>0 过滤）。
        // top=3 → 选中 A/C/D；B 因 score=0 被过滤（不是 tie-break）。
        assert_eq!(sig1.n_fresh, 4); // 所有 4 个标的都有数据
        assert_eq!(sig1.targets.len(), 3); // 入选 A, C, D（B score=0 被过滤）
        let targets_1: BTreeMap<String, f64> = sig1.targets.iter().cloned().collect();
        assert!(targets_1.contains_key("A"));
        assert!(targets_1.contains_key("C"));
        assert!(targets_1.contains_key("D"));
        assert!(!targets_1.contains_key("B"));
        // 等权：每个 1/3
        for v in targets_1.values() {
            assert!((v - 1.0 / 3.0).abs() < EPS);
        }

        // 验证初始交易（从空旧持仓）
        // A, C, D 各为 Buy（空持仓→1/3）；B 不在旧持仓也不在目标，不出现在 trades
        assert_eq!(sig1.trades.len(), 3);
        let trades_1: Vec<_> = sig1.trades.iter().filter(|t| t.action != TradeAction::Hold).collect();
        assert_eq!(trades_1.len(), 3);
        for trade in &trades_1 {
            assert_eq!(trade.from_w, 0.0);
            assert!((trade.to_w - 1.0 / 3.0).abs() < EPS);
            assert!(matches!(trade.action, TradeAction::Buy));
        }

        // 设置旧状态：{A:0.5, B:0.5}
        let old_holdings = HoldingsState {
            version: 1,
            tree_name: "portfolio_test".to_string(),
            last_time: sig1.t.into(),
            holdings: BTreeMap::from([
                ("A".to_string(), 0.5),
                ("B".to_string(), 0.5),
            ]),
        };
        write_holdings_state(&state_path, &old_holdings).unwrap();

        // 第二轮：加载旧状态，生成新信号
        let (sig2, _state2) = run_signal_portfolio(&cfg1, &LlmEvaluator::Disabled)
            .await
            .unwrap();

        // 验证第二轮信号
        assert_eq!(sig2.targets.len(), 3);

        // 验证交易清单
        let trades_map: BTreeMap<String, TradeInstr> =
            sig2.trades.iter().map(|t| (t.symbol.clone(), t.clone())).collect();

        // A: 0.5 → 1/3 = Adjust
        let trade_a = &trades_map["A"];
        assert_eq!(trade_a.from_w, 0.5);
        assert!((trade_a.to_w - 1.0 / 3.0).abs() < EPS);
        assert_eq!(trade_a.action, TradeAction::Adjust);

        // B: 0.5 → 0 = Sell
        let trade_b = &trades_map["B"];
        assert_eq!(trade_b.from_w, 0.5);
        assert_eq!(trade_b.to_w, 0.0);
        assert_eq!(trade_b.action, TradeAction::Sell);

        // C: 0 → 1/3 = Buy
        let trade_c = &trades_map["C"];
        assert_eq!(trade_c.from_w, 0.0);
        assert!((trade_c.to_w - 1.0 / 3.0).abs() < EPS);
        assert_eq!(trade_c.action, TradeAction::Buy);

        // D: 0 → 1/3 = Buy
        let trade_d = &trades_map["D"];
        assert_eq!(trade_d.from_w, 0.0);
        assert!((trade_d.to_w - 1.0 / 3.0).abs() < EPS);
        assert_eq!(trade_d.action, TradeAction::Buy);

        // 验证交易按 symbol 字典序
        let symbols: Vec<_> = sig2.trades.iter().map(|t| t.symbol.as_str()).collect();
        assert_eq!(symbols, vec!["A", "B", "C", "D"]);
    }

    /// 全 Hold：持仓与新目标完全一致 → 全部 Hold。
    #[tokio::test]
    async fn portfolio_all_hold() {
        let tree_f = write_file(&uniform_tree(), ".yaml");

        // 生成 A, B 的行情（都有数据，uniform_tree 都得分 0.5）
        let bars_a = gen_bars_csv(2, 8);
        let bars_b = gen_bars_csv(2, 8);

        let f_a = write_file(&bars_a, ".csv");
        let f_b = write_file(&bars_b, ".csv");

        let mut universe_content = String::from("symbol,primary\n");
        universe_content.push_str(&format!("A,{}\n", f_a.path().to_string_lossy()));
        universe_content.push_str(&format!("B,{}\n", f_b.path().to_string_lossy()));
        let universe_f = write_file(&universe_content, ".csv");

        let tmp_dir = tempfile::tempdir().unwrap();
        let state_path = tmp_dir.path().join("state.json");

        let cfg = SignalPortfolioConfig {
            tree_path: tree_f.path().to_path_buf(),
            universe_path: universe_f.path().to_string_lossy().to_string().into(),
            // top=5 > 入选数（2）：验证"入选数 < top 时等权 = 1/入选数"分支。
            top: 5,
            window: 100,
            warmup: 0,
            cost_bps: 0.0,
            soft: false,
            aux_paths: vec![],
            state_path: state_path.clone(),
        };

        // 第一轮：生成 {A:0.5, B:0.5} 目标
        let (sig1, _state1) = run_signal_portfolio(&cfg, &LlmEvaluator::Disabled)
            .await
            .unwrap();

        let targets_1: BTreeMap<String, f64> = sig1.targets.iter().cloned().collect();
        assert_eq!(targets_1.len(), 2);

        // 第二轮：设置旧状态 = 新目标
        let old_state = HoldingsState {
            version: 1,
            tree_name: "portfolio_test_uniform".to_string(),
            last_time: sig1.t.into(),
            holdings: targets_1.clone(),
        };
        write_holdings_state(&state_path, &old_state).unwrap();

        let (sig2, _state2) = run_signal_portfolio(&cfg, &LlmEvaluator::Disabled)
            .await
            .unwrap();

        // 验证所有交易都是 Hold
        for trade in &sig2.trades {
            assert_eq!(trade.action, TradeAction::Hold, "trade for {} should be Hold", trade.symbol);
        }
    }

    /// 新鲜度检查：A/B/C 新鲜（末 bar 时间同 t_last），D 不新鲜（末 bar 时间早于 t_last）。
    /// D 因不新鲜被过滤出候选（score None），不参与目标权重计算。
    /// 路由演算表（末收盘→条件→叶→得分）：
    /// - A/B/C day 2 各 8 根 bar，末收盘 10.05（< 10.15 → leaf_a → score=0.9）
    /// - D day 1 仅 1 根 bar（不新鲜，时间早于 t_last=2024-01-03）
    /// 验证：n_fresh=3，targets 仅含 A/B/C，各权重 1/3，D 不在 targets，trades 无 D 的 Buy。
    #[tokio::test]
    async fn portfolio_freshness_check() {
        let tree_f = write_file(&four_way_tree(), ".yaml");

        // A/B/C: day 2 各 8 根 bar，末收盘 10.05（< 10.15 → leaf_a → score=0.9）
        let bars_abc = {
            let mut s = String::from("time,open,high,low,close,volume\n");
            for i in 0..8usize {
                let p = 10.05;
                let hour = 9 + (45 + i * 15) / 60;
                let minute = (45 + i * 15) % 60;
                s.push_str(&format!(
                    "2024-01-03 {:02}:{:02}:00,{p},{p},{p},{p},1000\n",
                    hour,
                    minute
                ));
            }
            s
        };

        // D: day 1 仅 1 根 bar（stale，时间早于 t_last）
        let bars_d = String::from(
            "time,open,high,low,close,volume\n\
             2024-01-02 10:00:00,10.05,10.05,10.05,10.05,1000\n"
        );

        let f_a = write_file(&bars_abc, ".csv");
        let f_b = write_file(&bars_abc, ".csv");
        let f_c = write_file(&bars_abc, ".csv");
        let f_d = write_file(&bars_d, ".csv");

        let mut universe_content = String::from("symbol,primary\n");
        universe_content.push_str(&format!("A,{}\n", f_a.path().to_string_lossy()));
        universe_content.push_str(&format!("B,{}\n", f_b.path().to_string_lossy()));
        universe_content.push_str(&format!("C,{}\n", f_c.path().to_string_lossy()));
        universe_content.push_str(&format!("D,{}\n", f_d.path().to_string_lossy()));
        let universe_f = write_file(&universe_content, ".csv");

        let tmp_dir = tempfile::tempdir().unwrap();
        let state_path = tmp_dir.path().join("state.json");

        let cfg = SignalPortfolioConfig {
            tree_path: tree_f.path().to_path_buf(),
            universe_path: universe_f.path().to_string_lossy().to_string().into(),
            top: 3,
            window: 100,
            warmup: 0,
            cost_bps: 0.0,
            soft: false,
            aux_paths: vec![],
            state_path: state_path.clone(),
        };

        let (sig, _state) = run_signal_portfolio(&cfg, &LlmEvaluator::Disabled)
            .await
            .unwrap();

        // 验证新鲜度：A/B/C 新鲜（各有末 bar），D 不新鲜（末 bar 时间早）
        assert_eq!(sig.n_fresh, 3, "n_fresh must be 3 (only A, B, C are fresh)");

        // 验证目标权重
        assert_eq!(sig.targets.len(), 3, "exactly 3 targets selected (A, B, C)");
        let targets_map: BTreeMap<String, f64> = sig.targets.iter().cloned().collect();
        assert!(targets_map.contains_key("A"));
        assert!(targets_map.contains_key("B"));
        assert!(targets_map.contains_key("C"));
        assert!(!targets_map.contains_key("D"), "stale symbol D must not appear in targets");

        // 验证权重（等权 1/3，浮点容差 1e-12）
        for (sym, w) in &targets_map {
            assert!((w - 1.0 / 3.0).abs() < EPS,
                "{sym} weight {w} should be 1/3, margin {}", (w - 1.0 / 3.0).abs());
        }

        // 验证交易清单：D 不出现 Buy
        assert!(
            sig.trades.iter().all(|t| !(t.symbol == "D" && t.action == TradeAction::Buy)),
            "stale symbol D must not appear as a Buy trade"
        );
    }

    // ── holdings_state corrupt 校验 ──────────────────────────────────────────

    #[test]
    fn holdings_state_corrupt_returns_err() {
        // 非法 JSON → Err，消息含 "corrupt"
        let f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        std::fs::write(f.path(), b"{not json").unwrap();
        let err = read_holdings_state(f.path(), "any_tree").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("corrupt"),
            "corrupt holdings state must mention 'corrupt', got: {msg}"
        );

        // 空文件也应被拒绝（不静默返回 None）
        let f_empty = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        std::fs::write(f_empty.path(), "").unwrap();
        let err_empty = read_holdings_state(f_empty.path(), "any_tree").unwrap_err();
        let msg_empty = err_empty.to_string();
        assert!(
            msg_empty.contains("corrupt"),
            "empty holdings state file must be treated as corrupt, got: {msg_empty}"
        );
    }

    #[test]
    fn write_paper_state_is_atomic_and_overwrites() {
        let td = tempfile::tempdir().unwrap();
        let path = td.path().join("st.json");
        let tree_name = "t".to_string();
        let mk = |nav: f64| {
            let mut acc = SimAccount::default();
            acc.nav = nav;
            PaperState {
                version: 1,
                tree_name: tree_name.clone(),
                last_time: None,
                account: acc.snapshot(),
            }
        };
        write_paper_state(&path, &mk(1.0)).unwrap();
        write_paper_state(&path, &mk(1.5)).unwrap(); // 覆盖既有文件(Windows rename 替换语义)
        let back = read_paper_state(&path, &tree_name).unwrap().unwrap();
        assert!((back.account.nav - 1.5).abs() < 1e-12);
        // 临时文件不得残留
        let leftovers: Vec<_> = std::fs::read_dir(td.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|x| x == "tmp").unwrap_or(false))
            .collect();
        assert!(leftovers.is_empty(), "tmp leftover: {:?}", leftovers);
    }

    #[test]
    fn write_holdings_state_is_atomic_and_overwrites() {
        let td = tempfile::tempdir().unwrap();
        let path = td.path().join("h.json");
        let mk = |w: f64| {
            let mut holdings = std::collections::BTreeMap::new();
            holdings.insert("sh600000".to_string(), w);
            HoldingsState { version: 1, tree_name: "t".into(), last_time: None, holdings }
        };
        write_holdings_state(&path, &mk(0.5)).unwrap();
        write_holdings_state(&path, &mk(1.0)).unwrap();
        let back = read_holdings_state(&path, "t").unwrap().unwrap();
        assert!((back.holdings["sh600000"] - 1.0).abs() < 1e-12);
        let leftovers: Vec<_> = std::fs::read_dir(td.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|x| x == "tmp").unwrap_or(false))
            .collect();
        assert!(leftovers.is_empty(), "tmp leftover: {:?}", leftovers);
    }
}
