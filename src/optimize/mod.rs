pub mod grid;

use crate::backtest::costs::CostModel;
use crate::backtest::forward_return::forward_return;
use crate::backtest::sim::{finalize, sim_step, SimAccount};
use crate::backtest::soft::{score_soft, SoftScore};
use crate::data::aux_table::AuxTable;
use crate::data::bar::Bar;
use crate::data::news::NewsRecord;
use crate::eval::llm::LlmEvaluator;
use crate::features::context::{build_context, SimState};
use crate::tree::loader::{Node, Tree};
use crate::tree::schema::Stance;
use crate::{Error, Result};
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// 目标函数口径：打分硬 / 打分软 / 顺序模拟。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectiveMode {
    /// active（非 Flat）叶加权 net 均值（与 runner eval_point 同口径）。
    ScoreHard,
    /// engaged（engaged>0）点 expected_net 均值（与 run_soft 同口径）。
    ScoreSoft,
    /// 顺序 sim → Sharpe（跨度不足 30 天退化为 total_return）；nav < 2 点 → None。
    Sim,
}

/// 评估共享数据（一次加载，多组合复用）。
pub struct EvalData<'a> {
    /// 主行情序列（用于前瞻收益 / sim 执行）。
    pub primary: &'a [Bar],
    /// 上下文行情序列（DSL context 窗口）。
    pub context: &'a [Bar],
    /// 新闻记录（LLM 节点 inputs 引用）。
    pub news: &'a [NewsRecord],
    /// 外部 aux 序列。
    pub aux: &'a BTreeMap<String, AuxTable>,
    /// 每次评估时回看的窗口长度（bar 数）。
    pub window: usize,
    /// 成本模型。
    pub costs: CostModel,
}

const EPS: f64 = 1e-12;

/// 在决策索引范围 `range` 上评估一棵树的目标值。
/// 无可评估点 → Ok(None)；Sim 模式 nav < 2 点 → Ok(None)。
pub async fn evaluate(
    tree: &Tree,
    data: &EvalData<'_>,
    llm: &LlmEvaluator,
    range: std::ops::Range<usize>,
    mode: ObjectiveMode,
) -> Result<Option<f64>> {
    match mode {
        ObjectiveMode::ScoreHard => evaluate_score_hard(tree, data, llm, range).await,
        ObjectiveMode::ScoreSoft => evaluate_score_soft(tree, data, llm, range).await,
        ObjectiveMode::Sim => evaluate_sim(tree, data, llm, range).await,
    }
}

// ── ScoreHard ────────────────────────────────────────────────────────────────

async fn evaluate_score_hard(
    tree: &Tree,
    data: &EvalData<'_>,
    llm: &LlmEvaluator,
    range: std::ops::Range<usize>,
) -> Result<Option<f64>> {
    let fw = tree.meta.forward_window;
    let mut nets: Vec<f64> = Vec::new();

    for i in range {
        let t = data.primary[i].time;
        let ctx = build_context(data.primary, data.context, data.news, data.aux, None, t, data.window);
        let trace = crate::engine::traversal::traverse(tree, &ctx, llm).await?;

        // Skip flat stances — only "active" points count
        if trace.stance == Stance::Flat {
            continue;
        }

        let leaf = match tree.leaves.get(&trace.leaf) {
            Some(l) => l,
            None => continue,
        };

        // forward_return per leaf horizon/weight, mirroring eval_point
        let fr = forward_return(data.primary, i, leaf.horizon, trace.stance, &data.costs)
            .or_else(|| forward_return(data.primary, i, fw, trace.stance, &data.costs));
        if let Some(f) = fr {
            nets.push(f.net * leaf.weight_at(&ctx));
        }
    }

    if nets.is_empty() {
        Ok(None)
    } else {
        Ok(Some(nets.iter().sum::<f64>() / nets.len() as f64))
    }
}

// ── ScoreSoft ────────────────────────────────────────────────────────────────

async fn evaluate_score_soft(
    tree: &Tree,
    data: &EvalData<'_>,
    llm: &LlmEvaluator,
    range: std::ops::Range<usize>,
) -> Result<Option<f64>> {
    let mut engaged_nets: Vec<f64> = Vec::new();

    for i in range {
        let t = data.primary[i].time;
        let ctx = build_context(data.primary, data.context, data.news, data.aux, None, t, data.window);
        let soft = crate::engine::soft::traverse_soft(tree, &ctx, llm).await?;
        let score: Option<SoftScore> = score_soft(&soft, tree, data.primary, i, &data.costs, &ctx);

        // Mirror run_soft walk_forward engaged filter: engaged > 0.0
        if let Some(s) = score
            && s.engaged > 0.0
        {
            engaged_nets.push(s.expected_net);
        }
    }

    if engaged_nets.is_empty() {
        Ok(None)
    } else {
        Ok(Some(engaged_nets.iter().sum::<f64>() / engaged_nets.len() as f64))
    }
}

// ── Sim ──────────────────────────────────────────────────────────────────────

