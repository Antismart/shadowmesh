//! API Key Management for ShadowMesh Gateway
//!
//! Provides endpoints for creating, listing, rotating, and revoking API keys.
//! Keys are stored in Redis when available, with in-memory fallback.

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{delete, post},
    Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::lock_utils::{read_lock, write_lock};
use crate::redis_client::RedisClient;

/// Stored API key metadata (key itself is never stored, only hash)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    /// Unique identifier for this key
    pub id: String,
    /// SHA-256 hash of the actual key
    pub key_hash: String,
    /// Human-readable name
    pub name: String,
    /// When the key was created
    pub created_at: DateTime<Utc>,
    /// When the key expires (None = never)
    pub expires_at: Option<DateTime<Utc>>,
    /// Last time this key was used
    pub last_used: Option<DateTime<Utc>>,
    /// Whether this key is currently enabled
    pub enabled: bool,
    /// Scopes/permissions for this key
    pub scopes: Vec<String>,
}

impl ApiKey {
    /// Create a new API key
    pub fn new(name: String, expires_in_days: Option<u32>, scopes: Vec<String>) -> (Self, String) {
        let raw_key = generate_secure_key();
        let key_hash = hash_key(&raw_key);
        let id = uuid::Uuid::new_v4().to_string();

        let expires_at =
            expires_in_days.map(|days| Utc::now() + chrono::Duration::days(days as i64));

        let api_key = Self {
            id,
            key_hash,
            name,
            created_at: Utc::now(),
            expires_at,
            last_used: None,
            enabled: true,
            scopes,
        };

        (api_key, raw_key)
    }

    /// Check if key is expired
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            Utc::now() > expires_at
        } else {
            false
        }
    }

    /// Check if key is valid (enabled and not expired)
    pub fn is_valid(&self) -> bool {
        self.enabled && !self.is_expired()
    }

    /// Rotate this key (generate new key, keep same metadata)
    pub fn rotate(&mut self) -> String {
        let raw_key = generate_secure_key();
        self.key_hash = hash_key(&raw_key);
        raw_key
    }
}

/// Response when listing API keys (hides sensitive data)
#[derive(Serialize)]
pub struct ApiKeyInfo {
    pub id: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub last_used: Option<DateTime<Utc>>,
    pub enabled: bool,
    pub scopes: Vec<String>,
    pub is_expired: bool,
}

impl From<&ApiKey> for ApiKeyInfo {
    fn from(key: &ApiKey) -> Self {
        Self {
            id: key.id.clone(),
            name: key.name.clone(),
            created_at: key.created_at,
            expires_at: key.expires_at,
            last_used: key.last_used,
            enabled: key.enabled,
            scopes: key.scopes.clone(),
            is_expired: key.is_expired(),
        }
    }
}

/// In-memory API key store (fallback when Redis unavailable)
#[derive(Default)]
pub struct ApiKeyStore {
    /// Map from key_hash to ApiKey
    keys_by_hash: HashMap<String, ApiKey>,
    /// Map from key ID to key_hash
    id_to_hash: HashMap<String, String>,
}

impl ApiKeyStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, key: ApiKey) {
        self.id_to_hash.insert(key.id.clone(), key.key_hash.clone());
        self.keys_by_hash.insert(key.key_hash.clone(), key);
    }

    pub fn get_by_hash(&self, hash: &str) -> Option<&ApiKey> {
        self.keys_by_hash.get(hash)
    }

    pub fn get_by_id(&self, id: &str) -> Option<&ApiKey> {
        self.id_to_hash
            .get(id)
            .and_then(|hash| self.keys_by_hash.get(hash))
    }

    pub fn get_by_id_mut(&mut self, id: &str) -> Option<&mut ApiKey> {
        let hash = self.id_to_hash.get(id)?.clone();
        self.keys_by_hash.get_mut(&hash)
    }

    pub fn remove(&mut self, id: &str) -> Option<ApiKey> {
        if let Some(hash) = self.id_to_hash.remove(id) {
            self.keys_by_hash.remove(&hash)
        } else {
            None
        }
    }

    pub fn list(&self) -> Vec<&ApiKey> {
        self.keys_by_hash.values().collect()
    }

    pub fn update_hash(&mut self, id: &str, old_hash: &str, new_hash: String) {
        if let Some(key) = self.keys_by_hash.remove(old_hash) {
            self.id_to_hash.insert(id.to_string(), new_hash.clone());
            self.keys_by_hash.insert(new_hash, key);
        }
    }
}

/// Generate a cryptographically secure API key
fn generate_secure_key() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: [u8; 32] = rng.gen();
    format!("sm_{}", hex::encode(bytes))
}

