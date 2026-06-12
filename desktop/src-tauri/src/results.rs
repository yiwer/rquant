//! 留档读取:摘要(指标卡+资金换算)/资产曲线/交易明细。sim 解析强类型;score 原样透传。
use crate::dto::{EquityPointDto, RunSummaryDto, TradeDto};
use crate::paths::Workspace;
use crate::runs;
use rquant::backtest::sim::{SimReport, SimStepRecord};

fn iso(t: &chrono::NaiveDateTime) -> String {
    t.format("%Y-%m-%dT%H:%M:%S").to_string()
}

pub fn run_summary(ws: &Workspace, id: &str) -> Result<RunSummaryDto, String> {
    let meta = runs::read_meta(ws, id).ok_or_else(|| format!("run {} not found", id))?;
    let config = runs::read_config(ws, id).map_err(|e| e.to_string())?;
    let rp = runs::run_paths(ws, id);
    let txt = std::fs::read_to_string(&rp.result_json).map_err(|e| e.to_string())?;
    let cap = config.initial_capital;
    let mut s = RunSummaryDto {
        meta,
        config,
        total_return: None,
        max_drawdown: None,
        n_round_trips: None,
        win_rate: None,
        avg_hold_bars: None,
        turnover: None,
        buy_and_hold: None,
        sharpe: None,
        final_equity: None,
        net_pnl: None,
        raw: None,
    };
    if s.meta.kind.starts_with("sim") {
        let r: SimReport = serde_json::from_str(&txt).map_err(|e| e.to_string())?;
        s.total_return = Some(r.total_return);
        s.max_drawdown = Some(r.max_drawdown);
        s.n_round_trips = Some(r.n_round_trips as u32);
        s.win_rate = Some(r.win_rate);
        s.avg_hold_bars = Some(r.avg_hold_bars);
        s.turnover = Some(r.turnover);
        s.buy_and_hold = Some(r.buy_and_hold);
        s.sharpe = r.risk.as_ref().and_then(|k| k.sharpe);
        s.final_equity = Some(cap * (1.0 + r.total_return));
        s.net_pnl = Some(cap * r.total_return);
    } else {
        s.raw = Some(serde_json::from_str(&txt).map_err(|e| e.to_string())?);
    }
    Ok(s)
}

pub fn equity_series(ws: &Workspace, id: &str) -> Result<Vec<EquityPointDto>, String> {
    let config = runs::read_config(ws, id).map_err(|e| e.to_string())?;
    let rp = runs::run_paths(ws, id);
    let txt = std::fs::read_to_string(&rp.traces_jsonl).map_err(|e| e.to_string())?;
    let cap = config.initial_capital;
    Ok(txt
        .lines()
        .filter_map(|l| serde_json::from_str::<SimStepRecord>(l).ok())
        .map(|r| EquityPointDto { t: iso(&r.t), nav: r.nav, equity: r.nav * cap, pos: r.pos })
        .collect())
}

pub fn trades(ws: &Workspace, id: &str) -> Result<Vec<TradeDto>, String> {
    let meta = crate::runs::read_meta(ws, id).ok_or_else(|| format!("run {} not found", id))?;
    if !meta.kind.starts_with("sim") {
        return Err(format!("trades not available for {} runs", meta.kind));
    }
    let config = runs::read_config(ws, id).map_err(|e| e.to_string())?;
    let rp = runs::run_paths(ws, id);
    let txt = std::fs::read_to_string(&rp.result_json).map_err(|e| e.to_string())?;
    let r: SimReport = serde_json::from_str(&txt).map_err(|e| e.to_string())?;
    let cap = config.initial_capital;
    Ok(r.trades
        .iter()
        .map(|t| TradeDto {
            entry_t: iso(&t.entry_t),
            exit_t: iso(&t.exit_t),
            entry_px: t.entry_px,
            exit_px: t.exit_px,
            max_abs_pos: t.max_abs_pos,
            trip_return: t.trip_return,
            bars_held: t.bars_held as u32,
            reason: t.reason.clone(),
            pnl_amount: cap * t.trip_return,
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
        let id = out["run_id"].as_str().unwrap().to_string();
        (td, w, id)
    }

    #[test]
    fn sim_summary_has_metrics_and_money() {
        let (_td, w, id) = run_one("sim_hard");
        let s = run_summary(&w, &id).unwrap();
        assert_eq!(s.meta.kind, "sim_hard");
        let tr = s.total_return.unwrap();
        assert!((s.final_equity.unwrap() - 100000.0 * (1.0 + tr)).abs() < 1e-6);
        assert!((s.net_pnl.unwrap() - 100000.0 * tr).abs() < 1e-6);
        assert!(s.raw.is_none());
    }

    #[test]
    fn score_summary_is_raw_passthrough() {
        let (_td, w, id) = run_one("score_hard");
        let s = run_summary(&w, &id).unwrap();
        assert!(s.total_return.is_none());
        assert!(s.raw.is_some());
        assert_eq!(s.raw.unwrap()["tree_name"], "m2-mini");
    }

    #[test]
    fn equity_series_scales_nav_by_capital() {
        let (_td, w, id) = run_one("sim_hard");
        let pts = equity_series(&w, &id).unwrap();
        assert!(!pts.is_empty());
        for p in &pts {
            assert!((p.equity - p.nav * 100000.0).abs() < 1e-6);
        }
        // 升序
        assert!(pts.windows(2).all(|w2| w2[0].t <= w2[1].t));
    }

    #[test]
    fn trades_have_amount_column() {
        let (_td, w, id) = run_one("sim_hard");
        let ts = trades(&w, &id).unwrap();
        for t in &ts {
            assert!((t.pnl_amount - 100000.0 * t.trip_return).abs() < 1e-6);
        }
    }

    #[test]
    fn sim_soft_summary_parses_ok() {
        let (_td, w, id) = run_one("sim_soft");
        let s = run_summary(&w, &id).unwrap();
        assert_eq!(s.meta.kind, "sim_soft");
        assert!(s.total_return.is_some());
        assert!(s.raw.is_none());
    }
}
