use rquant::backtest::runner::{run, BacktestConfig};
use rquant::backtest::sim::run_sim;
use rquant::eval::llm::{LlmEvaluator, StubLlm};
use std::collections::HashMap;
use std::io::Write;
use rquant::backtest::soft::run_soft;
use rquant::optimize::{OptimizeConfig, OptimizeReport, run_optimize};
use rquant::signal::{
    run_signal_single, run_signal_portfolio,
    read_paper_state, write_paper_state,
    write_holdings_state,
    SignalSingleConfig, SignalPortfolioConfig,
    TradeAction,
};

fn write_file(content: &str, suffix: &str) -> tempfile::NamedTempFile {
    let mut f = tempfile::Builder::new().suffix(suffix).tempfile().unwrap();
    write!(f, "{content}").unwrap();
    f.flush().unwrap();
    f
}

fn tree_yaml() -> String {
    r#"
meta: { name: e2e, forward_window: 2, stances: [long, flat] }
root: entry
nodes:
  entry:
    type: quant
    branches: [ { when: "close > sma(close,5)", goto: leaf_long, label: above } ]
    default: { goto: leaf_flat, label: below }
leaves:
  leaf_long: { stance: long }
  leaf_flat: { stance: flat }
"#
    .to_string()
}

fn gen_primary_csv() -> String {
    let mut s = String::from("time,open,high,low,close,volume\n");
    let mut idx = 0;
    for day in 0..5 {
        for k in 0..8 {
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
         2024-01-03 10:30:00,10.2,10.2,10.2,10.2,1\n",
    )
}

#[tokio::test]
async fn end_to_end_uptrend_yields_positive_long_edge() {
    let tree_f = write_file(&tree_yaml(), ".yaml");
    let primary_f = write_file(&gen_primary_csv(), ".csv");
    let context_f = write_file(&gen_context_csv(), ".csv");
    let out_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();

    let cfg = BacktestConfig {
        tree_path: tree_f.path().to_path_buf(),
        primary_path: primary_f.path().to_path_buf(),
        context_path: context_f.path().to_path_buf(),
        news_path: None,
        out_path: out_f.path().to_path_buf(),
        traces_path: None,
        cost_bps: 10.0,
        warmup: 5,
        window: 100,
        concurrency: 4,
        holidays_path: None,
        folds: 3,
        aux_paths: vec![],
    };

    let report = run(&cfg, &LlmEvaluator::Disabled).await.unwrap();
    let m = &report.metrics;
    assert!(m.scored > 0, "should have scored signals");
    assert!(m.active.count > 0, "uptrend should trigger long signals");
    assert!(m.active.mean_net > 0.0, "long edge in an uptrend should be positive after costs");
    assert!(m.t1_executable.count > 0, "some signals should cross a day boundary (T+1)");
    // buy&hold baseline must span the same warmup-onward window as the signals
    // (warmup=5 → buy at bar 5 open, hold to last close); price(i)=10.0+0.1*i over 40 bars.
    let expected_bh = (10.0 + 0.1 * 39.0) / (10.0 + 0.1 * 5.0) - 1.0;
    assert!(
        (m.buy_and_hold - expected_bh).abs() < 1e-9,
        "buy&hold={} expected warmup-onward {}",
        m.buy_and_hold,
        expected_bh
    );

    let content = std::fs::read_to_string(out_f.path()).unwrap();
    assert!(content.contains("e2e"));
    assert!(report.gaps.is_empty(), "synthetic data should have no gaps");
    // F4 T3 — scoring e2e: t_stat field on active SignalStat is structurally accessible.
    // With uptrend + long signals, mean_net > 0 → t_stat is Some (n > 1 and std > 0).
    // We only assert structural access compiles and the field is Some or None without crashing.
    let _t_stat: Option<f64> = report.metrics.active.t_stat;
    // For this fixture (uptrend, multiple long signals, non-zero std) it should be Some.
    assert!(
        report.metrics.active.t_stat.is_some(),
        "active t_stat should be Some for uptrend fixture with n>1 scored signals"
    );
    // H6 — walk-forward folds shape check
    let wf = report.walk_forward.as_ref().unwrap();
    assert_eq!(wf.folds.len(), 3, "folds=3 should produce 3 fold entries");
}

fn llm_tree_yaml() -> String {
    r#"
meta: { name: e2e_llm, forward_window: 2, stances: [long, flat] }
root: gate
nodes:
  gate:
    type: quant
    branches: [ { when: "close > sma(close,5)", goto: judge, label: above } ]
    default: { goto: leaf_flat, label: below }
  judge:
    type: llm
    inputs: [news_score]
    prompt: "go or not"
    labels: { go: leaf_long }
    default: leaf_flat
leaves:
  leaf_long: { stance: long }
  leaf_flat: { stance: flat }
"#
    .to_string()
}

async fn run_llm_e2e(ev: &LlmEvaluator) -> rquant::report::Report {
    let tree_f = write_file(&llm_tree_yaml(), ".yaml");
    let primary_f = write_file(&gen_primary_csv(), ".csv");
    let context_f = write_file(&gen_context_csv(), ".csv");
    let out_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
    let cfg = BacktestConfig {
        tree_path: tree_f.path().to_path_buf(),
        primary_path: primary_f.path().to_path_buf(),
        context_path: context_f.path().to_path_buf(),
        news_path: None,
        out_path: out_f.path().to_path_buf(),
        traces_path: None,
        cost_bps: 10.0,
        warmup: 5,
        window: 100,
        concurrency: 4,
        holidays_path: None,
        folds: 0,
        aux_paths: vec![],
    };
    run(&cfg, ev).await.unwrap()
}

#[tokio::test]
async fn llm_node_changes_path_vs_disabled() {
    let stub = LlmEvaluator::Stub(StubLlm { answers: HashMap::from([("judge".to_string(), "go".to_string())]) });
    let with_llm = run_llm_e2e(&stub).await;
    assert!(with_llm.metrics.active.count > 0, "stub 'go' should produce long signals");

    let disabled = run_llm_e2e(&LlmEvaluator::Disabled).await;
    assert_eq!(disabled.metrics.active.count, 0, "disabled LLM should take default -> all flat");
}

#[tokio::test]
async fn soft_mode_yields_positive_engaged_edge() {
    // Reuse the same fixtures as llm_node_changes_path_vs_disabled:
    // LLM tree (quant gate -> LLM judge "go" -> leaf_long), uptrend data, stub answers "go" (c=0.9).
    let tree_f = write_file(&llm_tree_yaml(), ".yaml");
    let primary_f = write_file(&gen_primary_csv(), ".csv");
    let context_f = write_file(&gen_context_csv(), ".csv");
    let out_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();

    let cfg = BacktestConfig {
        tree_path: tree_f.path().to_path_buf(),
        primary_path: primary_f.path().to_path_buf(),
        context_path: context_f.path().to_path_buf(),
        news_path: None,
        out_path: out_f.path().to_path_buf(),
        traces_path: None,
        cost_bps: 10.0,
        warmup: 5,
        window: 100,
        concurrency: 4,
        holidays_path: None,
        folds: 3,
        aux_paths: vec![],
    };

    let ev = LlmEvaluator::Stub(StubLlm {
        answers: HashMap::from([("judge".to_string(), "go".to_string())]),
    });

    let report = rquant::backtest::soft::run_soft(&cfg, &ev).await.unwrap();
    let m = &report.soft;
    assert!(m.scored > 0, "should score points");
    assert!(m.engaged.count > 0, "soft mode should engage (some long mass)");
    assert!(
        m.engaged.mean_net > 0.0,
        "uptrend + judge go(c=0.9) => positive expected net; got mean_net={}",
        m.engaged.mean_net
    );
    assert!(m.position.count > 0, "uptrend long mass => nonzero exposure points");
    assert!(m.position.mean_net > 0.0, "net-position metric should also be positive on uptrend");
    let content = std::fs::read_to_string(out_f.path()).unwrap();
    assert!(content.contains("engaged"));
    let wf = report.walk_forward.as_ref().expect("folds=3 should produce walk_forward");
    assert_eq!(wf.folds.len(), 3);
    assert!(wf.worst_mean_net > 0.0, "uptrend: every non-empty fold should be positive");
    assert!(wf.positive_folds >= 1);
}

#[tokio::test]
async fn soft_traces_written_when_path_given() {
    // Reuse the same fixtures as soft_mode_yields_positive_engaged_edge
    // (LLM tree with quant gate -> LLM judge -> leaf_long, uptrend data, Stub ev),
    // but set traces_path to a tempfile .jsonl.
    let tree_f = write_file(&llm_tree_yaml(), ".yaml");
    let primary_f = write_file(&gen_primary_csv(), ".csv");
    let context_f = write_file(&gen_context_csv(), ".csv");
    let out_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
    let traces_f = tempfile::Builder::new().suffix(".jsonl").tempfile().unwrap();

    let cfg = BacktestConfig {
        tree_path: tree_f.path().to_path_buf(),
        primary_path: primary_f.path().to_path_buf(),
        context_path: context_f.path().to_path_buf(),
        news_path: None,
        out_path: out_f.path().to_path_buf(),
        traces_path: Some(traces_f.path().to_path_buf()),
        cost_bps: 10.0,
        warmup: 5,
        window: 100,
        concurrency: 4,
        holidays_path: None,
        folds: 0,
        aux_paths: vec![],
    };

    let ev = LlmEvaluator::Stub(StubLlm {
        answers: HashMap::from([("judge".to_string(), "go".to_string())]),
    });

    let report = rquant::backtest::soft::run_soft(&cfg, &ev).await.unwrap();
    let content = std::fs::read_to_string(traces_f.path()).unwrap();
    let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), report.soft.total_decisions, "one line per decision point");
    let first: rquant::backtest::soft::SoftStepRecord = serde_json::from_str(lines[0]).unwrap();
    assert!(!first.leaf_probs.is_empty(), "each record carries a leaf distribution");
}