/// Hash an API key using SHA-256
pub fn hash_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}

// Redis key constants
const REDIS_KEYS_SET: &str = "apikeys:list";
const REDIS_KEY_PREFIX: &str = "apikeys:";

/// API Key Manager - handles key operations with Redis or in-memory storage
pub struct ApiKeyManager {
    redis: Option<Arc<RedisClient>>,
    local_store: Arc<RwLock<ApiKeyStore>>,
    admin_key_hash: Option<String>,
}

impl ApiKeyManager {
    pub fn new(redis: Option<Arc<RedisClient>>, admin_key: Option<String>) -> Self {
        let admin_key_hash = admin_key.map(|k| hash_key(&k));

        Self {
            redis,
            local_store: Arc::new(RwLock::new(ApiKeyStore::new())),
            admin_key_hash,
        }
    }

    /// Verify an admin key
    pub fn verify_admin(&self, key: &str) -> bool {
        if let Some(ref admin_hash) = self.admin_key_hash {
            hash_key(key) == *admin_hash
        } else {
            false
        }
    }

    /// Create a new API key
    pub async fn create_key(
        &self,
        name: String,
        expires_in_days: Option<u32>,
        scopes: Vec<String>,
    ) -> Result<(ApiKey, String), ApiKeyError> {
        let (key, raw_key) = ApiKey::new(name, expires_in_days, scopes);

        if let Some(redis) = &self.redis {
            // Store in Redis
            let redis_key = format!("{}{}", REDIS_KEY_PREFIX, key.id);
            redis
                .set_json(&redis_key, &key, None)
                .await
                .map_err(|e| ApiKeyError::Storage(e.to_string()))?;
            redis
                .sadd(REDIS_KEYS_SET, &key.id)
                .await
                .map_err(|e| ApiKeyError::Storage(e.to_string()))?;
        }

        // Also store locally for fast lookups
        {
            let mut store = write_lock(&self.local_store);
            store.add(key.clone());
        }

        tracing::info!(key_id = %key.id, key_name = %key.name, "Created new API key");
        Ok((key, raw_key))
    }

    /// List all API keys
    pub async fn list_keys(&self) -> Result<Vec<ApiKeyInfo>, ApiKeyError> {
        if let Some(redis) = &self.redis {
            let ids: Vec<String> = redis
                .smembers(REDIS_KEYS_SET)
                .await
                .map_err(|e| ApiKeyError::Storage(e.to_string()))?;

            let mut keys = Vec::new();
            for id in ids {
                let redis_key = format!("{}{}", REDIS_KEY_PREFIX, id);
                if let Ok(Some(key)) = redis.get_json::<ApiKey>(&redis_key).await {
                    keys.push(ApiKeyInfo::from(&key));
                }
            }
            Ok(keys)
        } else {
            let store = read_lock(&self.local_store);
            Ok(store.list().iter().map(|k| ApiKeyInfo::from(*k)).collect())
        }
    }

    /// Get a key by ID
    pub async fn get_key(&self, id: &str) -> Result<Option<ApiKey>, ApiKeyError> {
        if let Some(redis) = &self.redis {
            let redis_key = format!("{}{}", REDIS_KEY_PREFIX, id);
            redis
                .get_json(&redis_key)
                .await
                .map_err(|e| ApiKeyError::Storage(e.to_string()))
        } else {
            let store = read_lock(&self.local_store);
            Ok(store.get_by_id(id).cloned())
        }
    }

    /// Validate an API key and update last_used
    pub async fn validate_key(&self, raw_key: &str) -> Option<ApiKey> {
        let key_hash = hash_key(raw_key);

        // Check local store first for speed
        {
            let store = read_lock(&self.local_store);
            if let Some(key) = store.get_by_hash(&key_hash) {
                if key.is_valid() {
                    return Some(key.clone());
                }
            }
        }

        // If Redis is available, also check there
        if let Some(redis) = &self.redis {
            let ids: Vec<String> = redis.smembers(REDIS_KEYS_SET).await.ok()?;

            for id in ids {
                let redis_key = format!("{}{}", REDIS_KEY_PREFIX, id);
                if let Ok(Some(key)) = redis.get_json::<ApiKey>(&redis_key).await {
                    if key.key_hash == key_hash && key.is_valid() {
                        // Update local store
                        {
                            let mut store = write_lock(&self.local_store);
                            store.add(key.clone());
                        }

                        // Update last_used in Redis (fire and forget)
                        let mut updated_key = key.clone();
                        updated_key.last_used = Some(Utc::now());
                        let _ = redis.set_json(&redis_key, &updated_key, None).await;

                        return Some(key);
                    }
                }
            }
        }

        None
    }

