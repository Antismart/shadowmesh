//! End-to-end integration tests for the gateway ↔ node-runner content pipeline.
//!
//! Spins up a real node-runner TCP server, constructs a minimal gateway router,
//! and tests the full content flow: upload → resolve → serve.

use axum::body::Body;
use axum::Router;
use http_body_util::BodyExt;
use hyper::Request;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, RwLock};
use tower::ServiceExt;

use gateway::{
    cache, circuit_breaker, config, audit,
    AppState, Metrics,
};
use node_runner::api;
use node_runner::config::NodeConfig;
use node_runner::metrics::MetricsCollector;
use node_runner::storage::StorageManager;

// ── Test Helpers ────────────────────────────────────────────────

/// Start a real node-runner TCP server on a random port.
/// Returns the base URL, shared state, and a temp directory (must be held alive).
async fn start_node_runner() -> (String, Arc<node_runner::AppState>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let storage = Arc::new(
        StorageManager::new(dir.path().to_path_buf(), 100 * 1024 * 1024)
            .await
            .unwrap(),
    );
    let metrics = Arc::new(MetricsCollector::new());
    let (shutdown_tx, _) = broadcast::channel::<()>(1);

    let state = Arc::new(node_runner::AppState {
        peer_id: "12D3KooWTestPeerId12345".to_string(),
        config: RwLock::new(NodeConfig::default()),
        metrics,
        storage,
        shutdown_signal: shutdown_tx,
        p2p: None,
        bridge: None,
        replication: None,
    });

    let app = Router::new()
        .nest("/api", api::api_routes())
        .with_state(state.clone());

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://127.0.0.1:{}", addr.port());

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (url, state, dir)
}

/// Build a minimal gateway Router backed by the given node-runner URL.
fn gateway_app(node_runner_url: &str) -> Router {
    let mut cfg = config::Config::default();
    cfg.p2p.node_runners = vec![node_runner_url.to_string()];

    let state = AppState {
        storage: None,
        cache: Arc::new(cache::ContentCache::with_config(100, Duration::from_secs(300))),
        config: Arc::new(cfg),
        metrics: Arc::new(Metrics::default()),
        start_time: Instant::now(),
        deployments: Arc::new(std::sync::RwLock::new(Vec::new())),
        github_auth: Arc::new(std::sync::RwLock::new(None)),
        github_oauth_states: Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
        ipfs_circuit_breaker: Arc::new(circuit_breaker::CircuitBreaker::new(
            "ipfs-test", 5, Duration::from_secs(60),
        )),
        audit_logger: Arc::new(audit::AuditLogger::new(100)),
        redis: None,
        naming: Arc::new(std::sync::RwLock::new(protocol::NamingManager::new())),
        naming_key: Arc::new(libp2p::identity::Keypair::generate_ed25519()),
        p2p: None,
        node_health_tracker: None,
    };

    gateway::content_router(state)
}

/// Upload a file to the node-runner via multipart POST, return the CID.
async fn upload_to_node_runner(base_url: &str, filename: &str, data: &[u8]) -> String {
    let client = reqwest::Client::new();
    let part = reqwest::multipart::Part::bytes(data.to_vec())
        .file_name(filename.to_string())
        .mime_str("application/octet-stream")
        .unwrap();
    let form = reqwest::multipart::Form::new().part("file", part);

    let resp = client
        .post(format!("{}/api/storage/upload", base_url))
        .multipart(form)
        .send()
        .await
        .expect("upload request failed");

    assert_eq!(resp.status(), 200, "Upload failed");
    let json: serde_json::Value = resp.json().await.unwrap();
    json["cid"].as_str().unwrap().to_string()
}

/// Helper: send a GET request to the gateway router via tower::oneshot.
async fn gateway_get(app: Router, uri: &str) -> hyper::Response<Body> {
    let req = Request::builder()
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    app.oneshot(req).await.unwrap()
}

// ── Tests ───────────────────────────────────────────────────────

