//! API Key Authentication for ShadowMesh Gateway
//!
//! Provides middleware for protecting API endpoints with Bearer token authentication.

use axum::{
    body::Body,
    extract::Request,
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Json, Response},
};
use crate::metrics;
use serde::Serialize;
use std::sync::Arc;
use subtle::ConstantTimeEq;

use crate::audit;

/// Authentication configuration
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// Valid API keys (stored as Vec for constant-time iteration)
    valid_keys: Vec<String>,
    /// Whether authentication is enabled
    enabled: bool,
    /// Routes that don't require authentication (exact match or prefix with *)
    public_routes: Vec<String>,
}

impl AuthConfig {
    /// Create a new AuthConfig
    pub fn new(keys: Vec<String>, enabled: bool) -> Self {
        Self {
            valid_keys: keys,
            enabled,
            public_routes: vec![
                // Health and monitoring
                "GET:/health".to_string(),
                "GET:/ready".to_string(),
                "GET:/metrics".to_string(),
                "GET:/metrics/prometheus".to_string(),
                // SPA dashboard (root + all SPA routes served by fallback)
                "GET:/".to_string(),
                "GET:/login".to_string(),
                "GET:/new".to_string(),
                "GET:/domains".to_string(),
                "GET:/analytics".to_string(),
                "GET:/settings".to_string(),
                "GET:/projects/*".to_string(),
                "GET:/deployments/*".to_string(),
                "GET:/assets/*".to_string(),
                // Public content retrieval
                "GET:/ipfs/*".to_string(),
                // GitHub OAuth flow (needed for initial auth)
                "GET:/api/github/login".to_string(),
                "GET:/api/github/callback".to_string(),
                "GET:/api/github/status".to_string(),
                // Name resolution (public read access)
                "GET:/api/names/*".to_string(),
                // Content retrieval by CID (single segment paths that look like CIDs)
                "GET:/:cid".to_string(),
            ],
        }
    }

    /// Create disabled auth config (all routes public)
    pub fn disabled() -> Self {
        Self {
            valid_keys: Vec::new(),
            enabled: false,
            public_routes: Vec::new(),
        }
    }

    /// Load from environment variable
    pub fn from_env() -> Self {
        let keys_str = std::env::var("SHADOWMESH_API_KEYS").unwrap_or_default();
        let keys: Vec<String> = keys_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let enabled = !keys.is_empty();

        if enabled {
            tracing::info!("API authentication enabled with {} key(s)", keys.len());
        } else {
            tracing::warn!("API authentication DISABLED - all endpoints are public");
        }

        Self::new(keys, enabled)
    }

    /// Check if a key is valid (constant-time to prevent timing side-channel attacks).
    ///
    /// Iterates over ALL valid keys and uses `subtle::ConstantTimeEq` for each
    /// comparison, then bitwise-ORs the results.  This ensures that the execution
    /// time does not reveal *which* key matched (or how close a guess was).
    ///
    /// Note: length comparison is done first per-key which may leak key lengths,
    /// but this is an acceptable trade-off.
    pub fn is_valid_key(&self, key: &str) -> bool {
        let input = key.as_bytes();
        let mut result = 0u8;

        for valid_key in &self.valid_keys {
            let valid = valid_key.as_bytes();
            if input.len() == valid.len() {
                // Constant-time byte comparison — does NOT short-circuit.
                result |= input.ct_eq(valid).unwrap_u8();
            }
        }

        result == 1
    }

    /// Check if a route is public (doesn't require auth)
    pub fn is_public_route(&self, method: &str, path: &str) -> bool {
        if !self.enabled {
            return true;
        }

        let route_key = format!("{}:{}", method, path);

        for public_route in &self.public_routes {
            // Exact match
            if &route_key == public_route {
                return true;
            }

            // Wildcard match (e.g., "GET:/ipfs/*" matches "GET:/ipfs/Qm...")
            if public_route.ends_with('*') {
                let prefix = &public_route[..public_route.len() - 1];
                if route_key.starts_with(prefix) {
                    return true;
                }
            }

            // Parameter match (e.g., "GET:/:cid" matches single-segment GET paths)
            if public_route == "GET:/:cid" && method == "GET" {
                // Match paths that are just a CID (single segment, starts with Qm or bafy)
                let trimmed = path.trim_start_matches('/');
                if !trimmed.contains('/')
                    && (trimmed.starts_with("Qm") || trimmed.starts_with("bafy"))
                {
                    return true;
                }
            }
        }

        false
    }

