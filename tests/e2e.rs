use rquant::backtest::runner::{run, BacktestConfig};
use rquant::backtest::sim::run_sim;
use rquant::eval::llm::{LlmEvaluator, StubLlm};
use std::collections::HashMap;
use std::io::Write;
use rquant::backtest::soft::run_soft;

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
