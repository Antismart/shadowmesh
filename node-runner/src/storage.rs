//! Fragment Storage Manager
//!
//! Manages local storage of content fragments with automatic cleanup.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs;
use tokio::sync::RwLock;

/// Storage statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StorageStats {
    /// Total bytes stored
    pub total_bytes: u64,
    /// Number of fragments stored
    pub fragment_count: u64,
    /// Number of unique content items
    pub content_count: u64,
    /// Number of pinned items
    pub pinned_count: u64,
    /// Storage capacity in bytes
    pub capacity_bytes: u64,
    /// Last garbage collection timestamp
    pub last_gc: Option<u64>,
}

impl StorageStats {
    pub fn usage_percentage(&self) -> f64 {
        if self.capacity_bytes == 0 {
            return 0.0;
        }
        (self.total_bytes as f64 / self.capacity_bytes as f64) * 100.0
    }

    pub fn available_bytes(&self) -> u64 {
        self.capacity_bytes.saturating_sub(self.total_bytes)
    }
}

/// Stored fragment metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredFragment {
    /// Fragment hash
    pub hash: String,
    /// Content ID this fragment belongs to
    pub cid: String,
    /// Fragment index within content
    pub index: u32,
    /// Size in bytes
    pub size: u64,
    /// When the fragment was stored
    pub stored_at: u64,
    /// Last accessed timestamp
    pub last_accessed: u64,
    /// Access count
    pub access_count: u64,
    /// Whether this fragment is pinned
    pub pinned: bool,
}

/// Content metadata for stored items
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredContent {
    /// Content ID
    pub cid: String,
    /// Content name
    pub name: String,
    /// Total size in bytes
    pub total_size: u64,
    /// Number of fragments
    pub fragment_count: u32,
    /// Fragment hashes
    pub fragments: Vec<String>,
    /// When content was stored
    pub stored_at: u64,
    /// Whether content is pinned
    pub pinned: bool,
    /// MIME type
    pub mime_type: String,
}

/// Fragment storage manager
pub struct StorageManager {
    /// Data directory
    data_dir: PathBuf,
    /// Maximum storage capacity
    max_capacity: u64,
    /// Fragment metadata cache
    fragments: Arc<RwLock<HashMap<String, StoredFragment>>>,
    /// Content metadata cache
    content: Arc<RwLock<HashMap<String, StoredContent>>>,
    /// Current storage usage
    current_usage: Arc<RwLock<u64>>,
    /// Statistics
    stats: Arc<RwLock<StorageStats>>,
}

impl StorageManager {
    /// Create a new storage manager
    pub async fn new(data_dir: PathBuf, max_capacity: u64) -> Result<Self, std::io::Error> {
        // Ensure directories exist
        fs::create_dir_all(&data_dir).await?;
        fs::create_dir_all(data_dir.join("fragments")).await?;
        fs::create_dir_all(data_dir.join("metadata")).await?;

        let manager = Self {
            data_dir,
            max_capacity,
            fragments: Arc::new(RwLock::new(HashMap::new())),
            content: Arc::new(RwLock::new(HashMap::new())),
            current_usage: Arc::new(RwLock::new(0)),
            stats: Arc::new(RwLock::new(StorageStats {
                capacity_bytes: max_capacity,
                ..Default::default()
            })),
        };

        // Load existing metadata
        manager.load_metadata().await?;

        Ok(manager)
    }

    /// Load metadata from disk
    async fn load_metadata(&self) -> Result<(), std::io::Error> {
        let metadata_path = self.data_dir.join("metadata").join("index.json");

        if metadata_path.exists() {
            let data = fs::read_to_string(&metadata_path).await?;
            if let Ok((fragments, content)) = serde_json::from_str::<(
                HashMap<String, StoredFragment>,
                HashMap<String, StoredContent>,
            )>(&data)
            {
                let mut frags = self.fragments.write().await;
                let mut cont = self.content.write().await;
                *frags = fragments;
                *cont = content;

                // Calculate current usage
                let usage: u64 = frags.values().map(|f| f.size).sum();
                *self.current_usage.write().await = usage;

                // Update stats
                let mut stats = self.stats.write().await;
                stats.total_bytes = usage;
                stats.fragment_count = frags.len() as u64;
                stats.content_count = cont.len() as u64;
                stats.pinned_count = cont.values().filter(|c| c.pinned).count() as u64;
            }
        }

        Ok(())
    }