    /// Check if authentication is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

/// Error response for authentication failures
#[derive(Serialize)]
struct AuthError {
    error: String,
    code: String,
}

/// Extract Bearer token from Authorization header (case-insensitive per RFC 7235).
pub fn extract_bearer_token(req: &Request<Body>) -> Option<String> {
    req.headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            // RFC 7235: auth-scheme is case-insensitive
            if value.len() > 7 && value[..7].eq_ignore_ascii_case("bearer ") {
                Some(value[7..].trim_start().to_string())
            } else {
                None
            }
        })
}

/// API Key authentication middleware
pub async fn api_key_auth(
    auth_config: Arc<AuthConfig>,
    audit_logger: Arc<audit::AuditLogger>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let method = req.method().as_str();
    let path = req.uri().path();

    // Check if route is public
    if auth_config.is_public_route(method, path) {
        return next.run(req).await;
    }

    // Extract and validate API key
    match extract_bearer_token(&req) {
        Some(key) if auth_config.is_valid_key(&key) => {
            // Valid key, proceed
            next.run(req).await
        }
        Some(_) => {
            // Invalid key
            tracing::warn!(
                method = %method,
                path = %path,
                "Invalid API key provided"
            );
            metrics::record_auth_failure("invalid_key");
            audit::log_auth_failure(
                &audit_logger,
                &format!("Invalid API key for {} {}", method, path),
                None,
                None,
            )
            .await;
            (
                StatusCode::UNAUTHORIZED,
                Json(AuthError {
                    error: "Invalid API key".to_string(),
                    code: "INVALID_API_KEY".to_string(),
                }),
            )
                .into_response()
        }
        None => {
            // No key provided
            tracing::warn!(
                method = %method,
                path = %path,
                "Missing API key"
            );
            metrics::record_auth_failure("missing_key");
            audit::log_auth_failure(
                &audit_logger,
                &format!("Missing API key for {} {}", method, path),
                None,
                None,
            )
            .await;
            (
                StatusCode::UNAUTHORIZED,
                Json(AuthError {
                    error: "API key required. Provide via 'Authorization: Bearer <key>' header"
                        .to_string(),
                    code: "MISSING_API_KEY".to_string(),
                }),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_config_creation() {
        let config = AuthConfig::new(vec!["key1".to_string(), "key2".to_string()], true);
        assert!(config.is_enabled());
        assert!(config.is_valid_key("key1"));
        assert!(config.is_valid_key("key2"));
        assert!(!config.is_valid_key("key3"));
    }

    #[test]
    fn test_disabled_auth() {
        let config = AuthConfig::disabled();
        assert!(!config.is_enabled());
        assert!(config.is_public_route("POST", "/api/deploy"));
    }

    #[test]
    fn test_public_routes() {
        let config = AuthConfig::new(vec!["key1".to_string()], true);

        // Public routes
        assert!(config.is_public_route("GET", "/health"));
        assert!(config.is_public_route("GET", "/metrics"));
        assert!(config.is_public_route("GET", "/"));
        assert!(config.is_public_route("GET", "/login"));
        assert!(config.is_public_route("GET", "/analytics"));
        assert!(config.is_public_route("GET", "/settings"));
        assert!(config.is_public_route("GET", "/assets/index-abc.js"));
        assert!(config.is_public_route("GET", "/ipfs/QmTest123"));
        assert!(config.is_public_route("GET", "/QmTest123"));
        assert!(config.is_public_route("GET", "/bafyTest123"));

        // Protected routes
        assert!(!config.is_public_route("POST", "/api/deploy"));
        assert!(!config.is_public_route("POST", "/api/upload"));
        assert!(!config.is_public_route("DELETE", "/api/deployments/cid"));
    }
}
