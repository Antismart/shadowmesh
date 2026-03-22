//! WASM-Compatible Name Resolver for ShadowMesh Browser SDK
//!
//! Resolves `.shadow` names by querying gateway naming APIs via `fetch()`,
//! and `.eth` names by querying the eth.limo HTTPS gateway and extracting
//! the CID from response headers (mirrors `protocol/src/ens_bridge.rs`).
//!
//! Designed for browser environments where direct DHT access is not available.

use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::HashMap;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestInit, RequestMode, Response};

/// Default resolution timeout in milliseconds
const RESOLVE_TIMEOUT_MS: u32 = 10_000;

/// Maximum cache entries
const MAX_CACHE_ENTRIES: usize = 500;

/// Cache TTL in milliseconds (5 minutes)
const CACHE_TTL_MS: f64 = 5.0 * 60.0 * 1000.0;

/// Base domain for the eth.limo ENS gateway.
const ETH_LIMO_GATEWAY: &str = "eth.limo";

/// Result of an ENS resolution via eth.limo.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EnsResolution {
    /// Maps to a `.shadow` name (resolve further via gateway).
    ShadowName { name: String },
    /// Maps directly to a content CID.
    ContentId { cid: String },
    /// ENS name not found or no contenthash set.
    NotFound,
}

/// Well-known service names
#[wasm_bindgen]
pub struct WellKnownNames;

#[wasm_bindgen]
impl WellKnownNames {
    #[wasm_bindgen(getter)]
    pub fn gateway() -> String {
        "_gateway.shadow".into()
    }

    #[wasm_bindgen(getter)]
    pub fn signaling() -> String {
        "_signaling.shadow".into()
    }

    #[wasm_bindgen(getter)]
    pub fn bootstrap() -> String {
        "_bootstrap.shadow".into()
    }

    #[wasm_bindgen(getter)]
    pub fn stun() -> String {
        "_stun.shadow".into()
    }
}

/// A cached resolution entry
#[derive(Clone)]
struct CachedEntry {
    data: String, // JSON string of the resolution result
    expires_at: f64,
}

/// WASM-compatible name resolver.
///
/// Resolves `.shadow` names via HTTP requests to known gateway endpoints,
/// and `.eth` names via the eth.limo gateway (extracting CIDs from headers).
#[wasm_bindgen]
pub struct WasmNameResolver {
    gateway_urls: RefCell<Vec<String>>,
    cache: RefCell<HashMap<String, CachedEntry>>,
}

#[wasm_bindgen]
impl WasmNameResolver {
    /// Create a new resolver with initial gateway URLs.
    #[wasm_bindgen(constructor)]
    pub fn new(gateway_urls: Vec<String>) -> Self {
        Self {
            gateway_urls: RefCell::new(gateway_urls),
            cache: RefCell::new(HashMap::new()),
        }
    }

    /// Resolve a `.shadow` or `.eth` name.
    ///
    /// For `.shadow` names: queries the gateway's `/api/names/{name}` endpoint.
    /// For `.eth` names: queries `https://{name}.eth.limo` and extracts CID from headers.
    ///
    /// Returns a JSON string with the resolution result, or null if not found.
    pub async fn resolve(&self, name: &str) -> Result<JsValue, JsValue> {
        // Check cache first
        if let Some(cached) = self.get_cached(name) {
            return Ok(JsValue::from_str(&cached));
        }

        // Dispatch based on name suffix
        let result = if name.ends_with(".eth") {
            self.resolve_eth(name).await
        } else if name.ends_with(".shadow") {
            self.resolve_shadow(name).await
        } else {
            // Unknown suffix — try as a .shadow name against gateways
            self.resolve_shadow(name).await
        };

        match result {
            Ok(Some(json)) => {
                self.set_cached(name, &json);
                Ok(JsValue::from_str(&json))
            }
            Ok(None) => Ok(JsValue::NULL),
            Err(e) => {
                tracing::warn!("Name resolution failed for {}: {:?}", name, e);
                Ok(JsValue::NULL)
            }
        }
    }

