//! 工作区路径唯一出口——桥接层任何文件访问都经此模块取路径。
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    pub fn new(root: PathBuf) -> Self {
        Workspace { root }
    }

    /// 自 start 向上找仓库根:同时存在 Cargo.toml 与 deploy/paper_run.cmd 的目录。
    pub fn detect(start: &Path) -> Option<Self> {
        let mut cur = Some(start);
        while let Some(d) = cur {
            if d.join("Cargo.toml").exists() && d.join("deploy").join("paper_run.cmd").exists() {
                return Some(Workspace::new(d.to_path_buf()));
            }
            cur = d.parent();
        }
        None
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn paper_dir(&self) -> PathBuf {
        self.root.join("paper")
    }
    pub fn deploy_dir(&self) -> PathBuf {
        self.root.join("deploy")
    }
    pub fn desktop_data_dir(&self) -> PathBuf {
        self.root.join(".rquant-desktop")
    }
    pub fn journal_path(&self) -> PathBuf {
        self.desktop_data_dir().join("journal").join("paper-journal.jsonl")
    }
    pub fn runs_dir(&self) -> PathBuf {
        self.desktop_data_dir().join("runs")
    }
    pub fn data_dir(&self) -> PathBuf {
        self.desktop_data_dir().join("data")
    }
    pub fn universes_dir(&self) -> PathBuf {
        self.desktop_data_dir().join("universes")
    }
    pub fn run_log_path(&self) -> PathBuf {
        self.paper_dir().join("run.log")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_paths_join_correctly() {
        let ws = Workspace::new(std::path::PathBuf::from("E:/rust-app/rquant"));
        assert!(ws.paper_dir().ends_with("paper"));
        assert!(ws.deploy_dir().ends_with("deploy"));
        // Path::ends_with 按组件比较,/ 与 \ 在 Windows 解析为相同组件序列
        assert!(ws.journal_path().ends_with(".rquant-desktop/journal/paper-journal.jsonl"));
        assert!(ws.runs_dir().ends_with(".rquant-desktop/runs"));
        assert!(ws.data_dir().ends_with(".rquant-desktop/data"));
        assert!(ws.universes_dir().ends_with(".rquant-desktop/universes"));
    }

    #[test]
    fn detect_workspace_walks_up_to_cargo_toml_with_paper_run() {
        // detect 规则:从给定起点向上找同时含 Cargo.toml 与 deploy/paper_run.cmd 的目录
        let here = std::env::current_dir().unwrap();
        let ws = Workspace::detect(&here).expect("repo root should be detectable from src-tauri cwd");
        assert!(ws.root().join("deploy").join("paper_run.cmd").exists());
    }
}
