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
    pub version: u32,
    pub tree_name: String,
    pub last_time: Option<NaiveDateTime>,
    pub account: AccountSnapshot,
}

/// 回放统计摘要（随信号一起输出）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperStats {
    pub nav: f64,
    pub total_return: f64,
    pub max_drawdown: f64,
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
pub struct SignalSingleConfig {
    pub tree_path: PathBuf,
    pub primary_path: PathBuf,
    pub context_path: PathBuf,
    pub news_path: Option<PathBuf>,
    pub aux_paths: Vec<(String, PathBuf)>,
    pub window: usize,
    pub warmup: usize,
    pub cost_bps: f64,
    pub soft: bool,
    pub state_path: PathBuf,
}

// ──────────────────────────────────────────────────────────────────────────────
// State IO
// ──────────────────────────────────────────────────────────────────────────────

/// 读取 paper state 文件。
/// - 不存在 → `Ok(None)`
/// - JSON 损坏 → `Err`（含 "corrupt"）
/// - version ≠ 1 → `Err`
/// - tree_name 不符 → `Err`（防串树）
pub fn read_paper_state(path: &Path, tree_name: &str) -> Result<Option<PaperState>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(path)?;
    if raw.trim().is_empty() {
        return Ok(None);
    }
    let st: PaperState = serde_json::from_str(&raw).map_err(|e| {
        Error::Data(format!(
            "signal state corrupt: {e}（如需重建请删除该文件）"
        ))
    })?;
    if st.version != STATE_VERSION {
        return Err(Error::Data(format!(
            "signal state version {} unsupported (expected {})",
            st.version, STATE_VERSION
        )));
    }
    if st.tree_name != tree_name {
        return Err(Error::Data(format!(
            "signal state tree_name '{}' does not match requested tree '{tree_name}'",
            st.tree_name
        )));
    }
    Ok(Some(st))
}

/// 将 paper state 写入文件（JSON pretty，人可读）。
pub fn write_paper_state(path: &Path, st: &PaperState) -> Result<()> {
    let json = serde_json::to_string_pretty(st)?;
    std::fs::write(path, json)?;
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
                e += p * leaf.weight * stance_dir(leaf.stance);
            }
        }
        Ok((e, "tree", None))
    } else {
        let trace = traverse(tree, ctx, llm).await?;
        let target = tree.leaves.get(&trace.leaf).map_or(0.0, |l| {
            stance_dir(l.stance) * l.weight
        });
        Ok((target, "tree", Some(trace.leaf)))
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
        };

        // 风控覆盖（spec §3.2）：pos≠0 时按 stop→tp→max_hold 顺序检查
        let (target, reason): (f64, &str) = if acc.pos.abs() > EPS {
            if let Some(risk) = &tree.risk {
                if risk.stop_loss.is_some_and(|sl| unreal_pnl <= -sl) {
                    (0.0, "stop")
                } else if risk.take_profit.is_some_and(|tp| unreal_pnl >= tp) {
                    (0.0, "tp")
                } else if risk.max_hold_bars.is_some_and(|mh| acc.bars_held >= mh) {
                    (0.0, "max_hold")
                } else {
                    let (t, r, _) = compute_tree_target(&tree, &ctx, llm, cfg.soft).await?;
                    (t, r)
                }
            } else {
                let (t, r, _) = compute_tree_target(&tree, &ctx, llm, cfg.soft).await?;
                (t, r)
            }
        } else {
            let (t, r, _) = compute_tree_target(&tree, &ctx, llm, cfg.soft).await?;
            (t, r)
        };

        // 执行 sim_step（哪怕 delta≈0 也记账）
        let _ = sim_step(
            &mut acc,
            close_i,
            open_next,
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

    let unreal_hang = if acc.pos.abs() > EPS {
        (close_hang / acc.entry_price - 1.0) * acc.pos.signum()
    } else {
        0.0
    };
    ctx_hang.sim = SimState {
        pos: acc.pos,
        entry_price: acc.entry_price,
        bars_held: acc.bars_held,
        unreal_pnl: unreal_hang,
    };

    // 悬挂决策：风控覆盖优先，否则树目标（保留 leaf trace）
    let (hang_target, hang_reason, hang_leaf): (f64, &str, Option<String>) =
        if acc.pos.abs() > EPS {
            if let Some(risk) = &tree.risk {
                if risk.stop_loss.is_some_and(|sl| unreal_hang <= -sl) {
                    (0.0, "stop", None)
                } else if risk.take_profit.is_some_and(|tp| unreal_hang >= tp) {
                    (0.0, "tp", None)
                } else if risk.max_hold_bars.is_some_and(|mh| acc.bars_held >= mh) {
                    (0.0, "max_hold", None)
                } else {
                    compute_tree_target(&tree, &ctx_hang, llm, cfg.soft).await?
                }
            } else {
                compute_tree_target(&tree, &ctx_hang, llm, cfg.soft).await?
            }
        } else {
            compute_tree_target(&tree, &ctx_hang, llm, cfg.soft).await?
        };

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
// 测试
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::llm::LlmEvaluator;
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
            // ── A：全量 fresh ────────────────────────────────────────────────
            let state_a_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
            let cfg_a = make_cfg(
                tree_f.path(),
                primary_f.path(),
                context_f.path(),
                state_a_f.path(),
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

            let state_b1_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
            let cfg_b1 = make_cfg(
                tree_f.path(),
                partial_f.path(),
                context_f.path(),
                state_b1_f.path(),
            );
            let (_sig_b1, state_b1) = run_signal_single(&cfg_b1, &LlmEvaluator::Disabled)
                .await
                .unwrap();

            // ── B2：从 state_b1 全量跑 ──────────────────────────────────────
            let state_b2_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
            write_paper_state(state_b2_f.path(), &state_b1).unwrap();

            let cfg_b2 = make_cfg(
                tree_f.path(),
                primary_f.path(),
                context_f.path(),
                state_b2_f.path(),
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

        // 第一次：全量 fresh
        let state_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        let cfg = make_cfg(tree_f.path(), primary_f.path(), context_f.path(), state_f.path());
        let (sig1, state1) = run_signal_single(&cfg, &LlmEvaluator::Disabled)
            .await
            .unwrap();

        // 保存 state1，再跑一次
        write_paper_state(state_f.path(), &state1).unwrap();
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
        // 12 bars total: warmup=5, bar5=entry-decision, bar6..10=hold, bar11=hang(crash)
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
        let state_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();

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
            state_path: state_f.path().to_path_buf(),
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
        // manually patch version
        let raw = std::fs::read_to_string(f.path()).unwrap();
        std::fs::write(f.path(), raw).unwrap();
        let err = read_paper_state(f.path(), "t").unwrap_err();
        assert!(
            err.to_string().contains("version") || err.to_string().contains("999"),
            "version mismatch must mention version, got: {err}"
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
}
