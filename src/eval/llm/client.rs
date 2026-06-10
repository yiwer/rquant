use crate::eval::llm::cache::{Cached, FileCache};
use crate::eval::llm::prompt::{parse_answer, render_user, LlmAnswer, SYSTEM_PROMPT};
use crate::eval::llm::{decision_from_dist, default_decision, dist_to_gotos, LlmConfig, LlmNode};
use crate::eval::Decision;
use crate::features::context::Context;
use crate::{Error, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::time::Duration;

pub struct OpenAiLlm {
    cfg: LlmConfig,
    cache: FileCache,
    http: reqwest::Client,
}

impl OpenAiLlm {
    pub fn new(cfg: LlmConfig) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(cfg.timeout_secs))
            .build()
            .map_err(|e| Error::Eval(format!("http client build: {e}")))?;
        let cache = FileCache::new(cfg.cache_dir.clone());
        Ok(Self { cfg, cache, http })
    }

    /// 取分布（label 概率）：缓存命中（且 keys ⊆ labels）→还原；未命中→调用(重试)→落缓存。
    async fn fetch_probs(&self, node_id: &str, node: &LlmNode<'_>, ctx: &Context) -> Result<(BTreeMap<String, f64>, String, bool)> {
        let rendered = render_user(node, ctx);
        let key = FileCache::key(&self.cfg.model, &self.cfg.base_url, SYSTEM_PROMPT, node_id, &rendered);
        if let Some(c) = self.cache.get(&key)
            && !c.probs.is_empty()
            && c.probs.keys().all(|k| node.labels.contains_key(k))
        {
            return Ok((c.probs, c.reason, true));
        }
        let ans = self.call_with_retries(&rendered, node).await?;
        let _ = self.cache.put(&key, &Cached {
            probs: ans.probs.clone(), reason: ans.reason.clone(), model: self.cfg.model.clone(),
        });
        Ok((ans.probs, ans.reason, false))
    }

    /// 硬模式：分布 argmax 派生 Decision；失败回退 default。
    pub async fn eval(&self, node_id: &str, node: &LlmNode<'_>, ctx: &Context) -> Result<Decision> {
        match self.fetch_probs(node_id, node, ctx).await {
            Ok((probs, reason, cached)) => {
                let tag = if cached { "LLM(cached)" } else { "LLM" };
                Ok(decision_from_dist(node, &probs, &format!("{tag}: {reason}")))
            }
            Err(e) => Ok(default_decision(node, &format!("LLM fallback({e})"))),
        }
    }

    /// 软模式：goto 分布；失败回退 [(default, 1.0)]。
    pub async fn eval_dist(&self, node_id: &str, node: &LlmNode<'_>, ctx: &Context) -> Result<(Vec<(String, f64)>, String)> {
        match self.fetch_probs(node_id, node, ctx).await {
            Ok((probs, reason, _)) => Ok((dist_to_gotos(node, &probs), reason)),
            Err(e) => Ok((vec![(node.default.to_string(), 1.0)], format!("LLM fallback({e})"))),
        }
    }

    async fn call_with_retries(&self, rendered: &str, node: &LlmNode<'_>) -> Result<LlmAnswer> {
        let mut last = String::from("no attempt");
        for _ in 0..=self.cfg.max_retries {
            match self.call_once(rendered, node).await {
                Ok(a) => return Ok(a),
                Err(e) => last = e.to_string(),
            }
        }
        Err(Error::Eval(last))
    }

    async fn call_once(&self, rendered: &str, node: &LlmNode<'_>) -> Result<LlmAnswer> {
        let body = build_request_body(&self.cfg.model, rendered);
        let url = format!("{}/chat/completions", self.cfg.base_url.trim_end_matches('/'));
        let resp = self.http.post(&url).bearer_auth(&self.cfg.api_key).json(&body).send().await
            .map_err(|e| Error::Eval(format!("request error: {e}")))?;
        if !resp.status().is_success() {
            return Err(Error::Eval(format!("http status {}", resp.status())));
        }
        let parsed: ChatResponse = resp.json().await
            .map_err(|e| Error::Eval(format!("response decode: {e}")))?;
        let content = parsed.choices.into_iter().next().map(|c| c.message.content)
            .ok_or_else(|| Error::Eval("no choices in response".into()))?;
        parse_answer(&content, node.labels)
    }
}

fn build_request_body(model: &str, rendered: &str) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "temperature": 0,
        "response_format": {"type": "json_object"},
        "messages": [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": rendered}
        ]
    })
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}
#[derive(Deserialize)]
struct Choice {
    message: Msg,
}
#[derive(Deserialize)]
struct Msg {
    content: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn request_body_shape() {
        let b = build_request_body("deepseek-chat", "hello");
        assert_eq!(b["model"], "deepseek-chat");
        assert_eq!(b["temperature"], 0);
        assert_eq!(b["response_format"]["type"], "json_object");
        assert_eq!(b["messages"][1]["content"], "hello");
    }

    #[test]
    fn parses_openai_style_response() {
        let raw = r#"{"choices":[{"message":{"role":"assistant","content":"{\"probs\":{\"go\":0.9},\"reason\":\"ok\"}"}}]}"#;
        let parsed: ChatResponse = serde_json::from_str(raw).unwrap();
        let content = parsed.choices.into_iter().next().unwrap().message.content;
        let allowed = HashMap::from([("go".to_string(), "leaf".to_string())]);
        let ans = crate::eval::llm::prompt::parse_answer(&content, &allowed).unwrap();
        assert!((ans.probs["go"] - 0.9).abs() < 1e-9);
    }
}