async fn evaluate_sim(
    tree: &Tree,
    data: &EvalData<'_>,
    llm: &LlmEvaluator,
    range: std::ops::Range<usize>,
) -> Result<Option<f64>> {
    // rate = single-leg cost (round_trip / 2 / 10_000)
    let rate = data.costs.round_trip_bps / 2.0 / 10_000.0;

    let mut acc = SimAccount::default();
    // nav_series: (time, nav) — collect at each step + after finalize
    let mut nav_series: Vec<(chrono::NaiveDateTime, f64)> = Vec::new();

    // loop_end = range.end (caller ensures range.end <= primary.len()-1)
    for i in range.clone() {
        let close_i = data.primary[i].close;
        let open_next = data.primary[i + 1].open;
        let high_next = data.primary[i + 1].high;
        let low_next = data.primary[i + 1].low;
        let close_next = data.primary[i + 1].close;
        let t_next = data.primary[i + 1].time;

        // Build context gated at primary[i].time
        let mut ctx = build_context(
            data.primary,
            data.context,
            data.news,
            data.aux,
            None,
            data.primary[i].time,
            data.window,
        );

        // Inject SimState (mirror run_sim §3.1)
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

        // Risk overlay (mirror run_sim §3.2): stop → tp → max_hold → tree
        let (target, reason): (f64, &str) = if acc.pos.abs() > EPS {
            if let Some(risk) = &tree.risk {
                if risk.stop_loss.is_some_and(|sl| unreal_pnl <= -sl) {
                    (0.0, "stop")
                } else if risk.take_profit.is_some_and(|tp| unreal_pnl >= tp) {
                    (0.0, "tp")
                } else if risk.max_hold_bars.is_some_and(|mh| acc.bars_held >= mh) {
                    (0.0, "max_hold")
                } else {
                    tree_target_hard(tree, &ctx, llm).await?
                }
            } else {
                tree_target_hard(tree, &ctx, llm).await?
            }
        } else {
            tree_target_hard(tree, &ctx, llm).await?
        };

        // Execute sim_step
        sim_step(&mut acc, close_i, open_next, high_next, low_next, close_next, t_next, target, rate, reason);

        nav_series.push((data.primary[i].time, acc.nav));
    }

    // Finalize: liquidate any open position at end of range
    if let Some(last_bar) = data.primary.get(range.end) {
        finalize(&mut acc, last_bar.time, last_bar.close, rate);
        nav_series.push((last_bar.time, acc.nav));
    } else if let Some(last_bar) = data.primary.last() {
        // range.end == primary.len()-1 case — use the bar at range.end-1 (last iterated)
        if !range.is_empty() {
            let last_i = range.end - 1;
            if last_i < data.primary.len() {
                finalize(&mut acc, data.primary[last_i].time, data.primary[last_i].close, rate);
                nav_series.push((last_bar.time, acc.nav));
            }
        }
    }

    if nav_series.len() < 2 {
        return Ok(None);
    }

    // risk_metrics → sharpe if Some, else total_return
    let rm = crate::report::risk::risk_metrics(&nav_series, acc.max_drawdown);
    let objective = match rm {
        Some(ref r) if r.sharpe.is_some() => r.sharpe,
        _ => {
            // total_return = final_nav - 1
            let final_nav = nav_series.last().map(|(_, v)| *v).unwrap_or(1.0);
            Some(final_nav - 1.0)
        }
    };

    Ok(objective)
}

/// Hard-mode tree target: stance × weight → position direction.
async fn tree_target_hard(
    tree: &Tree,
    ctx: &crate::features::context::Context,
    llm: &LlmEvaluator,
) -> Result<(f64, &'static str)> {
    let trace = crate::engine::traversal::traverse(tree, ctx, llm).await?;
    let target = tree.leaves.get(&trace.leaf).map_or(0.0, |l| {
        let dir = match l.stance {
            Stance::Long => 1.0,
            Stance::Short => -1.0,
            Stance::Flat => 0.0,
        };
        dir * l.weight_at(ctx)
    });
    Ok((target, "tree"))
}

// ─────────────────────────────────────────────────────────────────────────────
// Report types
// ─────────────────────────────────────────────────────────────────────────────

/// Per-combo IS score (stored for top-5 reporting).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ComboScore {
    pub params: BTreeMap<String, f64>,
    pub objective: Option<f64>,
}

/// Per OS-fold result.
#[derive(Debug, Serialize, Deserialize)]
pub struct FoldResult {
    /// OS fold index (1-based, i.e. 2nd fold onward when 0-based j=1..K−1).
    pub fold: usize,
    pub is_from: NaiveDateTime,
    pub is_to: NaiveDateTime,
    pub os_from: NaiveDateTime,
    pub os_to: NaiveDateTime,
    pub best_params: Option<BTreeMap<String, f64>>,
    pub is_objective: Option<f64>,
    pub os_objective: Option<f64>,
    /// os/is degradation ratio; None when |is| <= 1e-12 or is < 0 or either is None.
    pub degradation: Option<f64>,
}

/// Per-parameter best-value sequence across OS folds.
#[derive(Debug, Serialize, Deserialize)]
pub struct ParamDrift {
    pub name: String,
    /// One entry per OS fold (None when that fold had no best).
    pub values: Vec<Option<f64>>,
    /// Unique f64 bit-patterns among the Some values.
    pub n_unique: usize,
}

/// 单条网格轴的内部最优分析结果（仅 --auto-extend 时填充）。
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AxisOutcome {
    pub name: String,
    /// 延伸后该轴实际候选值（升序）。
    pub final_values: Vec<f64>,
    /// 全样本最优在该轴的取值。
    pub best_value: Option<f64>,
    /// best 是否为内部最优（内点收敛 / IS 转劣确认峰值=true；达 N 步仍贴边=false）。
    pub interior: bool,
    /// 实际追加的延伸步数（0=无需延伸）。
    pub extended_steps: usize,
}