#[tokio::test]
async fn report_html_renders_with_curve() {
    let tree_f = write_file(&tree_yaml(), ".yaml");
    let primary_f = write_file(&gen_primary_csv(), ".csv");
    let context_f = write_file(&gen_context_csv(), ".csv");
    let out_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
    let traces_f = tempfile::Builder::new().suffix(".jsonl").tempfile().unwrap();

    let cfg = BacktestConfig {
        tree_path: tree_f.path().to_path_buf(),
        primary_path: primary_f.path().to_path_buf(),
        context_path: context_f.path().to_path_buf(),
        news_path: None,
        out_path: out_f.path().to_path_buf(),
        traces_path: Some(traces_f.path().to_path_buf()),
        cost_bps: 10.0,
        warmup: 5,
        window: 100,
        concurrency: 4,
        holidays_path: None,
        folds: 0,
        aux_paths: vec![],
    };

    let _report = run(&cfg, &LlmEvaluator::Disabled).await.unwrap();

    let rep: rquant::report::Report =
        serde_json::from_str(&std::fs::read_to_string(out_f.path()).unwrap()).unwrap();
    let traces_content = std::fs::read_to_string(traces_f.path()).unwrap();
    let traces: Vec<rquant::engine::trace::Trace> = traces_content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let bars = rquant::data::reader::read_bars_csv(primary_f.path()).unwrap();
    let costs = rquant::backtest::costs::CostModel { round_trip_bps: rep.cost_bps };
    let series = rquant::report::curve::derive_series(&traces, &bars, rep.forward_window, &costs);
    assert!(!series.points.is_empty(), "uptrend should produce scored points");
    let html = rquant::report::viz::render_html(&rep, Some(&series));
    assert!(html.contains("<!doctype html>"));
    assert!(html.contains("<polyline"));
    assert!(html.contains(&rep.metrics.overlap_warning));
}

#[tokio::test]
async fn soft_report_html_renders() {
    let tree_f = write_file(&llm_tree_yaml(), ".yaml");
    let primary_f = write_file(&gen_primary_csv(), ".csv");
    let context_f = write_file(&gen_context_csv(), ".csv");
    let out_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
    let traces_f = tempfile::Builder::new().suffix(".jsonl").tempfile().unwrap();

    let cfg = BacktestConfig {
        tree_path: tree_f.path().to_path_buf(),
        primary_path: primary_f.path().to_path_buf(),
        context_path: context_f.path().to_path_buf(),
        news_path: None,
        out_path: out_f.path().to_path_buf(),
        traces_path: Some(traces_f.path().to_path_buf()),
        cost_bps: 10.0,
        warmup: 5,
        window: 100,
        concurrency: 4,
        holidays_path: None,
        folds: 0,
        aux_paths: vec![],
    };

    let ev = LlmEvaluator::Stub(StubLlm {
        answers: HashMap::from([("judge".to_string(), "go".to_string())]),
    });

    let report = rquant::backtest::soft::run_soft(&cfg, &ev).await.unwrap();
    let recs: Vec<rquant::backtest::soft::SoftStepRecord> = std::fs::read_to_string(traces_f.path())
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let series = rquant::report::curve::derive_soft_series(&recs);
    let avg = rquant::report::curve::avg_leaf_probs(&recs);
    let stack = rquant::report::curve::leaf_prob_stack(&recs);
    let html = rquant::report::viz::render_soft_html(&report, &series, &avg, Some(&stack));
    assert!(html.contains("<!doctype html>"));
    assert!(html.contains("<polyline"));
    assert!(html.contains(&report.soft.overlap_warning));
    assert!(!series.points.is_empty());
    assert!(html.contains("<polygon"), "stacked area chart present");
}

fn strength_tree_yaml() -> String {
    r#"
meta: { name: strength_demo, forward_window: 4, stances: [long, flat] }
root: trend
nodes:
  trend:
    type: quant
    branches:
      - when: "close > sma(close,5)"
        strength: "sigmoid((close - sma(close,5)) / (0.02 * sma(close,5)))"
        goto: leaf_long
        label: above_ma
    default: { goto: leaf_flat, label: below_ma }
leaves:
  leaf_long: { stance: long }
  leaf_flat: { stance: flat }
"#
    .to_string()
}

#[tokio::test]
async fn soft_quant_strength_engages() {
    let tree_f = write_file(&strength_tree_yaml(), ".yaml");
    let primary_f = write_file(&gen_primary_csv(), ".csv");
    let context_f = write_file(&gen_context_csv(), ".csv");
    let out_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();

    let cfg = BacktestConfig {
        tree_path: tree_f.path().to_path_buf(),
        primary_path: primary_f.path().to_path_buf(),
        context_path: context_f.path().to_path_buf(),
        news_path: None,
        out_path: out_f.path().to_path_buf(),
        traces_path: None,
        cost_bps: 10.0,
        warmup: 5,
        window: 100,
        concurrency: 4,
        holidays_path: None,
        folds: 0,
        aux_paths: vec![],
    };

    let report = run_soft(&cfg, &LlmEvaluator::Disabled).await.unwrap();
    let m = &report.soft;
    assert!(m.scored > 0, "should score points");
    assert!(m.engaged.count > 0, "strength-weighted quant should put mass on long");
}

// H5 — render_report_files soft end-to-end
#[tokio::test]
async fn render_report_files_soft_end_to_end() {
    // Use the same fixture as soft_traces_written_when_path_given (LLM tree, uptrend data, Stub ev)
    let tree_f = write_file(&llm_tree_yaml(), ".yaml");
    let primary_f = write_file(&gen_primary_csv(), ".csv");
    let context_f = write_file(&gen_context_csv(), ".csv");
    let out_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
    let traces_f = tempfile::Builder::new().suffix(".jsonl").tempfile().unwrap();

    let cfg = BacktestConfig {
        tree_path: tree_f.path().to_path_buf(),
        primary_path: primary_f.path().to_path_buf(),
        context_path: context_f.path().to_path_buf(),
        news_path: None,
        out_path: out_f.path().to_path_buf(),
        traces_path: Some(traces_f.path().to_path_buf()),
        cost_bps: 10.0,
        warmup: 5,
        window: 100,
        concurrency: 4,
        holidays_path: None,
        folds: 0,
        aux_paths: vec![],
    };

    let ev = LlmEvaluator::Stub(StubLlm {
        answers: HashMap::from([("judge".to_string(), "go".to_string())]),
    });

    let _report = rquant::backtest::soft::run_soft(&cfg, &ev).await.unwrap();

    let html_f = tempfile::Builder::new().suffix(".html").tempfile().unwrap();
    rquant::report::render_report_files(
        out_f.path(),
        html_f.path(),
        Some(traces_f.path()),
        None,
        rquant::report::ReportMode::Soft,
    )
    .unwrap();

    let html = std::fs::read_to_string(html_f.path()).unwrap();
    assert!(!html.is_empty(), "HTML output must be non-empty");
    assert!(html.contains("<polyline"), "HTML must contain a polyline chart element");
    assert!(html.contains("<polygon"), "HTML must contain a polygon stacked area element");
}

