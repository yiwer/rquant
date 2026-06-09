use rquant::backtest::runner::{run, BacktestConfig};
use rquant::eval::llm::{LlmEvaluator, StubLlm};
use std::collections::HashMap;
use std::io::Write;

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
    let content = std::fs::read_to_string(out_f.path()).unwrap();
    assert!(content.contains("engaged"));
}
