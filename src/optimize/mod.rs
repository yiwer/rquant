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
use crate::tree::loader::Tree;
use crate::tree::schema::Stance;
use crate::Result;
use std::collections::BTreeMap;

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
        let ctx = build_context(data.primary, data.context, data.news, data.aux, t, data.window);
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
            nets.push(f.net * leaf.weight);
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
        let ctx = build_context(data.primary, data.context, data.news, data.aux, t, data.window);
        let soft = crate::engine::soft::traverse_soft(tree, &ctx, llm).await?;
        let score: Option<SoftScore> = score_soft(&soft, tree, data.primary, i, &data.costs);

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
        let close_next = data.primary[i + 1].close;
        let t_next = data.primary[i + 1].time;

        // Build context gated at primary[i].time
        let mut ctx = build_context(
            data.primary,
            data.context,
            data.news,
            data.aux,
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
        sim_step(&mut acc, close_i, open_next, close_next, t_next, target, rate, reason);

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
        dir * l.weight
    });
    Ok((target, "tree"))
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
}
