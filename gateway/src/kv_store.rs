use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Default cap on the number of distinct KV namespaces.
pub const DEFAULT_MAX_NAMESPACES: usize = 10_000;

pub struct KvConfig {
    pub max_keys_per_namespace: usize,
    pub max_value_size_bytes: usize,
    /// Cap on the total number of distinct namespaces (bounds DashMap growth
    /// so a single identity cannot exhaust memory by creating namespaces).
    pub max_namespaces: usize,
}

struct KvEntry {
    value: Vec<u8>,
    expires_at: Option<Instant>,
}

impl KvEntry {
    fn is_expired(&self) -> bool {
        self.expires_at.map_or(false, |t| Instant::now() > t)
    }
}

pub struct KvStore {
    namespaces: DashMap<String, DashMap<String, KvEntry>>,
    redis: Option<Arc<crate::redis_client::RedisClient>>,
    config: KvConfig,
}

impl KvStore {
    pub fn new(config: KvConfig, redis: Option<Arc<crate::redis_client::RedisClient>>) -> Self {
        let store = Self {
            namespaces: DashMap::new(),
            redis,
            config,
        };
        store
    }

    /// Start a background task that cleans up expired KV entries every 60 seconds.
    pub fn start_cleanup_task(self: &Arc<Self>) {
        let store = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                store.cleanup_expired();
            }
        });
    }

    pub fn get(&self, namespace: &str, key: &str) -> Option<Vec<u8>> {
        let ns = self.namespaces.get(namespace)?;
        let entry = ns.get(key)?;
        if entry.is_expired() {
            drop(entry);
            ns.remove(key);
            return None;
        }
        Some(entry.value.clone())
    }

    pub fn put(
        &self,
        namespace: &str,
        key: &str,
        value: Vec<u8>,
        ttl_secs: Option<u64>,
    ) -> Result<(), String> {
        if value.len() > self.config.max_value_size_bytes {
            return Err(format!(
                "Value too large: {} bytes (max {})",
                value.len(),
                self.config.max_value_size_bytes
            ));
        }

        // Bound the number of distinct namespaces. Only reject when this call
        // would create a brand-new namespace beyond the cap; existing
        // namespaces are always writable.
        if !self.namespaces.contains_key(namespace)
            && self.namespaces.len() >= self.config.max_namespaces
        {
            return Err(format!(
                "Namespace limit reached ({} namespaces)",
                self.config.max_namespaces
            ));
        }

        let ns = self
            .namespaces
            .entry(namespace.to_string())
            .or_insert_with(DashMap::new);

        // P12: Check limit only for new keys (updates are always allowed).
        // DashMap operations are atomic per-key, so contains_key + len check
        // is safe against races for the same key. For different keys, we accept
        // a small over-count window (max overshoot = concurrent writer count).
        if !ns.contains_key(key) {
            let current_len = ns.len();
            if current_len >= self.config.max_keys_per_namespace {
                return Err(format!(
                    "Namespace '{}' key limit reached ({}/{})",
                    namespace, current_len, self.config.max_keys_per_namespace
                ));
            }
        }

        let expires_at = ttl_secs.map(|s| Instant::now() + Duration::from_secs(s));

        ns.insert(
            key.to_string(),
            KvEntry {
                value: value.clone(),
                expires_at,
            },
        );

        // Persist to Redis if available
        if let Some(ref redis) = self.redis {
            let redis_key = format!("kv:{}:{}", namespace, key);
            let redis = redis.clone();
            let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &value);
            tokio::spawn(async move {
                if let Err(e) = redis.set_json(&redis_key, &encoded, None).await {
                    tracing::warn!(key = %redis_key, error = %e, "KV Redis persist failed");
                }
            });
        }

        Ok(())
    }

    pub fn delete(&self, namespace: &str, key: &str) -> bool {
        let Some(ns) = self.namespaces.get(namespace) else {
            return false;
        };
        let removed = ns.remove(key).is_some();

        if removed {
            if let Some(ref redis) = self.redis {
                let redis_key = format!("kv:{}:{}", namespace, key);
                let redis = redis.clone();
                tokio::spawn(async move {
                    let _ = redis.delete(&redis_key).await;
                });
            }
        }

        removed
    }

    pub fn list_keys(&self, namespace: &str, prefix: Option<&str>) -> Vec<String> {
        let Some(ns) = self.namespaces.get(namespace) else {
            return vec![];
        };
        ns.iter()
            .filter(|entry| !entry.value().is_expired())
            .filter(|entry| {
                prefix.map_or(true, |p| entry.key().starts_with(p))
            })
            .map(|entry| entry.key().clone())
            .collect()
    }

    pub fn namespace_size(&self, namespace: &str) -> usize {
        self.namespaces
            .get(namespace)
            .map_or(0, |ns| ns.len())
    }

    pub fn cleanup_expired(&self) {
        let mut total_cleaned = 0usize;
        for ns_entry in self.namespaces.iter() {
            let ns = ns_entry.value();
            let expired_keys: Vec<String> = ns
                .iter()
                .filter(|e| e.value().is_expired())
                .map(|e| e.key().clone())
                .collect();
            for key in &expired_keys {
                ns.remove(key);
            }
            total_cleaned += expired_keys.len();
        }
        if total_cleaned > 0 {
            tracing::debug!(cleaned = total_cleaned, "KV expired entries cleaned");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> KvStore {
        KvStore::new(
            KvConfig {
                max_keys_per_namespace: 100,
                max_value_size_bytes: 1024,
                max_namespaces: 1000,
            },
            None,
        )
    }

    #[test]
    fn put_and_get() {
        let store = test_store();
        store.put("ns1", "key1", b"value1".to_vec(), None).unwrap();
        assert_eq!(store.get("ns1", "key1"), Some(b"value1".to_vec()));
    }

    #[test]
    fn get_missing() {
        let store = test_store();
        assert_eq!(store.get("ns1", "missing"), None);
    }

    #[test]
    fn delete_key() {
        let store = test_store();
        store.put("ns1", "key1", b"val".to_vec(), None).unwrap();
        assert!(store.delete("ns1", "key1"));
        assert_eq!(store.get("ns1", "key1"), None);
    }

    #[test]
    fn delete_missing() {
        let store = test_store();
        assert!(!store.delete("ns1", "missing"));
    }

    #[test]
    fn value_too_large() {
        let store = test_store();
        let big = vec![0u8; 2048];
        assert!(store.put("ns1", "key1", big, None).is_err());
    }

    #[test]
    fn key_limit_enforced() {
        let store = KvStore::new(
            KvConfig {
                max_keys_per_namespace: 2,
                max_value_size_bytes: 1024,
                max_namespaces: 1000,
            },
            None,
        );
        store.put("ns1", "a", b"1".to_vec(), None).unwrap();
        store.put("ns1", "b", b"2".to_vec(), None).unwrap();
        assert!(store.put("ns1", "c", b"3".to_vec(), None).is_err());
    }

    #[test]
    fn ttl_expiration() {
        let store = test_store();
        // TTL of 0 seconds = immediately expired
        store.put("ns1", "key1", b"val".to_vec(), Some(0)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        assert_eq!(store.get("ns1", "key1"), None);
    }

    #[test]
    fn list_keys_with_prefix() {
        let store = test_store();
        store.put("ns1", "user:1", b"a".to_vec(), None).unwrap();
        store.put("ns1", "user:2", b"b".to_vec(), None).unwrap();
        store.put("ns1", "post:1", b"c".to_vec(), None).unwrap();

        let mut keys = store.list_keys("ns1", Some("user:"));
        keys.sort();
        assert_eq!(keys, vec!["user:1", "user:2"]);
    }

    #[test]
    fn cleanup_expired_removes_entries() {
        let store = test_store();
        store.put("ns1", "a", b"1".to_vec(), Some(0)).unwrap();
        store.put("ns1", "b", b"2".to_vec(), Some(0)).unwrap();
        store.put("ns1", "c", b"3".to_vec(), Some(0)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        store.cleanup_expired();
        assert_eq!(store.namespace_size("ns1"), 0);
    }

    #[test]
    fn update_existing_key_bypasses_limit() {
        let store = KvStore::new(
            KvConfig {
                max_keys_per_namespace: 2,
                max_value_size_bytes: 1024,
                max_namespaces: 1000,
            },
            None,
        );
        store.put("ns1", "a", b"1".to_vec(), None).unwrap();
        store.put("ns1", "b", b"2".to_vec(), None).unwrap();
        // Updating existing key should succeed even at limit
        store.put("ns1", "a", b"updated".to_vec(), None).unwrap();
        assert_eq!(store.get("ns1", "a"), Some(b"updated".to_vec()));
    }

    #[test]
    fn namespace_isolation() {
        let store = test_store();
        store.put("ns1", "key", b"a".to_vec(), None).unwrap();
        store.put("ns2", "key", b"b".to_vec(), None).unwrap();
        assert_eq!(store.get("ns1", "key"), Some(b"a".to_vec()));
        assert_eq!(store.get("ns2", "key"), Some(b"b".to_vec()));
    }

    #[test]
    fn namespace_limit_enforced() {
        let store = KvStore::new(
            KvConfig {
                max_keys_per_namespace: 100,
                max_value_size_bytes: 1024,
                max_namespaces: 2,
            },
            None,
        );
        store.put("ns1", "k", b"1".to_vec(), None).unwrap();
        store.put("ns2", "k", b"2".to_vec(), None).unwrap();
        // Third distinct namespace should be rejected.
        assert!(store.put("ns3", "k", b"3".to_vec(), None).is_err());
        // Writing to an existing namespace still works.
        store.put("ns1", "k2", b"x".to_vec(), None).unwrap();
    }
}