// E1+E2 T4 — factor_tree full chain: params/factors + weight + horizon through hard and soft scoring
#[tokio::test]
async fn factor_tree_full_chain() {
    // Inline tree: same shape as examples/factor_tree.yaml but ma_n=5, horizon=4, and
    // `hour < 23` (always true for any fixture bar) instead of `hour < 14`.
    const FACTOR_TREE: &str = r#"
meta:
  name: factor_chain_e2e
  forward_window: 16
  stances: [long, flat]

params: { ma_n: 5, mom_n: 3 }

factors:
  mom: "slope(ema(close, ma_n), mom_n)"
  above: "close > sma(close, ma_n)"

root: entry

nodes:
  entry:
    type: quant
    branches:
      - when: "above and mom > 0 and hour < 23"
        strength: "sigmoid(mom * 50)"
        goto: leaf_half
        label: trend
    default: { goto: leaf_flat, label: none }

leaves:
  leaf_half: { stance: long, weight: 0.5, horizon: 4 }
  leaf_flat: { stance: flat }
"#;

    let tree_f = write_file(FACTOR_TREE, ".yaml");
    let primary_f = write_file(&gen_primary_csv(), ".csv");
    let context_f = write_file(&gen_context_csv(), ".csv");
    let out_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
    let out_soft_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();

    let base_cfg = BacktestConfig {
        tree_path: tree_f.path().to_path_buf(),
        primary_path: primary_f.path().to_path_buf(),
        context_path: context_f.path().to_path_buf(),
        news_path: None,
        out_path: out_f.path().to_path_buf(),
        traces_path: None,
        cost_bps: 10.0,
        warmup: 5,
        window: 100,
        concurrency: 4,
        holidays_path: None,
        folds: 0,
        aux_paths: vec![],
    };

    // Hard run
    let report = run(&base_cfg, &LlmEvaluator::Disabled).await.unwrap();
    let m = &report.metrics;
    assert!(m.scored > 0, "factor_tree hard: expected scored > 0, got {}", m.scored);

    // Soft run
    let soft_cfg = BacktestConfig {
        out_path: out_soft_f.path().to_path_buf(),
        ..base_cfg
    };
    let soft_report = rquant::backtest::soft::run_soft(&soft_cfg, &LlmEvaluator::Disabled)
        .await
        .unwrap();
    let sm = &soft_report.soft;
    assert!(
        sm.engaged.count > 0,
        "factor_tree soft: expected engaged.count > 0, got {}",
        sm.engaged.count
    );
}

// M5 — holidays integration: detect_gaps + read_holidays
// Jan 2 and Jan 4 only (Jan 3 missing), holidays file contains 2024-01-03
// → report.gaps.missing_trading_days should be empty
#[tokio::test]
async fn holidays_suppress_missing_day_in_gaps() {
    use std::io::Write;
    // Build data: Jan 2 and Jan 4 only (Jan 3 intentionally absent)
    let csv = "time,open,high,low,close,volume\n\
        2024-01-02 09:45:00,10.0,10.0,10.0,10.0,1\n\
        2024-01-02 10:00:00,10.1,10.1,10.1,10.1,1\n\
        2024-01-04 09:45:00,10.2,10.2,10.2,10.2,1\n\
        2024-01-04 10:00:00,10.3,10.3,10.3,10.3,1\n";

    let mut holidays_f = tempfile::Builder::new().suffix(".txt").tempfile().unwrap();
    writeln!(holidays_f, "2024-01-03").unwrap();
    holidays_f.flush().unwrap();

    let holidays = rquant::data::calendar::read_holidays(holidays_f.path()).unwrap();
    let cal = rquant::data::calendar::AShareCalendar::new(holidays);

    let bars: Vec<rquant::data::bar::Bar> = {
        let mut tmp_f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        write!(tmp_f, "{csv}").unwrap();
        tmp_f.flush().unwrap();
        rquant::data::reader::read_bars_csv(tmp_f.path()).unwrap()
    };

    let gaps = rquant::backtest::gaps::detect_gaps(&bars, &cal);
    assert!(
        gaps.missing_trading_days.is_empty(),
        "2024-01-03 is a declared holiday, must not appear as missing; got: {:?}",
        gaps.missing_trading_days
    );
}

// E3 T4 — aux relative-strength full chain:
// primary rises fast (close = 10 + 0.1*i); aux.idx.v rises slow (v = 10 + 0.01*i).
// Tree: when close/close[-5] > aux.idx.v/aux.idx.v[-5] → long
// Primary momentum > aux momentum → branch fires → m.scored > 0 && m.active.count > 0.
#[tokio::test]
async fn aux_relative_strength_full_chain() {
    use std::io::Write;

    // Build aux CSV: same timestamps as gen_primary_csv, v rising slower than close
    fn gen_aux_csv() -> String {
        let mut s = String::from("time,v\n");
        let mut idx = 0;
        for day in 0..5 {
            for k in 0..8 {
                let v = 10.0 + 0.01 * idx as f64;
                let hour = 9 + (45 + k * 15) / 60;
                let minute = (45 + k * 15) % 60;
                s.push_str(&format!(
                    "2024-01-{:02} {:02}:{:02}:00,{v}\n",
                    2 + day,
                    hour,
                    minute,
                ));
                idx += 1;
            }
        }
        s
    }

    const AUX_TREE: &str = r#"
meta: { name: aux_rs, forward_window: 4, stances: [long, flat] }
root: entry
nodes:
  entry:
    type: quant
    branches:
      - when: "close/close[-5] > aux.idx.v/aux.idx.v[-5]"
        goto: leaf_long
        label: rs_up
    default: { goto: leaf_flat, label: flat }
leaves:
  leaf_long: { stance: long }
  leaf_flat: { stance: flat }
"#;

    let tree_f = write_file(AUX_TREE, ".yaml");
    let primary_f = write_file(&gen_primary_csv(), ".csv");
    let context_f = write_file(&gen_context_csv(), ".csv");
    let out_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();

    let mut aux_f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
    write!(aux_f, "{}", gen_aux_csv()).unwrap();
    aux_f.flush().unwrap();

    let cfg = BacktestConfig {
        tree_path: tree_f.path().to_path_buf(),
        primary_path: primary_f.path().to_path_buf(),
        context_path: context_f.path().to_path_buf(),
        news_path: None,
        out_path: out_f.path().to_path_buf(),
        traces_path: None,
        cost_bps: 10.0,
        warmup: 6,
        window: 100,
        concurrency: 4,
        holidays_path: None,
        folds: 0,
        aux_paths: vec![("idx".into(), aux_f.path().to_path_buf())],
    };

    let report = run(&cfg, &LlmEvaluator::Disabled).await.unwrap();
    let m = &report.metrics;
    assert!(
        m.scored > 0,
        "aux relative-strength: expected scored > 0, got {}",
        m.scored
    );
    assert!(
        m.active.count > 0,
        "primary outperforms aux => long branch should fire; active.count={}",
        m.active.count
    );
}

