use crate::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cached {
    pub label: String,
    pub confidence: f64,
    pub reason: String,
    pub model: String,
}

pub struct FileCache {
    dir: PathBuf,
}

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

impl FileCache {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// 缓存键 = sha256_hex(model \0 base_url \0 system_prompt \0 node_id \0 rendered)。
    /// 纳入 base_url 与 system_prompt：换端点或改系统提示词时旧缓存自动失效。
    pub fn key(model: &str, base_url: &str, system_prompt: &str, node_id: &str, rendered: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        for part in [model, base_url, system_prompt, node_id, rendered] {
            h.update(part.as_bytes());
            h.update([0u8]);
        }
        let d = h.finalize();
        let mut s = String::with_capacity(64);
        for b in d {
            s.push_str(&format!("{b:02x}"));
        }
        s
    }

    fn path_for(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.json"))
    }

    pub fn get(&self, key: &str) -> Option<Cached> {
        let s = std::fs::read_to_string(self.path_for(key)).ok()?;
        serde_json::from_str(&s).ok()
    }

    /// 原子写：写唯一临时文件再 rename（并发各写各的、崩溃不留半截）。
    pub fn put(&self, key: &str, c: &Cached) -> Result<()> {
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
        let c = Cached { label: "go".into(), confidence: 0.7, reason: "ok".into(), model: "m".into() };
        cache.put(&key, &c).unwrap();
        assert_eq!(cache.get(&key).unwrap(), c);
    }
}
