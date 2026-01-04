use axum::{
    routing::{get, post},
    Router, Json,
    response::Html,
};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use std::sync::Arc;
use tokio::sync::RwLock;

mod dashboard;

#[derive(Debug, Clone, Serialize)]
pub struct NodeStatus {
    peer_id: String,
    uptime: u64,
    bandwidth_served: u64,
    fragments_stored: u32,
    earnings: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NodeConfig {
    max_storage: u64,
    max_bandwidth: u64,
}

struct AppState {
    start_time: std::time::Instant,
    config: RwLock<NodeConfig>,
    bandwidth_served: RwLock<u64>,
    fragments_stored: RwLock<u32>,
}

#[tokio::main]
async fn main() {
    println!("🌐 Starting ShadowMesh Node Runner...");

    // Initialize shared state
    let state = Arc::new(AppState {
        start_time: std::time::Instant::now(),
        config: RwLock::new(NodeConfig {
            max_storage: 10 * 1024 * 1024 * 1024, // 10 GB
            max_bandwidth: 100 * 1024 * 1024,     // 100 MB/s
        }),
        bandwidth_served: RwLock::new(0),
        fragments_stored: RwLock::new(0),
    });

    // Initialize the ShadowMesh protocol node
    let node = match shadowmesh_protocol::ShadowNode::new().await {
        Ok(node) => {
            println!("✅ Protocol node initialized: {:?}", node.peer_id());
            Some(node)
        }
        Err(e) => {
            println!("⚠️  Failed to initialize protocol node: {}", e);
            println!("   Running in dashboard-only mode");
            None
        }
    };
    
    // Start P2P networking in background if node initialized
    if let Some(mut node) = node {
        tokio::spawn(async move {
            if let Err(e) = node.start().await {
                eprintln!("P2P network error: {}", e);
            }
        });
    }
    
    // Build the web dashboard router
    let app = Router::new()
        .route("/", get(dashboard_handler))
        .route("/api/status", get({
            let state = Arc::clone(&state);
            move || status_handler(state)
        }))
        .route("/api/config", get({
            let state = Arc::clone(&state);
            move || get_config_handler(state)
        }))
        .route("/api/config", post({
            let state = Arc::clone(&state);
            move |body| config_handler(state, body)
        }));
    
    let listener = TcpListener::bind("127.0.0.1:3030")
        .await
        .expect("Failed to bind to port 3030");
    
    println!("🚀 Node dashboard running at http://localhost:3030");
    println!("📊 API available at http://localhost:3030/api/status");
    
    axum::serve(listener, app).await.unwrap();
}

async fn dashboard_handler() -> Html<&'static str> {
    Html(include_str!("../assets/dashboard.html"))
}

async fn status_handler(state: Arc<AppState>) -> Json<NodeStatus> {
    let uptime = state.start_time.elapsed().as_secs();
    let bandwidth = *state.bandwidth_served.read().await;
    let fragments = *state.fragments_stored.read().await;

    Json(NodeStatus {
        peer_id: "12D3KooW...".to_string(), // TODO: Get real peer ID
        uptime,
        bandwidth_served: bandwidth,
        fragments_stored: fragments,
        earnings: 0.0, // Future: actual token earnings
    })
}

async fn get_config_handler(state: Arc<AppState>) -> Json<NodeConfig> {
    let config = state.config.read().await;
    Json(config.clone())
}

async fn config_handler(state: Arc<AppState>, Json(new_config): Json<NodeConfig>) -> Json<NodeConfig> {
    let mut config = state.config.write().await;
    *config = new_config.clone();
    println!("📝 Configuration updated: {:?}", new_config);
    Json(new_config)
}
