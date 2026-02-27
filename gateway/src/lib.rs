//! ShadowMesh Gateway — library crate
//!
//! Exposes core types and content-serving logic so that integration tests
//! can construct a minimal gateway router without the full production
//! startup ceremony.

pub mod api_keys;
pub mod audit;
pub mod auth;
pub mod cache;
pub mod circuit_breaker;
pub mod config;
pub mod config_watcher;
pub mod dashboard;
pub mod deploy;
pub mod distributed_rate_limit;
pub mod error;
pub mod lock_utils;
pub mod metrics;
pub mod middleware;
pub mod node_health;
pub mod node_resolver;
pub mod p2p;
pub mod p2p_commands;
pub mod p2p_resolver;
pub mod production;
pub mod rate_limit;
pub mod redis_client;
pub mod signaling;
pub mod spa;
pub mod telemetry;
pub mod upload;

use axum::{
    body::Body,
    extract::{Path, State},
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Instant;

/// Metrics for monitoring
#[derive(Default)]
pub struct Metrics {
    requests_total: AtomicU64,
    requests_success: AtomicU64,
    requests_error: AtomicU64,
    bytes_served: AtomicU64,
}

impl Metrics {
    pub fn increment_requests(&self) {
        self.requests_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_success(&self) {
        self.requests_success.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_error(&self) {
        self.requests_error.fetch_add(1, Ordering::Relaxed);
    }

    pub fn add_bytes(&self, bytes: u64) {
        self.bytes_served.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn requests_total(&self) -> u64 {
        self.requests_total.load(Ordering::Relaxed)
    }

    pub fn requests_success(&self) -> u64 {
        self.requests_success.load(Ordering::Relaxed)
    }

    pub fn requests_error(&self) -> u64 {
        self.requests_error.load(Ordering::Relaxed)
    }

    pub fn bytes_served(&self) -> u64 {
        self.bytes_served.load(Ordering::Relaxed)
    }
}

/// Escape a string for safe embedding in HTML content.
pub fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

#[derive(Clone)]
pub struct AppState {
    pub storage: Option<Arc<protocol::StorageLayer>>,
    pub cache: Arc<cache::ContentCache>,
    pub config: Arc<tokio::sync::RwLock<config::Config>>,
    pub metrics: Arc<Metrics>,
    pub start_time: Instant,
    pub deployments: Arc<RwLock<Vec<dashboard::Deployment>>>,
    pub github_auth: Arc<RwLock<Option<dashboard::GithubAuth>>>,
    pub github_oauth_states: Arc<RwLock<std::collections::HashMap<String, Instant>>>,
    /// Circuit breaker for IPFS operations
    pub ipfs_circuit_breaker: Arc<circuit_breaker::CircuitBreaker>,
    /// Audit logger for security events
    pub audit_logger: Arc<audit::AuditLogger>,
    /// Redis client for persistence (optional)
    pub redis: Option<Arc<redis_client::RedisClient>>,
    /// Decentralized naming manager
    pub naming: Arc<RwLock<protocol::NamingManager>>,
    /// Ed25519 keypair for signing name records
    pub naming_key: Arc<libp2p::identity::Keypair>,
    /// P2P mesh state (None if P2P is disabled or failed to start)
    pub p2p: Option<Arc<p2p::P2pState>>,
    /// Node-runner health tracker (None when no node-runners configured)
    pub node_health_tracker: Option<Arc<node_health::NodeHealthTracker>>,
    /// Shared HTTP client (connection-pooled)
    pub http_client: reqwest::Client,
}

/// Build a Router with just the content-serving routes.
/// Useful for integration tests that don't need auth/dashboard/middleware.
pub fn content_router(state: AppState) -> Router {
    Router::new()
        .route("/ipfs/*path", get(ipfs_content_path_handler))
        .with_state(state)
}

// Handler for /ipfs/{*path} - handles /ipfs/cid and /ipfs/cid/path
pub async fn ipfs_content_path_handler(
    State(state): State<AppState>,
    Path(path): Path<String>,
) -> Response {
    // The path contains everything after /ipfs/
    // It could be just "cid" or "cid/subpath/..."
    let parts: Vec<&str> = path.splitn(2, '/').collect();
    let cid = parts[0];

    if parts.len() == 1 {
        // Just a CID, redirect to content_handler logic
        fetch_content(&state, cid.to_string(), Some(format!("/ipfs/{}/", cid))).await
    } else {
        // CID with path
        let full_path = path.clone();
        fetch_content(&state, full_path, Some(format!("/ipfs/{}/", cid))).await
    }
}

// Unified content fetching logic with circuit breaker protection
pub async fn fetch_content(state: &AppState, cid: String, base_prefix: Option<String>) -> Response {
    state.metrics.increment_requests();
    let cfg = state.config.read().await.clone();

    // Check cache first (bypasses circuit breaker)
    if let Some((data, content_type)) = state.cache.get(&cid) {
        state.metrics.increment_success();
        state.metrics.add_bytes(data.len() as u64);
        metrics::record_cache_hit();
        return (
            [
                (axum::http::header::CONTENT_TYPE, content_type),
                (
                    axum::http::header::HeaderName::from_static("x-cache"),
                    "HIT".to_string(),
                ),
            ],
            data,
        )
            .into_response();
    }

    metrics::record_cache_miss();

    // Try P2P network before IPFS
    if let Some(ref p2p) = state.p2p {
        let timeout_secs = cfg.p2p.resolve_timeout_seconds;
        match p2p_resolver::resolve_content(p2p, &cid, timeout_secs).await {
            Ok(resolved) => {
                let content_type = if resolved.mime_type.is_empty() {
                    content_type_from_path(&cid, &resolved.data)
                } else {
                    resolved.mime_type
                };
                let data =
                    rewrite_html_assets(&resolved.data, &content_type, base_prefix.as_deref());

                state
                    .cache
                    .set(cid.clone(), data.clone(), content_type.clone());
                metrics::update_cache_size(state.cache.stats().total_entries as i64);

                // Announce to DHT so other peers can discover us
                if cfg.p2p.announce_content {
                    let _ = p2p
                        .command_tx
                        .send(p2p_commands::P2pCommand::AnnounceContent {
                            content_hash: cid.clone(),
                            fragment_hashes: vec![cid],
                            total_size: data.len() as u64,
                            mime_type: content_type.clone(),
                        })
                        .await;
                }

                state.metrics.increment_success();
                state.metrics.add_bytes(data.len() as u64);
                metrics::record_bytes_served(data.len() as u64);

                return (
                    [
                        (axum::http::header::CONTENT_TYPE, content_type),
                        (
                            axum::http::header::HeaderName::from_static("x-cache"),
                            "MISS".to_string(),
                        ),
                        (
                            axum::http::header::HeaderName::from_static("x-source"),
                            "P2P".to_string(),
                        ),
                    ],
                    data,
                )
                    .into_response();
            }
            Err(e) => {
                tracing::debug!(cid = %cid, error = %e,
                    "P2P resolution failed, falling back to IPFS");
            }
        }
    }

    // Try configured node-runners via HTTP
    if !cfg.p2p.node_runners.is_empty() {
        let http = &state.http_client;
        let timeout = cfg.p2p.resolve_timeout_seconds;

        // Use health-aware tracker when available, otherwise fall back.
        let node_content = if let Some(ref tracker) = state.node_health_tracker {
            node_resolver::resolve_from_nodes_with_tracking(&http, tracker, &cid, timeout).await
        } else {
            node_resolver::resolve_from_nodes_adaptive(
                &http,
                &cfg.p2p.node_runners,
                &cid,
                timeout,
            )
            .await
        };

        if let Some(node_content) = node_content {
            match node_content {
                node_resolver::NodeContent::Buffered(resolved) => {
                    let content_type = if resolved.mime_type.is_empty() {
                        content_type_from_path(&cid, &resolved.data)
                    } else {
                        resolved.mime_type
                    };
                    let data = rewrite_html_assets(
                        &resolved.data,
                        &content_type,
                        base_prefix.as_deref(),
                    );

                    state
                        .cache
                        .set(cid.clone(), data.clone(), content_type.clone());
                    metrics::update_cache_size(state.cache.stats().total_entries as i64);

                    state.metrics.increment_success();
                    state.metrics.add_bytes(data.len() as u64);
                    metrics::record_bytes_served(data.len() as u64);

                    return (
                        [
                            (axum::http::header::CONTENT_TYPE, content_type),
                            (
                                axum::http::header::HeaderName::from_static("x-cache"),
                                "MISS".to_string(),
                            ),
                            (
                                axum::http::header::HeaderName::from_static("x-source"),
                                "NODE".to_string(),
                            ),
                        ],
                        data,
                    )
                        .into_response();
                }
                node_resolver::NodeContent::Streaming(streaming) => {
                    let content_type = if streaming.mime_type.is_empty() {
                        "application/octet-stream".to_string()
                    } else {
                        streaming.mime_type
                    };

                    state.metrics.increment_success();
                    state.metrics.add_bytes(streaming.content_length);
                    metrics::record_bytes_served(streaming.content_length);

                    let body = Body::from_stream(streaming.stream);

                    return match axum::http::Response::builder()
                        .header(axum::http::header::CONTENT_TYPE, &content_type)
                        .header(
                            axum::http::header::CONTENT_LENGTH,
                            streaming.content_length.to_string(),
                        )
                        .header("x-cache", "MISS")
                        .header("x-source", "NODE-STREAM")
                        .body(body)
                    {
                        Ok(resp) => resp.into_response(),
                        Err(_) => axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
                    };
                }
            }
        }
    }

    // Check circuit breaker before attempting IPFS operation
    if !state.ipfs_circuit_breaker.allow_request() {
        state.metrics.increment_error();
        tracing::warn!(cid = %cid, "IPFS circuit breaker is open - failing fast");
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            axum::response::Json(serde_json::json!({
                "error": "IPFS service temporarily unavailable",
                "code": "CIRCUIT_OPEN",
                "retry_after_seconds": 30
            })),
        )
            .into_response();
    }

    // Fetch from IPFS
    let Some(storage) = &state.storage else {
        state.metrics.increment_error();
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Html(r#"<html><body><h1>IPFS Not Connected</h1></body></html>"#),
        )
            .into_response();
    };

    let storage = Arc::clone(storage);
    let start_time = std::time::Instant::now();

    let result = storage.retrieve_content(&cid).await;

    let duration = start_time.elapsed();

    match result {
        Ok(data) => {
            // Record success with circuit breaker and metrics
            state.ipfs_circuit_breaker.record_success();
            metrics::record_ipfs_operation("retrieve", true, duration.as_secs_f64());
            metrics::update_circuit_breaker_state(false, false);

            let content_type = content_type_from_path(&cid, &data);
            let data = rewrite_html_assets(&data, &content_type, base_prefix.as_deref());

            state.cache.set(cid.clone(), data.clone(), content_type.clone());
            metrics::update_cache_size(state.cache.stats().total_entries as i64);

            // Announce to P2P network after IPFS fetch
            if let Some(ref p2p) = state.p2p {
                if cfg.p2p.announce_content {
                    let _ = p2p
                        .command_tx
                        .send(p2p_commands::P2pCommand::AnnounceContent {
                            content_hash: cid.clone(),
                            fragment_hashes: vec![cid],
                            total_size: data.len() as u64,
                            mime_type: content_type.clone(),
                        })
                        .await;
                }
            }

            state.metrics.increment_success();
            state.metrics.add_bytes(data.len() as u64);
            metrics::record_bytes_served(data.len() as u64);

            (
                [
                    (axum::http::header::CONTENT_TYPE, content_type),
                    (
                        axum::http::header::HeaderName::from_static("x-cache"),
                        "MISS".to_string(),
                    ),
                ],
                data,
            )
                .into_response()
        }
        Err(e) => {
            // Record failure with circuit breaker
            state.ipfs_circuit_breaker.record_failure();
            metrics::record_ipfs_operation("retrieve", false, duration.as_secs_f64());
            metrics::update_circuit_breaker_state(
                state.ipfs_circuit_breaker.is_open(),
                state.ipfs_circuit_breaker.state() == crate::circuit_breaker::CircuitState::HalfOpen,
            );
            state.metrics.increment_error();

            tracing::warn!(cid = %cid, error = %e, "IPFS retrieval failed");

            (
                axum::http::StatusCode::NOT_FOUND,
                Html(format!(
                    r#"<html><body><h1>Not Found</h1><p>{}</p></body></html>"#,
                    escape_html(&e.to_string())
                )),
            )
                .into_response()
        }
    }
}

pub fn content_type_from_path(path: &str, data: &[u8]) -> String {
    let lower = path.to_lowercase();
    if lower.ends_with(".html") || lower.ends_with(".htm") || looks_like_html(data) {
        return "text/html".to_string();
    }
    if lower.ends_with(".css") {
        return "text/css".to_string();
    }
    if lower.ends_with(".js")
        || lower.ends_with(".mjs")
        || lower.ends_with(".jsx")
        || lower.ends_with(".ts")
    {
        return "application/javascript".to_string();
    }
    if lower.ends_with(".json") || lower.ends_with(".map") {
        return "application/json".to_string();
    }
    if lower.ends_with(".svg") {
        return "image/svg+xml".to_string();
    }
    if lower.ends_with(".png") {
        return "image/png".to_string();
    }
    if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        return "image/jpeg".to_string();
    }
    if lower.ends_with(".gif") {
        return "image/gif".to_string();
    }
    if lower.ends_with(".webp") {
        return "image/webp".to_string();
    }
    if lower.ends_with(".ico") {
        return "image/x-icon".to_string();
    }

    infer::get(data)
        .map(|t| t.mime_type().to_string())
        .unwrap_or_else(|| "application/octet-stream".to_string())
}

fn looks_like_html(data: &[u8]) -> bool {
    let snippet = String::from_utf8_lossy(&data[..data.len().min(512)]).to_lowercase();
    snippet.contains("<!doctype html") || snippet.contains("<html")
}

pub fn rewrite_html_assets(data: &[u8], content_type: &str, base_prefix: Option<&str>) -> Vec<u8> {
    if content_type != "text/html" {
        return data.to_vec();
    }

    let Some(base_prefix) = base_prefix else {
        return data.to_vec();
    };

    let base_prefix = if base_prefix.ends_with('/') {
        base_prefix.to_string()
    } else {
        format!("{}/", base_prefix)
    };

    let mut html = String::from_utf8_lossy(data).to_string();
    let guard = "__IPFS_GUARD__";

    if !html.to_lowercase().contains("<base ") {
        if let Some(head_pos) = html.to_lowercase().find("<head") {
            if let Some(end_pos) = html[head_pos..].find('>') {
                let insert_pos = head_pos + end_pos + 1;
                let base_tag = format!("\n    <base href=\"{}\">", base_prefix);
                html.insert_str(insert_pos, &base_tag);
            }
        }
    }

    for attr in ["href", "src", "srcset"] {
        let guard_token = format!("{}=\"{}/", attr, guard);
        html = html.replace(&format!("{}=\"/ipfs/", attr), &guard_token);
        html = html.replace(
            &format!("{}=\"/", attr),
            &format!("{}=\"{}", attr, base_prefix),
        );
        html = html.replace(&guard_token, &format!("{}=\"/ipfs/", attr));
    }

    html.into_bytes()
}

/// Sanitize a deployment name into a valid .shadow domain label.
fn sanitize_domain_label(name: &str) -> String {
    let label: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
        .collect();
    // Trim leading/trailing hyphens and limit length
    let label = label.trim_matches('-');
    let label = if label.len() > 40 { &label[..40] } else { label };
    let label = label.trim_end_matches('-');
    if label.is_empty() {
        "site".to_string()
    } else {
        label.to_string()
    }
}

/// Best-effort publish of a name record to DHT + GossipSub (sync context).
fn publish_name_best_effort(state: &AppState, record: &protocol::NameRecord) {
    if let Some(ref p2p) = state.p2p {
        if let Ok(bytes) = protocol::NamingManager::serialize_record(record) {
            let _ = p2p
                .command_tx
                .try_send(p2p_commands::P2pCommand::PublishName {
                    record_bytes: bytes,
                    name: record.name.clone(),
                });
        }
    }
}

/// Try to auto-assign a `.shadow` domain for a deployment.
/// Returns the assigned domain name, or None if naming is disabled or registration fails.
pub async fn auto_assign_domain(state: &AppState, deploy_name: &str, cid: &str) -> Option<String> {
    if !state.config.read().await.naming.enabled {
        return None;
    }

    let base_label = sanitize_domain_label(deploy_name);
    let records = vec![protocol::NameRecordType::Content {
        cid: cid.to_string(),
    }];

    let mut naming = lock_utils::write_lock(&state.naming);

    // Try the base name first
    let full_name = format!("{}.shadow", base_label);
    match naming.register_name(
        &full_name,
        records.clone(),
        &state.naming_key,
        protocol::naming::DEFAULT_NAME_TTL,
    ) {
        Ok(record) => {
            tracing::info!(domain = %full_name, cid = %cid, "Auto-assigned domain");
            publish_name_best_effort(state, &record);
            return Some(full_name);
        }
        Err(protocol::NamingError::NameTaken) => {
            // Name taken, try with random suffix
        }
        Err(e) => {
            tracing::warn!(name = %full_name, error = %e, "Failed to auto-assign domain");
            return None;
        }
    }

    // Try with random suffixes (up to 3 attempts)
    for _ in 0..3 {
        let suffix: String = (0..4)
            .map(|_| {
                let idx = rand::random::<u8>() % 36;
                if idx < 10 { (b'0' + idx) as char } else { (b'a' + idx - 10) as char }
            })
            .collect();
        let name_with_suffix = format!("{}-{}.shadow", base_label, suffix);
        match naming.register_name(
            &name_with_suffix,
            records.clone(),
            &state.naming_key,
            protocol::naming::DEFAULT_NAME_TTL,
        ) {
            Ok(record) => {
                tracing::info!(domain = %name_with_suffix, cid = %cid, "Auto-assigned domain (with suffix)");
                publish_name_best_effort(state, &record);
                return Some(name_with_suffix);
            }
            Err(_) => continue,
        }
    }

    tracing::warn!(name = %base_label, "Could not auto-assign domain after retries");
    None
}
