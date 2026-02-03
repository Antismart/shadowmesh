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
use serde::Serialize;
use std::collections::HashSet;
use std::sync::Arc;

/// Authentication configuration
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// Set of valid API keys
    valid_keys: HashSet<String>,
    /// Whether authentication is enabled
    enabled: bool,
    /// Routes that don't require authentication (exact match or prefix with *)
    public_routes: Vec<String>,
}

impl AuthConfig {
    /// Create a new AuthConfig
    pub fn new(keys: Vec<String>, enabled: bool) -> Self {
        Self {
            valid_keys: keys.into_iter().collect(),
            enabled,
            public_routes: vec![
                // Health and monitoring
                "GET:/health".to_string(),
                "GET:/metrics".to_string(),
                "GET:/metrics/prometheus".to_string(),
                // Public content retrieval
                "GET:/".to_string(),
                "GET:/dashboard".to_string(),
                "GET:/ipfs/*".to_string(),
                // GitHub OAuth flow (needed for initial auth)
                "GET:/api/github/login".to_string(),
                "GET:/api/github/callback".to_string(),
                "GET:/api/github/status".to_string(),
                // Content retrieval by CID (single segment paths that look like CIDs)
                "GET:/:cid".to_string(),
            ],
        }
    }

    /// Create disabled auth config (all routes public)
    pub fn disabled() -> Self {
        Self {
            valid_keys: HashSet::new(),
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

    /// Check if a key is valid
    pub fn is_valid_key(&self, key: &str) -> bool {
        self.valid_keys.contains(key)
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
                if !trimmed.contains('/') &&
                   (trimmed.starts_with("Qm") || trimmed.starts_with("bafy")) {
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

/// Extract Bearer token from Authorization header
fn extract_bearer_token(req: &Request<Body>) -> Option<String> {
    req.headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            if value.starts_with("Bearer ") {
                Some(value[7..].to_string())
            } else {
                None
            }
        })
}

/// API Key authentication middleware
pub async fn api_key_auth(
    auth_config: Arc<AuthConfig>,
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
            (
                StatusCode::UNAUTHORIZED,
                Json(AuthError {
                    error: "API key required. Provide via 'Authorization: Bearer <key>' header".to_string(),
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
        assert!(config.is_public_route("GET", "/dashboard"));
        assert!(config.is_public_route("GET", "/ipfs/QmTest123"));
        assert!(config.is_public_route("GET", "/QmTest123"));
        assert!(config.is_public_route("GET", "/bafyTest123"));

        // Protected routes
        assert!(!config.is_public_route("POST", "/api/deploy"));
        assert!(!config.is_public_route("POST", "/api/upload"));
        assert!(!config.is_public_route("DELETE", "/api/deployments/cid"));
    }
}
