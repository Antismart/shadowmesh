//! HTTP-level integration tests for the node-runner API.
//!
//! Uses `tower::ServiceExt::oneshot()` to fire requests through the Axum
//! router without spinning up a real TCP server.

use axum::body::Body;
use axum::Router;
use http_body_util::BodyExt;
use hyper::Request;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tower::ServiceExt;

use node_runner::api;
use node_runner::config::NodeConfig;
use node_runner::metrics::MetricsCollector;
use node_runner::storage::StorageManager;
use node_runner::AppState;

/// Build a test app with a minimal AppState (no P2P, no bridge, no replication).
async fn test_app() -> (Router, Arc<AppState>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let storage = Arc::new(
        StorageManager::new(dir.path().to_path_buf(), 10 * 1024 * 1024)
            .await
            .unwrap(),
    );
    let metrics = Arc::new(MetricsCollector::new());
    let (shutdown_tx, _) = broadcast::channel::<()>(1);

    let state = Arc::new(AppState {
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

    (app, state, dir)
}

/// Helper: GET request, parse JSON body.
async fn get_json(app: Router, uri: &str) -> (hyper::StatusCode, serde_json::Value) {
    let req = Request::builder()
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    (status, json)
}

// ── Status & Health ──────────────────────────────────────────────

#[tokio::test]
async fn test_get_status() {
    let (app, _, _dir) = test_app().await;
    let (status, json) = get_json(app, "/api/status").await;

    assert_eq!(status, 200);
    assert_eq!(json["peer_id"], "12D3KooWTestPeerId12345");
    assert_eq!(json["status"], "running");
    assert!(json["version"].is_string());
    assert!(json["uptime_secs"].is_number());
    assert!(json["uptime_formatted"].is_string());
    assert!(json["bandwidth_served"].is_string());
}

#[tokio::test]
async fn test_health_check() {
    let (app, _, _dir) = test_app().await;
    let (status, json) = get_json(app, "/api/health").await;

    assert_eq!(status, 200);
    // Storage is empty (< 95%) so storage_ok = true.
    // P2P is None but default config has enable_dht=true, so network_ok depends
    // on p2p.is_some() || !enable_dht. With p2p=None and enable_dht=true,
    // network_ok=false, so healthy=false.
    assert_eq!(json["checks"]["storage"], true);
    assert_eq!(json["checks"]["api"], true);
}

#[tokio::test]
async fn test_ready_check() {
    let (app, _, _dir) = test_app().await;
    let req = Request::builder()
        .uri("/api/ready")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();

    // With default config (enable_dht=true) and p2p=None, network_ok=false → 503
    assert_eq!(resp.status(), 503);
}

#[tokio::test]
async fn test_get_metrics() {
    let (app, _, _dir) = test_app().await;
    let (status, json) = get_json(app, "/api/metrics").await;

    assert_eq!(status, 200);
    assert!(json["requests"].is_object());
    assert!(json["bandwidth"].is_object());
    assert!(json["network"].is_object());
    assert!(json["uptime_secs"].is_number());
}

// ── Configuration ────────────────────────────────────────────────

#[tokio::test]
async fn test_get_config() {
    let (app, _, _dir) = test_app().await;
    let (status, json) = get_json(app, "/api/config").await;

    assert_eq!(status, 200);
    assert!(json["identity"].is_object());
    assert!(json["storage"].is_object());
    assert!(json["network"].is_object());
    assert!(json["dashboard"].is_object());
}

#[tokio::test]
async fn test_update_config() {
    let (app, state, _dir) = test_app().await;

    let req = Request::builder()
        .method("PUT")
        .uri("/api/config")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"node_name": "test-node", "max_peers": 25}"#))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 200);

    // Verify the config was updated
    let config = state.config.read().await;
    assert_eq!(config.identity.name, "test-node");
    assert_eq!(config.network.max_peers, 25);
}

#[tokio::test]
async fn test_update_config_invalid() {
    let (app, _, _dir) = test_app().await;

    let req = Request::builder()
        .method("PUT")
        .uri("/api/config")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"max_peers": 0}"#))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 400);
}

// ── Storage ──────────────────────────────────────────────────────

