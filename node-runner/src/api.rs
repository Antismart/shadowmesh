//! Node Runner API Routes
//!
//! REST API endpoints for node management and monitoring.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::metrics::{BandwidthStats, MetricsSnapshot};
use crate::storage::{StorageStats, StoredContent};
use crate::AppState;

/// API routes
pub fn api_routes() -> Router<Arc<AppState>> {
    Router::new()
        // Status and health
        .route("/status", get(get_status))
        .route("/health", get(health_check))
        .route("/ready", get(ready_check))
        .route("/metrics", get(get_metrics))
        .route("/metrics/history", get(get_metrics_history))
        // Configuration
        .route("/config", get(get_config))
        .route("/config", put(update_config))
        // Storage
        .route("/storage", get(get_storage_stats))
        .route("/storage/content", get(list_content))
        .route("/storage/content/:cid", get(get_content))
        .route("/storage/content/:cid", delete(delete_content))
        .route("/storage/pin/:cid", post(pin_content))
        .route("/storage/unpin/:cid", post(unpin_content))
        .route("/storage/gc", post(run_garbage_collection))
        // Network
        .route("/network/peers", get(get_peers))
        .route("/network/bandwidth", get(get_bandwidth))
        // Node control
        .route("/node/shutdown", post(shutdown_node))
}

/// Node status response
#[derive(Debug, Serialize)]
pub struct NodeStatusResponse {
    pub peer_id: String,
    pub name: String,
    pub version: String,
    pub uptime_secs: u64,
    pub uptime_formatted: String,
    pub status: String,
    pub storage_used: u64,
    pub storage_capacity: u64,
    pub connected_peers: u32,
    pub bandwidth_served: String,
}

/// Get node status
async fn get_status(State(state): State<Arc<AppState>>) -> Json<NodeStatusResponse> {
    let metrics = state.metrics.get_snapshot().await;
    let storage = state.storage.get_stats().await;

    Json(NodeStatusResponse {
        peer_id: state.peer_id.clone(),
        name: state.config.read().await.identity.name.clone(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_secs: metrics.uptime_secs,
        uptime_formatted: metrics.uptime_formatted,
        status: "running".to_string(),
        storage_used: storage.total_bytes,
        storage_capacity: storage.capacity_bytes,
        connected_peers: metrics.network.connected_peers,
        bandwidth_served: BandwidthStats::format_bytes(metrics.bandwidth.total_served),
    })
}

/// Health check response
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub healthy: bool,
    pub checks: HealthChecks,
}

#[derive(Debug, Serialize)]
pub struct HealthChecks {
    pub storage: bool,
    pub network: bool,
    pub api: bool,
}

/// Health check endpoint
async fn health_check(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    let storage = state.storage.get_stats().await;
    let network = state.metrics.get_network_stats().await;

    let storage_ok = storage.usage_percentage() < 95.0;
    let network_ok = network.connected_peers > 0 || !state.config.read().await.network.enable_dht;

    Json(HealthResponse {
        healthy: storage_ok && network_ok,
        checks: HealthChecks {
            storage: storage_ok,
            network: network_ok,
            api: true,
        },
    })
}

/// Readiness check endpoint (returns 503 when not ready)
async fn ready_check(State(state): State<Arc<AppState>>) -> Response {
    let storage = state.storage.get_stats().await;
    let network = state.metrics.get_network_stats().await;

    let storage_ok = storage.usage_percentage() < 95.0;
    let network_ok = network.connected_peers > 0 || !state.config.read().await.network.enable_dht;
    let all_ready = storage_ok && network_ok;

    let response = HealthResponse {
        healthy: all_ready,
        checks: HealthChecks {
            storage: storage_ok,
            network: network_ok,
            api: true,
        },
    };

    if all_ready {
        (StatusCode::OK, Json(response)).into_response()
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, Json(response)).into_response()
    }
}

/// Get detailed metrics
async fn get_metrics(State(state): State<Arc<AppState>>) -> Json<MetricsSnapshot> {
    Json(state.metrics.get_snapshot().await)
}

/// Query parameters for history
#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    60
}

/// Get metrics history
async fn get_metrics_history(
    State(state): State<Arc<AppState>>,
    Query(query): Query<HistoryQuery>,
) -> Json<Vec<crate::metrics::DataPoint>> {
    Json(state.metrics.get_history(query.limit).await)
}

/// Configuration update request
#[derive(Debug, Deserialize)]
pub struct ConfigUpdateRequest {
    pub max_storage_gb: Option<f64>,
    pub max_bandwidth_mbps: Option<u64>,
    pub max_peers: Option<usize>,
    pub node_name: Option<String>,
}