    /// Save metadata to disk
    async fn save_metadata(&self) -> Result<(), std::io::Error> {
        let fragments = self.fragments.read().await;
        let content = self.content.read().await;

        let data = serde_json::to_string_pretty(&(&*fragments, &*content))
            .map_err(std::io::Error::other)?;

        let metadata_path = self.data_dir.join("metadata").join("index.json");
        fs::write(&metadata_path, data).await?;

        Ok(())
    }

    /// Store a fragment
    pub async fn store_fragment(
        &self,
        hash: &str,
        cid: &str,
        index: u32,
        data: &[u8],
    ) -> Result<(), StorageError> {
        let size = data.len() as u64;

        // Check capacity
        let current = *self.current_usage.read().await;
        if current + size > self.max_capacity {
            return Err(StorageError::InsufficientSpace {
                required: size,
                available: self.max_capacity.saturating_sub(current),
            });
        }

        // Ensure fragment subdirectory exists and write fragment data
        let fragment_path = self.fragment_path(hash)?;
        if let Some(parent) = fragment_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(&fragment_path, data).await?;

        // Update metadata
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let fragment = StoredFragment {
            hash: hash.to_string(),
            cid: cid.to_string(),
            index,
            size,
            stored_at: now,
            last_accessed: now,
            access_count: 0,
            pinned: false,
        };

        {
            let mut fragments = self.fragments.write().await;
            fragments.insert(hash.to_string(), fragment);
        }

        // Update usage
        {
            let mut usage = self.current_usage.write().await;
            *usage += size;
        }

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.total_bytes += size;
            stats.fragment_count += 1;
        }

        // Persist metadata
        self.save_metadata().await?;

