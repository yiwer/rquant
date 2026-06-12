//! 决策回放:frames=Trace(±SimStepRecord 对齐);因子值=resolve_factor_exprs+build_context 现算。
//!
//! # 关于 sim_hard 的 frames 计数
//!
//! `replay_frames` 用 decision_traces.jsonl（Trace 行）与 traces.jsonl（SimStepRecord 行）
//! 按时间戳 BTreeMap 对齐。sim_hard 中，风控覆盖 bar（stop/tp/max_hold 触发的 bar）**不遍历树**，
//! 因此无对应 Trace 行——frames 数 < SimStepRecord 数是预期行为，不是 bug。
//! BTreeMap by-t 对齐天然容忍这种缺位。
use crate::dto::{FactorValueDto, ReplayFrameDto, ReplayStepDto};
use crate::paths::Workspace;
use crate::runs;
use rquant::backtest::sim::SimStepRecord;
use rquant::engine::trace::Trace;
use std::collections::BTreeMap;

fn iso(t: &chrono::NaiveDateTime) -> String {
    t.format("%Y-%m-%dT%H:%M:%S").to_string()
}

fn read_traces<T: serde::de::DeserializeOwned>(path: &std::path::Path) -> Vec<T> {
    std::fs::read_to_string(path)
        .map(|txt| txt.lines().filter_map(|l| serde_json::from_str(l).ok()).collect())
        .unwrap_or_default()
}

pub fn replay_frames(ws: &Workspace, id: &str) -> Result<Vec<ReplayFrameDto>, String> {
    let meta = runs::read_meta(ws, id).ok_or_else(|| format!("run {} not found", id))?;
    let rp = runs::run_paths(ws, id);
    // 路径来源:sim_hard=decision_traces.jsonl;score_*=traces.jsonl(本身就是 Trace 行)
    let traces: Vec<Trace> = if meta.kind == "sim_hard" {
        read_traces(&rp.decision_jsonl)
    } else if meta.kind.starts_with("score") {
        read_traces(&rp.traces_jsonl)
    } else {
        return Err("replay paths unavailable for sim_soft (no single-path traversal)".into());
    };
    if traces.is_empty() {
        return Err("no decision traces archived for this run".into());
    }
    // sim 账户线按 t 对齐
    let steps: BTreeMap<String, SimStepRecord> = if meta.kind.starts_with("sim") {
        read_traces::<SimStepRecord>(&rp.traces_jsonl)
            .into_iter()
            .map(|s| (iso(&s.t), s))
            .collect()
    } else {
        BTreeMap::new()
    };
    let mut frames: Vec<ReplayFrameDto> = traces
        .into_iter()
        .map(|tr| {
            let key = iso(&tr.t);
            let st = steps.get(&key);
            ReplayFrameDto {
                t: key,
                leaf: tr.leaf,
                stance: format!("{:?}", tr.stance),
                path: tr
                    .path
                    .into_iter()
                    .map(|s| ReplayStepDto {
                        node_id: s.node_id,
                        label: s.label,
                        confidence: s.confidence,
                        rationale: s.rationale,
                    })
                    .collect(),
                target: st.map(|s| s.target),
                pos: st.map(|s| s.pos),
                nav: st.map(|s| s.nav),
            }
        })
        .collect();
    frames.sort_by(|a, b| a.t.cmp(&b.t));
    Ok(frames)
}

