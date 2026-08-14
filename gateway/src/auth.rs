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
use std::collections::HashSet;
use std::sync::Arc;
use subtle::ConstantTimeEq;

use crate::audit;

/// Identity attached to a request once its API key has been validated.
///
/// Every API key maps to exactly one identity. An identity is either an
/// `admin` superuser (may access every `:cid` namespace) or an ordinary
/// tenant scoped to an explicit set of namespaces. The default posture for
/// scoped keys is deny-cross-tenant: a key may only touch namespaces in
/// `namespaces` unless it is `admin`.
#[derive(Debug, Clone)]
pub struct ApiIdentity {
    /// Short, non-sensitive identifier for logging/audit.
    pub id: String,
    /// Superuser — authorized for all namespaces.
    pub admin: bool,
    /// Explicit set of `:cid` namespaces this identity may access.
    pub namespaces: HashSet<String>,
}

impl ApiIdentity {
    /// Whether this identity is authorized to access the given namespace/cid.
    pub fn is_authorized_for(&self, namespace: &str) -> bool {
        self.admin || self.namespaces.contains(namespace)
    }
}

/// Derive a short, non-sensitive id for an API key (first 8 hex chars of its
/// SHA-256 hash). Used only for logging/audit — never reveals the key.
fn short_id(key: &str) -> String {
    let hash = crate::api_keys::hash_key(key);
    hash.chars().take(8).collect()
}

/// Authentication configuration
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// Valid API keys paired with their identity (stored as Vec for
    /// constant-time iteration).
    keys: Vec<(String, ApiIdentity)>,
    /// Whether authentication is enabled
    enabled: bool,
    /// Routes that don't require authentication (exact match or prefix with *)
    public_routes: Vec<String>,
}

impl AuthConfig {
    /// Create a new AuthConfig from a flat list of keys.
    ///
    /// Each key is granted an **admin** (all-namespaces) identity. This keeps
    /// the single-admin / dev case working out of the box. To scope keys to
    /// specific namespaces, use [`AuthConfig::from_env`] with the
    /// `key:ns1|ns2` grammar.
    pub fn new(keys: Vec<String>, enabled: bool) -> Self {
        let keys = keys
            .into_iter()
            .map(|k| {
                let id = short_id(&k);
                (
                    k,
                    ApiIdentity {
                        id,
                        admin: true,
                        namespaces: HashSet::new(),
                    },
                )
            })
            .collect();
        Self::with_keys(keys, enabled)
    }

    /// Build an AuthConfig from pre-resolved (key, identity) pairs.
    fn with_keys(keys: Vec<(String, ApiIdentity)>, enabled: bool) -> Self {
        Self {
            keys,
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
            keys: Vec::new(),
            enabled: false,
            public_routes: Vec::new(),
        }
    }

    /// Load from environment variable.
    ///
    /// `SHADOWMESH_API_KEYS` is a comma-separated list of entries. Each entry
    /// binds a key to an identity using the grammar:
    ///
    /// * `key`            — admin superuser (all namespaces). Preserves the
    ///                      single-admin / dev case for backward compatibility.
    /// * `key:*`          — admin superuser (explicit).
    /// * `key:ns1|ns2`    — ordinary tenant key scoped to the listed `:cid`
    ///                      namespaces. Deny-cross-tenant: it may access ONLY
    ///                      those namespaces.
    ///
    /// `SHADOWMESH_ADMIN_API_KEYS` (optional) is a comma-separated list of
    /// bare admin keys, always granted all-namespaces access.
    pub fn from_env() -> Self {
        let mut keys: Vec<(String, ApiIdentity)> = Vec::new();
        let mut scoped_count = 0usize;

        let keys_str = std::env::var("SHADOWMESH_API_KEYS").unwrap_or_default();
        for entry in keys_str.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
            let (raw_key, identity) = Self::parse_key_entry(entry);
            if !identity.admin {
                scoped_count += 1;
            }
            keys.push((raw_key, identity));
        }