        Ok(())
    }

    /// Retrieve a fragment
    pub async fn get_fragment(&self, hash: &str) -> Result<Vec<u8>, StorageError> {
        // Check if fragment exists
        {
            let fragments = self.fragments.read().await;
            if !fragments.contains_key(hash) {
                return Err(StorageError::NotFound(hash.to_string()));
            }
        }

        // Read fragment data
        let fragment_path = self.fragment_path(hash)?;
        let data = fs::read(&fragment_path).await?;

        // Update access metadata
        {
            let mut fragments = self.fragments.write().await;
            if let Some(frag) = fragments.get_mut(hash) {
                frag.last_accessed = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                frag.access_count += 1;
            }
        }

        Ok(data)
    }

    /// Return the on-disk path for a fragment without reading it into memory.
    /// Updates access metadata. Used by the streaming download path.
    pub async fn get_fragment_path(&self, hash: &str) -> Result<PathBuf, StorageError> {
        {
            let fragments = self.fragments.read().await;
            if !fragments.contains_key(hash) {
                return Err(StorageError::NotFound(hash.to_string()));
            }
        }

        // Update access metadata
        {
            let mut fragments = self.fragments.write().await;
            if let Some(frag) = fragments.get_mut(hash) {
                frag.last_accessed = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                frag.access_count += 1;
            }
        }

        self.fragment_path(hash)
    }

    /// Check if fragment exists
    pub async fn has_fragment(&self, hash: &str) -> bool {
        let fragments = self.fragments.read().await;
        fragments.contains_key(hash)
    }

    /// Store content metadata
    pub async fn store_content(&self, content: StoredContent) -> Result<(), StorageError> {
        {
            let mut cont = self.content.write().await;
            cont.insert(content.cid.clone(), content);
        }

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.content_count = self.content.read().await.len() as u64;
        }

        self.save_metadata().await?;
        Ok(())
    }

    /// Get content metadata
    pub async fn get_content(&self, cid: &str) -> Option<StoredContent> {
        let content = self.content.read().await;
        content.get(cid).cloned()
    }

    /// List all stored content
    pub async fn list_content(&self) -> Vec<StoredContent> {
        let content = self.content.read().await;
        content.values().cloned().collect()
    }

    /// Pin content (prevent garbage collection)
    pub async fn pin(&self, cid: &str) -> Result<(), StorageError> {
        let mut content = self.content.write().await;
        let cont = content
            .get_mut(cid)
            .ok_or_else(|| StorageError::NotFound(cid.to_string()))?;
        cont.pinned = true;

        // Pin all fragments
        let fragment_hashes = cont.fragments.clone();
        drop(content);

        {
            let mut fragments = self.fragments.write().await;
            for hash in fragment_hashes {
                if let Some(frag) = fragments.get_mut(&hash) {
                    frag.pinned = true;
                }
            }
        }

        // Update stats
        {
            let content = self.content.read().await;
            let mut stats = self.stats.write().await;
            stats.pinned_count = content.values().filter(|c| c.pinned).count() as u64;
        }

        self.save_metadata().await?;
        Ok(())
    }

    /// Unpin content
    pub async fn unpin(&self, cid: &str) -> Result<(), StorageError> {
        let mut content = self.content.write().await;
        let cont = content
            .get_mut(cid)
            .ok_or_else(|| StorageError::NotFound(cid.to_string()))?;
        cont.pinned = false;

        // Unpin all fragments
        let fragment_hashes = cont.fragments.clone();
        drop(content);

        {
            let mut fragments = self.fragments.write().await;
            for hash in fragment_hashes {
                if let Some(frag) = fragments.get_mut(&hash) {
                    frag.pinned = false;
                }
            }
        }

        // Update stats
        {
            let content = self.content.read().await;
            let mut stats = self.stats.write().await;
            stats.pinned_count = content.values().filter(|c| c.pinned).count() as u64;
        }

        self.save_metadata().await?;
        Ok(())
    }

    /// Delete content and its fragments
    pub async fn delete(&self, cid: &str) -> Result<(), StorageError> {
        let content = {
            let content = self.content.read().await;
            content.get(cid).cloned()
        };

        let content = content.ok_or_else(|| StorageError::NotFound(cid.to_string()))?;

        if content.pinned {
            return Err(StorageError::Pinned(cid.to_string()));
        }

        // Delete fragments
        for hash in &content.fragments {
            let _ = self.delete_fragment(hash).await;
        }

        // Remove from metadata
        {
            let mut cont = self.content.write().await;
            cont.remove(cid);
        }

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.content_count = self.content.read().await.len() as u64;
        }

        self.save_metadata().await?;
        Ok(())
    }

    /// Delete a fragment
    async fn delete_fragment(&self, hash: &str) -> Result<(), StorageError> {
        let fragment_path = self.fragment_path(hash)?;

        let size = {
            let fragments = self.fragments.read().await;
            fragments.get(hash).map(|f| f.size).unwrap_or(0)
        };

        // Delete file
        if fragment_path.exists() {
            fs::remove_file(&fragment_path).await?;
        }

        // Remove from metadata
        {
            let mut fragments = self.fragments.write().await;
            fragments.remove(hash);
        }

        // Update usage
        {
            let mut usage = self.current_usage.write().await;
            *usage = usage.saturating_sub(size);
        }

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.total_bytes = stats.total_bytes.saturating_sub(size);
            stats.fragment_count = stats.fragment_count.saturating_sub(1);
        }

        Ok(())
    }

    /// Run garbage collection (remove least recently used unpinned content)
    pub async fn garbage_collect(&self, target_free_bytes: u64) -> Result<GCResult, StorageError> {
        let mut freed_bytes = 0u64;
        let mut removed_fragments = 0u64;
        let mut removed_content = 0u64;

        let current = *self.current_usage.read().await;
        let available = self.max_capacity.saturating_sub(current);

        if available >= target_free_bytes {
            return Ok(GCResult {
                freed_bytes: 0,
                removed_fragments: 0,
                removed_content: 0,
            });
        }

        let bytes_to_free = target_free_bytes - available;

        // Get unpinned content sorted by last access time
        let mut unpinned: Vec<StoredContent> = {
            let content = self.content.read().await;
            content.values().filter(|c| !c.pinned).cloned().collect()
        };

        // Get last access time from fragments
        let fragments = self.fragments.read().await;
        unpinned.sort_by(|a, b| {
            let a_last = a
                .fragments
                .iter()
                .filter_map(|h| fragments.get(h))
                .map(|f| f.last_accessed)
                .min()
                .unwrap_or(0);
            let b_last = b
                .fragments
                .iter()
                .filter_map(|h| fragments.get(h))
                .map(|f| f.last_accessed)
                .min()
                .unwrap_or(0);
            a_last.cmp(&b_last)
        });
        drop(fragments);

        // Remove content until we have enough space
        for content in unpinned {
            if freed_bytes >= bytes_to_free {
                break;
            }

            let content_size = content.total_size;
            let frag_count = content.fragments.len() as u64;

            if self.delete(&content.cid).await.is_ok() {
                freed_bytes += content_size;
                removed_fragments += frag_count;
                removed_content += 1;
            }
        }

        // Update last GC time
        {
            let mut stats = self.stats.write().await;
            stats.last_gc = Some(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            );
        }

        Ok(GCResult {
            freed_bytes,
            removed_fragments,
            removed_content,
        })
    }

    /// Get storage statistics
    pub async fn get_stats(&self) -> StorageStats {
        self.stats.read().await.clone()
    }

    /// Get fragment path.
    ///
    /// Validates that the hash contains only safe alphanumeric characters
    /// to prevent path traversal attacks (e.g. "../../etc/passwd").
    fn fragment_path(&self, hash: &str) -> Result<PathBuf, StorageError> {
        if hash.is_empty()
            || hash.len() > 512
            || !hash.chars().all(|c| c.is_ascii_alphanumeric())
        {
            return Err(StorageError::InvalidHash(
                hash[..hash.len().min(32)].to_string(),
            ));
        }
        // Use first 2 chars as subdirectory for better filesystem performance
        let subdir = &hash[..2.min(hash.len())];
        Ok(self.data_dir.join("fragments").join(subdir).join(hash))
    }

}

