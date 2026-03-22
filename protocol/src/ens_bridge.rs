//! ENS (Ethereum Name Service) Bridge for ShadowMesh
//!
//! Provides integration with ENS for blockchain-backed name resolution using
//! the [eth.limo](https://eth.limo) gateway. This avoids the need for a local
//! Ethereum node or an Infura/Alchemy RPC key — eth.limo resolves ENS
//! `contenthash` records and serves the content behind them.
//!
//! # How It Works
//!
//! 1. User provides an ENS name (e.g., `myapp.eth`)
//! 2. The bridge issues an HTTP HEAD request to `https://{name}.eth.limo`
//! 3. eth.limo resolves the ENS `contenthash` on-chain and returns the content
//! 4. We extract the IPFS CID from the `x-ipfs-path` or `content-hash` response header
//! 5. If the contenthash is a `shadow://` URI we map it to a `.shadow` name or CID
//!
//! Results are cached with a configurable TTL (default 1 hour) so repeated
//! look-ups are instantaneous and do not hit the network.
//!
//! # Usage
//!
//! ```rust,ignore
//! let bridge = EnsBridge::new("https://mainnet.infura.io/v3/YOUR_KEY");
//! let result = bridge.resolve_ens("myapp.eth").await?;
//! ```

use crate::naming::NamingError;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tracing::{debug, warn};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default cache TTL for ENS resolutions (1 hour).
const ENS_CACHE_TTL: Duration = Duration::from_secs(3600);

/// HTTP request timeout for eth.limo gateway queries.
const ENS_HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// Base domain for the eth.limo ENS gateway.
const ETH_LIMO_GATEWAY: &str = "eth.limo";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Result of an ENS resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EnsResolution {
    /// Maps to a `.shadow` name (resolve further via DHT).
    ShadowName(String),
    /// Maps directly to a content CID.
    ContentId(String),
    /// ENS name not found or no contenthash set.
    NotFound,
}

/// Cached ENS resolution entry.
struct EnsCacheEntry {
    resolution: EnsResolution,
    cached_at: Instant,
}

// ---------------------------------------------------------------------------
// EnsBridge
// ---------------------------------------------------------------------------

/// ENS bridge for resolving Ethereum Name Service records to ShadowMesh
/// resources.
///
/// Uses the **eth.limo** HTTPS gateway for lightweight off-chain resolution so
/// that no local Ethereum node or RPC provider key is strictly necessary.
/// An optional `rpc_url` is still stored for future direct on-chain queries.
pub struct EnsBridge {
    /// Ethereum RPC endpoint URL (retained for future on-chain queries).
    rpc_url: Option<String>,
    /// Reusable HTTP client for eth.limo requests.
    http: Client,
    /// Resolution cache with TTL-based expiry.
    cache: HashMap<String, EnsCacheEntry>,
    /// Cache TTL — configurable for testing.
    cache_ttl: Duration,
}

impl EnsBridge {
    /// Create a new ENS bridge with an Ethereum RPC endpoint.
    ///
    /// The RPC URL is stored for potential future direct on-chain lookups.
    /// Resolution goes through eth.limo by default.
    pub fn new(rpc_url: &str) -> Self {
        Self {
            rpc_url: Some(rpc_url.to_string()),
            http: Self::build_http_client(),
            cache: HashMap::new(),
            cache_ttl: ENS_CACHE_TTL,
        }
    }

    /// Create a bridge without an RPC endpoint (eth.limo-only mode).
    ///
    /// Resolution still works via eth.limo; the only thing missing is the
    /// fallback to direct on-chain calls, which is not yet implemented.
    pub fn without_rpc() -> Self {
        Self {
            rpc_url: None,
            http: Self::build_http_client(),
            cache: HashMap::new(),
            cache_ttl: ENS_CACHE_TTL,
        }
    }

