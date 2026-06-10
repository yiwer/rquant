pub mod cache;
pub mod client;
pub mod prompt;

use crate::eval::Decision;
use crate::features::context::Context;
use crate::Result;
use std::collections::{BTreeMap, HashMap};
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

/// label 分布 → goto 分布：label→labels[label]（未知→default）、同 goto 合并、残余补 default。
/// 前置：probs 已清洗（Σ ≤ 1）。产出 Σ = 1，按 goto 名排序（确定性）。
pub fn dist_to_gotos(node: &LlmNode<'_>, probs: &BTreeMap<String, f64>) -> Vec<(String, f64)> {
    let mut acc: BTreeMap<String, f64> = BTreeMap::new();
    let mut sum = 0.0;
    for (label, &p) in probs {
        if p > 0.0 {
            let goto = node.labels.get(label).cloned().unwrap_or_else(|| node.default.to_string());
            *acc.entry(goto).or_insert(0.0) += p;
            sum += p;
        }
    }
    let rem = 1.0 - sum;
    if rem > 0.0 {
        *acc.entry(node.default.to_string()).or_insert(0.0) += rem;
    }
    acc.into_iter().collect()
}

/// 硬模式派生：在 (label, p) + ("default", 残余) 上取 argmax（并列取字典序小，BTreeMap 序保证）。
pub fn decision_from_dist(node: &LlmNode<'_>, probs: &BTreeMap<String, f64>, rationale: &str) -> Decision {
    let mut candidates: BTreeMap<String, f64> = probs.clone();
    let sum: f64 = probs.values().sum();
    let rem = 1.0 - sum;
    if rem > 0.0 {
        *candidates.entry("default".to_string()).or_insert(0.0) += rem;
    }
    let (label, confidence) = candidates
        .iter()
        .fold(("default".to_string(), 0.0), |best, (k, &v)| if v > best.1 { (k.clone(), v) } else { best });
    let goto = node.labels.get(&label).cloned().unwrap_or_else(|| node.default.to_string());
    Decision { goto, label, confidence, rationale: rationale.to_string() }
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

pub enum LlmEvaluator {
    OpenAi(client::OpenAiLlm),
    Disabled,
    Stub(StubLlm),
}

impl LlmEvaluator {
    pub async fn eval_llm(&self, node_id: &str, node: &LlmNode<'_>, ctx: &Context) -> Result<Decision> {
        match self {
            LlmEvaluator::OpenAi(c) => c.eval(node_id, node, ctx).await,
            LlmEvaluator::Disabled => Ok(default_decision(node, "LLM disabled")),
            LlmEvaluator::Stub(s) => s.eval(node_id, node, ctx),
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

    #[tokio::test]
    async fn disabled_returns_default() {
        let lbl = labels();
        let node = LlmNode { inputs: &[], prompt: "q", labels: &lbl, default: "leaf_f" };
        let d = LlmEvaluator::Disabled.eval_llm("n", &node, &ctx()).await.unwrap();
        assert_eq!(d.goto, "leaf_f");
        assert_eq!(d.label, "default");
    }

    #[tokio::test]
    async fn stub_via_enum_returns_label() {
        let lbl = labels();
        let node = LlmNode { inputs: &[], prompt: "q", labels: &lbl, default: "leaf_f" };
        let ev = LlmEvaluator::Stub(StubLlm { answers: HashMap::from([("n".to_string(), "go".to_string())]) });
        let d = ev.eval_llm("n", &node, &ctx()).await.unwrap();
        assert_eq!(d.goto, "leaf_l");
    }

    #[test]
    fn dist_to_gotos_maps_merges_and_fills_default() {
        use std::collections::BTreeMap;
        let lbl = HashMap::from([
            ("a".to_string(), "leaf_x".to_string()),
            ("b".to_string(), "leaf_x".to_string()),  // 同 goto，应合并
            ("c".to_string(), "leaf_y".to_string()),
        ]);
        let node = LlmNode { inputs: &[], prompt: "q", labels: &lbl, default: "leaf_f" };
        let probs = BTreeMap::from([("a".to_string(), 0.3), ("b".to_string(), 0.2), ("c".to_string(), 0.1)]);
        let dist = dist_to_gotos(&node, &probs);
        // leaf_x: 0.3+0.2=0.5, leaf_y: 0.1, 残余 0.4 → leaf_f；BTreeMap 序：leaf_f, leaf_x, leaf_y
        assert_eq!(dist.len(), 3);
        let m: std::collections::HashMap<_, _> = dist.iter().cloned().collect();
        assert!((m["leaf_x"] - 0.5).abs() < 1e-9);
        assert!((m["leaf_y"] - 0.1).abs() < 1e-9);
        assert!((m["leaf_f"] - 0.4).abs() < 1e-9);
        let sum: f64 = dist.iter().map(|(_, w)| w).sum();
        assert!((sum - 1.0).abs() < 1e-9);
    }

    #[test]
    fn decision_from_dist_argmax_and_default_remainder() {
        use std::collections::BTreeMap;
        let lbl = labels(); // {"go" → "leaf_l"}
        let node = LlmNode { inputs: &[], prompt: "q", labels: &lbl, default: "leaf_f" };
        // go=0.9 胜出
        let d = decision_from_dist(&node, &BTreeMap::from([("go".to_string(), 0.9)]), "r");
        assert_eq!(d.goto, "leaf_l");
        assert_eq!(d.label, "go");
        assert!((d.confidence - 0.9).abs() < 1e-9);
        // go=0.3 → 残余 0.7 给 default 胜出
        let d2 = decision_from_dist(&node, &BTreeMap::from([("go".to_string(), 0.3)]), "r");
        assert_eq!(d2.goto, "leaf_f");
        assert_eq!(d2.label, "default");
        assert!((d2.confidence - 0.7).abs() < 1e-9);
    }
}
