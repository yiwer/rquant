use std::process::Command;

fn bin() -> &'static str { env!("CARGO_BIN_EXE_rquant") }

#[test]
fn eval_emits_verdict_and_exit_code() {
    let dir = tempfile::tempdir().unwrap();
    // Two symbols, both have only negative OS folds → must not certify → exit code 1
    for (name, os) in [("sh000001", -1.0), ("sh000002", -1.0)] {
        let json = format!(r#"{{"mode":"sim","objective_name":"sharpe","folds":2,"n_combos":1,
            "fold_results":[{{"fold":2,"is_from":"2025-01-01T00:00:00","is_to":"2025-01-01T00:00:00",
            "os_from":"2025-01-01T00:00:00","os_to":"2025-01-01T00:00:00","best_params":null,
            "is_objective":1.0,"os_objective":{os},"degradation":null}}],
            "os_mean_objective":null,"full_sample_best":null,"drift":[],"is_top5":[],
            "axes":[],"primary":"{name}"}}"#);
        std::fs::write(dir.path().join(format!("wfo_{name}.json")), json).unwrap();
    }
    let out_path = dir.path().join("verdict.json");
    let status = Command::new(bin())
        .args(["eval", "--name", "t",
               "--reports", dir.path().join("wfo_sh000001.json").to_str().unwrap(),
               "--reports", dir.path().join("wfo_sh000002.json").to_str().unwrap(),
               "--out", out_path.to_str().unwrap()])
        .status().unwrap();
    assert_eq!(status.code(), Some(1), "not certified → exit code 1");
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&out_path).unwrap()).unwrap();
    assert_eq!(v["certified"], serde_json::json!(false));
    assert_eq!(v["n_symbols"], serde_json::json!(2));
}
