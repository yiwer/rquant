//! 引擎/任意错误 → 前端 DTO 映射。kind 与 rquant::Error 十类一一对应。
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ErrorDto {
    pub kind: String,
    pub message: String,
    /// 可操作建议（如 state corrupt → 删除重建,重放幂等）。
    pub advice: Option<String>,
}

impl ErrorDto {
    pub fn from_anyhow(e: &anyhow::Error) -> Self {
        let (kind, message) = match e.downcast_ref::<rquant::Error>() {
            Some(re) => {
                let k = match re {
                    rquant::Error::Data(_) => "data",
                    rquant::Error::Dsl(_) => "dsl",
                    rquant::Error::Tree(_) => "tree",
                    rquant::Error::Eval(_) => "eval",
                    rquant::Error::Engine(_) => "engine",
                    rquant::Error::Backtest(_) => "backtest",
                    rquant::Error::Io(_) => "io",
                    rquant::Error::Csv(_) => "csv",
                    rquant::Error::Yaml(_) => "yaml",
                    rquant::Error::Json(_) => "json",
                };
                (k.to_string(), re.to_string())
            }
            None => ("internal".to_string(), e.to_string()),
        };
        let advice = if message.contains("corrupt") {
            Some("state 文件损坏:可删除该 state 后重新运行(重放幂等,会从头重建账本)".to_string())
        } else if message.contains("tree_name") {
            Some("state 与树不匹配:确认账本对应的冻结树未被改名".to_string())
        } else if message.contains("version") && message.contains("unsupported") {
            Some("state 协议版本不受支持:删除该 state 后重新运行(重放幂等,会从头重建账本)".to_string())
        } else {
            None
        };
        ErrorDto { kind, message, advice }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rquant_error_maps_kind_and_message() {
        let e = rquant::Error::Data("bad csv".into());
        let dto = ErrorDto::from_anyhow(&anyhow::Error::new(e));
        assert_eq!(dto.kind, "data");
        assert!(dto.message.contains("bad csv"));
    }

    #[test]
    fn corrupt_state_gets_actionable_advice() {
        let e = rquant::Error::Data("state corrupt: empty file".into());
        let dto = ErrorDto::from_anyhow(&anyhow::Error::new(e));
        assert!(dto.advice.as_deref().unwrap_or("").contains("删除"));
    }

    #[test]
    fn non_rquant_error_is_internal() {
        let dto = ErrorDto::from_anyhow(&anyhow::anyhow!("boom"));
        assert_eq!(dto.kind, "internal");
    }

    #[test]
    fn tree_name_mismatch_gets_actionable_advice() {
        let e = rquant::Error::Data(
            "signal state tree_name 'tree_a' does not match requested tree 'tree_b'".into(),
        );
        let dto = ErrorDto::from_anyhow(&anyhow::Error::new(e));
        assert!(dto.advice.as_deref().unwrap_or("").contains("改名"));
    }

    #[test]
    fn version_unsupported_gets_actionable_advice() {
        let e = rquant::Error::Data("signal state version 2 unsupported (expected 1)".into());
        let dto = ErrorDto::from_anyhow(&anyhow::Error::new(e));
        assert!(dto.advice.as_deref().unwrap_or("").contains("删除"));
    }
}