    /// Revoke (disable) an API key
    pub async fn revoke_key(&self, id: &str) -> Result<bool, ApiKeyError> {
        if let Some(redis) = &self.redis {
            let redis_key = format!("{}{}", REDIS_KEY_PREFIX, id);
            if let Some(mut key) = self.get_key(id).await? {
                key.enabled = false;
                redis
                    .set_json(&redis_key, &key, None)
                    .await
                    .map_err(|e| ApiKeyError::Storage(e.to_string()))?;

                // Update local store
                {
                    let mut store = write_lock(&self.local_store);
                    if let Some(local_key) = store.get_by_id_mut(id) {
                        local_key.enabled = false;
                    }
                }

                tracing::info!(key_id = %id, "Revoked API key");
                return Ok(true);
            }
        } else {
            let mut store = write_lock(&self.local_store);
            if let Some(key) = store.get_by_id_mut(id) {
                key.enabled = false;
                tracing::info!(key_id = %id, "Revoked API key");
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Delete an API key permanently
    pub async fn delete_key(&self, id: &str) -> Result<bool, ApiKeyError> {
        if let Some(redis) = &self.redis {
            let redis_key = format!("{}{}", REDIS_KEY_PREFIX, id);
            redis
                .delete(&redis_key)
                .await
                .map_err(|e| ApiKeyError::Storage(e.to_string()))?;
            redis
                .srem(REDIS_KEYS_SET, id)
                .await
                .map_err(|e| ApiKeyError::Storage(e.to_string()))?;
        }

        // Remove from local store
        {
            let mut store = write_lock(&self.local_store);
            store.remove(id);
        }

        tracing::info!(key_id = %id, "Deleted API key");
        Ok(true)
    }

    /// Rotate an API key (generate new key, same metadata)
    pub async fn rotate_key(&self, id: &str) -> Result<Option<String>, ApiKeyError> {
        let mut key = match self.get_key(id).await? {
            Some(k) => k,
            None => return Ok(None),
        };

        let old_hash = key.key_hash.clone();
        let new_raw_key = key.rotate();

        if let Some(redis) = &self.redis {
            let redis_key = format!("{}{}", REDIS_KEY_PREFIX, id);
            redis
                .set_json(&redis_key, &key, None)
                .await
                .map_err(|e| ApiKeyError::Storage(e.to_string()))?;
        }

        // Update local store
        {
            let mut store = write_lock(&self.local_store);
            store.update_hash(id, &old_hash, key.key_hash.clone());
            if let Some(local_key) = store.get_by_id_mut(id) {
                local_key.key_hash = key.key_hash;
            }
        }

        tracing::info!(key_id = %id, "Rotated API key");
        Ok(Some(new_raw_key))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ApiKeyError {
    #[error("Storage error: {0}")]
    Storage(String),
    #[error("Key not found")]
    NotFound,
    #[error("Unauthorized")]
    Unauthorized,
}

// ============================================
// HTTP Handlers
// ============================================

#[derive(Clone)]
pub struct ApiKeyState {
    pub manager: Arc<ApiKeyManager>,
}

#[derive(Deserialize)]
pub struct CreateKeyRequest {
    pub name: String,
    #[serde(default)]
    pub expires_in_days: Option<u32>,
    #[serde(default)]
    pub scopes: Vec<String>,
}

#[derive(Serialize)]
pub struct CreateKeyResponse {
    pub id: String,
    pub key: String,
    pub name: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub warning: &'static str,
}

#[derive(Serialize)]
pub struct RotateKeyResponse {
    pub id: String,
    pub new_key: String,
    pub warning: &'static str,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
    code: String,
}

/// Extract and verify admin authorization
fn extract_admin_auth(
    headers: &axum::http::HeaderMap,
    manager: &ApiKeyManager,
) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
    let auth_header = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "Admin authorization required".to_string(),
                    code: "ADMIN_AUTH_REQUIRED".to_string(),
                }),
            )
        })?;

    if !manager.verify_admin(auth_header) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                error: "Invalid admin key".to_string(),
                code: "INVALID_ADMIN_KEY".to_string(),
            }),
        ));
    }

    Ok(())
}

/// POST /api/keys - Create a new API key (admin only)
pub async fn create_api_key(
    State(state): State<ApiKeyState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<CreateKeyRequest>,
) -> impl IntoResponse {
    if let Err(e) = extract_admin_auth(&headers, &state.manager) {
        return e.into_response();
    }

    match state
        .manager
        .create_key(req.name, req.expires_in_days, req.scopes)
        .await
    {
        Ok((key, raw_key)) => (
            StatusCode::CREATED,
            Json(CreateKeyResponse {
                id: key.id,
                key: raw_key,
                name: key.name,
                expires_at: key.expires_at,
                warning: "Save this key securely - it cannot be retrieved later",
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
                code: "CREATE_FAILED".to_string(),
            }),
        )
            .into_response(),
    }
}

