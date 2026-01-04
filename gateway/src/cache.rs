//! Content caching for the ShadowMesh gateway
//! 
//! Provides in-memory LRU caching for frequently accessed content.

use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

/// Cache entry with expiration
struct CacheEntry {
    data: Vec<u8>,
    content_type: String,
    created_at: Instant,
    ttl: Duration,
}

impl CacheEntry {
    fn is_expired(&self) -> bool {
        self.created_at.elapsed() > self.ttl
    }
}

/// Simple in-memory cache for content
pub struct ContentCache {
    entries: RwLock<HashMap<String, CacheEntry>>,
    max_entries: usize,
    default_ttl: Duration,
}

impl ContentCache {
    /// Create a new cache with default settings
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            max_entries: 1000,
            default_ttl: Duration::from_secs(3600), // 1 hour
        }
    }

    /// Create a cache with custom settings
    pub fn with_config(max_entries: usize, default_ttl: Duration) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            max_entries,
            default_ttl,
        }
    }

    /// Get content from cache
    pub fn get(&self, cid: &str) -> Option<(Vec<u8>, String)> {
        let entries = self.entries.read().ok()?;
        let entry = entries.get(cid)?;
        
        if entry.is_expired() {
            return None;
        }
        
        Some((entry.data.clone(), entry.content_type.clone()))
    }

    /// Store content in cache
    pub fn set(&self, cid: String, data: Vec<u8>, content_type: String) {
        self.set_with_ttl(cid, data, content_type, self.default_ttl);
    }

    /// Store content with custom TTL
    pub fn set_with_ttl(&self, cid: String, data: Vec<u8>, content_type: String, ttl: Duration) {
        if let Ok(mut entries) = self.entries.write() {
            // Evict expired entries if at capacity
            if entries.len() >= self.max_entries {
                self.evict_expired(&mut entries);
            }
            
            // Still at capacity? Remove oldest entry
            if entries.len() >= self.max_entries {
                if let Some(oldest_key) = entries
                    .iter()
                    .min_by_key(|(_, v)| v.created_at)
                    .map(|(k, _)| k.clone())
                {
                    entries.remove(&oldest_key);
                }
            }
            
            entries.insert(cid, CacheEntry {
                data,
                content_type,
                created_at: Instant::now(),
                ttl,
            });
        }
    }

    /// Remove content from cache
    pub fn remove(&self, cid: &str) {
        if let Ok(mut entries) = self.entries.write() {
            entries.remove(cid);
        }
    }

    /// Clear all expired entries
    pub fn clear_expired(&self) {
        if let Ok(mut entries) = self.entries.write() {
            self.evict_expired(&mut entries);
        }
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        let entries = self.entries.read().ok();
        let (total, expired) = entries
            .map(|e| {
                let expired = e.values().filter(|v| v.is_expired()).count();
                (e.len(), expired)
            })
            .unwrap_or((0, 0));
        
        CacheStats {
            total_entries: total,
            expired_entries: expired,
            max_entries: self.max_entries,
        }
    }

    fn evict_expired(&self, entries: &mut HashMap<String, CacheEntry>) {
        entries.retain(|_, v| !v.is_expired());
    }
}

impl Default for ContentCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub total_entries: usize,
    pub expired_entries: usize,
    pub max_entries: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_set_get() {
        let cache = ContentCache::new();
        
        cache.set(
            "test-cid".to_string(),
            b"test data".to_vec(),
            "text/plain".to_string(),
        );
        
        let result = cache.get("test-cid");
        assert!(result.is_some());
        
        let (data, content_type) = result.unwrap();
        assert_eq!(data, b"test data");
        assert_eq!(content_type, "text/plain");
    }

    #[test]
    fn test_cache_miss() {
        let cache = ContentCache::new();
        assert!(cache.get("nonexistent").is_none());
    }

    #[test]
    fn test_cache_remove() {
        let cache = ContentCache::new();
        
        cache.set("test".to_string(), vec![1, 2, 3], "application/octet-stream".to_string());
        assert!(cache.get("test").is_some());
        
        cache.remove("test");
        assert!(cache.get("test").is_none());
    }

    #[test]
    fn test_cache_expiration() {
        let cache = ContentCache::with_config(100, Duration::from_millis(1));
        
        cache.set("test".to_string(), vec![1, 2, 3], "text/plain".to_string());
        
        // Wait for expiration
        std::thread::sleep(Duration::from_millis(10));
        
        assert!(cache.get("test").is_none());
    }
}