/// 在 t 时刻现算树的全部因子值(spec §5.2 回放因子表)。
/// 共享一个 Context 对整列因子求值：安全且高效（先序因子 Cached 槽命中）。
pub fn replay_factors(ws: &Workspace, id: &str, t: &str) -> Result<Vec<FactorValueDto>, String> {
    let config = runs::read_config(ws, id).map_err(|e| e.to_string())?;
    let yaml =
        std::fs::read_to_string(ws.root().join(&config.tree_path)).map_err(|e| e.to_string())?;
    let factors = rquant::tree::loader::resolve_factor_exprs(&yaml).map_err(|e| e.to_string())?;
    if factors.is_empty() {
        return Ok(Vec::new());
    }
    let bars =
        rquant::data::reader::read_bars_csv(&ws.root().join(&config.primary_path))
            .map_err(|e| e.to_string())?;
    let t_parsed =
        chrono::NaiveDateTime::parse_from_str(t, "%Y-%m-%dT%H:%M:%S").map_err(|e| e.to_string())?;
    let ctx = rquant::features::context::build_context(
        &bars,
        &bars,
        &[],
        &Default::default(),
        t_parsed,
        config.window as usize,
    );
    Ok(factors
        .iter()
        .map(|(name, expr)| {
            let v = rquant::dsl::eval::eval(expr, &ctx).ok().and_then(|val| match val {
                rquant::dsl::eval::Value::Scalar(x) => Some(x),
                rquant::dsl::eval::Value::Series(s) => s.last().copied(),
                rquant::dsl::eval::Value::Bool(b) => Some(if b { 1.0 } else { 0.0 }),
            });
            FactorValueDto { name: name.clone(), value: v.filter(|x| x.is_finite()) }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backtest_run::test_fixtures::{cfg, fixture_ws, NoopProgress};

    fn run_one(mode: &str) -> (tempfile::TempDir, crate::paths::Workspace, String) {
        let (td, w) = fixture_ws();
        let out = crate::backtest_run::execute_backtest(&w, &NoopProgress, &cfg(mode)).unwrap();
        (td, w, out["run_id"].as_str().unwrap().to_string())
    }

    #[test]
    fn sim_hard_frames_align_path_with_account() {
        let (_td, w, id) = run_one("sim_hard");
        let frames = replay_frames(&w, &id).unwrap();
        assert!(!frames.is_empty());
        for f in &frames {
            assert!(!f.path.is_empty(), "decision path recorded");
            assert!(f.nav.is_some(), "sim aligns SimStepRecord");
        }
        // 时间升序
        assert!(frames.windows(2).all(|w2| w2[0].t <= w2[1].t));
    }

    #[test]
    fn score_hard_frames_have_path_without_account() {
        let (_td, w, id) = run_one("score_hard");
        let frames = replay_frames(&w, &id).unwrap();
        assert!(!frames.is_empty());
        assert!(frames.iter().all(|f| f.nav.is_none() && !f.path.is_empty()));
    }

    #[test]
    fn replay_factors_evaluates_tree_factors_at_t() {
        // mini 树没有 factors 块 → 空表也合法;换带因子的树验证求值
        let (_td, w) = fixture_ws();
        // label: 字段是 loader 硬性要求(B2 已实证):branches/default 必须带 label
        const FACTOR_TREE: &str = r#"
meta: { name: "m2-fct", forward_window: 4, stances: [long, flat] }
params: { n: 5.0 }
factors:
  ma: "sma(close, n)"
root: r
nodes:
  r:
    type: quant
    branches:
      - when: "close > ma"
        goto: l
        label: above_ma
    default: { goto: f, label: below_ma }
leaves:
  l: { stance: long, weight: 1.0 }
  f: { stance: flat }
"#;
        std::fs::write(w.root().join("examples/fct.yaml"), FACTOR_TREE).unwrap();
        let mut c = cfg("sim_hard");
        c.tree_path = "examples/fct.yaml".into();
        let out = crate::backtest_run::execute_backtest(&w, &NoopProgress, &c).unwrap();
        let id = out["run_id"].as_str().unwrap();
        let frames = replay_frames(&w, id).unwrap();
        let t = frames.last().unwrap().t.clone();
        let vals = replay_factors(&w, id, &t).unwrap();
        assert_eq!(vals.len(), 1);
        assert_eq!(vals[0].name, "ma");
        assert!(vals[0].value.unwrap() > 0.0);
    }
}
