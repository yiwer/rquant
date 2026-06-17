//! 选股集成配置（数据驱动：加/裁树 = 改配置非改码）。

use crate::Result;
use chrono::NaiveDate;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// 集成合并参数（起始值经 spec §5 迭代定）。
#[derive(Debug, Clone, Deserialize)]
pub struct MergeConfig {
    #[serde(default = "default_theta_fire")]
    pub theta_fire: f64,
    #[serde(default = "default_vote_frac")]
    pub vote_frac: f64,
    #[serde(default = "default_q_floor")]
    pub q_floor: f64,
    #[serde(default = "default_top")]
    pub top: usize,
    /// 优质分分层数（回测质量分层用）。
    #[serde(default = "default_layers")]
    pub quality_layers: usize,
    /// 倾斜强度系数：combined = quality × (1 + lambda × tilt)。0 = 纯优质驱动。
    #[serde(default = "default_lambda")]
    pub lambda: f64,
    /// 参与选股倾斜的形态标签（其余形态仅标注不倾斜）。
    #[serde(default = "default_tilt_setups")]
    pub tilt_setups: Vec<String>,
}

fn default_theta_fire() -> f64 { 0.5 }
fn default_vote_frac() -> f64 { 0.5 }
fn default_q_floor() -> f64 { 0.5 }
fn default_top() -> usize { 10 }
fn default_layers() -> usize { 3 }
fn default_lambda() -> f64 { 1.0 }
fn default_tilt_setups() -> Vec<String> { vec!["动量延续".to_string()] }

impl Default for MergeConfig {
    fn default() -> Self {
        MergeConfig {
            theta_fire: default_theta_fire(),
            vote_frac: default_vote_frac(),
            q_floor: default_q_floor(),
            top: default_top(),
            quality_layers: default_layers(),
            lambda: default_lambda(),
            tilt_setups: default_tilt_setups(),
        }
    }
}

/// 命名 regime 窗口（回测跨牛熊切片用）。
#[derive(Debug, Clone, Deserialize)]
pub struct RegimeWindow {
    pub label: String,
    pub from: NaiveDate,
    pub to: NaiveDate,
}

/// 选股集成配置。树路径相对 cwd（同 portfolio 约定）。
#[derive(Debug, Clone, Deserialize)]
pub struct ScreenConfig {
    pub quality_trees: Vec<PathBuf>,
    /// 形态标签 -> 该形态的树集（可多树投票）。
    pub setup_trees: BTreeMap<String, Vec<PathBuf>>,
    #[serde(default)]
    pub merge: MergeConfig,
    #[serde(default)]
    pub regimes: Vec<RegimeWindow>,
    /// 横截面价值闸：Some(f) → 每次调仓先按优质分（价值分）保留最便宜的 ceil(f×n) 只，
    /// 再在廉价池内按动量倾斜选 top-N。None → 原有 combined 排名直接选（不变）。
    #[serde(default)]
    pub value_frac: Option<f64>,
}

impl ScreenConfig {
    /// 校验：至少 1 棵优质树、至少 1 个形态、各形态非空、参数范围合法。
    pub fn validate(&self) -> Result<()> {
        if self.quality_trees.is_empty() {
            return Err(crate::Error::Data("screen config: quality_trees must be non-empty".into()));
        }
        if self.setup_trees.is_empty() {
            return Err(crate::Error::Data("screen config: setup_trees must be non-empty".into()));
        }
        for (tag, trees) in &self.setup_trees {
            if trees.is_empty() {
                return Err(crate::Error::Data(format!("screen config: setup '{tag}' has no trees")));
            }
        }
        let m = &self.merge;
        if !(0.0..=1.0).contains(&m.theta_fire) || !(0.0..=1.0).contains(&m.q_floor) {
            return Err(crate::Error::Data("screen config: theta_fire/q_floor must be in [0,1]".into()));
        }
        if !(m.vote_frac > 0.0 && m.vote_frac <= 1.0) {
            return Err(crate::Error::Data("screen config: vote_frac must be in (0,1]".into()));
        }
        if m.top == 0 {
            return Err(crate::Error::Data("screen config: top must be >= 1".into()));
        }
        if m.lambda < 0.0 {
            return Err(crate::Error::Data("screen config: lambda must be >= 0".into()));
        }
        if m.tilt_setups.is_empty() {
            return Err(crate::Error::Data("screen config: tilt_setups must be non-empty".into()));
        }
        for s in &m.tilt_setups {
            if !self.setup_trees.contains_key(s) {
                return Err(crate::Error::Data(format!(
                    "screen config: tilt_setup '{s}' not found in setup_trees"
                )));
            }
        }
        if let Some(f) = self.value_frac
            && !(f > 0.0 && f <= 1.0)
        {
            return Err(crate::Error::Data("screen config: value_frac must be in (0,1]".into()));
        }
        Ok(())
    }
}