// F4 T3 — risk_metrics_html_contains_sharpe: render_sim_html and render_portfolio_html include
// "Sharpe" in their headline tables (always emitted regardless of whether risk is None or Some).
#[tokio::test]
async fn risk_metrics_html_contains_sharpe() {
    use rquant::backtest::portfolio::{PortfolioConfig, run_portfolio};
    use std::io::Write as _;

    // ── sim HTML ─────────────────────────────────────────────────────────────
    const SIM_TREE: &str = r#"
meta: { name: sharpe_html_sim, forward_window: 1, stances: [long, flat] }
root: gate
nodes:
  gate:
    type: quant
    branches:
      - when: "pos == 0 and close > 0"
        goto: leaf_long
        label: enter
      - when: "pos > 0 and bars_held >= 8"
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
    let tree_f = write_file(SIM_TREE, ".yaml");
    let primary_f = write_file(&gen_primary_csv(), ".csv");
    let context_f = write_file(&gen_context_csv(), ".csv");
    let out_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();

    let sim_cfg = BacktestConfig {
        tree_path: tree_f.path().to_path_buf(),
        primary_path: primary_f.path().to_path_buf(),
        context_path: context_f.path().to_path_buf(),
        news_path: None,
        out_path: out_f.path().to_path_buf(),
        traces_path: None,
        cost_bps: 10.0,
        warmup: 5,
        window: 100,
        concurrency: 4,
        holidays_path: None,
        folds: 0,
        aux_paths: vec![],
    };
    let sim_report = run_sim(&sim_cfg, &LlmEvaluator::Disabled, false)
        .await
        .expect("run_sim should succeed");
    let sim_html = rquant::report::viz::render_sim_html(&sim_report, None);
    assert!(
        sim_html.contains("Sharpe"),
        "sim HTML must contain 'Sharpe' in headline table"
    );

    // ── portfolio HTML ────────────────────────────────────────────────────────
    const MOMENTUM_TREE: &str = r#"
meta: { name: sharpe_html_port, forward_window: 1, stances: [long, flat] }
root: gate
nodes:
  gate:
    type: quant
    branches:
      - when: "close > sma(close, 3)"
        goto: leaf_long
        label: up
    default: { goto: leaf_flat, label: flat }
leaves:
  leaf_long: { stance: long, weight: 1.0 }
  leaf_flat: { stance: flat }
"#;
    let days: Vec<u32> = (2u32..=11).collect();
    let hm: &[(u32, u32)] = &[(9, 30), (10, 0), (10, 30), (11, 0)];
    let mut timestamps = Vec::new();
    for &d in &days {
        for &(h, m) in hm {
            use chrono::NaiveDate;
            timestamps.push(
                NaiveDate::from_ymd_opt(2024, 1, d)
                    .unwrap()
                    .and_hms_opt(h, m, 0)
                    .unwrap(),
            );
        }
    }
    let write_bars_csv = |start: f64, pct: f64| -> tempfile::NamedTempFile {
        let mut f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        writeln!(f, "time,open,high,low,close,volume").unwrap();
        let mut price = start;
        for ts in &timestamps {
            writeln!(
                f,
                "{},{p:.6},{p:.6},{p:.6},{p:.6},1000",
                ts.format("%Y-%m-%d %H:%M:%S"),
                p = price
            )
            .unwrap();
            price *= 1.0 + pct;
        }
        f.flush().unwrap();
        f
    };
    let f_a = write_bars_csv(100.0, 0.01);
    let f_b = write_bars_csv(100.0, 0.0);
    let f_c = write_bars_csv(100.0, -0.01);
    let mut univ_f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
    writeln!(
        univ_f,
        "symbol,primary\nA,{}\nB,{}\nC,{}",
        f_a.path().display(),
        f_b.path().display(),
        f_c.path().display()
    )
    .unwrap();
    univ_f.flush().unwrap();
    let port_tree_f = write_file(MOMENTUM_TREE, ".yaml");
    let port_out_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
    let port_cfg = PortfolioConfig {
        tree_path: port_tree_f.path().to_path_buf(),
        universe_path: univ_f.path().to_path_buf(),
        top: 1,
        rebalance: 4,
        warmup: 6,
        window: 10,
        cost_bps: 10.0,
        soft: false,
        aux_paths: Vec::new(),
        out_path: port_out_f.path().to_path_buf(),
        traces_path: None,
    };
    let port_report = run_portfolio(&port_cfg, &LlmEvaluator::Disabled)
        .await
        .expect("run_portfolio should succeed");
    let port_html = rquant::report::viz::render_portfolio_html(&port_report);
    assert!(
        port_html.contains("Sharpe"),
        "portfolio HTML must contain 'Sharpe' in headline table"
    );
}

// E4 T5 — sim_full_chain: enter/exit/hold tree with pos conditions through run_sim (hard and soft)
// Tree: pos==0 and close>0 → long; pos>0 and bars_held>=8 → flat; pos>0 → long; default flat
// gen_primary_csv produces 40 bars (5 days × 8 bars/day), steadily uptrending from 10.0 to 13.9.
// warmup=5 → loop from bar5 to bar38 → 34 decision steps.
// bars_held>=8 fires after at least 8 held bars, yielding ≥1 round trip.
#[tokio::test]
async fn sim_full_chain() {
    const SIM_TREE: &str = r#"
meta: { name: sim_e2e, forward_window: 1, stances: [long, flat] }
root: gate
nodes:
  gate:
    type: quant
    branches:
      - when: "pos == 0 and close > 0"
        goto: leaf_long
        label: enter
      - when: "pos > 0 and bars_held >= 8"
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

    let tree_f = write_file(SIM_TREE, ".yaml");
    let primary_f = write_file(&gen_primary_csv(), ".csv");
    let context_f = write_file(&gen_context_csv(), ".csv");
    let out_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
    let out_soft_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();

    let base_cfg = BacktestConfig {
        tree_path: tree_f.path().to_path_buf(),
        primary_path: primary_f.path().to_path_buf(),
        context_path: context_f.path().to_path_buf(),
        news_path: None,
        out_path: out_f.path().to_path_buf(),
        traces_path: None,
        cost_bps: 10.0,
        warmup: 5,
        window: 100,
        concurrency: 4,
        holidays_path: None,
        folds: 0,
        aux_paths: vec![],
    };

    // Hard mode
    let report = run_sim(&base_cfg, &LlmEvaluator::Disabled, false)
        .await
        .expect("run_sim hard should succeed");
    assert!(
        report.total_return.is_finite(),
        "total_return must be finite, got {}",
        report.total_return
    );
    assert!(
        report.n_round_trips >= 1,
        "uptrend + bars_held>=8 exit should yield >=1 round trip, got {}",
        report.n_round_trips
    );
    // Verify the JSON output was written
    let json_content = std::fs::read_to_string(out_f.path()).unwrap();
    assert!(
        json_content.contains("sim_e2e"),
        "output JSON should contain tree name"
    );

    // F4 T3 — risk metrics: gen_primary_csv spans Jan 2–6 (4 calendar days < 30-day threshold).
    // Annualised metrics (ann_return / ann_vol / sharpe / sortino / calmar) must be None.
    // VaR95 must be finite (always computed from per-step returns regardless of span).
    let risk = report.risk.as_ref().expect("risk must be Some (>1 nav point)");
    assert!(
        risk.ann_return.is_none(),
        "span < 30 days → ann_return must be None"
    );
    assert!(
        risk.ann_vol.is_none(),
        "span < 30 days → ann_vol must be None"
    );
    assert!(
        risk.sharpe.is_none(),
        "span < 30 days → sharpe must be None"
    );
    assert!(
        risk.sortino.is_none(),
        "span < 30 days → sortino must be None"
    );
    assert!(
        risk.calmar.is_none(),
        "span < 30 days → calmar must be None"
    );
    assert!(
        risk.var95.is_finite(),
        "var95 must be finite regardless of span, got {}",
        risk.var95
    );
    assert!(
        risk.cvar95.is_finite(),
        "cvar95 must be finite regardless of span, got {}",
        risk.cvar95
    );

    // Soft mode: same tree, should complete without error and produce finite result
    let soft_cfg = BacktestConfig {
        out_path: out_soft_f.path().to_path_buf(),
        ..base_cfg
    };
    let soft_report = run_sim(&soft_cfg, &LlmEvaluator::Disabled, true)
        .await
        .expect("run_sim soft should succeed");
    assert!(
        soft_report.total_return.is_finite(),
        "soft total_return must be finite, got {}",
        soft_report.total_return
    );
}

