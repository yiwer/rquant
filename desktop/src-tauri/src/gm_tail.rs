//! gm 尾盘取数定时任务——控制面（装/卸/手动跑/查状态 + 配置读写）。
//! 运行时:计划任务 `rquant-gm-tail` 触发 `scripts/gm_tail_run.ps1`(自解析仓库根)→ Python tail --funnel。
//! 本模块零数据逻辑——只编排 schtasks 与读写 data/gm/ 下的配置/产物;路径全经 Workspace,移植即生效。
use crate::dto::SchtaskDto;
use crate::dto_gm::{GmTailConfig, GmTailStatusDto};
use crate::paths::Workspace;
use std::path::Path;

pub const TASK_NAME: &str = "rquant-gm-tail";

// ---------- 配置读写 ----------

/// 读配置;文件缺失/损坏 → 默认值(永不报错,驾驶舱降级)。
pub fn read_config(ws: &Workspace) -> GmTailConfig {
    std::fs::read_to_string(ws.gm_config_path())
        .ok()
        .and_then(|t| serde_json::from_str::<GmTailConfig>(&t).ok())
        .map(GmTailConfig::sanitized)
        .unwrap_or_default()
}

/// 写配置(先 sanitize;建 data/gm/)。
pub fn write_config(ws: &Workspace, cfg: &GmTailConfig) -> Result<(), String> {
    let cfg = cfg.clone().sanitized();
    std::fs::create_dir_all(ws.gm_dir())
        .map_err(|e| format!("建 {} 失败: {e}", ws.gm_dir().display()))?;
    let json = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    std::fs::write(ws.gm_config_path(), json).map_err(|e| e.to_string())
}

// ---------- 计划任务(schtasks) ----------

/// schtasks /Create 参数(纯函数,可测)。/TR 调可移植 launcher,周一至周五 time 触发,/F 覆盖。
pub fn create_args(launcher: &Path, time: &str) -> Vec<String> {
    let tr = format!(
        "powershell -NoProfile -ExecutionPolicy Bypass -File \"{}\"",
        launcher.display()
    );
    vec![
        "/Create".into(),
        "/TN".into(),
        TASK_NAME.into(),
        "/TR".into(),
        tr,
        "/SC".into(),
        "WEEKLY".into(),
        "/D".into(),
        "MON,TUE,WED,THU,FRI".into(),
        "/ST".into(),
        time.into(),
        "/F".into(),
    ]
}

fn run_schtasks(args: &[String]) -> Result<String, String> {
    let out = std::process::Command::new("schtasks")
        .args(args)
        .output()
        .map_err(|e| format!("schtasks 启动失败: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(format!(
            "schtasks 失败({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// 安装/更新计划任务(用配置里的 schedule_time;/F 覆盖)。先落配置,再注册;launcher 必须存在。
pub fn install(ws: &Workspace, cfg: &GmTailConfig) -> Result<(), String> {
    let cfg = cfg.clone().sanitized();
    write_config(ws, &cfg)?;
    let launcher = ws.gm_tail_launcher();
    if !launcher.exists() {
        return Err(format!(
            "launcher 缺失: {}（确认 scripts/gm_tail_run.ps1 在仓库内）",
            launcher.display()
        ));
    }
    run_schtasks(&create_args(&launcher, &cfg.schedule_time)).map(|_| ())
}

pub fn remove() -> Result<(), String> {
    run_schtasks(&[
        "/Delete".into(),
        "/TN".into(),
        TASK_NAME.into(),
        "/F".into(),
    ])
    .map(|_| ())
}

pub fn run_now() -> Result<(), String> {
    run_schtasks(&["/Run".into(), "/TN".into(), TASK_NAME.into()]).map(|_| ())
}

pub fn status() -> Option<SchtaskDto> {
    crate::schtask::query(TASK_NAME)
}

// ---------- 驾驶舱状态装配 ----------

pub fn status_dto(ws: &Workspace) -> GmTailStatusDto {
    let schtask = status();
    GmTailStatusDto {
        installed: schtask.is_some(),
        schtask,
        config: read_config(ws),
        token_present: file_nonempty(&ws.gm_token_path()),
        k15m_count: count_csv(&ws.gm_k15m_dir()),
        last_snapshot: latest_csv_name(&ws.gm_snapshot_dir()),
        log_tail: tail_lines(&ws.gm_tail_log_path(), 12),
    }
}

fn file_nonempty(p: &Path) -> bool {
    std::fs::metadata(p).map(|m| m.len() > 0).unwrap_or(false)
}

fn count_csv(dir: &Path) -> u32 {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().extension().map(|x| x == "csv").unwrap_or(false))
                .count() as u32
        })
        .unwrap_or(0)
}

fn latest_csv_name(dir: &Path) -> Option<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "csv").unwrap_or(false))
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort(); // snapshot_YYYYMMDD_HHMM.csv → 字典序=时间序
    names.pop()
}

fn tail_lines(path: &Path, n: usize) -> Vec<String> {
    std::fs::read_to_string(path)
        .map(|t| {
            let lines: Vec<&str> = t.lines().collect();
            let start = lines.len().saturating_sub(n);
            lines[start..].iter().map(|s| s.to_string()).collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_args_shape_and_portable_tr() {
        let launcher = std::path::Path::new("E:/x/scripts/gm_tail_run.ps1");
        let a = create_args(launcher, "14:46");
        assert_eq!(a[0], "/Create");
        assert_eq!(a[1], "/TN");
        assert_eq!(a[2], "rquant-gm-tail");
        assert_eq!(a[3], "/TR");
        // /TR 指向 launcher(自解析根),含 powershell -File,不写死仓库路径
        assert!(a[4].contains("powershell") && a[4].contains("gm_tail_run.ps1"));
        assert!(a.iter().any(|x| x == "WEEKLY"));
        assert!(a.iter().any(|x| x == "MON,TUE,WED,THU,FRI"));
        let st = a.iter().position(|x| x == "/ST").unwrap();
        assert_eq!(a[st + 1], "14:46");
        assert_eq!(a.last().unwrap(), "/F");
    }

    #[test]
    fn read_config_missing_returns_default() {
        let ws = Workspace::new(std::env::temp_dir().join("rquant_gm_test_missing"));
        assert_eq!(read_config(&ws), GmTailConfig::default());
    }

    #[test]
    fn write_then_read_config_roundtrips() {
        let root = std::env::temp_dir().join("rquant_gm_test_rw");
        let _ = std::fs::remove_dir_all(&root);
        let ws = Workspace::new(root.clone());
        let mut cfg = GmTailConfig::default();
        cfg.rank = "intraday".into();
        cfg.top = 150;
        write_config(&ws, &cfg).unwrap();
        assert_eq!(read_config(&ws), cfg);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn tail_lines_returns_last_n() {
        let root = std::env::temp_dir().join("rquant_gm_test_log");
        let _ = std::fs::create_dir_all(&root);
        let p = root.join("t.log");
        std::fs::write(&p, "a\nb\nc\nd\ne\n").unwrap();
        assert_eq!(tail_lines(&p, 2), vec!["d".to_string(), "e".to_string()]);
        assert_eq!(tail_lines(&p, 99).len(), 5);
        let _ = std::fs::remove_dir_all(&root);
    }
}