/// GET /api/keys - List all API keys (admin only)
pub async fn list_api_keys(
    State(state): State<ApiKeyState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = extract_admin_auth(&headers, &state.manager) {
        return e.into_response();
    }

    match state.manager.list_keys().await {
        Ok(keys) => Json(keys).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
                code: "LIST_FAILED".to_string(),
            }),
        )
            .into_response(),
    }
}

/// DELETE /api/keys/:id - Delete an API key (admin only)
pub async fn delete_api_key(
    State(state): State<ApiKeyState>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if let Err(e) = extract_admin_auth(&headers, &state.manager) {
        return e.into_response();
    }

    match state.manager.delete_key(&id).await {
        Ok(true) => (
            StatusCode::OK,
            Json(serde_json::json!({"status": "deleted", "id": id})),
        )
            .into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Key not found".to_string(),
                code: "NOT_FOUND".to_string(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
                code: "DELETE_FAILED".to_string(),
            }),
        )
            .into_response(),
    }
}

/// POST /api/keys/:id/revoke - Revoke (disable) an API key (admin only)
pub async fn revoke_api_key(
    State(state): State<ApiKeyState>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if let Err(e) = extract_admin_auth(&headers, &state.manager) {
        return e.into_response();
    }

    match state.manager.revoke_key(&id).await {
        Ok(true) => (
            StatusCode::OK,
            Json(serde_json::json!({"status": "revoked", "id": id})),
        )
            .into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Key not found".to_string(),
                code: "NOT_FOUND".to_string(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
                code: "REVOKE_FAILED".to_string(),
            }),
        )
            .into_response(),
    }
}

/// POST /api/keys/:id/rotate - Rotate an API key (admin only)
pub async fn rotate_api_key(
    State(state): State<ApiKeyState>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if let Err(e) = extract_admin_auth(&headers, &state.manager) {
        return e.into_response();
    }

    match state.manager.rotate_key(&id).await {
        Ok(Some(new_key)) => (
            StatusCode::OK,
            Json(RotateKeyResponse {
                id,
                new_key,
                warning: "Save this key securely - it cannot be retrieved later. The old key is now invalid.",
            }),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Key not found".to_string(),
                code: "NOT_FOUND".to_string(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
                code: "ROTATE_FAILED".to_string(),
            }),
        )
            .into_response(),
    }
}

/// Build the API keys router
pub fn api_keys_router(state: ApiKeyState) -> Router {
    Router::new()
        .route("/", post(create_api_key).get(list_api_keys))
        .route("/:id", delete(delete_api_key))
        .route("/:id/revoke", post(revoke_api_key))
        .route("/:id/rotate", post(rotate_api_key))
        .with_state(state)
}

/// Hex encoding helper
mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        bytes
            .as_ref()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_generation() {
        let key = generate_secure_key();
        assert!(key.starts_with("sm_"));
        assert_eq!(key.len(), 3 + 64); // "sm_" + 32 bytes hex
    }

    #[test]
    fn test_key_hashing() {
        let key1 = "test_key_123";
        let key2 = "test_key_123";
        let key3 = "different_key";

        assert_eq!(hash_key(key1), hash_key(key2));
        assert_ne!(hash_key(key1), hash_key(key3));
    }

    #[test]
    fn test_api_key_creation() {
        let (key, raw) = ApiKey::new("Test Key".to_string(), Some(30), vec!["deploy".to_string()]);

        assert_eq!(key.name, "Test Key");
        assert!(key.enabled);
        assert!(key.expires_at.is_some());
        assert!(raw.starts_with("sm_"));
        assert_eq!(hash_key(&raw), key.key_hash);
    }

    #[test]
    fn test_api_key_expiry() {
        let (mut key, _) = ApiKey::new("Test".to_string(), None, vec![]);
        assert!(!key.is_expired());

        // Set expiry to past
        key.expires_at = Some(Utc::now() - chrono::Duration::hours(1));
        assert!(key.is_expired());
        assert!(!key.is_valid());
    }

    #[test]
    fn test_api_key_store() {
        let mut store = ApiKeyStore::new();
        let (key, _) = ApiKey::new("Test".to_string(), None, vec![]);
        let id = key.id.clone();
        let hash = key.key_hash.clone();

        store.add(key);

        assert!(store.get_by_id(&id).is_some());
        assert!(store.get_by_hash(&hash).is_some());

        store.remove(&id);
        assert!(store.get_by_id(&id).is_none());
    }
}
