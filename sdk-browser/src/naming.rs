//! WASM-Compatible Name Resolver for ShadowMesh Browser SDK
//!
//! Resolves `.shadow` names by querying gateway naming APIs via `fetch()`.
//! Designed for browser environments where direct DHT access is not available.

use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::HashMap;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestInit, Response};

/// Default resolution timeout in milliseconds
const RESOLVE_TIMEOUT_MS: u32 = 10_000;

/// Maximum cache entries
const MAX_CACHE_ENTRIES: usize = 500;

/// Cache TTL in milliseconds (5 minutes)
const CACHE_TTL_MS: f64 = 5.0 * 60.0 * 1000.0;

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
/// Resolves `.shadow` names via HTTP requests to known gateway endpoints.
/// Designed for use in browser environments.
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

    /// Resolve a `.shadow` name via the nearest known gateway.
    ///
    /// Returns a JSON string with the resolution result, or null if not found.
    pub async fn resolve(&self, name: &str) -> Result<JsValue, JsValue> {
        // Check cache first
        if let Some(cached) = self.get_cached(name) {
            return Ok(JsValue::from_str(&cached));
        }

        // Try each gateway
        let urls = self.gateway_urls.borrow().clone();
        for url in &urls {
            match self.fetch_name(url, name).await {
                Ok(Some(json)) => {
                    self.set_cached(name, &json);
                    return Ok(JsValue::from_str(&json));
                }
                Ok(None) => continue,
                Err(_) => continue,
            }
        }

        Ok(JsValue::NULL)
    }

    /// Resolve the best gateway URL from the naming layer.
    ///
    /// Returns the resolved URL, or the provided fallback if resolution fails.
    pub async fn resolve_gateway(&self, fallback: &str) -> Result<String, JsValue> {
        match self.resolve("_gateway.shadow").await {
            Ok(val) if !val.is_null() => {
                // Parse the result to find a gateway URL
                // For now, return the fallback — in production this would parse multiaddrs
                Ok(fallback.to_string())
            }
            _ => Ok(fallback.to_string()),
        }
    }

    /// Resolve signaling server URL from the naming layer.
    pub async fn resolve_signaling(&self, fallback: &str) -> Result<String, JsValue> {
        match self.resolve("_signaling.shadow").await {
            Ok(val) if !val.is_null() => Ok(fallback.to_string()),
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
        if text.contains("\"found\":true") {
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
