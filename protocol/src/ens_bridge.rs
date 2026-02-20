//! ENS (Ethereum Name Service) Bridge for ShadowMesh
//!
//! Provides optional integration with ENS for blockchain-backed name resolution.
//! This module is behind the `ens` feature flag and requires an Ethereum RPC endpoint.
//!
//! # How It Works
//!
//! 1. User provides an ENS name (e.g., `myapp.eth`)
//! 2. The bridge looks up the ENS `contenthash` record on Ethereum
//! 3. If it contains a `shadow://` URI, extract the `.shadow` name or CID
//! 4. Resolve via the DHT naming layer
//!
//! # Usage
//!
//! ```rust,ignore
//! let bridge = EnsBridge::new("https://mainnet.infura.io/v3/YOUR_KEY");
//! let result = bridge.resolve_ens("myapp.eth").await?;
//! ```

use crate::naming::NamingError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Default cache TTL for ENS resolutions (1 hour)
const ENS_CACHE_TTL: Duration = Duration::from_secs(3600);

/// Maximum ENS cache entries
const MAX_ENS_CACHE: usize = 1000;

/// Result of an ENS resolution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EnsResolution {
    /// Maps to a `.shadow` name (resolve further via DHT)
    ShadowName(String),
    /// Maps directly to a content CID
    ContentId(String),
    /// ENS name not found or no contenthash set
    NotFound,
}

/// Cached ENS resolution entry
struct EnsCacheEntry {
    resolution: EnsResolution,
    cached_at: Instant,
}

/// ENS bridge for resolving Ethereum Name Service records to ShadowMesh resources.
///
/// This is a lightweight bridge that parses ENS `contenthash` records.
/// Full on-chain resolution requires the `ens` feature flag.
pub struct EnsBridge {
    /// Ethereum RPC endpoint URL
    rpc_url: Option<String>,
    /// Resolution cache
    cache: HashMap<String, EnsCacheEntry>,
}

impl EnsBridge {
    /// Create a new ENS bridge with an Ethereum RPC endpoint.
    pub fn new(rpc_url: &str) -> Self {
        Self {
            rpc_url: Some(rpc_url.to_string()),
            cache: HashMap::new(),
        }
    }

    /// Create a bridge without an RPC endpoint (cache-only, for testing).
    pub fn without_rpc() -> Self {
        Self {
            rpc_url: None,
            cache: HashMap::new(),
        }
    }

    /// Resolve an ENS name to a ShadowMesh resource.
    ///
    /// Checks the cache first, then queries the Ethereum RPC if configured.
    pub async fn resolve_ens(&mut self, ens_name: &str) -> Result<EnsResolution, NamingError> {
        let normalized = ens_name.to_lowercase();

        // Check cache
        if let Some(entry) = self.cache.get(&normalized) {
            if entry.cached_at.elapsed() < ENS_CACHE_TTL {
                return Ok(entry.resolution.clone());
            }
        }

        // Without RPC, we can only use cache
        let Some(_rpc_url) = &self.rpc_url else {
            return Ok(EnsResolution::NotFound);
        };

        // NOTE: Full ENS resolution requires the `ethers` crate (behind `ens` feature).
        // This stub demonstrates the interface. When the `ens` feature is enabled,
        // this would:
        // 1. Connect to Ethereum via the RPC URL
        // 2. Resolve the ENS name to a contenthash record
        // 3. Parse the contenthash for shadow:// or ipfs:// URIs
        // 4. Return the appropriate EnsResolution variant

        let resolution = EnsResolution::NotFound;

        // Cache the result
        self.cache_result(&normalized, resolution.clone());

        Ok(resolution)
    }

    /// Parse a contenthash value to extract ShadowMesh references.
    ///
    /// Supported formats:
    /// - `shadow://myapp.shadow` -> `EnsResolution::ShadowName("myapp.shadow")`
    /// - `shadow://QmABC...` -> `EnsResolution::ContentId("QmABC...")`
    /// - `ipfs://QmABC...` -> `EnsResolution::ContentId("QmABC...")`
    pub fn parse_contenthash(contenthash: &str) -> EnsResolution {
        let trimmed = contenthash.trim();

        if let Some(shadow_ref) = trimmed.strip_prefix("shadow://") {
            if shadow_ref.ends_with(".shadow") {
                EnsResolution::ShadowName(shadow_ref.to_string())
            } else {
                EnsResolution::ContentId(shadow_ref.to_string())
            }
        } else if let Some(ipfs_ref) = trimmed.strip_prefix("ipfs://") {
            EnsResolution::ContentId(ipfs_ref.to_string())
        } else {
            EnsResolution::NotFound
        }
    }

    /// Clear expired cache entries.
    pub fn cleanup_cache(&mut self) {
        self.cache
            .retain(|_, entry| entry.cached_at.elapsed() < ENS_CACHE_TTL);
    }

    fn cache_result(&mut self, name: &str, resolution: EnsResolution) {
        // Evict if over capacity
        if self.cache.len() >= MAX_ENS_CACHE {
            if let Some(oldest_key) = self
                .cache
                .iter()
                .min_by_key(|(_, e)| e.cached_at)
                .map(|(k, _)| k.clone())
            {
                self.cache.remove(&oldest_key);
            }
        }

        self.cache.insert(
            name.to_string(),
            EnsCacheEntry {
                resolution,
                cached_at: Instant::now(),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_shadow_name() {
        let result = EnsBridge::parse_contenthash("shadow://myapp.shadow");
        assert!(matches!(result, EnsResolution::ShadowName(name) if name == "myapp.shadow"));
    }

    #[test]
    fn test_parse_shadow_cid() {
        let result = EnsBridge::parse_contenthash("shadow://QmABC123");
        assert!(matches!(result, EnsResolution::ContentId(cid) if cid == "QmABC123"));
    }

    #[test]
    fn test_parse_ipfs_cid() {
        let result = EnsBridge::parse_contenthash("ipfs://QmXYZ456");
        assert!(matches!(result, EnsResolution::ContentId(cid) if cid == "QmXYZ456"));
    }

    #[test]
    fn test_parse_unknown() {
        let result = EnsBridge::parse_contenthash("https://example.com");
        assert!(matches!(result, EnsResolution::NotFound));
    }

    #[tokio::test]
    async fn test_bridge_without_rpc() {
        let mut bridge = EnsBridge::without_rpc();
        let result = bridge.resolve_ens("test.eth").await.unwrap();
        assert!(matches!(result, EnsResolution::NotFound));
    }
}