// E5 T4 — portfolio_full_chain: 3 synthetic symbols (A+1%/bar, B flat, C-1%/bar),
// momentum tree (close > sma(close,3) → long else flat), top=1, rebalance=4, warmup=6,
// cost_bps=10, LlmEvaluator::Disabled → all selected == "A", total_return > benchmark_return,
// out JSON deserializes to PortfolioReport.
#[tokio::test]
async fn portfolio_full_chain() {
    use rquant::backtest::portfolio::{PortfolioConfig, PortfolioReport, run_portfolio};
    use rquant::eval::llm::LlmEvaluator;
    use std::io::Write as _;

    const MOMENTUM_TREE: &str = r#"
meta: { name: momentum_e2e, forward_window: 1, stances: [long, flat] }
root: gate
nodes:
  gate:
    type: quant
    branches:
      - when: "close > sma(close, 3)"
        goto: leaf_long
        label: up
    default: { goto: leaf_flat, label: flat }
leaves:
  leaf_long: { stance: long, weight: 1.0 }
  leaf_flat: { stance: flat }
"#;

    // Time grid: 10 days × 4 bars/day = 40 bars (warmup=6, rebalance=4 → rb_indices: 6,10,14,…)
    let days: Vec<u32> = (2u32..=11).collect(); // Jan 2–11
    let hm: &[(u32, u32)] = &[(9, 30), (10, 0), (10, 30), (11, 0)];
    let mut timestamps = Vec::new();
    for &d in &days {
        for &(h, m) in hm {
            use chrono::NaiveDate;
            timestamps.push(
                NaiveDate::from_ymd_opt(2024, 1, d)
                    .unwrap()
                    .and_hms_opt(h, m, 0)
                    .unwrap(),
            );
        }
    }

    // Write bars CSV for one symbol: start price, pct change per bar
    let write_bars_csv = |start: f64, pct: f64| -> tempfile::NamedTempFile {
        let mut f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        writeln!(f, "time,open,high,low,close,volume").unwrap();
        let mut price = start;
        for ts in &timestamps {
            writeln!(
                f,
                "{},{p:.6},{p:.6},{p:.6},{p:.6},1000",
                ts.format("%Y-%m-%d %H:%M:%S"),
                p = price
            )
            .unwrap();
            price *= 1.0 + pct;
        }
        f.flush().unwrap();
        f
    };

    let f_a = write_bars_csv(100.0, 0.01);   // A: +1%/bar
    let f_b = write_bars_csv(100.0, 0.0);    // B: flat
    let f_c = write_bars_csv(100.0, -0.01);  // C: -1%/bar

    // Universe CSV
    let mut univ_f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
    writeln!(
        univ_f,
        "symbol,primary\nA,{}\nB,{}\nC,{}",
        f_a.path().display(),
        f_b.path().display(),
        f_c.path().display()
    )
    .unwrap();
    univ_f.flush().unwrap();

    // Tree tempfile
    let tree_f = write_file(MOMENTUM_TREE, ".yaml");

    // Output files
    let out_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
    let traces_f = tempfile::Builder::new().suffix(".jsonl").tempfile().unwrap();

    let cfg = PortfolioConfig {
        tree_path: tree_f.path().to_path_buf(),
        universe_path: univ_f.path().to_path_buf(),
        top: 1,
        rebalance: 4,
        warmup: 6,
        window: 10,
        cost_bps: 10.0,
        soft: false,
        aux_paths: Vec::new(),
        out_path: out_f.path().to_path_buf(),
        traces_path: Some(traces_f.path().to_path_buf()),
    };

    let report = run_portfolio(&cfg, &LlmEvaluator::Disabled)
        .await
        .expect("portfolio_full_chain: run_portfolio should succeed");

    // All rebalances should select only "A"
    for rec in &report.holdings {
        assert_eq!(
            rec.selected.len(),
            1,
            "expected 1 selected, got {:?}",
            rec.selected
        );
        assert_eq!(
            rec.selected[0].0, "A",
            "expected A selected, got {}",
            rec.selected[0].0
        );
    }

    // Portfolio should beat benchmark (A outperforms equal-weight A/B/C)
    assert!(
        report.total_return > report.benchmark_return,
        "total_return ({}) must exceed benchmark_return ({})",
        report.total_return,
        report.benchmark_return
    );

    // Out JSON must round-trip as PortfolioReport
    let json_content = std::fs::read_to_string(out_f.path()).unwrap();
    let parsed: PortfolioReport =
        serde_json::from_str(&json_content).expect("out JSON must deserialize to PortfolioReport");
    assert_eq!(parsed.tree_name, report.tree_name);
    assert_eq!(parsed.n_rebalances, report.n_rebalances);

    // At least 2 rebalances
    assert!(
        report.n_rebalances >= 2,
        "expected n_rebalances >= 2, got {}",
        report.n_rebalances
    );

    // F4 T3 — risk metrics: portfolio fixture spans Jan 2–11 (9 calendar days < 30-day threshold).
    // Annualised metrics must be None; VaR95/CVaR95 must be finite.
    let prisk = report.risk.as_ref().expect("portfolio risk must be Some (>1 holdings point)");
    assert!(
        prisk.ann_return.is_none(),
        "portfolio span < 30 days → ann_return must be None"
    );
    assert!(
        prisk.var95.is_finite(),
        "portfolio var95 must be finite regardless of span, got {}",
        prisk.var95
    );
    assert!(
        prisk.cvar95.is_finite(),
        "portfolio cvar95 must be finite regardless of span, got {}",
        prisk.cvar95
    );
}

// E4 T5 — sim_legacy_tree_compat: legacy quant tree without pos conditions runs through --sim
// without panic; naive rebalancing semantics (always long when above SMA) → Ok result.
#[tokio::test]
async fn sim_legacy_tree_compat() {
    let tree_f = write_file(&tree_yaml(), ".yaml");
    let primary_f = write_file(&gen_primary_csv(), ".csv");
    let context_f = write_file(&gen_context_csv(), ".csv");
    let out_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();

    let cfg = BacktestConfig {
        tree_path: tree_f.path().to_path_buf(),
        primary_path: primary_f.path().to_path_buf(),
        context_path: context_f.path().to_path_buf(),
        news_path: None,
        out_path: out_f.path().to_path_buf(),
        traces_path: None,
        cost_bps: 10.0,
        warmup: 5,
        window: 100,
        concurrency: 4,
        holidays_path: None,
        folds: 0,
        aux_paths: vec![],
    };

    // Legacy tree (no pos conditions) through run_sim must not panic and return Ok
    let report = run_sim(&cfg, &LlmEvaluator::Disabled, false)
        .await
        .expect("sim on legacy tree should not panic");
    assert!(
        report.total_return.is_finite(),
        "legacy tree sim total_return must be finite"
    );
}

// T3 Step 3 — sim_report_html_renders: run_sim with traces, render ReportMode::Sim, assert HTML
// contains <polyline (nav curve from steps) and 回合 (round-trip table).
#[tokio::test]
async fn sim_report_html_renders() {
    const SIM_TREE: &str = r#"
meta: { name: sim_html_e2e, forward_window: 1, stances: [long, flat] }
root: gate
nodes:
  gate:
    type: quant
    branches:
      - when: "pos == 0 and close > 0"
        goto: leaf_long
        label: enter
      - when: "pos > 0 and bars_held >= 8"
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

    let tree_f = write_file(SIM_TREE, ".yaml");
    let primary_f = write_file(&gen_primary_csv(), ".csv");
    let context_f = write_file(&gen_context_csv(), ".csv");
    let out_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
    let traces_f = tempfile::Builder::new().suffix(".jsonl").tempfile().unwrap();

    let cfg = BacktestConfig {
        tree_path: tree_f.path().to_path_buf(),
        primary_path: primary_f.path().to_path_buf(),
        context_path: context_f.path().to_path_buf(),
        news_path: None,
        out_path: out_f.path().to_path_buf(),
        traces_path: Some(traces_f.path().to_path_buf()),
        cost_bps: 10.0,
        warmup: 5,
        window: 100,
        concurrency: 4,
        holidays_path: None,
        folds: 0,
        aux_paths: vec![],
    };

    run_sim(&cfg, &LlmEvaluator::Disabled, false)
        .await
        .expect("run_sim should succeed");

    let html_f = tempfile::Builder::new().suffix(".html").tempfile().unwrap();
    rquant::report::render_report_files(
        out_f.path(),
        html_f.path(),
        Some(traces_f.path()),
        None,
        rquant::report::ReportMode::Sim,
    )
    .unwrap();

    let html = std::fs::read_to_string(html_f.path()).unwrap();
    assert!(!html.is_empty(), "sim HTML must be non-empty");
    assert!(html.contains("<polyline"), "sim HTML must contain nav curve polyline");
    assert!(html.contains("回合"), "sim HTML must contain round-trip table");
}

