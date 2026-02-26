//! Integration tests exercising the CLI's NodeClient against a real
//! node-runner Axum server over HTTP.

use axum::Router;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

use node_runner::api;
use node_runner::config::NodeConfig;
use node_runner::metrics::MetricsCollector;
use node_runner::storage::StorageManager;
use node_runner::AppState;

use shadowmesh_cli::client::NodeClient;

// ── Test harness ────────────────────────────────────────────────

/// Spin up a real TCP server on a random port and return the base URL.
async fn start_test_server() -> (NodeClient, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let storage = Arc::new(
        StorageManager::new(dir.path().to_path_buf(), 10 * 1024 * 1024)
            .await
            .unwrap(),
    );
    let metrics = Arc::new(MetricsCollector::new());
    let (shutdown_tx, _) = broadcast::channel::<()>(1);

    let state = Arc::new(AppState {
        peer_id: "12D3KooWTestCLI".to_string(),
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
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });

    let base_url = format!("http://127.0.0.1:{}", port);
    (NodeClient::new(&base_url), dir)
}

/// Helper: create a small temp file for upload tests.
async fn write_temp_file(dir: &tempfile::TempDir, name: &str, data: &[u8]) -> std::path::PathBuf {
    let path = dir.path().join(name);
    tokio::fs::write(&path, data).await.unwrap();
    path
}

// ── Status & Health ─────────────────────────────────────────────

#[tokio::test]
async fn test_status() {
    let (client, _dir) = start_test_server().await;
    let val = client.get_status().await.unwrap();

    assert_eq!(val["peer_id"], "12D3KooWTestCLI");
    assert_eq!(val["status"], "running");
    assert!(val["version"].is_string());
    assert!(val["uptime_formatted"].is_string());
    assert!(val["connected_peers"].is_number());
    assert!(val["storage_used"].is_number());
    assert!(val["storage_capacity"].is_number());
    assert!(val["bandwidth_served"].is_string());
}

#[tokio::test]
async fn test_health() {
    let (client, _dir) = start_test_server().await;
    let val = client.get_health().await.unwrap();

    assert!(val["healthy"].is_boolean());
    assert!(val["checks"]["storage"].is_boolean());
    assert!(val["checks"]["network"].is_boolean());
    assert!(val["checks"]["api"].as_bool().unwrap());
}

#[tokio::test]
async fn test_ready() {
    let (client, _dir) = start_test_server().await;
    // Default config has enable_dht=true but no P2P → network check fails → 503.
    // The CLI client treats 503 as an error, so we verify the error message
    // contains the expected JSON body.
    let err = client.get_ready().await;
    assert!(err.is_err());
    let msg = err.unwrap_err().to_string();
    assert!(msg.contains("503"));
    assert!(msg.contains("healthy"));
}

// ── Peers & Metrics ─────────────────────────────────────────────

#[tokio::test]
async fn test_peers() {
    let (client, _dir) = start_test_server().await;
    let val = client.get_peers().await.unwrap();

    let peers = val.as_array().unwrap();
    // At minimum, self is returned
    assert!(!peers.is_empty());
    assert_eq!(peers[0]["peer_id"], "12D3KooWTestCLI");
}

#[tokio::test]
async fn test_metrics() {
    let (client, _dir) = start_test_server().await;
    let val = client.get_metrics().await.unwrap();

    assert!(val["uptime_formatted"].is_string());
    assert!(val["requests"]["total"].is_number());
    assert!(val["requests"]["successful"].is_number());
    assert!(val["requests"]["success_rate"].is_number());
    assert!(val["bandwidth"]["total_served"].is_number());
    assert!(val["network"]["connected_peers"].is_number());
    assert!(val["network"]["dht_records"].is_number());
}

// ── Config ──────────────────────────────────────────────────────

#[tokio::test]
async fn test_config() {
    let (client, _dir) = start_test_server().await;
    let val = client.get_config().await.unwrap();

    assert!(val["identity"]["name"].is_string());
    assert!(val["storage"]["max_storage_bytes"].is_number());
    assert!(val["network"]["max_peers"].is_number());
    assert!(val["dashboard"]["port"].is_number());
}