        let admin_str = std::env::var("SHADOWMESH_ADMIN_API_KEYS").unwrap_or_default();
        for key in admin_str.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
            let id = short_id(key);
            keys.push((
                key.to_string(),
                ApiIdentity {
                    id,
                    admin: true,
                    namespaces: HashSet::new(),
                },
            ));
        }

        let enabled = !keys.is_empty();

        if enabled {
            tracing::info!(
                "API authentication enabled with {} key(s) ({} scoped, {} admin)",
                keys.len(),
                scoped_count,
                keys.len() - scoped_count
            );
        } else {
            tracing::warn!("API authentication DISABLED - all endpoints are public");
        }

        Self::with_keys(keys, enabled)
    }

    /// Parse a single `SHADOWMESH_API_KEYS` entry into (key, identity).
    fn parse_key_entry(entry: &str) -> (String, ApiIdentity) {
        match entry.split_once(':') {
            // Bare key → admin (backward compatible with flat key lists).
            None => {
                let id = short_id(entry);
                (
                    entry.to_string(),
                    ApiIdentity {
                        id,
                        admin: true,
                        namespaces: HashSet::new(),
                    },
                )
            }
            Some((key, spec)) => {
                let key = key.trim();
                let spec = spec.trim();
                let id = short_id(key);
                if spec == "*" {
                    // Explicit admin.
                    (
                        key.to_string(),
                        ApiIdentity {
                            id,
                            admin: true,
                            namespaces: HashSet::new(),
                        },
                    )
                } else {
                    // Scoped tenant key: deny-cross-tenant.
                    let namespaces: HashSet<String> = spec
                        .split('|')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    (
                        key.to_string(),
                        ApiIdentity {
                            id,
                            admin: false,
                            namespaces,
                        },
                    )
                }
            }
        }
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

        for (valid_key, _) in &self.keys {
            let valid = valid_key.as_bytes();
            if input.len() == valid.len() {
                // Constant-time byte comparison — does NOT short-circuit.
                result |= input.ct_eq(valid).unwrap_u8();
            }
        }

        result == 1
    }

    /// Validate a key and return its identity if valid.
    ///
    /// The validity scan is constant-time across all configured keys (same as
    /// [`AuthConfig::is_valid_key`]); the matched identity is captured during
    /// the scan and only returned once the key is confirmed valid.
    pub fn authenticate(&self, key: &str) -> Option<ApiIdentity> {
        let input = key.as_bytes();
        let mut found = 0u8;
        let mut matched: Option<&ApiIdentity> = None;

        for (valid_key, identity) in &self.keys {
            let valid = valid_key.as_bytes();
            if input.len() == valid.len() {
                let eq = input.ct_eq(valid).unwrap_u8();
                if eq == 1 {
                    matched = Some(identity);
                }
                found |= eq;
            }
        }

        if found == 1 {
            matched.cloned()
        } else {
            None
        }
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
                // Match paths that are just a valid CID (single segment)
                let trimmed = path.trim_start_matches('/');
                if !trimmed.contains('/')
                    && crate::cid_validation::validate_cid(trimmed)
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
    mut req: Request<Body>,
    next: Next,
) -> Response {
    let method = req.method().as_str().to_string();
    let path = req.uri().path().to_string();

    // Check if route is public
    if auth_config.is_public_route(&method, &path) {
        return next.run(req).await;
    }

    // Extract and validate API key
    match extract_bearer_token(&req) {
        Some(key) => {
            if let Some(identity) = auth_config.authenticate(&key) {
                // Valid key — attach caller identity for downstream handlers to
                // enforce per-namespace (per-tenant) authorization.
                req.extensions_mut().insert(identity);
                return next.run(req).await;
            }
            // Invalid key.
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
    fn test_flat_keys_are_admin() {
        // Bare keys (AuthConfig::new) are admin: authorized for any namespace.
        let config = AuthConfig::new(vec!["key1".to_string()], true);
        let id = config.authenticate("key1").expect("valid");
        assert!(id.admin);
        assert!(id.is_authorized_for("QmAnyNamespace"));
        assert!(config.authenticate("wrong").is_none());
    }

    #[test]
    fn test_parse_admin_entry() {
        let (_key, id) = AuthConfig::parse_key_entry("secret:*");
        assert!(id.admin);
        assert!(id.is_authorized_for("anything"));
    }

    #[test]
    fn test_parse_scoped_entry_denies_cross_tenant() {
        let (_key, id) = AuthConfig::parse_key_entry("secret:cidA|cidB");
        assert!(!id.admin);
        assert!(id.is_authorized_for("cidA"));
        assert!(id.is_authorized_for("cidB"));
        // Cross-tenant access is denied by default.
        assert!(!id.is_authorized_for("cidC"));
    }

    #[test]
    fn test_authenticate_returns_scoped_identity() {
        let mut config = AuthConfig::new(vec![], true);
        // Inject one scoped key via the env parser to verify authenticate wiring.
        let (key, identity) = AuthConfig::parse_key_entry("tenantkey:cidA");
        config.keys.push((key, identity));
        let id = config.authenticate("tenantkey").expect("valid");
        assert!(!id.admin);
        assert!(id.is_authorized_for("cidA"));
        assert!(!id.is_authorized_for("cidZ"));
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
        assert!(config.is_public_route("GET", "/ipfs/QmYwAPJzv5CZsnN625s3Xf2nemtYgPpHdWEz79ojWnPbdG"));
        assert!(config.is_public_route("GET", "/QmYwAPJzv5CZsnN625s3Xf2nemtYgPpHdWEz79ojWnPbdG"));
        assert!(config.is_public_route("GET", "/bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi"));
        // Short/invalid CIDs should NOT match the /:cid public route
        assert!(!config.is_public_route("GET", "/QmTest123"));
        assert!(!config.is_public_route("GET", "/bafyTest123"));

        // Protected routes
        assert!(!config.is_public_route("POST", "/api/deploy"));
        assert!(!config.is_public_route("POST", "/api/upload"));
        assert!(!config.is_public_route("DELETE", "/api/deployments/cid"));
    }
}