#[tokio::test]
async fn test_get_storage_stats() {
    let (app, _, _dir) = test_app().await;
    let (status, json) = get_json(app, "/api/storage").await;

    assert_eq!(status, 200);
    assert_eq!(json["total_bytes"], 0);
    assert_eq!(json["fragment_count"], 0);
    assert_eq!(json["content_count"], 0);
    assert!(json["capacity_bytes"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn test_list_content_empty() {
    let (app, _, _dir) = test_app().await;
    let (status, json) = get_json(app, "/api/storage/content").await;

    assert_eq!(status, 200);
    assert!(json.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_upload_and_retrieve_content() {
    let (app, state, _dir) = test_app().await;

    // Upload a file via multipart
    let boundary = "----TestBoundary12345";
    let file_content = "Hello ShadowMesh!";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test.txt\"\r\nContent-Type: text/plain\r\n\r\n{file_content}\r\n--{boundary}--\r\n"
    );

    let req = Request::builder()
        .method("POST")
        .uri("/api/storage/upload")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 200);

    let resp_body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&resp_body).unwrap();

    assert_eq!(json["success"], true);
    assert_eq!(json["name"], "test.txt");
    assert_eq!(json["size"], file_content.len() as u64);
    let cid = json["cid"].as_str().unwrap().to_string();
    assert!(!cid.is_empty());

    // Retrieve content metadata
    let content = state.storage.get_content(&cid).await;
    assert!(content.is_some());
    let content = content.unwrap();
    assert_eq!(content.name, "test.txt");
    assert_eq!(content.total_size, file_content.len() as u64);
}

#[tokio::test]
async fn test_get_content_by_cid() {
    let (app, state, _dir) = test_app().await;

    // Store content directly
    let data = b"test data for retrieval";
    let cid = blake3::hash(data).to_hex().to_string();
    state
        .storage
        .store_fragment(&cid, &cid, 0, data)
        .await
        .unwrap();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    state
        .storage
        .store_content(node_runner::storage::StoredContent {
            cid: cid.clone(),
            name: "test.bin".to_string(),
            total_size: data.len() as u64,
            fragment_count: 1,
            fragments: vec![cid.clone()],
            stored_at: now,
            pinned: false,
            mime_type: "application/octet-stream".to_string(),
        })
        .await
        .unwrap();

    let (status, json) = get_json(app, &format!("/api/storage/content/{cid}")).await;
    assert_eq!(status, 200);
    assert_eq!(json["cid"], cid);
    assert_eq!(json["name"], "test.bin");
}

#[tokio::test]
async fn test_get_content_not_found() {
    let (app, _, _dir) = test_app().await;
    let req = Request::builder()
        .uri("/api/storage/content/nonexistent")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_pin_and_unpin_content() {
    let (app, state, _dir) = test_app().await;

    // Store content
    let data = b"pinnable content";
    let cid = blake3::hash(data).to_hex().to_string();
    state
        .storage
        .store_fragment(&cid, &cid, 0, data)
        .await
        .unwrap();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    state
        .storage
        .store_content(node_runner::storage::StoredContent {
            cid: cid.clone(),
            name: "pin-test.bin".to_string(),
            total_size: data.len() as u64,
            fragment_count: 1,
            fragments: vec![cid.clone()],
            stored_at: now,
            pinned: false,
            mime_type: "application/octet-stream".to_string(),
        })
        .await
        .unwrap();

    // Pin
    let req = Request::builder()
        .method("POST")
        .uri(&format!("/api/storage/pin/{cid}"))
        .body(Body::empty())
        .unwrap();
    // Clone app for multiple requests
    let app2 = Router::new()
        .nest("/api", api::api_routes())
        .with_state(state.clone());
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 200);

    // Verify pinned
    let content = state.storage.get_content(&cid).await.unwrap();
    assert!(content.pinned);

    // Unpin
    let req = Request::builder()
        .method("POST")
        .uri(&format!("/api/storage/unpin/{cid}"))
        .body(Body::empty())
        .unwrap();
    let resp = app2.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 200);

    // Verify unpinned
    let content = state.storage.get_content(&cid).await.unwrap();
    assert!(!content.pinned);
}

#[tokio::test]
async fn test_delete_content() {
    let (app, state, _dir) = test_app().await;

    // Store content
    let data = b"deletable content";
    let cid = blake3::hash(data).to_hex().to_string();
    state
        .storage
        .store_fragment(&cid, &cid, 0, data)
        .await
        .unwrap();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    state
        .storage
        .store_content(node_runner::storage::StoredContent {
            cid: cid.clone(),
            name: "delete-test.bin".to_string(),
            total_size: data.len() as u64,
            fragment_count: 1,
            fragments: vec![cid.clone()],
            stored_at: now,
            pinned: false,
            mime_type: "application/octet-stream".to_string(),
        })
        .await
        .unwrap();

    let req = Request::builder()
        .method("DELETE")
        .uri(&format!("/api/storage/content/{cid}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 204);

    // Verify deleted
    assert!(state.storage.get_content(&cid).await.is_none());
}

// ── Network ──────────────────────────────────────────────────────

#[tokio::test]
async fn test_get_peers() {
    let (app, _, _dir) = test_app().await;
    let (status, json) = get_json(app, "/api/network/peers").await;

    assert_eq!(status, 200);
    let peers = json.as_array().unwrap();
    // With p2p=None, only self entry
    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0]["peer_id"], "12D3KooWTestPeerId12345");
    assert_eq!(peers[0]["connected"], true);
    assert_eq!(peers[0]["latency_ms"], 0);
}

#[tokio::test]
async fn test_get_bandwidth() {
    let (app, _, _dir) = test_app().await;
    let (status, json) = get_json(app, "/api/network/bandwidth").await;

    assert_eq!(status, 200);
    assert!(json["inbound_bps"].is_number());
    assert!(json["outbound_bps"].is_number());
    assert!(json["total_inbound"].is_string());
    assert!(json["total_outbound"].is_string());
}

// ── P2P-dependent endpoints ──────────────────────────────────────

#[tokio::test]
async fn test_fetch_remote_without_p2p() {
    let (app, _, _dir) = test_app().await;

    let req = Request::builder()
        .method("POST")
        .uri("/api/storage/fetch/somecid123")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 503);
}