// F1 T5 — factor_full_chain: 6-symbol synthetic universe (ascending growth rates),
// dual factors mom=close/ref(close,4)-1 and rev=ref(close,4)/close-1,
// run_factor → JSON deserializes to FactorReport; mom rank_ic_mean > 0.9, rev < -0.9;
// corr[0][1] < -0.9; render_factor_html contains "RankIC".
#[test]
fn factor_full_chain() {
    use chrono::NaiveDate;
    use rquant::factor::{FactorConfig, FactorReport, FactorSpecItem, run_factor};
    use std::io::Write as _;

    // ── fixture helpers ────────────────────────────────────────────────────────
    // 12 days × 4 bars/day = 48 bars (same pattern as factor/mod.rs tests)
    let days: Vec<u32> = (2u32..=13).collect();
    let hm: Vec<(u32, u32)> = vec![(9, 30), (10, 0), (10, 30), (11, 0)];
    let mut timestamps = Vec::new();
    for &d in &days {
        for &(h, m) in &hm {
            timestamps.push(
                NaiveDate::from_ymd_opt(2024, 1, d)
                    .unwrap()
                    .and_hms_opt(h, m, 0)
                    .unwrap(),
            );
        }
    }

    // Write a price CSV for a symbol with constant growth rate g
    let write_price_csv = |g: f64| -> tempfile::NamedTempFile {
        let mut f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        writeln!(f, "time,open,high,low,close,volume").unwrap();
        let mut price = 10.0f64;
        for ts in &timestamps {
            writeln!(
                f,
                "{},{:.8},{:.8},{:.8},{:.8},1000",
                ts.format("%Y-%m-%d %H:%M:%S"),
                price,
                price,
                price,
                price
            )
            .unwrap();
            price *= 1.0 + g;
        }
        f.flush().unwrap();
        f
    };

    // 6 symbols with ascending growth rates → momentum factor ranks align with returns
    let growth_rates = [0.001f64, 0.002, 0.003, 0.004, 0.005, 0.006];
    let symbols = ["e2e_s1", "e2e_s2", "e2e_s3", "e2e_s4", "e2e_s5", "e2e_s6"];
    let bar_files: Vec<_> = growth_rates.iter().map(|&g| write_price_csv(g)).collect();

    // Universe CSV
    let mut univ_f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
    writeln!(univ_f, "symbol,primary").unwrap();
    for (sym, bf) in symbols.iter().zip(bar_files.iter()) {
        writeln!(univ_f, "{},{}", sym, bf.path().to_str().unwrap()).unwrap();
    }
    univ_f.flush().unwrap();

    // Output files
    let out_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
    let html_f = tempfile::Builder::new().suffix(".html").tempfile().unwrap();

    // ── run_factor ─────────────────────────────────────────────────────────────
    let cfg = FactorConfig {
        universe_path: univ_f.path().to_path_buf(),
        factors: vec![
            FactorSpecItem {
                name: "mom".into(),
                expr: "close/ref(close,4)-1".into(),
            },
            FactorSpecItem {
                name: "rev".into(),
                expr: "ref(close,4)/close-1".into(),
            },
        ],
        sample: 4,
        horizon: 4,
        layers: 3,
        warmup: 8,
        window: 20,
        out_path: out_f.path().to_path_buf(),
        html_path: Some(html_f.path().to_path_buf()),
    };

    let report = run_factor(&cfg).expect("factor_full_chain: run_factor should succeed");

    // ── JSON round-trip ────────────────────────────────────────────────────────
    let json_content = std::fs::read_to_string(out_f.path()).unwrap();
    let parsed: FactorReport =
        serde_json::from_str(&json_content).expect("out JSON must deserialize to FactorReport");
    assert_eq!(parsed.factors.len(), 2, "should have 2 factors in parsed report");

    // ── mom: rank_ic_mean > 0.9 ────────────────────────────────────────────────
    let mom = &report.factors[0];
    assert_eq!(mom.name, "mom");
    let mom_rim = mom.rank_ic_mean.expect("mom rank_ic_mean should be Some");
    assert!(
        mom_rim > 0.9,
        "mom rank_ic_mean should be > 0.9 (monotone growth rates), got {mom_rim}"
    );

    // ── rev: rank_ic_mean < -0.9 ───────────────────────────────────────────────
    let rev = &report.factors[1];
    assert_eq!(rev.name, "rev");
    let rev_rim = rev.rank_ic_mean.expect("rev rank_ic_mean should be Some");
    assert!(
        rev_rim < -0.9,
        "rev rank_ic_mean should be < -0.9 (reverse of monotone growth), got {rev_rim}"
    );

    // ── corr[0][1] < -0.9 (mom and rev are exact inverses) ────────────────────
    let corr = report.corr.as_ref().expect("corr should be Some for 2 factors");
    let c01 = corr.values[0][1].expect("corr[0][1] should be Some");
    assert!(
        c01 < -0.9,
        "corr[0][1] (mom vs rev) should be < -0.9, got {c01}"
    );

    // ── render HTML and check "RankIC" ─────────────────────────────────────────
    let html_str = rquant::report::viz::render_factor_html(&report);
    std::fs::write(html_f.path(), &html_str).unwrap();
    let html_content = std::fs::read_to_string(html_f.path()).unwrap();
    assert!(
        html_content.contains("RankIC"),
        "factor HTML must contain 'RankIC'"
    );
    assert!(
        html_content.contains("<!doctype html>"),
        "factor HTML must be a valid HTML document"
    );
    assert!(
        html_content.contains("<polyline"),
        "factor HTML must contain at least one polyline (IC decay chart)"
    );
}

// F2 T5 — optimize_finds_planted_optimum:
// Synthetic rising data (close 10→~20, ≥80 bars, multi-day) + inline tree
// (params: {thr: 5.0}, `close > thr` → long else flat, forward_window=2).
// Grid: thr ∈ {5, 15, 100}. With rising data close∈[10,20], thr=5 always fires long
// (positive edge), thr=15 fires only partially, thr=100 never fires (no edge).
// Assertions: every fold best_params["thr"]==5.0; drift[0].n_unique==1;
// full_sample_best params thr==5.0; os_mean_objective Some && >0;
// out JSON deserializes to OptimizeReport.
#[tokio::test]
async fn optimize_finds_planted_optimum() {
    use std::io::Write as IoWrite;

    // ── synthetic rising CSV (≥80 bars, multi-day, close 10→20) ─────────────
    fn gen_rising_csv(n_bars: usize) -> String {
        let mut s = String::from("time,open,high,low,close,volume\n");
        // Spread across 10 days (8 bars/day = 80 bars), price 10 → 20
        let price_step = 10.0 / (n_bars - 1) as f64;
        let mut day = 2u32;  // Jan 2
        let mut intraday = 0u32;
        for i in 0..n_bars {
            let price = 10.0 + price_step * i as f64;
            // 8 bars per day, slots: 09:45, 10:00, 10:15, 10:30, 10:45, 11:00, 11:15, 11:30
            let hour = 9u32 + (45 + intraday * 15) / 60;
            let minute = (45 + intraday * 15) % 60;
            s.push_str(&format!(
                "2024-01-{:02} {:02}:{:02}:00,{p:.4},{p:.4},{p:.4},{p:.4},1000\n",
                day, hour, minute, p = price
            ));
            intraday += 1;
            if intraday >= 8 {
                intraday = 0;
                day += 1;
            }
        }
        s
    }

    const N: usize = 80;
    let primary_csv = gen_rising_csv(N);

    // Context CSV: a few daily bars (will be used for context lookup)
    let context_csv = {
        let mut s = String::from("time,open,high,low,close,volume\n");
        for d in 2u32..=12 {
            s.push_str(&format!(
                "2024-01-{:02} 09:30:00,10.0,10.0,10.0,10.0,1\n",
                d
            ));
        }
        s
    };

    let mut primary_f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
    primary_f.write_all(primary_csv.as_bytes()).unwrap();
    primary_f.flush().unwrap();

    let mut context_f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
    context_f.write_all(context_csv.as_bytes()).unwrap();
    context_f.flush().unwrap();

    // Inline tree: params: {thr: 5.0}, `close > thr` → long else flat, forward_window=2
    const OPT_TREE: &str = r#"
meta: { name: planted_opt, forward_window: 2, stances: [long, flat] }
params: { thr: 5.0 }
root: gate
nodes:
  gate:
    type: quant
    branches:
      - when: "close > thr"
        goto: leaf_long
        label: above
    default: { goto: leaf_flat, label: below }
leaves:
  leaf_long: { stance: long }
  leaf_flat: { stance: flat }
"#;
    let tree_f = write_file(OPT_TREE, ".yaml");
    let out_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();

    let cfg = OptimizeConfig {
        tree_path: tree_f.path().to_path_buf(),
        primary_path: primary_f.path().to_path_buf(),
        context_path: context_f.path().to_path_buf(),
        news_path: None,
        aux_paths: vec![],
        window: 20,
        warmup: 5,
        cost_bps: 10.0,
        folds: 4,
        sim: false,
        soft: false,
        grids: vec!["thr=5,15,100".to_string()],
        max_combos: 500,
        out_path: out_f.path().to_path_buf(),
    };

    let report = run_optimize(&cfg, &LlmEvaluator::Disabled)
        .await
        .expect("optimize_finds_planted_optimum: run_optimize should succeed");

    // Every fold best_params["thr"] == 5.0
    for fr in &report.fold_results {
        let bp = fr.best_params.as_ref().expect("every fold should have a best_params");
        let thr = bp.get("thr").copied().expect("thr key must be present");
        assert!(
            (thr - 5.0).abs() < 1e-9,
            "fold {} best thr should be 5.0 (planted optimum), got {thr}",
            fr.fold
        );
    }

    // drift[0].n_unique == 1 (always picks thr=5.0)
    assert!(!report.drift.is_empty(), "drift should be non-empty");
    assert_eq!(
        report.drift[0].n_unique, 1,
        "all folds should converge on thr=5.0 → n_unique=1, got {}",
        report.drift[0].n_unique
    );

    // full_sample_best params["thr"] == 5.0
    let fsb = report.full_sample_best.as_ref().expect("full_sample_best should be Some");
    let fsb_thr = fsb.params.get("thr").copied().expect("full_sample_best thr key missing");
    assert!(
        (fsb_thr - 5.0).abs() < 1e-9,
        "full_sample_best thr should be 5.0, got {fsb_thr}"
    );

    // os_mean_objective Some && > 0
    let os_mean = report.os_mean_objective.expect("os_mean_objective should be Some");
    assert!(
        os_mean > 0.0,
        "os_mean_objective should be positive (rising data + always-above-threshold), got {os_mean}"
    );

    // out JSON deserializes to OptimizeReport
    let json_content = std::fs::read_to_string(out_f.path()).unwrap();
    let _parsed: OptimizeReport =
        serde_json::from_str(&json_content).expect("out JSON must deserialize to OptimizeReport");
}

