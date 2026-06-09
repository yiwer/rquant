use crate::eval::llm::LlmNode;
use crate::features::context::Context;
use crate::{Error, Result};
use serde::Deserialize;

pub const SYSTEM_PROMPT: &str = "You are a financial-analysis classifier. Choose exactly one label from the allowed list. Respond ONLY with a JSON object: {\"label\": <one of the allowed labels>, \"confidence\": <number 0..1>, \"reason\": <short string>}.";

/// 渲染 user message。必须确定性（它是缓存键的一部分）：label 排序、价格定宽、inputs 按声明顺序。
pub fn render_user(node: &LlmNode<'_>, ctx: &Context) -> String {
    let mut s = String::new();
    s.push_str(&format!("Question: {}\n", node.prompt));

    let mut labels: Vec<&str> = node.labels.keys().map(|k| k.as_str()).collect();
    labels.sort_unstable();
    s.push_str(&format!("Allowed labels: [{}]\n", labels.join(", ")));

    let closes = ctx.primary.closes();
    let start = closes.len().saturating_sub(20);
    let recent: Vec<String> = closes[start..].iter().map(|c| format!("{c:.4}")).collect();
    s.push_str(&format!("Recent primary closes: [{}]\n", recent.join(", ")));
    if let Some(last) = closes.last() {
        s.push_str(&format!("Latest close: {last:.4}\n"));
    }

    for input in node.inputs {
        match input.as_str() {
            "news_score" => {
                let v = ctx.news.as_ref()
                    .and_then(|n| n.recent.last())
                    .map(|r| format!("{:.4}", r.score))
                    .unwrap_or_else(|| "none".to_string());
                s.push_str(&format!("news_score: {v}\n"));
            }
            "recent_headlines" => {
                let v = ctx.news.as_ref()
                    .filter(|n| !n.recent.is_empty())
                    .map(|n| n.recent.iter().map(|r| r.headline.clone()).collect::<Vec<_>>().join("; "))
                    .unwrap_or_else(|| "none".to_string());
                s.push_str(&format!("recent_headlines: {v}\n"));
            }
            other => s.push_str(&format!("{other}: unavailable\n")),
        }
    }
    s
}

#[derive(Debug, Deserialize)]
pub struct LlmAnswer {
    pub label: String,
    #[serde(default)]
    pub confidence: f64,
    #[serde(default)]
    pub reason: String,
}

/// 解析 LLM content（应为 JSON），并校验 label ∈ allowed。
pub fn parse_answer(content: &str, allowed: &std::collections::HashMap<String, String>) -> Result<LlmAnswer> {
    let ans: LlmAnswer = serde_json::from_str(content.trim())
        .map_err(|e| Error::Eval(format!("LLM output not valid JSON: {e}")))?;
    if !allowed.contains_key(&ans.label) {
        return Err(Error::Eval(format!("LLM label '{}' not in allowed labels", ans.label)));
    }
    Ok(ans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::bar::{Bar, Window};
    use crate::data::news::{NewsRecord, NewsView};
    use crate::features::context::Context;
    use chrono::NaiveDate;
    use std::collections::HashMap;

    fn ctx_with(closes: &[f64], news: Option<NewsView>) -> Context {
        let base = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        let bars: Vec<Bar> = closes.iter().enumerate().map(|(i, &c)| Bar {
            time: base + chrono::Duration::minutes(i as i64 * 15), open: c, high: c, low: c, close: c, volume: 1.0,
        }).collect();
        Context { t: base, primary: Window { bars: bars.clone() }, context: Window { bars }, news }
    }

    #[test]
    fn render_includes_prompt_sorted_labels_and_price() {
        let labels = HashMap::from([("b".to_string(), "x".to_string()), ("a".to_string(), "y".to_string())]);
        let node = LlmNode { inputs: &[], prompt: "trend?", labels: &labels, default: "d" };
        let s = render_user(&node, &ctx_with(&[1.0, 2.0, 3.0], None));
        assert!(s.contains("Question: trend?"));
        assert!(s.contains("Allowed labels: [a, b]"));
        assert!(s.contains("Latest close: 3.0000"));
    }

    #[test]
    fn render_news_inputs_present_and_absent() {
        let labels = HashMap::from([("go".to_string(), "x".to_string())]);
        let inputs = vec!["news_score".to_string(), "recent_headlines".to_string()];
        let node = LlmNode { inputs: &inputs, prompt: "q", labels: &labels, default: "d" };
        let base = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 30, 0).unwrap();
        let nv = NewsView { recent: vec![NewsRecord { time: base, score: 0.5, headline: "H".into() }] };
        let s = render_user(&node, &ctx_with(&[1.0], Some(nv)));
        assert!(s.contains("news_score: 0.5000"));
        assert!(s.contains("recent_headlines: H"));
        let s2 = render_user(&node, &ctx_with(&[1.0], None));
        assert!(s2.contains("news_score: none"));
        assert!(s2.contains("recent_headlines: none"));
    }

    #[test]
    fn parse_answer_valid_invalid_and_label_check() {
        let allowed = HashMap::from([("go".to_string(), "x".to_string())]);
        let ok = parse_answer("{\"label\":\"go\",\"confidence\":0.8,\"reason\":\"r\"}", &allowed).unwrap();
        assert_eq!(ok.label, "go");
        assert!(parse_answer("not json", &allowed).is_err());
        assert!(parse_answer("{\"label\":\"nope\"}", &allowed).is_err());
    }
}