/// 从 YAML 文件加载并校验。
pub fn load_screen_config(path: &Path) -> Result<ScreenConfig> {
    let src = std::fs::read_to_string(path)?;
    let cfg: ScreenConfig = serde_yaml::from_str(&src)
        .map_err(|e| crate::Error::Data(format!("screen config parse error: {e}")))?;
    cfg.validate()?;
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_config_with_defaults() {
        let yaml = r#"
quality_trees: [examples/trees/screen/quality_v1.yaml]
setup_trees:
  动量延续: [examples/trees/screen/momentum_v1.yaml]
"#;
        let cfg: ScreenConfig = serde_yaml::from_str(yaml).unwrap();
        cfg.validate().unwrap();
        assert_eq!(cfg.quality_trees.len(), 1);
        assert_eq!(cfg.setup_trees.len(), 1);
        assert!((cfg.merge.theta_fire - 0.5).abs() < 1e-12);
        assert_eq!(cfg.merge.top, 10);
        assert_eq!(cfg.merge.quality_layers, 3);
        assert!(cfg.regimes.is_empty());
        assert!((cfg.merge.lambda - 1.0).abs() < 1e-12);
        assert_eq!(cfg.merge.tilt_setups, vec!["动量延续".to_string()]);
    }

    #[test]
    fn validate_rejects_empty_quality_trees() {
        let yaml = r#"
quality_trees: []
setup_trees:
  x: [a.yaml]
"#;
        let cfg: ScreenConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_rejects_bad_vote_frac() {
        let yaml = r#"
quality_trees: [q.yaml]
setup_trees:
  x: [a.yaml]
merge: { vote_frac: 0.0 }
"#;
        let cfg: ScreenConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn parses_regimes() {
        let yaml = r#"
quality_trees: [q.yaml]
setup_trees:
  x: [a.yaml]
regimes:
  - { label: "2018熊", from: 2018-01-02, to: 2018-12-28 }
"#;
        let cfg: ScreenConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.regimes.len(), 1);
        assert_eq!(cfg.regimes[0].label, "2018熊");
    }

    #[test]
    fn validate_rejects_negative_lambda() {
        let yaml = r#"
quality_trees: [q.yaml]
setup_trees:
  动量延续: [a.yaml]
merge: { lambda: -0.5 }
"#;
        let cfg: ScreenConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_rejects_tilt_setup_not_in_setups() {
        let yaml = r#"
quality_trees: [q.yaml]
setup_trees:
  突破临界: [a.yaml]
merge: { tilt_setups: ["动量延续"] }
"#;
        let cfg: ScreenConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(cfg.validate().is_err(), "tilt_setup not in setup_trees should fail");
    }

    #[test]
    fn validate_accepts_tilt_setup_in_setups() {
        let yaml = r#"
quality_trees: [q.yaml]
setup_trees:
  动量延续: [a.yaml]
  突破临界: [b.yaml]
merge: { tilt_setups: ["动量延续"] }
"#;
        let cfg: ScreenConfig = serde_yaml::from_str(yaml).unwrap();
        cfg.validate().unwrap();
    }

    #[test]
    fn parses_value_frac() {
        let yaml = "quality_trees: [q.yaml]\nsetup_trees:\n  动量延续: [a.yaml]\nvalue_frac: 0.3\n";
        let cfg: ScreenConfig = serde_yaml::from_str(yaml).unwrap();
        cfg.validate().unwrap();
        assert_eq!(cfg.value_frac, Some(0.3));
    }
}
