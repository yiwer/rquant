use std::process::Command;

fn bin() -> &'static str { env!("CARGO_BIN_EXE_rquant") }

#[test]
fn validate_data_flags_gross_jump_with_nonzero_exit() {
    let dir = tempfile::tempdir().unwrap();
    let csv = dir.path().join("bad.csv");
    // 表头 + 两行，+30% 跳（超 0.21）
    std::fs::write(&csv,
        "time,open,high,low,close,volume\n\
         2024-01-02 15:00:00,10,10,10,10,100\n\
         2024-01-03 15:00:00,13,13,13,13,100\n").unwrap();
    let status = Command::new(bin())
        .args(["validate-data", "--csv", csv.to_str().unwrap()])
        .status().unwrap();
    assert_eq!(status.code(), Some(1), "可疑跳空 → 退出码 1");
}

#[test]
fn validate_data_clean_series_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    let csv = dir.path().join("ok.csv");
    std::fs::write(&csv,
        "time,open,high,low,close,volume\n\
         2024-01-02 15:00:00,10,10,10,10.0,100\n\
         2024-01-03 15:00:00,10.1,10.1,10.1,10.1,100\n\
         2024-01-04 15:00:00,10.2,10.2,10.2,10.2,100\n").unwrap();
    let status = Command::new(bin())
        .args(["validate-data", "--csv", csv.to_str().unwrap()])
        .status().unwrap();
    assert_eq!(status.code(), Some(0), "干净序列 → 退出码 0");
}