#[tokio::test]
async fn test_replication_health_disabled() {
    let (app, _, _dir) = test_app().await;

    let req = Request::builder()
        .uri("/api/replication/health")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 503);
}

// ── Node Control ─────────────────────────────────────────────────

#[tokio::test]
async fn test_shutdown() {
    let (app, _, _dir) = test_app().await;

    let req = Request::builder()
        .method("POST")
        .uri("/api/node/shutdown")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["graceful"], true);
}

// ── Metrics History ──────────────────────────────────────────────

#[tokio::test]
async fn test_metrics_history() {
    let (app, _, _dir) = test_app().await;
    let (status, json) = get_json(app, "/api/metrics/history").await;

    assert_eq!(status, 200);
    assert!(json.as_array().is_some());
}

// ── Prometheus Metrics ───────────────────────────────────────

#[tokio::test]
async fn test_prometheus_metrics() {
    let (app, _, _dir) = test_app().await;

    let req = Request::builder()
        .uri("/api/metrics/prometheus")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 200);

    // Check content-type header
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.starts_with("text/plain"));
    assert!(ct.contains("version=0.0.4"));

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let text = std::str::from_utf8(&body).unwrap();

    // Verify key metrics are present
    assert!(text.contains("shadowmesh_uptime_seconds"));
    assert!(text.contains("shadowmesh_requests_total"));
    assert!(text.contains("shadowmesh_bandwidth_served_bytes_total"));
    assert!(text.contains("shadowmesh_connected_peers"));
    assert!(text.contains("shadowmesh_storage_used_bytes"));
    assert!(text.contains("shadowmesh_storage_capacity_bytes"));
    assert!(text.contains("shadowmesh_storage_usage_ratio"));

    // Verify Prometheus format (TYPE lines)
    assert!(text.contains("# TYPE shadowmesh_uptime_seconds gauge"));
    assert!(text.contains("# TYPE shadowmesh_requests_total counter"));
    assert!(text.contains("# TYPE shadowmesh_bandwidth_served_bytes_total counter"));
    assert!(text.contains("# TYPE shadowmesh_storage_used_bytes gauge"));

    // Replication metrics should NOT be present (replication=None)
    assert!(!text.contains("shadowmesh_replication_total"));
}