// T3 Step 3 — portfolio_report_html_renders: run_portfolio, render ReportMode::Portfolio,
// assert HTML has exactly 2 <polyline elements (portfolio + benchmark) and contains 基准.
#[tokio::test]
async fn portfolio_report_html_renders() {
    use rquant::backtest::portfolio::{PortfolioConfig, run_portfolio};
    use std::io::Write as _;

    const MOMENTUM_TREE: &str = r#"
meta: { name: momentum_html_e2e, forward_window: 1, stances: [long, flat] }
root: gate
nodes:
  gate:
    type: quant
    branches:
      - when: "close > sma(close, 3)"
        goto: leaf_long
        label: up
    default: { goto: leaf_flat, label: flat }
leaves:
  leaf_long: { stance: long, weight: 1.0 }
  leaf_flat: { stance: flat }
"#;

    let days: Vec<u32> = (2u32..=11).collect();
    let hm: &[(u32, u32)] = &[(9, 30), (10, 0), (10, 30), (11, 0)];
    let mut timestamps = Vec::new();
    for &d in &days {
        for &(h, m) in hm {
            use chrono::NaiveDate;
            timestamps.push(
                NaiveDate::from_ymd_opt(2024, 1, d)
                    .unwrap()
                    .and_hms_opt(h, m, 0)
                    .unwrap(),
            );
        }
    }

    let write_bars_csv = |start: f64, pct: f64| -> tempfile::NamedTempFile {
        let mut f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
        writeln!(f, "time,open,high,low,close,volume").unwrap();
        let mut price = start;
        for ts in &timestamps {
            writeln!(
                f,
                "{},{p:.6},{p:.6},{p:.6},{p:.6},1000",
                ts.format("%Y-%m-%d %H:%M:%S"),
                p = price
            )
            .unwrap();
            price *= 1.0 + pct;
        }
        f.flush().unwrap();
        f
    };

    let f_a = write_bars_csv(100.0, 0.01);
    let f_b = write_bars_csv(100.0, 0.0);
    let f_c = write_bars_csv(100.0, -0.01);

    let mut univ_f = tempfile::Builder::new().suffix(".csv").tempfile().unwrap();
    writeln!(
        univ_f,
        "symbol,primary\nA,{}\nB,{}\nC,{}",
        f_a.path().display(),
        f_b.path().display(),
        f_c.path().display()
    )
    .unwrap();
    univ_f.flush().unwrap();

    let tree_f = write_file(MOMENTUM_TREE, ".yaml");
    let out_f = tempfile::Builder::new().suffix(".json").tempfile().unwrap();

    let cfg = PortfolioConfig {
        tree_path: tree_f.path().to_path_buf(),
        universe_path: univ_f.path().to_path_buf(),
        top: 1,
        rebalance: 4,
        warmup: 6,
        window: 10,
        cost_bps: 10.0,
        soft: false,
        aux_paths: Vec::new(),
        out_path: out_f.path().to_path_buf(),
        traces_path: None,
    };

    run_portfolio(&cfg, &LlmEvaluator::Disabled)
        .await
        .expect("run_portfolio should succeed");

    let html_f = tempfile::Builder::new().suffix(".html").tempfile().unwrap();
    rquant::report::render_report_files(
        out_f.path(),
        html_f.path(),
        None,
        None,
        rquant::report::ReportMode::Portfolio,
    )
    .unwrap();

    let html = std::fs::read_to_string(html_f.path()).unwrap();
    assert!(!html.is_empty(), "portfolio HTML must be non-empty");
    assert_eq!(
        html.matches("<polyline").count(),
        2,
        "portfolio HTML must have exactly 2 polylines (portfolio + benchmark)"
    );
    assert!(html.contains("基准"), "portfolio HTML must contain 基准 legend");
}

// ──────────────────────────────────────────────────────────────────────────────
// F-9 Task 5 e2e tests
// ──────────────────────────────────────────────────────────────────────────────