/// Garbage collection result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GCResult {
    pub freed_bytes: u64,
    pub removed_fragments: u64,
    pub removed_content: u64,
}

/// Storage errors
#[derive(Debug)]
pub enum StorageError {
    /// Not enough space
    InsufficientSpace { required: u64, available: u64 },
    /// Content/fragment not found
    NotFound(String),
    /// Content is pinned and cannot be deleted
    Pinned(String),
    /// Invalid hash format
    InvalidHash(String),
    /// IO error
    Io(std::io::Error),
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InsufficientSpace {
                required,
                available,
            } => {
                write!(
                    f,
                    "Insufficient space: need {} bytes, have {} available",
                    required, available
                )
            }
            Self::NotFound(id) => write!(f, "Not found: {}", id),
            Self::Pinned(id) => write!(f, "Content is pinned: {}", id),
            Self::InvalidHash(h) => write!(f, "Invalid fragment hash: '{}'", h),
            Self::Io(e) => write!(f, "IO error: {}", e),
        }
    }
}

impl std::error::Error for StorageError {}

impl From<std::io::Error> for StorageError {
    fn from(err: std::io::Error) -> Self {
        StorageError::Io(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_store_and_retrieve_fragment() {
        let dir = tempdir().unwrap();
        let manager = StorageManager::new(dir.path().to_path_buf(), 1024 * 1024)
            .await
            .unwrap();

        let data = b"test fragment data";
        let hash = "testhash123";

        manager
            .store_fragment(hash, "cid123", 0, data)
            .await
            .unwrap();

        assert!(manager.has_fragment(hash).await);

        let retrieved = manager.get_fragment(hash).await.unwrap();
        assert_eq!(retrieved, data);
    }

    #[tokio::test]
    async fn test_storage_stats() {
        let dir = tempdir().unwrap();
        let manager = StorageManager::new(dir.path().to_path_buf(), 1024 * 1024)
            .await
            .unwrap();

        let data = vec![0u8; 1000];
        manager
            .store_fragment("hash1", "cid1", 0, &data)
            .await
            .unwrap();

        let stats = manager.get_stats().await;
        assert_eq!(stats.fragment_count, 1);
        assert_eq!(stats.total_bytes, 1000);
    }
}