// ── Raw Download ─────────────────────────────────────────────────

#[tokio::test]
async fn test_download_content() {
    let (app, state, _dir) = test_app().await;

    // Upload a file via multipart
    let boundary = "----DownloadTestBoundary";
    let file_content = "Hello raw download!";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"raw.txt\"\r\nContent-Type: text/plain\r\n\r\n{file_content}\r\n--{boundary}--\r\n"
    );

    let req = Request::builder()
        .method("POST")
        .uri("/api/storage/upload")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 200);

    let resp_body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&resp_body).unwrap();
    let cid = json["cid"].as_str().unwrap().to_string();

    // Download raw content
    let app2 = Router::new()
        .nest("/api", api::api_routes())
        .with_state(state.clone());

    let req = Request::builder()
        .uri(&format!("/api/storage/download/{cid}"))
        .body(Body::empty())
        .unwrap();

    let resp = app2.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 200);

    // Verify Content-Type is set
    let ct = resp.headers().get("content-type").unwrap().to_str().unwrap();
    assert!(!ct.is_empty());

    // Verify x-content-cid header
    let x_cid = resp
        .headers()
        .get("x-content-cid")
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(x_cid, cid);

    // Verify raw bytes match original content
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.as_ref(), file_content.as_bytes());
}

#[tokio::test]
async fn test_download_content_not_found() {
    let (app, _, _dir) = test_app().await;

    let req = Request::builder()
        .uri("/api/storage/download/nonexistent_cid")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_download_large_content_streams() {
    let (app, state, _dir) = test_app().await;

    // Create content above STREAM_THRESHOLD_BYTES (5 MB) using 6 x 1MB fragments
    let fragment_size = 1024 * 1024; // 1 MB
    let fragment_count = 6usize;
    let mut fragment_hashes = Vec::new();

    for i in 0..fragment_count {
        let data = vec![i as u8; fragment_size];
        let hash = format!("largefrag{:04x}", i);
        state
            .storage
            .store_fragment(&hash, "largecid", i as u32, &data)
            .await
            .unwrap();
        fragment_hashes.push(hash);
    }

    let total_size = (fragment_size * fragment_count) as u64;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    state
        .storage
        .store_content(node_runner::storage::StoredContent {
            cid: "largecid".to_string(),
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

    let req = Request::builder()
        .uri("/api/storage/download/largecid")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 200);

    // Verify Content-Length header
    let cl = resp
        .headers()
        .get("content-length")
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(cl, total_size.to_string());

    // Verify body length and content integrity
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.len() as u64, total_size);

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
async fn test_download_small_content_has_content_length() {
    let (app, state, _dir) = test_app().await;

    let data = b"small content for header test";
    let cid = blake3::hash(data).to_hex().to_string();
    state
        .storage
        .store_fragment(&cid, &cid, 0, data)
        .await
        .unwrap();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    state
        .storage
        .store_content(node_runner::storage::StoredContent {
            cid: cid.clone(),
            name: "small.bin".to_string(),
            total_size: data.len() as u64,
            fragment_count: 1,
            fragments: vec![cid.clone()],
            stored_at: now,
            pinned: false,
            mime_type: "application/octet-stream".to_string(),
        })
        .await
        .unwrap();

    let req = Request::builder()
        .uri(&format!("/api/storage/download/{cid}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 200);

    let cl = resp
        .headers()
        .get("content-length")
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(cl, data.len().to_string());

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.as_ref(), data);
}

// ── Garbage Collection ───────────────────────────────────────────

#[tokio::test]
async fn test_garbage_collection() {
    let (app, _, _dir) = test_app().await;

    let req = Request::builder()
        .method("POST")
        .uri("/api/storage/gc")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"target_free_gb": 1.0}"#))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["freed_bytes"], 0);
    assert_eq!(json["removed_fragments"], 0);
    assert_eq!(json["removed_content"], 0);
}