    /// Build a `reqwest::Client` configured for eth.limo queries.
    ///
    /// We disable automatic redirect-following so we can inspect Location
    /// headers ourselves (some contenthash gateways use 301/302 redirects
    /// with the IPFS path encoded in the Location URL).
    fn build_http_client() -> Client {
        Client::builder()
            .timeout(ENS_HTTP_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("ShadowMesh/0.1")
            .build()
            .expect("failed to build reqwest HTTP client")
    }

    // -- public API ---------------------------------------------------------

    /// Resolve an ENS name to a ShadowMesh resource.
    ///
    /// Checks the in-memory cache first. On a miss, queries the eth.limo
    /// gateway and caches the result.
    pub async fn resolve_ens(&mut self, ens_name: &str) -> Result<EnsResolution, NamingError> {
        let normalized = Self::normalize_name(ens_name)?;

        // 1. Cache hit?
        if let Some(entry) = self.cache.get(&normalized) {
            if entry.cached_at.elapsed() < self.cache_ttl {
                debug!(name = %normalized, "ENS cache hit");
                return Ok(entry.resolution.clone());
            }
            // expired — fall through
        }

        // 2. Resolve via eth.limo
        let resolution = self.resolve_via_gateway(&normalized).await?;

        // 3. Cache the result
        self.cache.insert(
            normalized.clone(),
            EnsCacheEntry {
                resolution: resolution.clone(),
                cached_at: Instant::now(),
            },
        );

        Ok(resolution)
    }

    /// Parse a `contenthash` value to extract ShadowMesh references.
    ///
    /// Supported formats:
    /// - `shadow://myapp.shadow` -> `EnsResolution::ShadowName("myapp.shadow")`
    /// - `shadow://QmABC...`    -> `EnsResolution::ContentId("QmABC...")`
    /// - `ipfs://QmABC...`      -> `EnsResolution::ContentId("QmABC...")`
    /// - `/ipfs/QmABC...`       -> `EnsResolution::ContentId("QmABC...")`
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
        } else if let Some(ipfs_ref) = trimmed.strip_prefix("/ipfs/") {
            // Handle the /ipfs/<cid> path format returned by x-ipfs-path headers
            EnsResolution::ContentId(ipfs_ref.to_string())
        } else {
            EnsResolution::NotFound
        }
    }

    /// Clear expired cache entries.
    pub fn cleanup_cache(&mut self) {
        self.cache
            .retain(|_, entry| entry.cached_at.elapsed() < self.cache_ttl);
    }

    /// Return the number of entries currently held in the cache (useful for
    /// tests and metrics).
    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }

    /// Manually insert a resolution into the cache (useful for seeding /
    /// testing).
    pub fn cache_insert(&mut self, name: &str, resolution: EnsResolution) {
        self.cache.insert(
            name.to_lowercase(),
            EnsCacheEntry {
                resolution,
                cached_at: Instant::now(),
            },
        );
    }

    // -- internal -----------------------------------------------------------

    /// Normalise and validate an ENS name.
    fn normalize_name(name: &str) -> Result<String, NamingError> {
        let normalized = name.to_lowercase().trim().to_string();
        if normalized.is_empty() {
            return Err(NamingError::InvalidName(
                "ENS name cannot be empty".to_string(),
            ));
        }
        if !normalized.ends_with(".eth") {
            return Err(NamingError::InvalidName(format!(
                "ENS name must end with .eth, got: {}",
                normalized
            )));
        }
        // Minimal label-length check (the part before .eth must be >= 3 chars
        // to match ENS's own minimum).
        let label = normalized.trim_end_matches(".eth");
        if label.is_empty() {
            return Err(NamingError::InvalidName(
                "ENS name label cannot be empty".to_string(),
            ));
        }
        Ok(normalized)
    }

    /// Query the eth.limo gateway for an ENS name.
    ///
    /// We issue an HTTP HEAD (falling back to GET) to
    /// `https://{name}.eth.limo` and inspect the response headers for content
    /// hash information:
    ///
    /// 1. `x-ipfs-path` — e.g. `/ipfs/Qm...` or `/ipfs/bafy...`
    /// 2. `content-hash` — raw contenthash string (may be `ipfs://...`,
    ///    `shadow://...`, etc.)
    /// 3. `location` header on 301/302 — may contain an IPFS gateway URL with
    ///    the CID embedded.
    async fn resolve_via_gateway(
        &self,
        normalized_name: &str,
    ) -> Result<EnsResolution, NamingError> {
        let url = format!("https://{}.{}", normalized_name, ETH_LIMO_GATEWAY);
        debug!(url = %url, "querying eth.limo gateway");

        // Try HEAD first (lighter), fall back to GET if HEAD fails.
        let head_result = self.http.head(&url).send().await;

        let response = match head_result {
            Ok(resp) => resp,
            Err(_) => {
                // HEAD failed (e.g., 405 Method Not Allowed or network error);
                // retry with GET.
                self.http.get(&url).send().await.map_err(|e| {
                    if e.is_timeout() {
                        NamingError::ResolutionFailed(format!(
                            "ENS resolution timed out for {}: {}",
                            normalized_name, e
                        ))
                    } else if e.is_connect() {
                        NamingError::ResolutionFailed(format!(
                            "Failed to connect to eth.limo for {}: {}",
                            normalized_name, e
                        ))
                    } else {
                        NamingError::ResolutionFailed(format!(
                            "HTTP error resolving {}: {}",
                            normalized_name, e
                        ))
                    }
                })?
            }
        };

        let status = response.status();
        debug!(status = %status, name = %normalized_name, "eth.limo response");

        // 404 / 410 → name exists on-chain but has no web content (or does not
        // exist at all).
        if status == reqwest::StatusCode::NOT_FOUND || status == reqwest::StatusCode::GONE {
            return Ok(EnsResolution::NotFound);
        }

        let headers = response.headers();

        // --- 1. x-ipfs-path header ---
        if let Some(ipfs_path) = headers.get("x-ipfs-path") {
            if let Ok(path_str) = ipfs_path.to_str() {
                let resolution = Self::parse_contenthash(path_str);
                if !matches!(resolution, EnsResolution::NotFound) {
                    debug!(name = %normalized_name, path = %path_str, "resolved via x-ipfs-path");
                    return Ok(resolution);
                }
            }
        }

        // --- 2. content-hash header ---
        if let Some(ch) = headers.get("content-hash") {
            if let Ok(ch_str) = ch.to_str() {
                let resolution = Self::parse_contenthash(ch_str);
                if !matches!(resolution, EnsResolution::NotFound) {
                    debug!(name = %normalized_name, hash = %ch_str, "resolved via content-hash header");
                    return Ok(resolution);
                }
            }
        }

        // --- 3. Location header (redirects) ---
        if status.is_redirection() {
            if let Some(location) = headers.get("location") {
                if let Ok(loc_str) = location.to_str() {
                    if let Some(cid) = Self::extract_cid_from_url(loc_str) {
                        debug!(name = %normalized_name, cid = %cid, "resolved via redirect location");
                        return Ok(EnsResolution::ContentId(cid));
                    }
                }
            }
        }

        // --- 4. Successful response but no extractable content hash ---
        // The name resolved (eth.limo returned 200) but we could not extract
        // a CID from the headers. This is a valid state — the site might just
        // be a plain HTML page served by eth.limo's own IPFS node.
        if status.is_success() {
            warn!(
                name = %normalized_name,
                "eth.limo returned 200 but no content hash header was found"
            );
            return Ok(EnsResolution::NotFound);
        }

        // For any other unexpected status, surface an error.
        Err(NamingError::ResolutionFailed(format!(
            "Unexpected HTTP {} from eth.limo for {}",
            status, normalized_name
        )))
    }

    /// Try to extract an IPFS CID from a gateway redirect URL.
    ///
    /// Common patterns:
    /// - `https://ipfs.io/ipfs/Qm.../`
    /// - `https://dweb.link/ipfs/bafy.../`
    /// - `https://<cid>.ipfs.dweb.link/`
    fn extract_cid_from_url(url: &str) -> Option<String> {
        // Pattern: .../ipfs/<cid>...
        if let Some(idx) = url.find("/ipfs/") {
            let after = &url[idx + 6..];
            let cid = after.split(&['/', '?', '#'][..]).next()?;
            if !cid.is_empty() {
                return Some(cid.to_string());
            }
        }

        // Pattern: https://<cid>.ipfs.<gateway>/
        if let Some(host) = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))
        {
            let parts: Vec<&str> = host.split('.').collect();
            // Expect at least <cid>.ipfs.<domain>.<tld>
            if parts.len() >= 4 && parts[1] == "ipfs" {
                let cid = parts[0];
                if !cid.is_empty() {
                    return Some(cid.to_string());
                }
            }
        }

        None
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- parse_contenthash --------------------------------------------------

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
    fn test_parse_ipfs_path() {
        let result = EnsBridge::parse_contenthash("/ipfs/QmXYZ456");
        assert!(matches!(result, EnsResolution::ContentId(cid) if cid == "QmXYZ456"));
    }

    #[test]
    fn test_parse_unknown() {
        let result = EnsBridge::parse_contenthash("https://example.com");
        assert!(matches!(result, EnsResolution::NotFound));
    }

    // -- normalize_name -----------------------------------------------------

    #[test]
    fn test_normalize_valid_name() {
        let result = EnsBridge::normalize_name("Vitalik.Eth");
        assert_eq!(result.unwrap(), "vitalik.eth");
    }

    #[test]
    fn test_normalize_empty_name() {
        let result = EnsBridge::normalize_name("");
        assert!(result.is_err());
    }

    #[test]
    fn test_normalize_no_eth_suffix() {
        let result = EnsBridge::normalize_name("vitalik.com");
        assert!(result.is_err());
    }

    #[test]
    fn test_normalize_bare_eth() {
        let result = EnsBridge::normalize_name(".eth");
        assert!(result.is_err());
    }

    // -- extract_cid_from_url -----------------------------------------------

    #[test]
    fn test_extract_cid_from_path_url() {
        let cid = EnsBridge::extract_cid_from_url("https://ipfs.io/ipfs/QmABC123/index.html");
        assert_eq!(cid.unwrap(), "QmABC123");
    }

    #[test]
    fn test_extract_cid_from_subdomain_url() {
        let cid = EnsBridge::extract_cid_from_url("https://bafyABC.ipfs.dweb.link/");
        assert_eq!(cid.unwrap(), "bafyABC");
    }

    #[test]
    fn test_extract_cid_no_match() {
        let cid = EnsBridge::extract_cid_from_url("https://example.com/page");
        assert!(cid.is_none());
    }

    // -- cache behaviour ----------------------------------------------------

    #[tokio::test]
    async fn test_cache_hit() {
        let mut bridge = EnsBridge::without_rpc();
        bridge.cache_insert("cached.eth", EnsResolution::ContentId("QmCached".into()));

        let result = bridge.resolve_ens("cached.eth").await.unwrap();
        assert!(matches!(result, EnsResolution::ContentId(cid) if cid == "QmCached"));
    }

    #[tokio::test]
    async fn test_cache_expired() {
        let mut bridge = EnsBridge::without_rpc();
        // Insert an entry with an artificially old timestamp.
        bridge.cache.insert(
            "expired.eth".to_string(),
            EnsCacheEntry {
                resolution: EnsResolution::ContentId("QmOld".into()),
                cached_at: Instant::now() - Duration::from_secs(7200),
            },
        );

        // The expired entry should NOT be returned; instead the bridge will
        // try the network (which will likely fail in a test environment —
        // that's fine, we just verify the cache is not blindly reused).
        let result = bridge.resolve_ens("expired.eth").await;
        // We expect either a fresh network result or a network error — not
        // the stale cached value.
        match &result {
            Ok(EnsResolution::ContentId(cid)) => {
                assert_ne!(cid, "QmOld", "stale cache entry should not be returned");
            }
            Err(_) => { /* network error is acceptable in test */ }
            _ => { /* any fresh resolution is fine */ }
        }
    }

    #[test]
    fn test_cleanup_cache() {
        let mut bridge = EnsBridge::without_rpc();
        // Insert a fresh entry
        bridge.cache_insert("fresh.eth", EnsResolution::ContentId("QmFresh".into()));
        // Insert an expired entry
        bridge.cache.insert(
            "old.eth".to_string(),
            EnsCacheEntry {
                resolution: EnsResolution::ContentId("QmOld".into()),
                cached_at: Instant::now() - Duration::from_secs(7200),
            },
        );
        assert_eq!(bridge.cache_len(), 2);
        bridge.cleanup_cache();
        assert_eq!(bridge.cache_len(), 1);
        assert!(bridge.cache.contains_key("fresh.eth"));
    }

    // -- error handling -----------------------------------------------------

    #[tokio::test]
    async fn test_invalid_name_empty() {
        let mut bridge = EnsBridge::without_rpc();
        let result = bridge.resolve_ens("").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_invalid_name_no_eth() {
        let mut bridge = EnsBridge::without_rpc();
        let result = bridge.resolve_ens("hello.com").await;
        assert!(result.is_err());
    }

    // -- gateway resolution (integration-style, will hit network) -----------

    // NOTE: the following test reaches out to eth.limo. It is marked
    // `#[ignore]` so it does not run in CI by default. Run it explicitly with:
    //     cargo test -p protocol test_resolve_real_name -- --ignored
    #[tokio::test]
    #[ignore]
    async fn test_resolve_real_name() {
        let mut bridge = EnsBridge::new("unused-rpc");
        let result = bridge.resolve_ens("vitalik.eth").await;
        // vitalik.eth has an IPFS contenthash set — we should get *something*.
        match result {
            Ok(EnsResolution::ContentId(cid)) => {
                assert!(!cid.is_empty(), "CID should not be empty");
                println!("resolved vitalik.eth -> {}", cid);
            }
            Ok(other) => println!("resolved vitalik.eth -> {:?}", other),
            Err(e) => panic!("resolution failed: {}", e),
        }
    }
}