/// Full optimize report (serialized to JSON).
#[derive(Debug, Serialize, Deserialize)]
pub struct OptimizeReport {
    pub mode: String,
    pub objective_name: String,
    pub folds: usize,
    pub n_combos: usize,
    pub fold_results: Vec<FoldResult>,
    pub os_mean_objective: Option<f64>,
    pub full_sample_best: Option<ComboScore>,
    pub drift: Vec<ParamDrift>,
    /// IS top-5 per OS fold (each inner Vec len <= 5, descending IS objective).
    pub is_top5: Vec<Vec<ComboScore>>,
    /// 每条网格轴的内部最优分析（仅 --auto-extend；否则空）。
    #[serde(default)]
    pub axes: Vec<AxisOutcome>,
    /// 主数据标识（primary 路径字符串），eval 用作 symbol 标签。
    #[serde(default)]
    pub primary: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// OptimizeConfig
// ─────────────────────────────────────────────────────────────────────────────

pub struct OptimizeConfig {
    pub tree_path: PathBuf,
    pub primary_path: PathBuf,
    pub context_path: PathBuf,
    pub news_path: Option<PathBuf>,
    pub aux_paths: Vec<(String, PathBuf)>,
    pub window: usize,
    pub warmup: usize,
    pub cost_bps: f64,
    pub folds: usize,
    pub sim: bool,
    pub soft: bool,
    pub grids: Vec<String>,
    pub max_combos: usize,
    /// --auto-extend N：门槛④边界逃逸最大步数（0=关，行为冻结）。
    pub auto_extend: usize,
    pub out_path: PathBuf,
}

// ─────────────────────────────────────────────────────────────────────────────
// run_optimize
// ─────────────────────────────────────────────────────────────────────────────

/// Returns true iff any node in the tree is an LLM node.
fn tree_has_llm_nodes(tree: &Tree) -> bool {
    tree.nodes.values().any(|n| matches!(n, Node::Llm { .. }))
}

pub async fn run_optimize(cfg: &OptimizeConfig, llm: &LlmEvaluator) -> Result<OptimizeReport> {
    // ── Step 1: parse grid, expand combos, probe-load, warn LLM ──────────────
    let yaml_src = std::fs::read_to_string(&cfg.tree_path)?;

    let axes: Vec<grid::GridAxis> = cfg
        .grids
        .iter()
        .map(|s| grid::parse_grid_axis(s))
        .collect::<Result<Vec<_>>>()?;

    let combos = grid::expand_grid(&axes, cfg.max_combos)?;

    // Probe-load with first combo to catch unknown param names early.
    crate::tree::loader::load_tree_str_with_overrides(&yaml_src, &combos[0])?;

    // Warn once if tree has LLM nodes (cost across many combos may be high).
    {
        let probe_tree = crate::tree::loader::load_tree_str(&yaml_src)?;
        if tree_has_llm_nodes(&probe_tree) {
            eprintln!(
                "[rquant] optimize: tree has LLM nodes — costs may be high across {} combos × folds; LLM cache will be reused across combos.",
                combos.len()
            );
        }
    }

    // ── Step 2: load data ─────────────────────────────────────────────────────
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
    let costs = CostModel { round_trip_bps: cfg.cost_bps };

    // Determine eligible decision-index range.
    // ScoreHard/ScoreSoft: warmup..len; Sim: warmup..len-1
    let mode = if cfg.sim {
        ObjectiveMode::Sim
    } else if cfg.soft {
        ObjectiveMode::ScoreSoft
    } else {
        ObjectiveMode::ScoreHard
    };
    let range_start = cfg.warmup.min(primary.len());
    let range_end = if cfg.sim {
        if primary.is_empty() {
            0
        } else {
            primary.len() - 1
        }
    } else {
        primary.len()
    };

    let n_points = range_end.saturating_sub(range_start);
    let k = cfg.folds;

    if k < 2 {
        return Err(Error::Data(format!(
            "optimize: --folds must be >= 2, got {k}"
        )));
    }
    if n_points < k * 2 {
        return Err(Error::Data(format!(
            "optimize: only {n_points} eligible points (warmup..range_end) but need >= {} for {k} folds × 2",
            k * 2
        )));
    }

    // ── Step 3: fold boundaries — mirror walkforward index-split convention ───
    // fold j: [range_start + j*n_points/k .. range_start + (j+1)*n_points/k)
    let fold_starts: Vec<usize> = (0..=k)
        .map(|j| range_start + j * n_points / k)
        .collect();

    let (mode_str, obj_name) = match mode {
        ObjectiveMode::ScoreHard => ("score_hard", "active_mean_net"),
        ObjectiveMode::ScoreSoft => ("score_soft", "engaged_mean_expected_net"),
        ObjectiveMode::Sim => ("sim", "sharpe_or_total_return"),
    };

    let n_combos = combos.len();
    // Number of evaluate calls: (K-1) IS evals per combo + (K-1) OS evals for best + 1 full-sample per combo
    let os_fold_count = k - 1; // folds 1..K-1 (0-based)
    let eval_count = n_combos * (os_fold_count * 2 + 1);
    println!(
        "[rquant] optimize: mode={mode_str} objective={obj_name} combos={n_combos} folds={k} eval_runs={eval_count}"
    );

    let data = EvalData {
        primary: &primary,
        context: &context,
        news: &news,
        aux: &aux_tables,
        window: cfg.window,
        costs,
    };

    // ── Step 4: per OS-fold loop ───────────────────────────────────────────────
    // OS fold k (0-based j: 1..K): IS = fold_starts[0]..fold_starts[j], OS = fold_starts[j]..fold_starts[j+1]
    let mut fold_results: Vec<FoldResult> = Vec::new();
    let mut is_top5_all: Vec<Vec<ComboScore>> = Vec::new();
    let mut best_per_fold: Vec<Option<BTreeMap<String, f64>>> = Vec::new();

    for j in 1..k {
        let is_start = fold_starts[0];
        let is_end = fold_starts[j];
        let os_start = fold_starts[j];
        let os_end = fold_starts[j + 1];

        let is_from = primary[is_start].time;
        let is_to = primary[is_end - 1].time;
        let os_from = primary[os_start].time;
        let os_to = primary[os_end - 1].time;

        // Evaluate all combos on IS range
        let mut combo_scores: Vec<ComboScore> = Vec::with_capacity(n_combos);
        for combo in &combos {
            let tree = crate::tree::loader::load_tree_str_with_overrides(&yaml_src, combo)?;
            let obj = evaluate(&tree, &data, llm, is_start..is_end, mode).await?;
            combo_scores.push(ComboScore { params: combo.clone(), objective: obj });
        }

        // Find best combo: max objective (None → -∞); ties → grid-order first
        let best_idx = combo_scores
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| {
                let av = a.objective.unwrap_or(f64::NEG_INFINITY);
                let bv = b.objective.unwrap_or(f64::NEG_INFINITY);
                av.partial_cmp(&bv).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(i, _)| i);

        let best_score = best_idx.map(|i| &combo_scores[i]);
        let all_neg_inf = best_score
            .map(|s| s.objective.is_none())
            .unwrap_or(true)
            || best_score
                .map(|s| s.objective.unwrap_or(f64::NEG_INFINITY).is_infinite())
                .unwrap_or(true);

        let (best_params, is_objective) = if all_neg_inf {
            (None, None)
        } else {
            let bs = best_score.unwrap();
            (Some(bs.params.clone()), bs.objective)
        };

        // Evaluate best on OS range
        let os_objective = if let Some(ref bp) = best_params {
            let tree = crate::tree::loader::load_tree_str_with_overrides(&yaml_src, bp)?;
            evaluate(&tree, &data, llm, os_start..os_end, mode).await?
        } else {
            None
        };

        // Degradation: os/is, only when is > 1e-12
        let degradation = match (is_objective, os_objective) {
            (Some(is_v), Some(os_v)) if is_v > 1e-12 => Some(os_v / is_v),
            _ => None,
        };

        // Top-5: sort descending by IS objective (None → -∞)
        let mut sorted = combo_scores.clone();
        sorted.sort_by(|a, b| {
            let av = a.objective.unwrap_or(f64::NEG_INFINITY);
            let bv = b.objective.unwrap_or(f64::NEG_INFINITY);
            bv.partial_cmp(&av).unwrap_or(std::cmp::Ordering::Equal)
        });
        let top5: Vec<ComboScore> = sorted.into_iter().take(5).collect();

        fold_results.push(FoldResult {
            fold: j + 1, // 1-based (first OS fold = fold 2)
            is_from,
            is_to,
            os_from,
            os_to,
            best_params: best_params.clone(),
            is_objective,
            os_objective,
            degradation,
        });
        is_top5_all.push(top5);
        best_per_fold.push(best_params);
    }

    // ── Step 5: full-sample best ───────────────────────────────────────────────
    let full_range = range_start..range_end;
    let mut full_scores: Vec<ComboScore> = Vec::with_capacity(n_combos);
    for combo in &combos {
        let tree = crate::tree::loader::load_tree_str_with_overrides(&yaml_src, combo)?;
        let obj = evaluate(&tree, &data, llm, full_range.clone(), mode).await?;
        full_scores.push(ComboScore { params: combo.clone(), objective: obj });
    }
    let full_sample_best = full_scores
        .iter()
        .max_by(|a, b| {
            let av = a.objective.unwrap_or(f64::NEG_INFINITY);
            let bv = b.objective.unwrap_or(f64::NEG_INFINITY);
            av.partial_cmp(&bv).unwrap_or(std::cmp::Ordering::Equal)
        })
        .and_then(|s| {
            if s.objective.unwrap_or(f64::NEG_INFINITY).is_finite() {
                Some(s.clone())
            } else {
                None
            }
        });

    // ── Step 5b: auto-extend (gate-4 boundary escape; only when cfg.auto_extend > 0) ──
    // Control flow mirrors analyze_axis_interior exactly; this version uses .await
    // instead of a synchronous objective closure, so the two cannot be unified without
    // block_on (which is an anti-pattern inside an async context). The pure function
    // analyze_axis_interior covers the same logic for unit tests.
    let axis_outcomes: Vec<AxisOutcome> = if cfg.auto_extend > 0 {
        if let Some(best) = &full_sample_best {
            let mut outs: Vec<AxisOutcome> = Vec::with_capacity(axes.len());
            for ax in &axes {
                let best_on_axis = match best.params.get(&ax.name).copied() {
                    Some(v) if v.is_finite() => v,
                    _ => continue,
                };
                let mut av: Vec<f64> = ax.values.clone();
                av.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

                // Single-value axis: already interior.
                if av.len() < 2 {
                    outs.push(AxisOutcome {
                        name: ax.name.clone(),
                        final_values: av,
                        best_value: Some(best_on_axis),
                        interior: true,
                        extended_steps: 0,
                    });
                    continue;
                }

                let lo = av[0];
                let hi = *av.last().unwrap();
                let dir: i32 = if (best_on_axis - lo).abs() < 1e-9 {
                    -1
                } else if (best_on_axis - hi).abs() < 1e-9 {
                    1
                } else {
                    // Already interior.
                    outs.push(AxisOutcome {
                        name: ax.name.clone(),
                        final_values: av,
                        best_value: Some(best_on_axis),
                        interior: true,
                        extended_steps: 0,
                    });
                    continue;
                };

                let step = if dir < 0 { av[1] - av[0] } else { hi - av[av.len() - 2] };
                let mut cur = best_on_axis;
                let mut cur_obj = {
                    let mut combo = best.params.clone();
                    combo.insert(ax.name.clone(), cur);
                    match crate::tree::loader::load_tree_str_with_overrides(&yaml_src, &combo) {
                        Ok(tree) => evaluate(&tree, &data, llm, full_range.clone(), mode)
                            .await
                            .ok()
                            .flatten()
                            .unwrap_or(f64::NEG_INFINITY),
                        Err(_) => f64::NEG_INFINITY,
                    }
                };
                let mut steps = 0usize;
                let mut interior = false;

                while steps < cfg.auto_extend {
                    let cand = cur + dir as f64 * step;
                    let cand_obj = {
                        let mut combo = best.params.clone();
                        combo.insert(ax.name.clone(), cand);
                        match crate::tree::loader::load_tree_str_with_overrides(&yaml_src, &combo) {
                            Ok(tree) => evaluate(&tree, &data, llm, full_range.clone(), mode)
                                .await
                                .ok()
                                .flatten()
                                .unwrap_or(f64::NEG_INFINITY),
                            Err(_) => f64::NEG_INFINITY,
                        }
                    };
                    match av.binary_search_by(|v| {
                        v.partial_cmp(&cand).unwrap_or(std::cmp::Ordering::Equal)
                    }) {
                        Ok(_) => {}
                        Err(pos) => av.insert(pos, cand),
                    }
                    steps += 1;
                    if cand_obj <= cur_obj {
                        interior = true;
                        break;
                    }
                    cur = cand;
                    cur_obj = cand_obj;
                }

                outs.push(AxisOutcome {
                    name: ax.name.clone(),
                    final_values: av,
                    best_value: Some(cur),
                    interior,
                    extended_steps: steps,
                });
            }
            outs
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    // ── Step 6: drift ──────────────────────────────────────────────────────────
    let param_names: Vec<String> = axes.iter().map(|a| a.name.clone()).collect();
    let drift: Vec<ParamDrift> = param_names
        .iter()
        .map(|pname| {
            let values: Vec<Option<f64>> =
                best_per_fold.iter().map(|bp| bp.as_ref().and_then(|m| m.get(pname).copied())).collect();
            let n_unique = {
                let mut bits: Vec<u64> = values
                    .iter()
                    .filter_map(|v| v.map(|x| x.to_bits()))
                    .collect();
                bits.sort_unstable();
                bits.dedup();
                bits.len()
            };
            ParamDrift { name: pname.clone(), values, n_unique }
        })
        .collect();

    // ── Step 7: os_mean_objective ─────────────────────────────────────────────
    let os_some: Vec<f64> =
        fold_results.iter().filter_map(|fr| fr.os_objective).collect();
    let os_mean_objective = if os_some.is_empty() {
        None
    } else {
        Some(os_some.iter().sum::<f64>() / os_some.len() as f64)
    };

    // ── Step 8: write JSON and return ─────────────────────────────────────────
    let report = OptimizeReport {
        mode: mode_str.to_string(),
        objective_name: obj_name.to_string(),
        folds: k,
        n_combos,
        fold_results,
        os_mean_objective,
        full_sample_best,
        drift,
        is_top5: is_top5_all,
        axes: axis_outcomes,
        primary: cfg.primary_path.to_string_lossy().to_string(),
    };

    let json = serde_json::to_string_pretty(&report)?;
    std::fs::write(&cfg.out_path, json.as_bytes())?;

    Ok(report)
}

// ─────────────────────────────────────────────────────────────────────────────
// print_optimize_summary
// ─────────────────────────────────────────────────────────────────────────────

fn fmt_opt(v: Option<f64>) -> String {
    match v {
        Some(x) => format!("{x:.6}"),
        None => "—".to_string(),
    }
}

fn fmt_params(p: &Option<BTreeMap<String, f64>>) -> String {
    match p {
        None => "—".to_string(),
        Some(m) => m.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(" "),
    }
}

pub fn print_optimize_summary(report: &OptimizeReport) {
    println!(
        "\n=== optimize: mode={} objective={} combos={} folds={} ===",
        report.mode, report.objective_name, report.n_combos, report.folds
    );

    // Fold table
    println!(
        "\n{:<6} {:<20} {:<20} {:<20} {:<20} {:<14} {:<14} {:<12} Best params",
        "Fold", "IS from", "IS to", "OS from", "OS to", "IS obj", "OS obj", "Degrad"
    );
    println!("{}", "-".repeat(140));
    for fr in &report.fold_results {
        println!(
            "{:<6} {:<20} {:<20} {:<20} {:<20} {:<14} {:<14} {:<12} {}",
            fr.fold,
            fr.is_from.format("%Y-%m-%d %H:%M"),
            fr.is_to.format("%Y-%m-%d %H:%M"),
            fr.os_from.format("%Y-%m-%d %H:%M"),
            fr.os_to.format("%Y-%m-%d %H:%M"),
            fmt_opt(fr.is_objective),
            fmt_opt(fr.os_objective),
            fmt_opt(fr.degradation),
            fmt_params(&fr.best_params),
        );
    }

    // Drift table
    if !report.drift.is_empty() {
        println!("\n--- Parameter drift (best-param per OS fold) ---");
        println!("{:<20} {:<10} Values per fold", "Param", "n_unique");
        println!("{}", "-".repeat(80));
        for pd in &report.drift {
            let vals: Vec<String> = pd.values.iter().map(|v| fmt_opt(*v)).collect();
            println!("{:<20} {:<10} {}", pd.name, pd.n_unique, vals.join("  "));
        }
    }

    // Full-sample vs OS mean
    println!("\n--- Full-sample best vs OS-mean ---");
    match &report.full_sample_best {
        Some(fs) => println!(
            "full-sample best: {} (obj={})",
            fmt_params(&Some(fs.params.clone())),
            fmt_opt(fs.objective)
        ),
        None => println!("full-sample best: —"),
    }
    println!("OS-mean objective: {}", fmt_opt(report.os_mean_objective));

    // Per-fold IS top-5
    for (j, top5) in report.is_top5.iter().enumerate() {
        let fold_num = j + 2; // OS folds are 2-based
        println!("\n--- Fold {fold_num} IS top-5 ---");
        for (rank, cs) in top5.iter().enumerate() {
            println!(
                "  #{}: {} obj={}",
                rank + 1,
                fmt_params(&Some(cs.params.clone())),
                fmt_opt(cs.objective)
            );
        }
    }
    println!();
}

// ─────────────────────────────────────────────────────────────────────────────
// analyze_axis_interior — boundary-escape pure function
// ─────────────────────────────────────────────────────────────────────────────

/// Analyse whether the full-sample optimum on a single axis is an interior
/// maximum, extending the grid boundary when it is not.
///
/// `best_on_axis`: the current optimum value on this axis.
/// `objective(x)`: synchronous objective for value `x` (other params fixed).
/// `max_steps`: upper bound on extension steps.
///
/// Returns an `AxisOutcome` that records:
/// - `interior = true`  iff the peak is confirmed interior (either already
///   not on a boundary, or "one step outside turns worse").
/// - `interior = false` iff `max_steps` exhausted while still improving
///   (boundary artefact suspected).
///
/// This pure function is used in unit tests. The async version of the same
/// control flow lives inside `run_optimize` (uses `.await` instead of the
/// synchronous closure). `allow(dead_code)` suppresses the "never used"
/// warning that arises because the function is only called from `#[test]`.
#[allow(dead_code)]
fn analyze_axis_interior(
    axis: &grid::GridAxis,
    best_on_axis: f64,
    max_steps: usize,
    objective: &dyn Fn(f64) -> f64,
) -> AxisOutcome {
    let mut values: Vec<f64> = axis.values.clone();
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    // Single-value axis: not a real search dimension — treat as interior.
    if values.len() < 2 {
        return AxisOutcome {
            name: axis.name.clone(),
            final_values: values,
            best_value: Some(best_on_axis),
            interior: true,
            extended_steps: 0,
        };
    }

    let lo = values[0];
    let hi = *values.last().unwrap();

    // Determine extension direction. If best is not on either boundary, it is
    // already an interior optimum — return immediately.
    let dir: i32 = if (best_on_axis - lo).abs() < 1e-9 {
        -1 // best on lower boundary → extend downward
    } else if (best_on_axis - hi).abs() < 1e-9 {
        1 // best on upper boundary → extend upward
    } else {
        return AxisOutcome {
            name: axis.name.clone(),
            final_values: values,
            best_value: Some(best_on_axis),
            interior: true,
            extended_steps: 0,
        };
    };

    // Step size = the local grid spacing at the boundary being extended.
    let step = if dir < 0 {
        values[1] - values[0] // downward: use gap between first two points
    } else {
        hi - values[values.len() - 2] // upward: use gap between last two points
    };

    let mut cur = best_on_axis;
    let mut cur_obj = objective(cur);
    let mut steps = 0usize;
    let mut interior = false;

    while steps < max_steps {
        let cand = cur + dir as f64 * step;
        let cand_obj = objective(cand);

        // Insert candidate into the sorted values list (skip if already present).
        match values.binary_search_by(|v| v.partial_cmp(&cand).unwrap_or(std::cmp::Ordering::Equal)) {
            Ok(_) => {}
            Err(pos) => values.insert(pos, cand),
        }
        steps += 1;

        if cand_obj <= cur_obj {
            // One step beyond the boundary is worse → cur is the peak → interior.
            interior = true;
            break;
        }

        // Still improving: advance cur and keep searching outward.
        cur = cand;
        cur_obj = cand_obj;
    }
    // If the loop exhausted max_steps while still improving, interior remains
    // false, signalling a suspected boundary artefact.

    AxisOutcome {
        name: axis.name.clone(),
        final_values: values,
        best_value: Some(cur),
        interior,
        extended_steps: steps,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::bar::Bar;
    use crate::tree::loader::load_tree_str;
    use chrono::NaiveDateTime;

    #[test]
    fn optimize_report_new_fields_default_empty() {
        // 旧 JSON（无 axes/primary）反序列化 → 字段取默认（serde default）
        let json = r#"{
            "mode":"sim","objective_name":"sharpe_or_total_return","folds":4,"n_combos":12,
            "fold_results":[],"os_mean_objective":null,"full_sample_best":null,"drift":[],"is_top5":[]
        }"#;
        let r: OptimizeReport = serde_json::from_str(json).unwrap();
        assert!(r.axes.is_empty(), "axes 默认空");
        assert_eq!(r.primary, "", "primary 默认空串");
    }

    #[test]
    fn axis_outcome_roundtrips() {
        let a = AxisOutcome {
            name: "n_s".into(),
            final_values: vec![40.0, 55.0, 60.0, 90.0],
            best_value: Some(55.0),
            interior: true,
            extended_steps: 0,
        };
        let s = serde_json::to_string(&a).unwrap();
        let b: AxisOutcome = serde_json::from_str(&s).unwrap();
        assert_eq!(b.best_value, Some(55.0));
        assert!(b.interior);
    }

    fn bar(t: &str, open: f64, close: f64) -> Bar {
        Bar {
            time: NaiveDateTime::parse_from_str(t, "%Y-%m-%d %H:%M:%S").unwrap(),
            open,
            high: open.max(close),
            low: open.min(close),
            close,
            volume: 1.0,
        }
    }

    /// Always-long tree: horizon=1, weight=1.0; close>0 → long (always true for our data).
    const ALWAYS_LONG_TREE: &str = r#"
meta: { name: always_long, forward_window: 1, stances: [long, flat] }
root: gate
nodes:
  gate:
    type: quant
    branches:
      - when: "close > 0"
        goto: leaf_l
        label: up
    default: { goto: leaf_f, label: flat }
leaves:
  leaf_l: { stance: long, horizon: 1, weight: 1.0 }
  leaf_f: { stance: flat }
"#;

    /// Always-flat tree: every decision routes to flat.
    const ALWAYS_FLAT_TREE: &str = r#"
meta: { name: always_flat, forward_window: 1, stances: [long, flat] }
root: gate
nodes:
  gate:
    type: quant
    branches:
      - when: "close > 1000000"
        goto: leaf_l
        label: impossible
    default: { goto: leaf_f, label: flat }
leaves:
  leaf_l: { stance: long }
  leaf_f: { stance: flat }
"#;

    /// Rising bars spanning multiple days (so T+1 constraint is satisfied).
    /// Prices: 10 → 11 → 12 → 13 → 14 → 15 (open = close of previous).
    fn rising_bars() -> Vec<Bar> {
        vec![
            bar("2024-01-02 09:45:00", 10.0, 10.5),
            bar("2024-01-02 15:00:00", 10.5, 11.0),
            bar("2024-01-03 09:45:00", 11.0, 11.5),
            bar("2024-01-03 15:00:00", 11.5, 12.0),
            bar("2024-01-04 09:45:00", 12.0, 12.5),
            bar("2024-01-04 15:00:00", 12.5, 13.0),
            bar("2024-01-05 09:45:00", 13.0, 13.5),
            bar("2024-01-05 15:00:00", 13.5, 14.0),
        ]
    }

    fn make_eval_data<'a>(
        primary: &'a [Bar],
        context: &'a [Bar],
        news: &'a [NewsRecord],
        aux: &'a BTreeMap<String, AuxTable>,
        costs: CostModel,
    ) -> EvalData<'a> {
        EvalData { primary, context, news, aux, window: 10, costs }
    }

    #[tokio::test]
    async fn score_hard_known_value_vs_manual_mean() {
        let tree = load_tree_str(ALWAYS_LONG_TREE).unwrap();
        let bars = rising_bars();
        let news: Vec<NewsRecord> = Vec::new();
        let aux: BTreeMap<String, AuxTable> = BTreeMap::new();
        let costs = CostModel { round_trip_bps: 10.0 };
        let data = make_eval_data(&bars, &bars, &news, &aux, costs);
        let llm = LlmEvaluator::Disabled;

        // Range 0..6: decisions at i=0,1,2,3,4,5 (horizon=1 → each needs i+1 bar)
        // i=0: entry=bars[1].open=10.5, exit=bars[1].close=11.0, gross=11/10.5-1, net=gross-0.001
        // i=1: entry=bars[2].open=11.0, exit=bars[2].close=11.5, gross=11.5/11-1, net=gross-0.001
        // etc.
        let result = evaluate(&tree, &data, &llm, 0..6, ObjectiveMode::ScoreHard)
            .await
            .unwrap();
        assert!(result.is_some(), "Expected Some for rising data with always-long tree");

        // Manually compute forward_return for each point in 0..6
        let mut manual_nets = Vec::new();
        for i in 0..6usize {
            // horizon=1: entry=bars[i+1].open, exit=bars[i+1].close
            let entry = bars[i + 1].open;
            let exit = bars[i + 1].close;
            let gross = exit / entry - 1.0;
            let net = gross - 0.001; // round_trip_bps=10 → 0.001
            manual_nets.push(net);
        }
        let manual_mean = manual_nets.iter().sum::<f64>() / manual_nets.len() as f64;
        let got = result.unwrap();
        assert!(
            (got - manual_mean).abs() < 1e-9,
            "evaluate ScoreHard={got} vs manual_mean={manual_mean}"
        );
    }

    #[tokio::test]
    async fn score_hard_range_restriction_rising_then_falling() {
        // First half: prices rise (10→15), second half: prices fall (15→10)
        let bars = vec![
            bar("2024-01-02 09:45:00", 10.0, 11.0),
            bar("2024-01-02 15:00:00", 11.0, 12.0),
            bar("2024-01-03 09:45:00", 12.0, 13.0),
            bar("2024-01-03 15:00:00", 13.0, 14.0),
            bar("2024-01-04 09:45:00", 14.0, 15.0),
            // falling
            bar("2024-01-04 15:00:00", 15.0, 14.0),
            bar("2024-01-05 09:45:00", 14.0, 13.0),
            bar("2024-01-05 15:00:00", 13.0, 12.0),
            bar("2024-01-06 09:45:00", 12.0, 11.0),
            bar("2024-01-06 15:00:00", 11.0, 10.0),
        ];
        let tree = load_tree_str(ALWAYS_LONG_TREE).unwrap();
        let news: Vec<NewsRecord> = Vec::new();
        let aux: BTreeMap<String, AuxTable> = BTreeMap::new();
        let costs = CostModel { round_trip_bps: 0.0 }; // no cost to keep sign clear
        let data = make_eval_data(&bars, &bars, &news, &aux, costs);
        let llm = LlmEvaluator::Disabled;

        let n = bars.len();
        let first_half = evaluate(&tree, &data, &llm, 0..(n / 2 - 1), ObjectiveMode::ScoreHard)
            .await
            .unwrap()
            .unwrap();
        let second_half =
            evaluate(&tree, &data, &llm, (n / 2)..(n - 1), ObjectiveMode::ScoreHard)
                .await
                .unwrap()
                .unwrap();

        assert!(
            first_half > 0.0,
            "first half of rising data should give positive objective, got {first_half}"
        );
        assert!(
            second_half < 0.0,
            "second half (falling data) should give negative objective, got {second_half}"
        );
    }

    #[tokio::test]
    async fn all_flat_tree_returns_none() {
        let tree = load_tree_str(ALWAYS_FLAT_TREE).unwrap();
        let bars = rising_bars();
        let news: Vec<NewsRecord> = Vec::new();
        let aux: BTreeMap<String, AuxTable> = BTreeMap::new();
        let costs = CostModel { round_trip_bps: 10.0 };
        let data = make_eval_data(&bars, &bars, &news, &aux, costs);
        let llm = LlmEvaluator::Disabled;

        let result_hard =
            evaluate(&tree, &data, &llm, 0..4, ObjectiveMode::ScoreHard).await.unwrap();
        assert_eq!(result_hard, None, "all-flat tree should return None for ScoreHard");

        let result_soft =
            evaluate(&tree, &data, &llm, 0..4, ObjectiveMode::ScoreSoft).await.unwrap();
        assert_eq!(result_soft, None, "all-flat tree should return None for ScoreSoft");
    }

    #[tokio::test]
    async fn sim_mode_rising_fixture_returns_some_finite() {
        // Multi-day rising bars; always-long tree drives entry; sim produces finite result
        let tree = load_tree_str(ALWAYS_LONG_TREE).unwrap();
        let bars = rising_bars();
        let news: Vec<NewsRecord> = Vec::new();
        let aux: BTreeMap<String, AuxTable> = BTreeMap::new();
        let costs = CostModel { round_trip_bps: 5.0 };
        let data = make_eval_data(&bars, &bars, &news, &aux, costs);
        let llm = LlmEvaluator::Disabled;

        // Sim range: 0..bars.len()-1 (caller ensures range.end <= primary.len()-1)
        let result =
            evaluate(&tree, &data, &llm, 0..(bars.len() - 1), ObjectiveMode::Sim).await.unwrap();
        assert!(result.is_some(), "sim mode on rising data should return Some(...)");
        assert!(result.unwrap().is_finite(), "sim mode objective should be finite");
    }

    // ── analyze_axis_interior unit tests ────────────────────────────────────

    #[test]
    fn auto_extend_detects_peak_just_outside_grid() {
        // obj(x) = -(x-30)^2  →  peak at 30.  Original grid [40, 55, 90]: best = 40 (lower boundary).
        // Extension step = 55 - 40 = 15.  Candidates: 25 (better than 40) → 10 (worse) → interior.
        let axis = crate::optimize::grid::GridAxis { name: "n_s".into(), values: vec![40.0, 55.0, 90.0] };
        let objective = |x: f64| -((x - 30.0).powi(2));
        let out = analyze_axis_interior(&axis, 40.0, 4, &objective);
        assert_eq!(out.name, "n_s");
        // After extending downward: 40→25 (better) → 10 (worse) → peak confirmed at 25 → interior.
        assert!(out.interior, "extension should confirm peak as interior");
        assert!(out.extended_steps >= 1);
        assert!(out.final_values.iter().any(|v| (*v - 25.0).abs() < 1e-9));
    }

    #[test]
    fn auto_extend_marks_boundary_artifact_when_monotone() {
        // obj(x) = x  →  monotone increasing; upper boundary 90 is best, always improving outward.
        // After max_steps=3 extensions, interior remains false (boundary artefact).
        let axis = crate::optimize::grid::GridAxis { name: "n_s".into(), values: vec![40.0, 55.0, 90.0] };
        let objective = |x: f64| x;
        let out = analyze_axis_interior(&axis, 90.0, 3, &objective);
        assert!(!out.interior, "monotone objective never confirms interior — should stay false");
        assert_eq!(out.extended_steps, 3);
    }

    #[test]
    fn auto_extend_no_op_when_interior() {
        // best = 2.0 is strictly between 1.0 and 3.0 → not on boundary → interior immediately.
        let axis = crate::optimize::grid::GridAxis { name: "k".into(), values: vec![1.0, 2.0, 3.0] };
        let out = analyze_axis_interior(&axis, 2.0, 4, &|x: f64| -((x - 2.0).powi(2)));
        assert!(out.interior);
        assert_eq!(out.extended_steps, 0);
        assert_eq!(out.best_value, Some(2.0));
    }
}