#[tokio::test]
async fn test_config_set() {
    let (client, _dir) = start_test_server().await;

    let body = serde_json::json!({ "node_name": "cli-test-node" });
    let val = client.update_config(body).await.unwrap();

    assert_eq!(val["identity"]["name"], "cli-test-node");
}

// ── Storage ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_storage_stats() {
    let (client, _dir) = start_test_server().await;
    let val = client.get_storage_stats().await.unwrap();

    assert!(val["total_bytes"].is_number());
    assert!(val["capacity_bytes"].is_number());
    assert!(val["fragment_count"].is_number());
    assert!(val["content_count"].is_number());
    assert!(val["pinned_count"].is_number());
}

#[tokio::test]
async fn test_upload_and_list() {
    let (client, dir) = start_test_server().await;

    let file = write_temp_file(&dir, "hello.txt", b"hello world").await;
    let upload = client.upload(&file).await.unwrap();
    assert!(upload["cid"].is_string());

    let list = client.list_content().await.unwrap();
    let items = list.as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["cid"], upload["cid"]);
}

#[tokio::test]
async fn test_upload_get_delete() {
    let (client, dir) = start_test_server().await;

    let file = write_temp_file(&dir, "data.bin", b"some data").await;
    let upload = client.upload(&file).await.unwrap();
    let cid = upload["cid"].as_str().unwrap();

    // Get content details
    let get = client.get_content(cid).await.unwrap();
    assert_eq!(get["cid"].as_str().unwrap(), cid);
    assert_eq!(get["total_size"].as_u64().unwrap(), 9);

    // Delete
    client.delete_content(cid).await.unwrap();

    // Now get should fail
    let err = client.get_content(cid).await;
    assert!(err.is_err());
}

#[tokio::test]
async fn test_upload_download() {
    let (client, dir) = start_test_server().await;
    let original = b"the quick brown fox jumps over the lazy dog";

    let file = write_temp_file(&dir, "fox.txt", original).await;
    let upload = client.upload(&file).await.unwrap();
    let cid = upload["cid"].as_str().unwrap();

    let (bytes, content_type, _name) = client.download(cid).await.unwrap();
    assert_eq!(bytes, original);
    assert!(content_type.is_some());
}

#[tokio::test]
async fn test_pin_unpin() {
    let (client, dir) = start_test_server().await;

    let file = write_temp_file(&dir, "pin.txt", b"pin me").await;
    let upload = client.upload(&file).await.unwrap();
    let cid = upload["cid"].as_str().unwrap();

    client.pin(cid).await.unwrap();

    let get = client.get_content(cid).await.unwrap();
    assert!(get["pinned"].as_bool().unwrap());

    client.unpin(cid).await.unwrap();

    let get2 = client.get_content(cid).await.unwrap();
    assert!(!get2["pinned"].as_bool().unwrap());
}

// ── GC ──────────────────────────────────────────────────────────

#[tokio::test]
async fn test_gc() {
    let (client, _dir) = start_test_server().await;
    let val = client.run_gc(1.0).await.unwrap();

    assert!(val["freed_bytes"].is_number());
    assert!(val["removed_fragments"].is_number());
    assert!(val["removed_content"].is_number());
}

// ── Bandwidth ───────────────────────────────────────────────────

#[tokio::test]
async fn test_bandwidth() {
    let (client, _dir) = start_test_server().await;
    let val = client.get_bandwidth().await.unwrap();

    assert!(val["inbound_bps"].is_number());
    assert!(val["outbound_bps"].is_number());
    assert!(val["total_inbound"].is_string());
    assert!(val["total_outbound"].is_string());
}

// ── Replication ─────────────────────────────────────────────────

#[tokio::test]
async fn test_replication_disabled() {
    let (client, _dir) = start_test_server().await;
    // No replication in test state → should return error
    let err = client.get_replication_health().await;
    assert!(err.is_err());
}

// ── Shutdown ────────────────────────────────────────────────────

#[tokio::test]
async fn test_shutdown() {
    let (client, _dir) = start_test_server().await;
    let val = client.shutdown().await.unwrap();

    assert!(val["message"].is_string());
    assert!(val["graceful"].as_bool().unwrap());
}