fn signal_enter_hold_exit_tree() -> String {
    r#"
meta: { name: sig_e2e, forward_window: 1, stances: [long, flat] }
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

fn signal_gen_primary_csv() -> String {
    // 4 天×8 根（非顶层 gen_primary_csv 的 5 天）：让 day-1 前缀恰切在 16 根
    // 32 bars: 4 days × 8 bars/day, 09:45 start, every 15 min
    let mut s = String::from("time,open,high,low,close,volume\n");
    let mut idx = 0usize;
    for day in 0..4usize {
        for k in 0..8usize {
            let price = 10.0 + 0.1 * idx as f64;
            let hour = 9 + (45 + k * 15) / 60;
            let minute = (45 + k * 15) % 60;
            s.push_str(&format!(
                "2024-01-{:02} {:02}:{:02}:00,{p},{p},{p},{p},1000\n",
                2 + day, hour, minute, p = price
            ));
            idx += 1;
        }
    }
    s
}

fn signal_gen_context_csv() -> String {
    String::from(
        "time,open,high,low,close,volume\n\
         2024-01-02 10:30:00,10.0,10.0,10.0,10.0,1\n\
         2024-01-02 11:30:00,10.1,10.1,10.1,10.1,1\n\
         2024-01-03 10:30:00,10.2,10.2,10.2,10.2,1\n\
         2024-01-04 10:30:00,10.3,10.3,10.3,10.3,1\n\
         2024-01-05 10:30:00,10.4,10.4,10.4,10.4,1\n",
    )
}

fn make_single_cfg(
    tree_path: &std::path::Path,
    primary_path: &std::path::Path,
    context_path: &std::path::Path,
    state_path: &std::path::Path,
) -> SignalSingleConfig {
    SignalSingleConfig {
        tree_path: tree_path.to_path_buf(),
        primary_path: primary_path.to_path_buf(),
        context_path: context_path.to_path_buf(),
        news_path: None,
        aux_paths: vec![],
        window: 100,
        warmup: 5, // warmup=5：32-bar 数据下 day-1 留 10 根可记账 bar，与断言数值对齐
        cost_bps: 10.0,
        soft: false,
        state_path: state_path.to_path_buf(),
    }
}

// Enter/hold/exit tree 与 gen_primary_csv (32 bar, 4 days × 8 bars) 相同的合成树。
// warmup=5, 全量 bars_replayed = 26 (i=5..30, loop 5..31, 共 26 根可记账决策 bar)。
// Day-1 前缀 = 前 16 bar (2 天 × 8, len=16)，loop warmup..len-1 = 5..15 → i=5..=14 (10 根)。
// Day-2 增量 = 从 state_1 跑全量 (len=32)，loop 5..31，跳过 i<=14，实际重放 i=15..=30 → 16 根。
// 断言：split==full 不变量（serde_json::Value 相等）+ 第二跑精确值 bars_replayed=16。
// F-9 T5 — signal_two_day_paper_flow:
// 合成 32-bar 上行数据 (4 days × 8 bars)，cut 前 16 bars 作"第一天"前缀。
// Step A: 第一天（16 bar 前缀）→ state_day1（bars_replayed=10，i=5..14）
// Step B: --commit → write_paper_state → 再跑全量 → state_full2（bars_replayed=16，i=15..=30）
// split==full 不变量：全量 fresh state_full 与 split 后的 state_full2 的 serde_json::Value 相等。
// 精确断言：第二跑 bars_replayed == 16（硬算：warmup=5, 全量 len=32, day-1 prefix len=16;
//   day-1 可记账 bar: i=5..14 共 10 根（state day1 last_time = bar[14].time）;
//   day-2 增量: i=15..=30 共 16 根 → bars_replayed_2 = 16）。
#[tokio::test]
async fn signal_two_day_paper_flow() {
    let tree_f = write_file(&signal_enter_hold_exit_tree(), ".yaml");
    let full_csv = signal_gen_primary_csv();
    let context_csv = signal_gen_context_csv();
    let context_f = write_file(&context_csv, ".csv");
    let full_f = write_file(&full_csv, ".csv");
    let tmp = tempfile::tempdir().unwrap();

    // ── A: full fresh run (baseline) ────────────────────────────────────────
    let state_full_path = tmp.path().join("state_full.json");
    let cfg_full = make_single_cfg(tree_f.path(), full_f.path(), context_f.path(), &state_full_path);
    let (_sig_full, state_full) = run_signal_single(&cfg_full, &LlmEvaluator::Disabled)
        .await
        .unwrap();

    // ── B: day-1 prefix = first 16 bars (header + 16 data rows) ────────────
    let lines: Vec<&str> = full_csv.lines().collect();
    // lines[0] = header, lines[1..=16] = bar[0..15]
    let prefix_csv = std::iter::once(lines[0])
        .chain(lines[1..=16].iter().copied())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let prefix_f = write_file(&prefix_csv, ".csv");

    let state_day1_path = tmp.path().join("state_day1.json");
    let cfg_day1 = make_single_cfg(tree_f.path(), prefix_f.path(), context_f.path(), &state_day1_path);
    let (_sig_day1, state_day1) = run_signal_single(&cfg_day1, &LlmEvaluator::Disabled)
        .await
        .unwrap();

    // ── Simulate --commit: write_paper_state ──────────────────────────────
    write_paper_state(&state_day1_path, &state_day1).unwrap();

    // ── C: second run from state_day1, on full data ──────────────────────
    let state_full2_path = tmp.path().join("state_full2.json");
    std::fs::copy(&state_day1_path, &state_full2_path).unwrap();

    let cfg_full2 = make_single_cfg(tree_f.path(), full_f.path(), context_f.path(), &state_full2_path);
    let (sig_full2, state_full2) = run_signal_single(&cfg_full2, &LlmEvaluator::Disabled)
        .await
        .unwrap();

    // ── split==full invariant ─────────────────────────────────────────────
    let val_full = serde_json::to_value(&state_full).unwrap();
    let val_full2 = serde_json::to_value(&state_full2).unwrap();
    assert_eq!(
        val_full, val_full2,
        "split==full invariant FAILED:\nfull={val_full}\nfull2={val_full2}"
    );

    // ── second run bars_replayed == 16 (precise) ─────────────────────────
    // warmup=5, full len=32, day-1 prefix len=16
    // day-1 accountable: i=5..14 → loop warmup..len-1 = 5..15, so i=5..=14 (10 bars),
    //   last_time = bar[14].time
    // day-2 incremental: full len=32, loop 5..31, skip i<=14, replay i=15..=30 → 16 bars
    assert_eq!(
        sig_full2.paper.bars_replayed, 16,
        "second run must replay exactly 16 new accountable bars (i=15..30), got {}",
        sig_full2.paper.bars_replayed
    );

    // ── verify read_paper_state round-trip ───────────────────────────────
    let loaded = read_paper_state(&state_full2_path, "sig_e2e")
        .unwrap()
        .expect("state_full2 must exist after second run");
    assert_eq!(loaded.tree_name, "sig_e2e");
}

// F-9 T5 — signal_portfolio_diff_chain:
// 3-symbol universe (uniform tree → all long, weight=0.5, score=0.5)
// Run 1: empty state → all 3 symbols Buy → write_holdings_state (--commit)
// Run 2: same data → all Hold (持仓与目标一致)
#[tokio::test]
async fn signal_portfolio_diff_chain() {
    const UNIFORM_TREE: &str = r#"
meta: { name: port_e2e, forward_window: 1, stances: [long] }
root: router
nodes:
  router:
    type: quant
    branches: []
    default: { goto: leaf_long, label: uniform }
leaves:
  leaf_long: { stance: long, weight: 0.5 }
"#;

    let tree_f = write_file(UNIFORM_TREE, ".yaml");

    // 3 symbols, each with 8 bars on the same day
    let bars_csv = {
        let mut s = String::from("time,open,high,low,close,volume\n");
        for i in 0..8usize {
            let price = 10.0 + 0.1 * i as f64;
            let hour = 9 + (45 + i * 15) / 60;
            let minute = (45 + i * 15) % 60;
            s.push_str(&format!(
                "2024-01-02 {:02}:{:02}:00,{p},{p},{p},{p},1000\n",
                hour, minute, p = price
            ));
        }
        s
    };

    let tmp = tempfile::tempdir().unwrap();
    let f_a = write_file(&bars_csv, ".csv");
    let f_b = write_file(&bars_csv, ".csv");
    let f_c = write_file(&bars_csv, ".csv");

    let mut universe_content = String::from("symbol,primary\n");
    universe_content.push_str(&format!("A,{}\n", f_a.path().to_string_lossy()));
    universe_content.push_str(&format!("B,{}\n", f_b.path().to_string_lossy()));
    universe_content.push_str(&format!("C,{}\n", f_c.path().to_string_lossy()));
    let universe_f = write_file(&universe_content, ".csv");

    let state_path = tmp.path().join("hold_state.json");

    let cfg = SignalPortfolioConfig {
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

    // ── Run 1: empty state → expect 3 Buy signals ─────────────────────────
    let (sig1, state1) = run_signal_portfolio(&cfg, &LlmEvaluator::Disabled)
        .await
        .unwrap();

    assert_eq!(sig1.targets.len(), 3, "3 symbols should all be selected");
    // All symbols should be Buy from empty holdings
    for trade in &sig1.trades {
        assert_eq!(
            trade.action,
            TradeAction::Buy,
            "first run from empty state: {} should be Buy, got {:?}",
            trade.symbol,
            trade.action
        );
    }
    // Verify weights (equal weight = 1/3)
    for (_, w) in &sig1.targets {
        assert!(
            (w - 1.0 / 3.0).abs() < 1e-12,
            "equal weight should be 1/3, got {w}"
        );
    }

    // ── Simulate --commit: write_holdings_state ───────────────────────────
    write_holdings_state(&state_path, &state1).unwrap();

    // ── Run 2: state loaded → all Hold ────────────────────────────────────
    let (sig2, _state2) = run_signal_portfolio(&cfg, &LlmEvaluator::Disabled)
        .await
        .unwrap();

    assert_eq!(sig2.targets.len(), 3, "run 2 should still select 3 symbols");
    for trade in &sig2.trades {
        assert_eq!(
            trade.action,
            TradeAction::Hold,
            "second run with matching holdings: {} should be Hold, got {:?}",
            trade.symbol,
            trade.action
        );
    }
}

// F-9 T5 — signal_cli_mutex:
// CLI 互斥校验（子进程）：
// 1. --primary + --universe → 非零退出，stderr 含互斥措辞
// 2. --universe + --fetch → 非零退出，stderr 含 fetch 仅单口径报错
#[test]
fn signal_cli_mutex() {
    let bin = env!("CARGO_BIN_EXE_rquant");
    let tmp = tempfile::tempdir().unwrap();

    // 为满足 clap 解析（--tree/--state 是必填），给一个虚路径；互斥校验在加载前执行
    let fake_tree = tmp.path().join("fake.yaml");
    let fake_state = tmp.path().join("fake_state.json");
    let fake_primary = tmp.path().join("fake_primary.csv");
    let fake_universe = tmp.path().join("fake_universe.csv");

    // ── case 1: --primary + --universe → exactly-one-of error ───────────
    let out1 = std::process::Command::new(bin)
        .args([
            "signal",
            "--tree",
            fake_tree.to_str().unwrap(),
            "--state",
            fake_state.to_str().unwrap(),
            "--primary",
            fake_primary.to_str().unwrap(),
            "--universe",
            fake_universe.to_str().unwrap(),
        ])
        .output()
        .expect("failed to spawn rquant");

    assert!(
        !out1.status.success(),
        "signal --primary + --universe must exit non-zero"
    );
    let stderr1 = String::from_utf8_lossy(&out1.stderr);
    assert!(
        stderr1.contains("exactly one of"),
        "stderr must contain 'exactly one of' error message, got: {stderr1}"
    );

    // ── case 2: --universe + --fetch → fetch requires --primary error ────
    let out2 = std::process::Command::new(bin)
        .args([
            "signal",
            "--tree",
            fake_tree.to_str().unwrap(),
            "--state",
            fake_state.to_str().unwrap(),
            "--universe",
            fake_universe.to_str().unwrap(),
            "--fetch",
            "sh600519",
        ])
        .output()
        .expect("failed to spawn rquant");

    assert!(
        !out2.status.success(),
        "signal --universe + --fetch must exit non-zero"
    );
    let stderr2 = String::from_utf8_lossy(&out2.stderr);
    assert!(
        stderr2.contains("--fetch requires --primary"),
        "stderr must contain '--fetch requires --primary' error message, got: {stderr2}"
    );
}