#[tokio::test]
async fn test_e2e_upload_and_fetch() {
    let (node_url, _state, _dir) = start_node_runner().await;
    let app = gateway_app(&node_url);

    // Upload content to node-runner
    let data = b"Hello from the ShadowMesh e2e test!";
    let cid = upload_to_node_runner(&node_url, "hello.txt", data).await;

    // Fetch through gateway
    let resp = gateway_get(app, &format!("/ipfs/{}", cid)).await;
    assert_eq!(resp.status(), 200);

    let headers = resp.headers();
    assert_eq!(
        headers.get("x-source").unwrap().to_str().unwrap(),
        "NODE",
    );
    assert_eq!(
        headers.get("x-cache").unwrap().to_str().unwrap(),
        "MISS",
    );

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&body[..], data);
}

#[tokio::test]
async fn test_e2e_cache_hit() {
    let (node_url, _state, _dir) = start_node_runner().await;

    // Upload content
    let data = b"cache test content";
    let cid = upload_to_node_runner(&node_url, "cache.txt", data).await;

    // First request — cache MISS
    let app = gateway_app(&node_url);
    let resp = gateway_get(app, &format!("/ipfs/{}", cid)).await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers().get("x-cache").unwrap(), "MISS");

    // tower::oneshot consumes the router, so we need a fresh one
    // BUT the cache is per-AppState, so we rebuild with a shared cache
    let mut cfg = config::Config::default();
    cfg.p2p.node_runners = vec![node_url.clone()];
    let shared_cache = Arc::new(cache::ContentCache::with_config(100, Duration::from_secs(300)));

    // Populate cache manually from first fetch
    let state = AppState {
        storage: None,
        cache: shared_cache.clone(),
        config: Arc::new(cfg.clone()),
        metrics: Arc::new(Metrics::default()),
        start_time: Instant::now(),
        deployments: Arc::new(std::sync::RwLock::new(Vec::new())),
        github_auth: Arc::new(std::sync::RwLock::new(None)),
        github_oauth_states: Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
        ipfs_circuit_breaker: Arc::new(circuit_breaker::CircuitBreaker::new(
            "ipfs-test", 5, Duration::from_secs(60),
        )),
        audit_logger: Arc::new(audit::AuditLogger::new(100)),
        redis: None,
        naming: Arc::new(std::sync::RwLock::new(protocol::NamingManager::new())),
        naming_key: Arc::new(libp2p::identity::Keypair::generate_ed25519()),
        p2p: None,
        node_health_tracker: None,
    };

    let app1 = gateway::content_router(state.clone());
    let resp1 = gateway_get(app1, &format!("/ipfs/{}", cid)).await;
    assert_eq!(resp1.status(), 200);
    assert_eq!(resp1.headers().get("x-cache").unwrap(), "MISS");
    // consume body to ensure response completes
    let _ = resp1.into_body().collect().await.unwrap();

    // Second request with same AppState — should be cache HIT
    let app2 = gateway::content_router(state);
    let resp2 = gateway_get(app2, &format!("/ipfs/{}", cid)).await;
    assert_eq!(resp2.status(), 200);
    assert_eq!(resp2.headers().get("x-cache").unwrap(), "HIT");

    let body = resp2.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&body[..], data);
}

#[tokio::test]
async fn test_e2e_missing_content_returns_error() {
    let (node_url, _state, _dir) = start_node_runner().await;
    let app = gateway_app(&node_url);

    // Request a CID that doesn't exist
    let resp = gateway_get(app, "/ipfs/nonexistent_cid_12345").await;
    // Without IPFS, this should return 503 (IPFS Not Connected)
    assert!(
        resp.status() == 503 || resp.status() == 404,
        "Expected 503 or 404, got {}",
        resp.status()
    );
}

#[tokio::test]
async fn test_e2e_content_type_preserved() {
    let (node_url, _state, _dir) = start_node_runner().await;
    let app = gateway_app(&node_url);

    // Upload HTML content
    let html = b"<!DOCTYPE html><html><head><title>Test</title></head><body>Hello</body></html>";
    let cid = upload_to_node_runner(&node_url, "index.html", html).await;

    // Fetch through gateway
    let resp = gateway_get(app, &format!("/ipfs/{}", cid)).await;
    assert_eq!(resp.status(), 200);

    let ct = resp.headers().get("content-type").unwrap().to_str().unwrap();
    // The gateway should detect this as text/html
    assert!(
        ct.contains("text/html"),
        "Expected text/html content type, got: {}",
        ct,
    );
}

