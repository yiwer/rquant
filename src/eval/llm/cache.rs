use crate::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write as FmtWrite;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Cached {
    pub(crate) probs: BTreeMap<String, f64>,
    pub(crate) reason: String,
    pub(crate) model: String,
}

pub(crate) struct FileCache {
    dir: PathBuf,
}

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

impl FileCache {
    pub(crate) fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// 缓存键 = sha256_hex(model \0 base_url \0 system_prompt \0 node_id \0 rendered)。
    /// 纳入 base_url 与 system_prompt：换端点或改系统提示词时旧缓存自动失效。
    pub(crate) fn key(model: &str, base_url: &str, system_prompt: &str, node_id: &str, rendered: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        for part in [model, base_url, system_prompt, node_id, rendered] {
            h.update(part.as_bytes());
            h.update([0u8]);
        }
        let d = h.finalize();
        let mut s = String::with_capacity(64);
        for b in d {
            let _ = write!(s, "{b:02x}");
        }
        s
    }

    fn path_for(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.json"))
    }

    pub(crate) fn get(&self, key: &str) -> Option<Cached> {
        let s = std::fs::read_to_string(self.path_for(key)).ok()?;
        serde_json::from_str(&s).ok()
    }

    /// 原子写：写唯一临时文件再 rename（并发各写各的、崩溃不留半截）。
    pub(crate) fn put(&self, key: &str, c: &Cached) -> Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let uniq = format!(".{}.{}.tmp", std::process::id(), TMP_COUNTER.fetch_add(1, Ordering::Relaxed));
        let tmp = self.dir.join(uniq);
        std::fs::write(&tmp, serde_json::to_string(c)?)?;
        std::fs::rename(&tmp, self.path_for(key))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_is_stable_and_sensitive() {
        let base = FileCache::key("m", "https://a", "sys", "n", "rendered");
        assert_eq!(base, FileCache::key("m", "https://a", "sys", "n", "rendered"));
        assert_eq!(base.len(), 64);
        // 每个组件都影响键（换 model/端点/系统提示词/节点/输入 → 缓存自动失效）
        assert_ne!(base, FileCache::key("m2", "https://a", "sys", "n", "rendered"));
        assert_ne!(base, FileCache::key("m", "https://b", "sys", "n", "rendered"));
        assert_ne!(base, FileCache::key("m", "https://a", "sys2", "n", "rendered"));
        assert_ne!(base, FileCache::key("m", "https://a", "sys", "n2", "rendered"));
        assert_ne!(base, FileCache::key("m", "https://a", "sys", "n", "rendered2"));
    }

    #[test]
    fn put_then_get_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let cache = FileCache::new(dir.path());
        let key = FileCache::key("m", "u", "sys", "n", "r");
        assert!(cache.get(&key).is_none());
        let c = Cached { probs: BTreeMap::from([("go".to_string(), 0.7)]), reason: "ok".into(), model: "m".into() };
        cache.put(&key, &c).unwrap();
        assert_eq!(cache.get(&key).unwrap(), c);
    }

    /// 组合级不变量：共享 judge 的两个物化节点（map/default 不同）渲染串逐字节一致，
    /// 配合相同 scope → 同一缓存键。锁定「判定复用 = 一次网络调用」的机制基础——
    /// 若未来 render_user 误把 default 或 labels 的 value 渲染进 prompt，本测试变红。
    #[test]
    fn shared_judge_nodes_render_to_same_cache_key() {
        use crate::data::bar::{Bar, Window};
        use crate::eval::llm::prompt::render_user;
        use crate::eval::llm::LlmNode;
        use crate::tree::loader::{load_tree_str, Node};

        let tree = load_tree_str(r#"
meta: { name: t, forward_window: 3, stances: [long, flat, short] }
judges:
  veto:
    prompt: "veto?"
    labels: [bad, ok]
root: a
nodes:
  a:
    type: quant
    branches: [ { when: "close > 100", goto: g_hi, label: hi } ]
    default: { goto: g_lo, label: lo }
  g_hi: { type: llm, judge: veto, map: { ok: leaf_l }, default: leaf_f }
  g_lo: { type: llm, judge: veto, map: { ok: leaf_s }, default: leaf_l }
leaves:
  leaf_l: { stance: long }
  leaf_s: { stance: short }
  leaf_f: { stance: flat }
"#).unwrap();

        let t0 = chrono::NaiveDate::from_ymd_opt(2024, 1, 2).unwrap().and_hms_opt(9, 45, 0).unwrap();
        let bars = vec![Bar { time: t0, open: 1.0, high: 1.0, low: 1.0, close: 1.0, volume: 1.0 }];
        let ctx = crate::features::context::Context {
            t: t0,
            primary: Window { bars: bars.clone() },
            context: Window { bars },
            news: None,
            aux: BTreeMap::new(),
            sim: crate::features::context::SimState::default(),
            eval_cache: Default::default(),
        };

        let render_key = |id: &str| match tree.nodes.get(id).unwrap() {
            Node::Llm { inputs, prompt, labels, default, scope } => {
                let ln = LlmNode { inputs, prompt, labels, default };
                let rendered = render_user(&ln, &ctx);
                FileCache::key("m", "https://a", "sys", scope.as_deref().unwrap(), &rendered)
            }
            _ => panic!("expected llm node"),
        };
        assert_eq!(render_key("g_hi"), render_key("g_lo"));
    }

    // M6 — concurrent cache put: 8 threads each writing the same key with distinct reasons
    #[test]
    fn concurrent_put_same_key_survives() {
        use std::sync::Arc;
        let dir = tempfile::tempdir().unwrap();
        let cache = Arc::new(FileCache::new(dir.path()));
        let key = FileCache::key("m", "u", "sys", "n", "concurrent");
        let mut handles = Vec::new();
        for i in 0..8 {
            let cache = Arc::clone(&cache);
            let key = key.clone();
            handles.push(std::thread::spawn(move || {
                let c = Cached {
                    probs: BTreeMap::from([("go".to_string(), i as f64 * 0.1)]),
                    reason: format!("reason_{i}"),
                    model: "m".into(),
                };
                cache.put(&key, &c).unwrap();
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        // After all writes, the key must be present and parseable (any one value wins)
        let got = cache.get(&key).expect("key must exist after concurrent puts");
        assert!(got.reason.starts_with("reason_"), "reason should be one of the written values, got: {}", got.reason);
    }
}
