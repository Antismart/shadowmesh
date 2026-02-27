//! Node Runner API Routes
//!
//! REST API endpoints for node management and monitoring.

use axum::{
    body::Body,
    extract::{Multipart, Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Json, Router,
};
use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use std::fmt::Write;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio_util::io::ReaderStream;

use crate::metrics::{BandwidthStats, MetricsSnapshot};
use crate::replication::ReplicationHealthReport;
use crate::storage::{StorageStats, StoredContent};
use crate::AppState;

/// API routes
pub fn api_routes() -> Router<Arc<AppState>> {
    // Read-only routes — always open (gateway, monitoring, dashboards)
    let public = Router::new()
        .route("/status", get(get_status))
        .route("/health", get(health_check))
        .route("/ready", get(ready_check))
        .route("/metrics", get(get_metrics))
        .route("/metrics/history", get(get_metrics_history))
        .route("/metrics/prometheus", get(get_prometheus_metrics))
        .route("/config", get(get_config))
        .route("/storage", get(get_storage_stats))
        .route("/storage/content", get(list_content))
        .route("/storage/content/:cid", get(get_content))
        .route("/storage/download/:cid", get(download_content))
        .route("/network/peers", get(get_peers))
        .route("/network/bandwidth", get(get_bandwidth))
        .route("/replication/health", get(get_replication_health));

    // Destructive routes — require NODE_API_KEY when set
    let protected = Router::new()
        .route("/config", put(update_config))
        .route("/storage/content/:cid", delete(delete_content))
        .route("/storage/pin/:cid", post(pin_content))
        .route("/storage/unpin/:cid", post(unpin_content))
        .route("/storage/upload", post(upload_content))
        .route("/storage/fetch/:cid", post(fetch_remote_content))
        .route("/storage/gc", post(run_garbage_collection))
        .route("/node/shutdown", post(shutdown_node))
        .layer(axum::middleware::from_fn(crate::auth::require_api_key));

    public.merge(protected)
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

    let storage_ok = storage.usage_percentage() < 95.0;
    // P2P is considered OK if the event loop is running (p2p is Some) or DHT is disabled
    let network_ok = state.p2p.is_some() || !state.config.read().await.network.enable_dht;

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

    let storage_ok = storage.usage_percentage() < 95.0;
    let network_ok = state.p2p.is_some() || !state.config.read().await.network.enable_dht;
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

/// Get metrics in Prometheus text exposition format
async fn get_prometheus_metrics(State(state): State<Arc<AppState>>) -> Response {
    let snapshot = state.metrics.get_snapshot().await;
    let storage = state.storage.get_stats().await;
    let repl = match &state.replication {
        Some(r) => Some(r.health_report().await),
        None => None,
    };

    let body = render_prometheus(&snapshot, &storage, &repl);

    (
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
        .into_response()
}

/// Render all node metrics in Prometheus text exposition format.
fn render_prometheus(
    snapshot: &MetricsSnapshot,
    storage: &StorageStats,
    repl: &Option<ReplicationHealthReport>,
) -> String {
    let mut out = String::with_capacity(4096);

    // ── Node ────────────────────────────────────────────────────────
    prom_gauge(&mut out, "shadowmesh_uptime_seconds", "Node uptime in seconds", snapshot.uptime_secs as f64);

    // ── Requests ────────────────────────────────────────────────────
    prom_counter(&mut out, "shadowmesh_requests_total", "Total requests handled", snapshot.requests.total);
    prom_counter(&mut out, "shadowmesh_requests_successful_total", "Total successful requests", snapshot.requests.successful);
    prom_counter(&mut out, "shadowmesh_requests_failed_total", "Total failed requests", snapshot.requests.failed);
    prom_counter(&mut out, "shadowmesh_cache_hits_total", "Total cache hits", snapshot.requests.cache_hits);
    prom_counter(&mut out, "shadowmesh_cache_misses_total", "Total cache misses", snapshot.requests.cache_misses);
    prom_gauge(&mut out, "shadowmesh_request_success_rate", "Request success rate percentage", snapshot.requests.success_rate);
    prom_gauge(&mut out, "shadowmesh_cache_hit_rate", "Cache hit rate percentage", snapshot.requests.cache_hit_rate);

    // ── Bandwidth ───────────────────────────────────────────────────
    prom_counter(&mut out, "shadowmesh_bandwidth_served_bytes_total", "Total bytes served", snapshot.bandwidth.total_served);
    prom_counter(&mut out, "shadowmesh_bandwidth_received_bytes_total", "Total bytes received", snapshot.bandwidth.total_received);
    prom_counter(&mut out, "shadowmesh_bandwidth_uploaded_bytes_total", "Total bytes uploaded", snapshot.bandwidth.total_uploaded);
    prom_gauge(&mut out, "shadowmesh_bandwidth_served_bps", "Average bytes per second served", snapshot.bandwidth.avg_served_bps as f64);
    prom_gauge(&mut out, "shadowmesh_bandwidth_received_bps", "Average bytes per second received", snapshot.bandwidth.avg_received_bps as f64);
    prom_gauge(&mut out, "shadowmesh_bandwidth_uploaded_bps", "Average bytes per second uploaded", snapshot.bandwidth.avg_uploaded_bps as f64);

    // ── Network ─────────────────────────────────────────────────────
    prom_gauge(&mut out, "shadowmesh_connected_peers", "Number of connected peers", snapshot.network.connected_peers as f64);
    prom_gauge(&mut out, "shadowmesh_discovered_peers", "Number of discovered peers", snapshot.network.discovered_peers as f64);
    prom_gauge(&mut out, "shadowmesh_active_connections", "Number of active connections", snapshot.network.active_connections as f64);
    prom_gauge(&mut out, "shadowmesh_pending_dials", "Number of pending dial attempts", snapshot.network.pending_dials as f64);
    prom_gauge(&mut out, "shadowmesh_dht_records", "Number of DHT records", snapshot.network.dht_records as f64);
    prom_gauge(&mut out, "shadowmesh_gossip_topics", "Number of gossip topics", snapshot.network.gossip_topics as f64);

    // ── Storage ─────────────────────────────────────────────────────
    prom_gauge(&mut out, "shadowmesh_storage_used_bytes", "Bytes currently stored", storage.total_bytes as f64);
    prom_gauge(&mut out, "shadowmesh_storage_capacity_bytes", "Storage capacity in bytes", storage.capacity_bytes as f64);
    prom_gauge(&mut out, "shadowmesh_storage_fragments", "Number of stored fragments", storage.fragment_count as f64);
    prom_gauge(&mut out, "shadowmesh_storage_content_items", "Number of stored content items", storage.content_count as f64);
    prom_gauge(&mut out, "shadowmesh_storage_pinned_items", "Number of pinned content items", storage.pinned_count as f64);

    let usage_ratio = if storage.capacity_bytes > 0 {
        storage.total_bytes as f64 / storage.capacity_bytes as f64
    } else {
        0.0
    };
    prom_gauge(&mut out, "shadowmesh_storage_usage_ratio", "Storage usage ratio (0.0-1.0)", usage_ratio);

    // ── Replication (only when active) ──────────────────────────────
    if let Some(ref report) = repl {
        prom_counter(&mut out, "shadowmesh_replication_total", "Total content items replicated", report.stats.total_replicated);
        prom_counter(&mut out, "shadowmesh_replication_bytes_total", "Total bytes replicated", report.stats.total_bytes_replicated);
        prom_counter(&mut out, "shadowmesh_replication_failures_total", "Total replication failures", report.stats.total_failures);
        prom_gauge(&mut out, "shadowmesh_replication_last_cycle_count", "Items replicated in last cycle", report.stats.last_cycle_replicated as f64);
        prom_gauge(&mut out, "shadowmesh_replication_scan_in_progress", "Whether a replication scan is in progress (0 or 1)", if report.stats.scan_in_progress { 1.0 } else { 0.0 });
        prom_gauge(&mut out, "shadowmesh_replication_content_total", "Total content tracked by replication", report.health.total_content as f64);
        prom_gauge(&mut out, "shadowmesh_replication_content_healthy", "Content items with sufficient replicas", report.health.healthy_content as f64);
        prom_gauge(&mut out, "shadowmesh_replication_content_under_replicated", "Content items with fewer replicas than target", report.health.under_replicated_content as f64);
        prom_gauge(&mut out, "shadowmesh_replication_content_critical", "Content items with critically low replicas", report.health.critical_content as f64);
        prom_gauge(&mut out, "shadowmesh_replication_health_ratio", "Replication health ratio (0.0-1.0)", report.health.health_percentage() / 100.0);
    }

    out
}

fn prom_gauge(out: &mut String, name: &str, help: &str, val: f64) {
    let _ = writeln!(out, "# HELP {name} {help}");
    let _ = writeln!(out, "# TYPE {name} gauge");
    let _ = writeln!(out, "{name} {val}");
}

fn prom_counter(out: &mut String, name: &str, help: &str, val: u64) {
    let _ = writeln!(out, "# HELP {name} {help}");
    let _ = writeln!(out, "# TYPE {name} counter");
    let _ = writeln!(out, "{name} {val}");
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
async fn get_peers(State(state): State<Arc<AppState>>) -> Json<Vec<PeerInfo>> {
    let config = state.config.read().await;

    // Start with self as the first entry
    let mut peers = vec![PeerInfo {
        peer_id: state.peer_id.clone(),
        addresses: config.network.listen_addresses.clone(),
        connected: true,
        latency_ms: Some(0),
    }];

    // Append connected peers from the P2P layer
    if let Some(ref p2p) = state.p2p {
        let connected = p2p.peers.read().await;
        for peer in connected.values() {
            peers.push(PeerInfo {
                peer_id: peer.peer_id.clone(),
                addresses: vec![peer.address.clone()],
                connected: true,
                latency_ms: None,
            });
        }
    }

    Json(peers)
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

/// Maximum upload size: 100 MB
const MAX_UPLOAD_SIZE: usize = 100 * 1024 * 1024;

/// Upload response
#[derive(Debug, Serialize)]
pub struct UploadResponse {
    pub success: bool,
    pub cid: String,
    pub name: String,
    pub size: u64,
    pub mime_type: String,
}

/// Upload content via multipart form
async fn upload_content(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<UploadResponse>, (StatusCode, String)> {
    // Read the first file field
    let field = multipart
        .next_field()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid multipart: {}", e)))?
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "No file field provided".to_string()))?;

    let filename = field
        .file_name()
        .unwrap_or("unnamed")
        .to_string();

    // Read data with size limit enforced incrementally
    let mut data = Vec::new();
    let mut stream = field;

    loop {
        match stream.chunk().await {
            Ok(Some(chunk)) => {
                if data.len() + chunk.len() > MAX_UPLOAD_SIZE {
                    return Err((
                        StatusCode::PAYLOAD_TOO_LARGE,
                        format!("Upload exceeds {} MB limit", MAX_UPLOAD_SIZE / (1024 * 1024)),
                    ));
                }
                data.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Err(e) => {
                return Err((StatusCode::BAD_REQUEST, format!("Read error: {}", e)));
            }
        }
    }

    if data.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Empty file".to_string()));
    }

    // Hash content to produce CID
    let cid = blake3::hash(&data).to_hex().to_string();

    // Detect MIME type
    let mime_type = infer::get(&data)
        .map(|t| t.mime_type().to_string())
        .unwrap_or_else(|| "application/octet-stream".to_string());

    let total_size = data.len() as u64;

    // Store as a single fragment
    state
        .storage
        .store_fragment(&cid, &cid, 0, &data)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Store content metadata
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let content = StoredContent {
        cid: cid.clone(),
        name: filename.clone(),
        total_size,
        fragment_count: 1,
        fragments: vec![cid.clone()],
        stored_at: now,
        pinned: false,
        mime_type: mime_type.clone(),
    };

    state
        .storage
        .store_content(content)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Record bandwidth
    state.metrics.record_received(total_size);

    // Announce content availability to the DHT and GossipSub via P2P
    if let Some(ref p2p) = state.p2p {
        let _ = p2p
            .command_tx
            .send(crate::p2p_commands::P2pCommand::AnnounceContent {
                content_hash: cid.clone(),
                fragment_hashes: vec![cid.clone()],
                total_size,
                mime_type: mime_type.clone(),
            })
            .await;

        let _ = p2p
            .command_tx
            .send(crate::p2p_commands::P2pCommand::BroadcastContentAnnouncement {
                cid: cid.clone(),
                total_size,
                fragment_count: 1,
                mime_type: mime_type.clone(),
            })
            .await;
    }

    Ok(Json(UploadResponse {
        success: true,
        cid,
        name: filename,
        size: total_size,
        mime_type,
    }))
}

/// Content larger than this threshold is streamed fragment-by-fragment.
const STREAM_THRESHOLD_BYTES: u64 = 5 * 1024 * 1024;

/// Download raw content bytes by CID.
///
/// Returns the raw file data with the correct Content-Type header,
/// suitable for direct consumption by gateways or browsers.
/// Content above 5 MB is streamed fragment-by-fragment to avoid
/// buffering the entire file in memory.
async fn download_content(
    State(state): State<Arc<AppState>>,
    Path(cid): Path<String>,
) -> Response {
    // 1. Look up content metadata
    let content = match state.storage.get_content(&cid).await {
        Some(c) => c,
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    let headers = [
        (header::CONTENT_TYPE, content.mime_type.clone()),
        (header::CONTENT_LENGTH, content.total_size.to_string()),
        (header::HeaderName::from_static("x-content-cid"), cid),
    ];

    // 2. Decide: buffer small content, stream large content
    if content.total_size <= STREAM_THRESHOLD_BYTES {
        // --- BUFFERED PATH ---
        let mut data = Vec::with_capacity(content.total_size as usize);
        for frag_hash in &content.fragments {
            match state.storage.get_fragment(frag_hash).await {
                Ok(bytes) => data.extend_from_slice(&bytes),
                Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            }
        }
        (headers, data).into_response()
    } else {
        // --- STREAMING PATH ---
        let storage = state.storage.clone();
        let fragment_hashes = content.fragments.clone();

        let byte_stream = stream::iter(fragment_hashes)
            .then(move |frag_hash| {
                let storage = storage.clone();
                async move {
                    match storage.get_fragment_path(&frag_hash).await {
                        Ok(path) => {
                            let file = tokio::fs::File::open(&path).await.map_err(|e| {
                                std::io::Error::other(format!(
                                    "Failed to open fragment {}: {}",
                                    frag_hash, e
                                ))
                            })?;
                            Ok::<_, std::io::Error>(ReaderStream::new(file))
                        }
                        Err(e) => Err(std::io::Error::other(format!(
                            "Fragment not found {}: {}",
                            frag_hash, e
                        ))),
                    }
                }
            })
            .flat_map(|result| match result {
                Ok(reader_stream) => reader_stream.boxed(),
                Err(e) => stream::once(async move { Err(e) }).boxed(),
            });

        let body = Body::from_stream(byte_stream);
        (headers, body).into_response()
    }
}

/// Fetch content from the P2P network by CID.
///
/// Checks local storage first. If not found, queries the DHT for providers,
/// fetches the manifest and fragments from a peer, verifies hashes, and
/// stores the content locally.
async fn fetch_remote_content(
    State(state): State<Arc<AppState>>,
    Path(cid): Path<String>,
) -> Result<Json<UploadResponse>, (StatusCode, String)> {
    // 1. Check if we already have it locally
    if let Some(content) = state.storage.get_content(&cid).await {
        return Ok(Json(UploadResponse {
            success: true,
            cid: content.cid,
            name: content.name,
            size: content.total_size,
            mime_type: content.mime_type,
        }));
    }

    // 2. Need P2P to fetch remotely
    let p2p = state.p2p.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "P2P not available".to_string(),
    ))?;

    // 3. Find providers via DHT
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    p2p.command_tx
        .send(crate::p2p_commands::P2pCommand::FindProviders {
            content_hash: cid.clone(),
            reply: reply_tx,
        })
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "P2P channel closed".to_string(),
            )
        })?;

    let providers = reply_rx
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "P2P reply channel closed".to_string(),
            )
        })?
        .unwrap_or_default(); // DHT miss is not fatal — fallback below

    // 3b. If DHT returned nothing, check GossipSub announcements and connected peers
    let providers = if providers.is_empty() {
        // Check if any GossipSub announcement told us who has this CID
        let announcements = p2p.content_announcements.read().await;
        let mut gossip_peers: Vec<libp2p::PeerId> = announcements
            .iter()
            .filter(|a| a.cid == cid)
            .filter_map(|a| a.peer_id.parse().ok())
            .collect();

        if gossip_peers.is_empty() {
            // Last resort: try all connected peers
            let peers = p2p.peers.read().await;
            gossip_peers = peers.keys().copied().collect();
        }

        if gossip_peers.is_empty() {
            return Err((
                StatusCode::NOT_FOUND,
                "No providers found for this CID".to_string(),
            ));
        }
        gossip_peers
    } else {
        providers
    };

    // 4. Get manifest from first available provider
    let peer = providers[0];
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    p2p.command_tx
        .send(crate::p2p_commands::P2pCommand::FetchManifest {
            peer_id: peer,
            content_hash: cid.clone(),
            reply: reply_tx,
        })
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "P2P channel closed".to_string(),
            )
        })?;

    let manifest = reply_rx
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "P2P reply channel closed".to_string(),
            )
        })?
        .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;

    // 5. Fetch each fragment from the provider
    for (idx, frag_hash) in manifest.fragment_hashes.iter().enumerate() {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        p2p.command_tx
            .send(crate::p2p_commands::P2pCommand::FetchFragment {
                peer_id: peer,
                fragment_hash: frag_hash.clone(),
                reply: reply_tx,
            })
            .await
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "P2P channel closed".to_string(),
                )
            })?;

        let data = reply_rx
            .await
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "P2P reply channel closed".to_string(),
                )
            })?
            .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;

        state
            .storage
            .store_fragment(frag_hash, &cid, idx as u32, &data)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    // 6. Store content metadata locally
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let content = StoredContent {
        cid: cid.clone(),
        name: cid.clone(),
        total_size: manifest.total_size,
        fragment_count: manifest.fragment_hashes.len() as u32,
        fragments: manifest.fragment_hashes,
        stored_at: now,
        pinned: false,
        mime_type: manifest.mime_type.clone(),
    };

    state
        .storage
        .store_content(content)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(UploadResponse {
        success: true,
        cid,
        name: manifest.content_hash,
        size: manifest.total_size,
        mime_type: manifest.mime_type,
    }))
}

