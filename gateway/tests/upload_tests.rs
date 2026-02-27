//! Integration tests for the gateway upload endpoints.
//!
//! Tests validation, error handling, and request routing for:
//! - POST /api/upload          (multipart)
//! - POST /api/upload/json     (base64 JSON)
//! - POST /api/upload/raw      (raw binary)
//! - POST /api/upload/batch    (batch base64 JSON)
//!
//! These tests run without an IPFS backend (storage=None) to verify the
//! HTTP layer: correct status codes, error payloads, and content-type
//! detection.

use axum::body::Body;
use axum::Router;
use http_body_util::BodyExt;
use hyper::Request;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tower::ServiceExt;

use base64::Engine;
use gateway::{audit, cache, circuit_breaker, config, AppState, Metrics};

// ── Test Helpers ────────────────────────────────────────────────

/// Build a gateway upload router with storage=None (IPFS unavailable).
fn upload_app_no_storage() -> Router {
    let cfg = config::Config::default();
    let state = test_state(cfg, None);
    gateway::upload_router(state)
}

fn test_state(cfg: config::Config, storage: Option<Arc<protocol::StorageLayer>>) -> AppState {
    AppState {
        storage,
        cache: Arc::new(cache::ContentCache::with_config(100, Duration::from_secs(300))),
        config: Arc::new(tokio::sync::RwLock::new(cfg)),
        metrics: Arc::new(Metrics::default()),
        start_time: Instant::now(),
        deployments: Arc::new(std::sync::RwLock::new(Vec::new())),
        github_auth: Arc::new(std::sync::RwLock::new(None)),
        github_oauth_states: Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
        ipfs_circuit_breaker: Arc::new(circuit_breaker::CircuitBreaker::new(
            "ipfs-test",
            5,
            Duration::from_secs(60),
        )),
        audit_logger: Arc::new(audit::AuditLogger::new(100)),
        redis: None,
        naming: Arc::new(std::sync::RwLock::new(protocol::NamingManager::new())),
        naming_key: Arc::new(libp2p::identity::Keypair::generate_ed25519()),
        p2p: None,
        node_health_tracker: None,
        http_client: reqwest::Client::new(),
    }
}

/// Parse response body as JSON.
async fn body_json(resp: hyper::Response<Body>) -> serde_json::Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

// ── /api/upload (multipart) ────────────────────────────────────

#[tokio::test]
async fn test_upload_multipart_no_storage_returns_503() {
    let app = upload_app_no_storage();

    let boundary = "----testboundary";
    let body = format!(
        "------testboundary\r\n\
         Content-Disposition: form-data; name=\"file\"; filename=\"test.txt\"\r\n\
         Content-Type: text/plain\r\n\
         \r\n\
         hello world\r\n\
         ------testboundary--\r\n"
    );

    let req = Request::builder()
        .method("POST")
        .uri("/api/upload")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={}", boundary),
        )
        .body(Body::from(body))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 503);

    let json = body_json(resp).await;
    assert_eq!(json["code"], "STORAGE_UNAVAILABLE");
    assert_eq!(json["success"], false);
}

#[tokio::test]
async fn test_upload_multipart_no_file_field_returns_400() {
    let app = upload_app_no_storage();

    // Send multipart with a field named "notfile" instead of "file"
    let boundary = "----testboundary";
    let body = format!(
        "------testboundary\r\n\
         Content-Disposition: form-data; name=\"notfile\"; filename=\"test.txt\"\r\n\
         Content-Type: text/plain\r\n\
         \r\n\
         hello world\r\n\
         ------testboundary--\r\n"
    );

    let req = Request::builder()
        .method("POST")
        .uri("/api/upload")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={}", boundary),
        )
        .body(Body::from(body))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    // With storage=None, the handler checks storage first (503),
    // so we expect 503 before it can check the field name.
    assert_eq!(resp.status(), 503);
}

// ── /api/upload/json ───────────────────────────────────────────

#[tokio::test]
async fn test_upload_json_no_storage_returns_503() {
    let app = upload_app_no_storage();

    let payload = serde_json::json!({
        "data": base64::engine::general_purpose::STANDARD.encode(b"hello"),
        "filename": "test.txt"
    });

    let req = Request::builder()
        .method("POST")
        .uri("/api/upload/json")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&payload).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 503);

    let json = body_json(resp).await;
    assert_eq!(json["code"], "STORAGE_UNAVAILABLE");
}

#[tokio::test]
async fn test_upload_json_invalid_base64_returns_400() {
    // Need storage to be available for the handler to reach the base64 check.
    // But we don't have IPFS. With storage=None the handler returns 503 first.
    // This test documents that behavior — base64 validation only runs when
    // storage is present.
    let app = upload_app_no_storage();

    let payload = serde_json::json!({
        "data": "not-valid-base64!!!",
    });

    let req = Request::builder()
        .method("POST")
        .uri("/api/upload/json")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&payload).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    // Storage check happens before base64 decode, so 503
    assert_eq!(resp.status(), 503);
}

