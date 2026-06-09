pub mod cache;

use crate::eval::Decision;
use crate::features::context::Context;
use crate::Result;
use std::collections::HashMap;
use std::path::PathBuf;

/// traverse 传入的 LLM 节点借用视图。
pub struct LlmNode<'a> {
    pub inputs: &'a [String],
    pub prompt: &'a str,
    pub labels: &'a HashMap<String, String>,
    pub default: &'a str,
}

#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub timeout_secs: u64,
    pub max_retries: u32,
    pub cache_dir: PathBuf,
}

/// LLM 不可用/失败时的回退（走节点 default）。
pub fn default_decision(node: &LlmNode<'_>, why: &str) -> Decision {
    Decision {
        goto: node.default.to_string(),
        label: "default".to_string(),
        confidence: 0.0,
        rationale: format!("{why}: default branch"),
    }
}

/// 把 LLM 给的 label 映射成 Decision（goto = node.labels[label]，缺失则回退 default）。
pub fn decision_from_answer(node: &LlmNode<'_>, label: &str, confidence: f64, reason: &str, cached: bool) -> Decision {
    let goto = node.labels.get(label).cloned().unwrap_or_else(|| node.default.to_string());
    let tag = if cached { "LLM(cached)" } else { "LLM" };
    Decision { goto, label: label.to_string(), confidence, rationale: format!("{tag}: {reason}") }
}

/// 测试用 stub：node_id -> label（"ERROR" 模拟失败 → 回退 default）。
pub struct StubLlm {
    pub answers: HashMap<String, String>,
}
impl StubLlm {
    pub fn eval(&self, node_id: &str, node: &LlmNode<'_>, _ctx: &Context) -> Result<Decision> {
        match self.answers.get(node_id) {
            Some(l) if l == "ERROR" => Ok(default_decision(node, "LLM stub error")),
            Some(l) if node.labels.contains_key(l) => Ok(decision_from_answer(node, l, 0.9, "stub", false)),
            _ => Ok(default_decision(node, "LLM stub no-answer")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::bar::Window;
    use chrono::NaiveDate;

    pub(super) fn ctx() -> Context {
        let t = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        Context { t, primary: Window { bars: vec![] }, context: Window { bars: vec![] }, news: None }
    }
    fn labels() -> HashMap<String, String> {
        HashMap::from([("go".to_string(), "leaf_l".to_string())])
    }

    #[test]
    fn stub_known_label_maps_goto() {
        let lbl = labels();
        let node = LlmNode { inputs: &[], prompt: "q", labels: &lbl, default: "leaf_f" };
        let stub = StubLlm { answers: HashMap::from([("n".to_string(), "go".to_string())]) };
        let d = stub.eval("n", &node, &ctx()).unwrap();
        assert_eq!(d.goto, "leaf_l");
        assert_eq!(d.label, "go");
    }

    #[test]
    fn stub_error_falls_back_to_default() {
        let lbl = labels();
        let node = LlmNode { inputs: &[], prompt: "q", labels: &lbl, default: "leaf_f" };
        let stub = StubLlm { answers: HashMap::from([("n".to_string(), "ERROR".to_string())]) };
        let d = stub.eval("n", &node, &ctx()).unwrap();
        assert_eq!(d.goto, "leaf_f");
        assert_eq!(d.label, "default");
    }
}