#[tokio::test]
async fn test_e2e_large_content_streams() {
    let (node_url, nr_state, _dir) = start_node_runner().await;

    // Store content above 5MB streaming threshold directly via storage manager
    // (avoids Axum's default 2MB multipart body limit)
    let fragment_size = 1024 * 1024; // 1 MB
    let fragment_count = 6usize;
    let cid = "e2elargecidtest";
    let mut fragment_hashes = Vec::new();

    for i in 0..fragment_count {
        let data = vec![i as u8; fragment_size];
        let hash = format!("e2elargefrag{:04x}", i);
        nr_state
            .storage
            .store_fragment(&hash, cid, i as u32, &data)
            .await
            .unwrap();
        fragment_hashes.push(hash);
    }

    let total_size = (fragment_size * fragment_count) as u64;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    nr_state
        .storage
        .store_content(node_runner::storage::StoredContent {
            cid: cid.to_string(),
            name: "large.bin".to_string(),
            total_size,
            fragment_count: fragment_count as u32,
            fragments: fragment_hashes,
            stored_at: now,
            pinned: false,
            mime_type: "application/octet-stream".to_string(),
        })
        .await
        .unwrap();

    let app = gateway_app(&node_url);

    // Fetch through gateway
    let resp = gateway_get(app, &format!("/ipfs/{}", cid)).await;
    assert_eq!(resp.status(), 200);

    let headers = resp.headers();
    // Large content should be streamed (NODE-STREAM source)
    assert_eq!(
        headers.get("x-source").unwrap().to_str().unwrap(),
        "NODE-STREAM",
    );
    assert_eq!(
        headers.get("content-length").unwrap().to_str().unwrap(),
        total_size.to_string(),
    );

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.len() as u64, total_size);

    // Verify each fragment's content integrity
    for i in 0..fragment_count {
        let start = i * fragment_size;
        let end = start + fragment_size;
        assert!(
            body[start..end].iter().all(|&b| b == i as u8),
            "Fragment {} content mismatch",
            i
        );
    }
}

#[tokio::test]
async fn test_e2e_failover_to_second_node() {
    // Two node-runners: first has no content, second has it.
    let (url_empty, _s1, _d1) = start_node_runner().await;
    let (url_with_data, _s2, _d2) = start_node_runner().await;

    // Upload content only to the second node-runner.
    let data = b"failover payload";
    let cid = upload_to_node_runner(&url_with_data, "failover.txt", data).await;

    // Gateway configured with empty node first, then the populated one.
    let mut cfg = config::Config::default();
    cfg.p2p.node_runners = vec![url_empty.clone(), url_with_data.clone()];

    let state = AppState {
        storage: None,
        cache: Arc::new(cache::ContentCache::with_config(100, Duration::from_secs(300))),
        config: Arc::new(cfg),
        metrics: Arc::new(Metrics::default()),
        start_time: Instant::now(),
        deployments: Arc::new(std::sync::RwLock::new(Vec::new())),
        github_auth: Arc::new(std::sync::RwLock::new(None)),
        github_oauth_states: Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
        ipfs_circuit_breaker: Arc::new(circuit_breaker::CircuitBreaker::new(
            "ipfs-test", 5, Duration::from_secs(60),
        )),
        audit_logger: Arc::new(audit::AuditLogger::new(100)),
        redis: None,
        naming: Arc::new(std::sync::RwLock::new(protocol::NamingManager::new())),
        naming_key: Arc::new(libp2p::identity::Keypair::generate_ed25519()),
        p2p: None,
        node_health_tracker: None,
    };

    let app = gateway::content_router(state);
    let resp = gateway_get(app, &format!("/ipfs/{}", cid)).await;
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers().get("x-source").unwrap().to_str().unwrap(),
        "NODE",
    );

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&body[..], data);
}