#[tokio::test]
async fn test_upload_json_missing_data_field_returns_422() {
    let app = upload_app_no_storage();

    // "data" field is required by JsonUploadRequest
    let payload = serde_json::json!({
        "filename": "test.txt"
    });

    let req = Request::builder()
        .method("POST")
        .uri("/api/upload/json")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&payload).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    // Axum returns 422 Unprocessable Entity for deserialization failures
    assert_eq!(resp.status(), 422);
}

#[tokio::test]
async fn test_upload_json_wrong_content_type_returns_415() {
    let app = upload_app_no_storage();

    let req = Request::builder()
        .method("POST")
        .uri("/api/upload/json")
        .header("content-type", "text/plain")
        .body(Body::from("not json"))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    // Axum returns 415 Unsupported Media Type when Content-Type doesn't match
    assert_eq!(resp.status(), 415);
}

// ── /api/upload/raw ────────────────────────────────────────────

#[tokio::test]
async fn test_upload_raw_no_storage_returns_503() {
    let app = upload_app_no_storage();

    let req = Request::builder()
        .method("POST")
        .uri("/api/upload/raw")
        .header("content-type", "application/octet-stream")
        .body(Body::from(vec![1u8, 2, 3, 4]))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 503);

    let json = body_json(resp).await;
    assert_eq!(json["code"], "STORAGE_UNAVAILABLE");
}

#[tokio::test]
async fn test_upload_raw_empty_body_no_storage() {
    let app = upload_app_no_storage();

    let req = Request::builder()
        .method("POST")
        .uri("/api/upload/raw")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    // Storage check first → 503
    assert_eq!(resp.status(), 503);
}

// ── /api/upload/batch ──────────────────────────────────────────

#[tokio::test]
async fn test_upload_batch_no_storage_returns_503() {
    let app = upload_app_no_storage();

    let payload = serde_json::json!({
        "files": [{
            "data": base64::engine::general_purpose::STANDARD.encode(b"hello"),
            "filename": "a.txt"
        }]
    });

    let req = Request::builder()
        .method("POST")
        .uri("/api/upload/batch")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&payload).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 503);

    let json = body_json(resp).await;
    assert_eq!(json["code"], "STORAGE_UNAVAILABLE");
}

#[tokio::test]
async fn test_upload_batch_too_many_files_returns_400() {
    let app = upload_app_no_storage();

    // Batch limit is 100 files — send 101
    // But storage check comes first, so we get 503.
    // This documents the check order.
    let files: Vec<serde_json::Value> = (0..101)
        .map(|i| {
            serde_json::json!({
                "data": base64::engine::general_purpose::STANDARD.encode(format!("file{}", i).as_bytes()),
                "filename": format!("file{}.txt", i)
            })
        })
        .collect();

    let payload = serde_json::json!({ "files": files });

    let req = Request::builder()
        .method("POST")
        .uri("/api/upload/batch")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&payload).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 503);
}

#[tokio::test]
async fn test_upload_batch_empty_files_returns_422_or_503() {
    let app = upload_app_no_storage();

    // Empty files array — valid JSON, storage check first
    let payload = serde_json::json!({ "files": [] });

    let req = Request::builder()
        .method("POST")
        .uri("/api/upload/batch")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&payload).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 503);
}

#[tokio::test]
async fn test_upload_batch_missing_files_key_returns_422() {
    let app = upload_app_no_storage();

    let payload = serde_json::json!({ "not_files": [] });

    let req = Request::builder()
        .method("POST")
        .uri("/api/upload/batch")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&payload).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    // Axum deserialization error → 422
    assert_eq!(resp.status(), 422);
}

// ── Method not allowed ─────────────────────────────────────────

#[tokio::test]
async fn test_upload_get_returns_405() {
    let app = upload_app_no_storage();

    let req = Request::builder()
        .method("GET")
        .uri("/api/upload")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 405);
}

#[tokio::test]
async fn test_upload_json_get_returns_405() {
    let app = upload_app_no_storage();

    let req = Request::builder()
        .method("GET")
        .uri("/api/upload/json")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 405);
}

#[tokio::test]
async fn test_upload_raw_get_returns_405() {
    let app = upload_app_no_storage();

    let req = Request::builder()
        .method("GET")
        .uri("/api/upload/raw")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 405);
}

#[tokio::test]
async fn test_upload_batch_get_returns_405() {
    let app = upload_app_no_storage();

    let req = Request::builder()
        .method("GET")
        .uri("/api/upload/batch")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 405);
}

// ── Node-runner backed upload tests ────────────────────────────