/// Get current configuration
async fn get_config(State(state): State<Arc<AppState>>) -> Json<crate::config::NodeConfig> {
    Json(state.config.read().await.clone())
}

/// Update configuration
async fn update_config(
    State(state): State<Arc<AppState>>,
    Json(update): Json<ConfigUpdateRequest>,
) -> Result<Json<crate::config::NodeConfig>, StatusCode> {
    let mut config = state.config.write().await;

    if let Some(storage_gb) = update.max_storage_gb {
        config.storage.max_storage_bytes = (storage_gb * 1024.0 * 1024.0 * 1024.0) as u64;
    }

    if let Some(bw) = update.max_bandwidth_mbps {
        config.network.max_outbound_bandwidth = Some(bw * 1024 * 1024);
    }

    if let Some(peers) = update.max_peers {
        config.network.max_peers = peers;
    }

    if let Some(name) = update.node_name {
        config.identity.name = name;
    }

    // Validate
    if let Err(errors) = config.validate() {
        eprintln!("Config validation failed: {:?}", errors);
        return Err(StatusCode::BAD_REQUEST);
    }

    Ok(Json(config.clone()))
}

/// Get storage statistics
async fn get_storage_stats(State(state): State<Arc<AppState>>) -> Json<StorageStats> {
    Json(state.storage.get_stats().await)
}

/// List stored content
async fn list_content(State(state): State<Arc<AppState>>) -> Json<Vec<StoredContent>> {
    Json(state.storage.list_content().await)
}

/// Get specific content
async fn get_content(
    State(state): State<Arc<AppState>>,
    Path(cid): Path<String>,
) -> Result<Json<StoredContent>, StatusCode> {
    state
        .storage
        .get_content(&cid)
        .await
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

/// Delete content
async fn delete_content(
    State(state): State<Arc<AppState>>,
    Path(cid): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .storage
        .delete(&cid)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))
}

/// Pin content
async fn pin_content(
    State(state): State<Arc<AppState>>,
    Path(cid): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .storage
        .pin(&cid)
        .await
        .map(|_| StatusCode::OK)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))
}

/// Unpin content
async fn unpin_content(
    State(state): State<Arc<AppState>>,
    Path(cid): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .storage
        .unpin(&cid)
        .await
        .map(|_| StatusCode::OK)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))
}

/// GC request
#[derive(Debug, Deserialize)]
pub struct GCRequest {
    #[serde(default = "default_gc_target")]
    pub target_free_gb: f64,
}

fn default_gc_target() -> f64 {
    1.0
}

/// Run garbage collection
async fn run_garbage_collection(
    State(state): State<Arc<AppState>>,
    Json(req): Json<GCRequest>,
) -> Result<Json<crate::storage::GCResult>, (StatusCode, String)> {
    let target_bytes = (req.target_free_gb * 1024.0 * 1024.0 * 1024.0) as u64;

    state
        .storage
        .garbage_collect(target_bytes)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

/// Peer information
#[derive(Debug, Serialize)]
pub struct PeerInfo {
    pub peer_id: String,
    pub addresses: Vec<String>,
    pub connected: bool,
    pub latency_ms: Option<u32>,
}

/// Get connected peers
async fn get_peers(State(_state): State<Arc<AppState>>) -> Json<Vec<PeerInfo>> {
    // TODO: Integrate with actual P2P layer
    // Placeholder - would normally come from P2P layer
    Json(vec![])
}

/// Bandwidth response
#[derive(Debug, Serialize)]
pub struct BandwidthResponse {
    pub inbound_bps: u64,
    pub outbound_bps: u64,
    pub total_inbound: String,
    pub total_outbound: String,
    pub limit_inbound: Option<u64>,
    pub limit_outbound: Option<u64>,
}

/// Get bandwidth statistics
async fn get_bandwidth(State(state): State<Arc<AppState>>) -> Json<BandwidthResponse> {
    let stats = state.metrics.get_bandwidth_stats();
    let config = state.config.read().await;

    Json(BandwidthResponse {
        inbound_bps: stats.avg_received_bps,
        outbound_bps: stats.avg_served_bps,
        total_inbound: stats.received_formatted(),
        total_outbound: stats.served_formatted(),
        limit_inbound: config.network.max_inbound_bandwidth,
        limit_outbound: config.network.max_outbound_bandwidth,
    })
}

/// Shutdown response
#[derive(Debug, Serialize)]
pub struct ShutdownResponse {
    pub message: String,
    pub graceful: bool,
}

/// Shutdown node
async fn shutdown_node(State(state): State<Arc<AppState>>) -> Json<ShutdownResponse> {
    // Signal shutdown
    state.shutdown_signal.send(()).ok();

    Json(ShutdownResponse {
        message: "Shutdown initiated".to_string(),
        graceful: true,
    })
}