/// Get replication health report
async fn get_replication_health(
    State(state): State<Arc<AppState>>,
) -> Response {
    match &state.replication {
        Some(repl) => {
            let report = repl.health_report().await;
            (StatusCode::OK, Json(report)).into_response()
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "Replication is not enabled"
            })),
        )
            .into_response(),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── PeerInfo serialization ──────────────────────────────────────

    #[test]
    fn peer_info_serializes_all_fields() {
        let peer = PeerInfo {
            peer_id: "12D3KooWTest".to_string(),
            addresses: vec!["/ip4/127.0.0.1/tcp/4001".to_string()],
            connected: true,
            latency_ms: Some(42),
        };

        let value = serde_json::to_value(&peer).unwrap();
        assert_eq!(value["peer_id"], "12D3KooWTest");
        assert_eq!(value["addresses"], json!(["/ip4/127.0.0.1/tcp/4001"]));
        assert_eq!(value["connected"], true);
        assert_eq!(value["latency_ms"], 42);
    }

    #[test]
    fn peer_info_serializes_none_latency_as_null() {
        let peer = PeerInfo {
            peer_id: "12D3KooWTest".to_string(),
            addresses: vec![],
            connected: false,
            latency_ms: None,
        };

        let value = serde_json::to_value(&peer).unwrap();
        assert!(value["latency_ms"].is_null());
        assert_eq!(value["connected"], false);
        assert!(value["addresses"].as_array().unwrap().is_empty());
    }

    // ── BandwidthResponse serialization ─────────────────────────────

    #[test]
    fn bandwidth_response_serializes_all_fields() {
        let bw = BandwidthResponse {
            inbound_bps: 1024,
            outbound_bps: 2048,
            total_inbound: "1.00 KB".to_string(),
            total_outbound: "2.00 KB".to_string(),
            limit_inbound: Some(10_000_000),
            limit_outbound: None,
        };

        let value = serde_json::to_value(&bw).unwrap();
        assert_eq!(value["inbound_bps"], 1024);
        assert_eq!(value["outbound_bps"], 2048);
        assert_eq!(value["total_inbound"], "1.00 KB");
        assert_eq!(value["total_outbound"], "2.00 KB");
        assert_eq!(value["limit_inbound"], 10_000_000);
        assert!(value["limit_outbound"].is_null());
    }

    #[test]
    fn bandwidth_response_roundtrip_json_string() {
        let bw = BandwidthResponse {
            inbound_bps: 0,
            outbound_bps: 0,
            total_inbound: "0 B".to_string(),
            total_outbound: "0 B".to_string(),
            limit_inbound: None,
            limit_outbound: None,
        };

        let json_str = serde_json::to_string(&bw).unwrap();
        assert!(json_str.contains("\"inbound_bps\":0"));
        assert!(json_str.contains("\"outbound_bps\":0"));
    }

    // ── ShutdownResponse serialization ──────────────────────────────

    #[test]
    fn shutdown_response_serializes_correctly() {
        let resp = ShutdownResponse {
            message: "Shutdown initiated".to_string(),
            graceful: true,
        };

        let value = serde_json::to_value(&resp).unwrap();
        assert_eq!(value["message"], "Shutdown initiated");
        assert_eq!(value["graceful"], true);
    }

    #[test]
    fn shutdown_response_non_graceful() {
        let resp = ShutdownResponse {
            message: "Force shutdown".to_string(),
            graceful: false,
        };

        let value = serde_json::to_value(&resp).unwrap();
        assert_eq!(value["message"], "Force shutdown");
        assert_eq!(value["graceful"], false);
    }

    // ── StorageStats serialization ──────────────────────────────────

    #[test]
    fn storage_stats_serializes_with_defaults() {
        let stats = StorageStats::default();
        let value = serde_json::to_value(&stats).unwrap();

        assert_eq!(value["total_bytes"], 0);
        assert_eq!(value["fragment_count"], 0);
        assert_eq!(value["content_count"], 0);
        assert_eq!(value["pinned_count"], 0);
        assert_eq!(value["capacity_bytes"], 0);
        assert!(value["last_gc"].is_null());
    }

    #[test]
    fn storage_stats_serializes_populated() {
        let stats = StorageStats {
            total_bytes: 5_000_000,
            fragment_count: 10,
            content_count: 3,
            pinned_count: 1,
            capacity_bytes: 10_000_000,
            last_gc: Some(1700000000),
        };

        let value = serde_json::to_value(&stats).unwrap();
        assert_eq!(value["total_bytes"], 5_000_000);
        assert_eq!(value["fragment_count"], 10);
        assert_eq!(value["content_count"], 3);
        assert_eq!(value["pinned_count"], 1);
        assert_eq!(value["capacity_bytes"], 10_000_000);
        assert_eq!(value["last_gc"], 1_700_000_000);
    }

    #[test]
    fn storage_stats_usage_percentage() {
        let stats = StorageStats {
            total_bytes: 500,
            capacity_bytes: 1000,
            ..Default::default()
        };
        let pct = stats.usage_percentage();
        assert!((pct - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn storage_stats_usage_percentage_zero_capacity() {
        let stats = StorageStats::default();
        assert!((stats.usage_percentage() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn storage_stats_available_bytes() {
        let stats = StorageStats {
            total_bytes: 300,
            capacity_bytes: 1000,
            ..Default::default()
        };
        assert_eq!(stats.available_bytes(), 700);
    }

    #[test]
    fn storage_stats_available_bytes_saturates() {
        let stats = StorageStats {
            total_bytes: 2000,
            capacity_bytes: 1000,
            ..Default::default()
        };
        assert_eq!(stats.available_bytes(), 0);
    }

    // ── StoredContent serialization ─────────────────────────────────

    #[test]
    fn stored_content_serializes_correctly() {
        let content = StoredContent {
            cid: "QmTestCid123".to_string(),
            name: "test-file.txt".to_string(),
            total_size: 4096,
            fragment_count: 2,
            fragments: vec!["hash_a".to_string(), "hash_b".to_string()],
            stored_at: 1700000000,
            pinned: true,
            mime_type: "text/plain".to_string(),
        };

        let value = serde_json::to_value(&content).unwrap();
        assert_eq!(value["cid"], "QmTestCid123");
        assert_eq!(value["name"], "test-file.txt");
        assert_eq!(value["total_size"], 4096);
        assert_eq!(value["fragment_count"], 2);
        assert_eq!(value["fragments"], json!(["hash_a", "hash_b"]));
        assert_eq!(value["stored_at"], 1_700_000_000);
        assert_eq!(value["pinned"], true);
        assert_eq!(value["mime_type"], "text/plain");
    }

    #[test]
    fn stored_content_deserializes_roundtrip() {
        let original = StoredContent {
            cid: "QmRoundTrip".to_string(),
            name: "round.bin".to_string(),
            total_size: 999,
            fragment_count: 1,
            fragments: vec!["frag0".to_string()],
            stored_at: 1600000000,
            pinned: false,
            mime_type: "application/octet-stream".to_string(),
        };

        let json_str = serde_json::to_string(&original).unwrap();
        let deserialized: StoredContent = serde_json::from_str(&json_str).unwrap();

        assert_eq!(deserialized.cid, original.cid);
        assert_eq!(deserialized.name, original.name);
        assert_eq!(deserialized.total_size, original.total_size);
        assert_eq!(deserialized.fragment_count, original.fragment_count);
        assert_eq!(deserialized.fragments, original.fragments);
        assert_eq!(deserialized.stored_at, original.stored_at);
        assert_eq!(deserialized.pinned, original.pinned);
        assert_eq!(deserialized.mime_type, original.mime_type);
    }

    // ── NodeStatusResponse serialization ────────────────────────────

    #[test]
    fn node_status_response_serializes_correctly() {
        let resp = NodeStatusResponse {
            peer_id: "12D3KooWStatus".to_string(),
            name: "my-node".to_string(),
            version: "0.1.0".to_string(),
            uptime_secs: 3600,
            uptime_formatted: "1h 0m 0s".to_string(),
            status: "running".to_string(),
            storage_used: 1024,
            storage_capacity: 1_073_741_824,
            connected_peers: 5,
            bandwidth_served: "10.00 MB".to_string(),
        };

        let value = serde_json::to_value(&resp).unwrap();
        assert_eq!(value["peer_id"], "12D3KooWStatus");
        assert_eq!(value["name"], "my-node");
        assert_eq!(value["uptime_secs"], 3600);
        assert_eq!(value["status"], "running");
        assert_eq!(value["connected_peers"], 5);
        assert_eq!(value["bandwidth_served"], "10.00 MB");
    }

    // ── HealthResponse / HealthChecks serialization ─────────────────

    #[test]
    fn health_response_serializes_correctly() {
        let resp = HealthResponse {
            healthy: true,
            checks: HealthChecks {
                storage: true,
                network: true,
                api: true,
            },
        };

        let value = serde_json::to_value(&resp).unwrap();
        assert_eq!(value["healthy"], true);
        assert_eq!(value["checks"]["storage"], true);
        assert_eq!(value["checks"]["network"], true);
        assert_eq!(value["checks"]["api"], true);
    }

    #[test]
    fn health_response_unhealthy() {
        let resp = HealthResponse {
            healthy: false,
            checks: HealthChecks {
                storage: false,
                network: true,
                api: true,
            },
        };

        let value = serde_json::to_value(&resp).unwrap();
        assert_eq!(value["healthy"], false);
        assert_eq!(value["checks"]["storage"], false);
    }

    // ── Helper function tests ───────────────────────────────────────

    #[test]
    fn default_limit_returns_60() {
        assert_eq!(default_limit(), 60);
    }

    #[test]
    fn default_gc_target_returns_1() {
        assert!((default_gc_target() - 1.0).abs() < f64::EPSILON);
    }

    // ── HistoryQuery deserialization ────────────────────────────────

    #[test]
    fn history_query_with_explicit_limit() {
        let q: HistoryQuery = serde_json::from_str(r#"{"limit": 100}"#).unwrap();
        assert_eq!(q.limit, 100);
    }

    #[test]
    fn history_query_uses_default_limit() {
        let q: HistoryQuery = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(q.limit, 60);
    }

    // ── ConfigUpdateRequest deserialization ──────────────────────────

    #[test]
    fn config_update_request_all_fields() {
        let req: ConfigUpdateRequest = serde_json::from_str(
            r#"{
                "max_storage_gb": 10.5,
                "max_bandwidth_mbps": 100,
                "max_peers": 50,
                "node_name": "my-node"
            }"#,
        )
        .unwrap();

        assert_eq!(req.max_storage_gb, Some(10.5));
        assert_eq!(req.max_bandwidth_mbps, Some(100));
        assert_eq!(req.max_peers, Some(50));
        assert_eq!(req.node_name, Some("my-node".to_string()));
    }

    #[test]
    fn config_update_request_partial_fields() {
        let req: ConfigUpdateRequest =
            serde_json::from_str(r#"{"node_name": "updated"}"#).unwrap();

        assert!(req.max_storage_gb.is_none());
        assert!(req.max_bandwidth_mbps.is_none());
        assert!(req.max_peers.is_none());
        assert_eq!(req.node_name, Some("updated".to_string()));
    }

    #[test]
    fn config_update_request_empty() {
        let req: ConfigUpdateRequest = serde_json::from_str(r#"{}"#).unwrap();

        assert!(req.max_storage_gb.is_none());
        assert!(req.max_bandwidth_mbps.is_none());
        assert!(req.max_peers.is_none());
        assert!(req.node_name.is_none());
    }

    // ── GCRequest deserialization ───────────────────────────────────

    #[test]
    fn gc_request_with_explicit_target() {
        let req: GCRequest = serde_json::from_str(r#"{"target_free_gb": 5.0}"#).unwrap();
        assert!((req.target_free_gb - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn gc_request_uses_default_target() {
        let req: GCRequest = serde_json::from_str(r#"{}"#).unwrap();
        assert!((req.target_free_gb - 1.0).abs() < f64::EPSILON);
    }

    // ── GCResult serialization ──────────────────────────────────────

    #[test]
    fn gc_result_serializes_correctly() {
        let result = crate::storage::GCResult {
            freed_bytes: 1024,
            removed_fragments: 3,
            removed_content: 1,
        };

        let value = serde_json::to_value(&result).unwrap();
        assert_eq!(value["freed_bytes"], 1024);
        assert_eq!(value["removed_fragments"], 3);
        assert_eq!(value["removed_content"], 1);
    }
}