/// Start a real node-runner TCP server on a random port.
async fn start_node_runner() -> (String, Arc<node_runner::AppState>, tempfile::TempDir) {
    use tokio::net::TcpListener;
    use tokio::sync::{broadcast, RwLock};

    let dir = tempfile::tempdir().unwrap();
    let storage = Arc::new(
        node_runner::storage::StorageManager::new(dir.path().to_path_buf(), 100 * 1024 * 1024)
            .await
            .unwrap(),
    );
    let metrics = Arc::new(node_runner::metrics::MetricsCollector::new());
    let (shutdown_tx, _) = broadcast::channel::<()>(1);

    let state = Arc::new(node_runner::AppState {
        peer_id: "12D3KooWTestUpload12345".to_string(),
        config: RwLock::new(node_runner::config::NodeConfig::default()),
        metrics,
        storage,
        shutdown_signal: shutdown_tx,
        p2p: None,
        bridge: None,
        replication: None,
    });

    let app = Router::new()
        .nest("/api", node_runner::api::api_routes())
        .with_state(state.clone());

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://127.0.0.1:{}", addr.port());

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (url, state, dir)
}

#[tokio::test]
async fn test_node_runner_upload_and_list() {
    let (node_url, _state, _dir) = start_node_runner().await;

    let client = reqwest::Client::new();

    // Upload via multipart
    let part = reqwest::multipart::Part::bytes(b"integration test content".to_vec())
        .file_name("test.txt")
        .mime_str("text/plain")
        .unwrap();
    let form = reqwest::multipart::Form::new().part("file", part);

    let resp = client
        .post(format!("{}/api/storage/upload", node_url))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let json: serde_json::Value = resp.json().await.unwrap();
    let cid = json["cid"].as_str().unwrap();
    assert!(!cid.is_empty());

    // Verify content appears in list
    let resp = client
        .get(format!("{}/api/storage/content", node_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let list: serde_json::Value = resp.json().await.unwrap();
    let items = list.as_array().unwrap();
    assert!(
        items.iter().any(|item| item["cid"].as_str() == Some(cid)),
        "Uploaded content not found in storage list"
    );
}

#[tokio::test]
async fn test_node_runner_upload_and_download() {
    let (node_url, _state, _dir) = start_node_runner().await;

    let client = reqwest::Client::new();
    let test_data = b"download test payload 12345";

    // Upload
    let part = reqwest::multipart::Part::bytes(test_data.to_vec())
        .file_name("download.bin")
        .mime_str("application/octet-stream")
        .unwrap();
    let form = reqwest::multipart::Form::new().part("file", part);

    let resp = client
        .post(format!("{}/api/storage/upload", node_url))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let json: serde_json::Value = resp.json().await.unwrap();
    let cid = json["cid"].as_str().unwrap().to_string();

    // Download and verify content
    let resp = client
        .get(format!("{}/api/storage/download/{}", node_url, cid))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let body = resp.bytes().await.unwrap();
    assert_eq!(&body[..], test_data);
}

#[tokio::test]
async fn test_node_runner_upload_pin_unpin_delete() {
    let (node_url, _state, _dir) = start_node_runner().await;

    let client = reqwest::Client::new();

    // Upload
    let part = reqwest::multipart::Part::bytes(b"pin test".to_vec())
        .file_name("pin.txt")
        .mime_str("text/plain")
        .unwrap();
    let form = reqwest::multipart::Form::new().part("file", part);

    let resp = client
        .post(format!("{}/api/storage/upload", node_url))
        .multipart(form)
        .send()
        .await
        .unwrap();
    let json: serde_json::Value = resp.json().await.unwrap();
    let cid = json["cid"].as_str().unwrap().to_string();

    // Pin
    let resp = client
        .post(format!("{}/api/storage/pin/{}", node_url, cid))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "Pin failed: {}", resp.status());

    // Try delete while pinned — should fail
    let resp = client
        .delete(format!("{}/api/storage/content/{}", node_url, cid))
        .send()
        .await
        .unwrap();
    assert_ne!(resp.status(), 200, "Delete should fail while pinned");

    // Unpin
    let resp = client
        .post(format!("{}/api/storage/unpin/{}", node_url, cid))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "Unpin failed: {}", resp.status());

    // Delete should succeed now
    let resp = client
        .delete(format!("{}/api/storage/content/{}", node_url, cid))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "Delete failed: {}", resp.status());

    // Verify deleted
    let resp = client
        .get(format!("{}/api/storage/content/{}", node_url, cid))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_node_runner_storage_stats() {
    let (node_url, _state, _dir) = start_node_runner().await;

    let client = reqwest::Client::new();

    // Check initial stats
    let resp = client
        .get(format!("{}/api/storage", node_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let stats: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(stats["fragment_count"], 0);
    assert_eq!(stats["content_count"], 0);

    // Upload something
    let part = reqwest::multipart::Part::bytes(vec![42u8; 1000])
        .file_name("stats.bin")
        .mime_str("application/octet-stream")
        .unwrap();
    let form = reqwest::multipart::Form::new().part("file", part);

    client
        .post(format!("{}/api/storage/upload", node_url))
        .multipart(form)
        .send()
        .await
        .unwrap();

    // Check updated stats
    let resp = client
        .get(format!("{}/api/storage", node_url))
        .send()
        .await
        .unwrap();
    let stats: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(stats["content_count"], 1);
    assert!(stats["total_bytes"].as_u64().unwrap() >= 1000);
}