    /// Resolve the best gateway URL from the naming layer.
    ///
    /// Tries to resolve `_gateway.shadow` to discover a gateway URL.
    /// Returns the resolved URL, or the provided fallback if resolution fails.
    pub async fn resolve_gateway(&self, fallback: &str) -> Result<String, JsValue> {
        match self.resolve("_gateway.shadow").await {
            Ok(val) if !val.is_null() => {
                // Try to parse the result JSON to extract a URL
                if let Some(s) = val.as_string() {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&s) {
                        // Look for a "url" or "gateway_url" field in the response
                        if let Some(url) = parsed
                            .get("url")
                            .or_else(|| parsed.get("gateway_url"))
                            .and_then(|v| v.as_str())
                        {
                            return Ok(url.to_string());
                        }
                        // Look for multiaddrs that contain HTTP addresses
                        if let Some(addrs) = parsed.get("multiaddrs").and_then(|v| v.as_array()) {
                            for addr in addrs {
                                if let Some(a) = addr.as_str() {
                                    if a.starts_with("http://") || a.starts_with("https://") {
                                        return Ok(a.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
                // Could not extract a URL from the response — use fallback
                Ok(fallback.to_string())
            }
            _ => Ok(fallback.to_string()),
        }
    }

    /// Resolve signaling server URL from the naming layer.
    pub async fn resolve_signaling(&self, fallback: &str) -> Result<String, JsValue> {
        match self.resolve("_signaling.shadow").await {
            Ok(val) if !val.is_null() => {
                if let Some(s) = val.as_string() {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&s) {
                        if let Some(url) = parsed
                            .get("url")
                            .or_else(|| parsed.get("signaling_url"))
                            .and_then(|v| v.as_str())
                        {
                            return Ok(url.to_string());
                        }
                        if let Some(addrs) = parsed.get("multiaddrs").and_then(|v| v.as_array()) {
                            for addr in addrs {
                                if let Some(a) = addr.as_str() {
                                    if a.starts_with("ws://") || a.starts_with("wss://") {
                                        return Ok(a.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(fallback.to_string())
            }
            _ => Ok(fallback.to_string()),
        }
    }

    /// Discover services by type via a gateway's service discovery API.
    ///
    /// Returns a JSON string of service entries, or empty array.
    pub async fn discover_services(&self, service_type: &str) -> Result<JsValue, JsValue> {
        let urls = self.gateway_urls.borrow().clone();
        for url in &urls {
            match self.fetch_services(url, service_type).await {
                Ok(Some(json)) => return Ok(JsValue::from_str(&json)),
                _ => continue,
            }
        }

        Ok(JsValue::from_str("[]"))
    }

    /// Add a gateway URL for resolution queries.
    pub fn add_gateway_url(&self, url: String) {
        let mut urls = self.gateway_urls.borrow_mut();
        if !urls.contains(&url) {
            urls.push(url);
        }
    }

    /// Clear the resolution cache.
    pub fn clear_cache(&self) {
        self.cache.borrow_mut().clear();
    }

    /// Get the number of cached entries.
    pub fn cache_size(&self) -> usize {
        self.cache.borrow().len()
    }
}

// Private implementation
impl WasmNameResolver {
    fn get_cached(&self, name: &str) -> Option<String> {
        let cache = self.cache.borrow();
        if let Some(entry) = cache.get(name) {
            let now = js_sys::Date::now();
            if now < entry.expires_at {
                return Some(entry.data.clone());
            }
        }
        None
    }

    fn set_cached(&self, name: &str, data: &str) {
        let mut cache = self.cache.borrow_mut();

        // Evict if over capacity
        if cache.len() >= MAX_CACHE_ENTRIES {
            // Remove the first key (simple eviction)
            if let Some(key) = cache.keys().next().cloned() {
                cache.remove(&key);
            }
        }

        cache.insert(
            name.to_string(),
            CachedEntry {
                data: data.to_string(),
                expires_at: js_sys::Date::now() + CACHE_TTL_MS,
            },
        );
    }

    /// Resolve a `.shadow` name by querying each known gateway's `/api/names/{name}` endpoint.
    async fn resolve_shadow(&self, name: &str) -> Result<Option<String>, JsValue> {
        let urls = self.gateway_urls.borrow().clone();
        for url in &urls {
            match self.fetch_name(url, name).await {
                Ok(Some(json)) => return Ok(Some(json)),
                Ok(None) => continue,
                Err(_) => continue,
            }
        }
        Ok(None)
    }

    /// Resolve a `.eth` name by querying `https://{name}.eth.limo`.
    ///
    /// Issues an HTTP HEAD request (via fetch with method "HEAD") and inspects
    /// response headers for content hash information:
    ///
    /// 1. `x-ipfs-path` — e.g. `/ipfs/Qm...` or `/ipfs/bafy...`
    /// 2. `content-hash` — raw contenthash string (`ipfs://...`, `shadow://...`, etc.)
    /// 3. `location` header on redirects — may contain an IPFS gateway URL with the CID
    ///
    /// Falls back to gateway URL resolution if eth.limo fails.
    async fn resolve_eth(&self, name: &str) -> Result<Option<String>, JsValue> {
        let normalized = name.to_lowercase();
        if !normalized.ends_with(".eth") {
            return Ok(None);
        }

        // Strip the trailing ".eth" to get the label for eth.limo
        // eth.limo expects: https://{full-name-with-.eth}.eth.limo
        // Actually the URL format is: https://{name}.eth.limo where name already
        // ends with .eth — but eth.limo uses the subdomain format:
        //   https://{label}.eth.limo  where label = name without ".eth"
        // Wait — looking at the protocol code: `format!("https://{}.{}", normalized_name, ETH_LIMO_GATEWAY)`
        // where normalized_name is "vitalik.eth" => "https://vitalik.eth.eth.limo"
        // That's actually the correct format for eth.limo (the full ENS name as subdomain).
        let url = format!("https://{}.{}", normalized, ETH_LIMO_GATEWAY);

        tracing::debug!("Querying eth.limo for: {}", url);

        // Use fetch with no-cors mode isn't useful here; try a normal fetch.
        // Note: In browsers, cross-origin HEAD requests may be blocked by CORS.
        // We attempt a HEAD, then fall back to GET with opaque response.
        match self.fetch_eth_limo_head(&url).await {
            Ok(Some(resolution_json)) => Ok(Some(resolution_json)),
            Ok(None) => {
                // HEAD returned no useful headers — try GET
                match self.fetch_eth_limo_get(&url).await {
                    Ok(result) => Ok(result),
                    Err(e) => {
                        tracing::warn!("eth.limo GET failed for {}: {:?}", normalized, e);
                        // Fall back: try resolving via gateway's ENS endpoint
                        self.resolve_eth_via_gateway(&normalized).await
                    }
                }
            }
            Err(e) => {
                tracing::warn!("eth.limo HEAD failed for {}: {:?}", normalized, e);
                // Fall back: try resolving via gateway's ENS endpoint
                self.resolve_eth_via_gateway(&normalized).await
            }
        }
    }

    /// Issue an HTTP HEAD to eth.limo and extract CID from headers.
    async fn fetch_eth_limo_head(&self, url: &str) -> Result<Option<String>, JsValue> {
        let opts = RequestInit::new();
        opts.set_method("HEAD");
        // Use cors mode so we can read response headers
        opts.set_mode(RequestMode::Cors);

        let request = Request::new_with_str_and_init(url, &opts)?;

        let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
        let resp_value = JsFuture::from(window.fetch_with_request(&request)).await?;
        let resp: Response = resp_value.dyn_into()?;

        self.extract_cid_from_response(&resp)
    }

    /// Issue an HTTP GET to eth.limo and extract CID from headers.
    async fn fetch_eth_limo_get(&self, url: &str) -> Result<Option<String>, JsValue> {
        let opts = RequestInit::new();
        opts.set_method("GET");
        opts.set_mode(RequestMode::Cors);

        let request = Request::new_with_str_and_init(url, &opts)?;

        let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
        let resp_value = JsFuture::from(window.fetch_with_request(&request)).await?;
        let resp: Response = resp_value.dyn_into()?;

        self.extract_cid_from_response(&resp)
    }

    /// Extract a CID from eth.limo response headers.
    ///
    /// Checks (in order):
    /// 1. `x-ipfs-path` header
    /// 2. `content-hash` header
    /// 3. `location` header (for redirects)
    fn extract_cid_from_response(&self, resp: &Response) -> Result<Option<String>, JsValue> {
        let headers = resp.headers();
        let status = resp.status();

        // 404/410 = not found
        if status == 404 || status == 410 {
            let resolution = EnsResolution::NotFound;
            let json = serde_json::to_string(&resolution)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            return Ok(Some(json));
        }

        // 1. x-ipfs-path header
        if let Ok(Some(ipfs_path)) = headers.get("x-ipfs-path") {
            if let Some(resolution) = Self::parse_contenthash(&ipfs_path) {
                let json = serde_json::to_string(&resolution)
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;
                tracing::debug!("Resolved via x-ipfs-path: {}", ipfs_path);
                return Ok(Some(json));
            }
        }

        // 2. content-hash header
        if let Ok(Some(ch)) = headers.get("content-hash") {
            if let Some(resolution) = Self::parse_contenthash(&ch) {
                let json = serde_json::to_string(&resolution)
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;
                tracing::debug!("Resolved via content-hash: {}", ch);
                return Ok(Some(json));
            }
        }

        // 3. Location header (redirects: 301, 302, 307, 308)
        if status == 301 || status == 302 || status == 307 || status == 308 {
            if let Ok(Some(location)) = headers.get("location") {
                if let Some(cid) = Self::extract_cid_from_url(&location) {
                    let resolution = EnsResolution::ContentId { cid };
                    let json = serde_json::to_string(&resolution)
                        .map_err(|e| JsValue::from_str(&e.to_string()))?;
                    tracing::debug!("Resolved via redirect location: {}", location);
                    return Ok(Some(json));
                }
            }
        }

        Ok(None)
    }

    /// Parse a contenthash string into an `EnsResolution`.
    ///
    /// Supported formats:
    /// - `shadow://myapp.shadow` -> ShadowName
    /// - `shadow://QmABC...`    -> ContentId
    /// - `ipfs://QmABC...`      -> ContentId
    /// - `/ipfs/QmABC...`       -> ContentId
    pub(crate) fn parse_contenthash(contenthash: &str) -> Option<EnsResolution> {
        let trimmed = contenthash.trim();

        if let Some(shadow_ref) = trimmed.strip_prefix("shadow://") {
            if shadow_ref.ends_with(".shadow") {
                return Some(EnsResolution::ShadowName {
                    name: shadow_ref.to_string(),
                });
            } else {
                return Some(EnsResolution::ContentId {
                    cid: shadow_ref.to_string(),
                });
            }
        }

        if let Some(ipfs_ref) = trimmed.strip_prefix("ipfs://") {
            if !ipfs_ref.is_empty() {
                return Some(EnsResolution::ContentId {
                    cid: ipfs_ref.to_string(),
                });
            }
        }

        if let Some(ipfs_ref) = trimmed.strip_prefix("/ipfs/") {
            // Handle the /ipfs/<cid>[/...] path format
            let cid = ipfs_ref.split('/').next().unwrap_or("");
            if !cid.is_empty() {
                return Some(EnsResolution::ContentId {
                    cid: cid.to_string(),
                });
            }
        }

        None
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

    /// Fallback: resolve an `.eth` name via a gateway's ENS resolution endpoint.
    ///
    /// Some ShadowMesh gateways expose `/api/ens/{name}` which performs server-side
    /// ENS resolution and returns the CID.
    async fn resolve_eth_via_gateway(&self, name: &str) -> Result<Option<String>, JsValue> {
        let urls = self.gateway_urls.borrow().clone();
        for gateway_url in &urls {
            let url = format!(
                "{}/api/ens/{}",
                gateway_url.trim_end_matches('/'),
                name
            );

            let opts = RequestInit::new();
            opts.set_method("GET");

            let request = match Request::new_with_str_and_init(&url, &opts) {
                Ok(r) => r,
                Err(_) => continue,
            };

            let window = match web_sys::window() {
                Some(w) => w,
                None => return Err(JsValue::from_str("no window")),
            };

            let resp_value = match JsFuture::from(window.fetch_with_request(&request)).await {
                Ok(r) => r,
                Err(_) => continue,
            };

            let resp: Response = match resp_value.dyn_into() {
                Ok(r) => r,
                Err(_) => continue,
            };

            if !resp.ok() {
                continue;
            }

            let json = match JsFuture::from(resp.text()?).await {
                Ok(j) => j,
                Err(_) => continue,
            };

            if let Some(text) = json.as_string() {
                // The gateway returns a JSON with a CID or resolution info
                return Ok(Some(text));
            }
        }
        Ok(None)
    }

    /// Fetch a `.shadow` name from a specific gateway.
    async fn fetch_name(&self, gateway_url: &str, name: &str) -> Result<Option<String>, JsValue> {
        let url = format!("{}/api/names/{}", gateway_url.trim_end_matches('/'), name);

        let opts = RequestInit::new();
        opts.set_method("GET");

        let request = Request::new_with_str_and_init(&url, &opts)?;

        let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
        let resp_value = JsFuture::from(window.fetch_with_request(&request)).await?;
        let resp: Response = resp_value.dyn_into()?;

        if !resp.ok() {
            return Ok(None);
        }

        let json = JsFuture::from(resp.text()?).await?;
        let text = json
            .as_string()
            .ok_or_else(|| JsValue::from_str("invalid response"))?;

        // Check if the response indicates "found"
        if text.contains("\"found\":true") || text.contains("\"cid\"") {
            Ok(Some(text))
        } else {
            Ok(None)
        }
    }

    async fn fetch_services(
        &self,
        gateway_url: &str,
        service_type: &str,
    ) -> Result<Option<String>, JsValue> {
        let url = format!(
            "{}/api/services/{}",
            gateway_url.trim_end_matches('/'),
            service_type
        );

        let opts = RequestInit::new();
        opts.set_method("GET");

        let request = Request::new_with_str_and_init(&url, &opts)?;

        let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
        let resp_value = JsFuture::from(window.fetch_with_request(&request)).await?;
        let resp: Response = resp_value.dyn_into()?;

        if !resp.ok() {
            return Ok(None);
        }

        let json = JsFuture::from(resp.text()?).await?;
        Ok(json.as_string())
    }
}
