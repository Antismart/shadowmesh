//! ShadowMesh Node Runner
//!
//! A complete node implementation for the ShadowMesh decentralized CDN.

use axum::{response::Html, routing::get, Router};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, RwLock};
use tower_http::cors::{Any, CorsLayer};

mod api;
mod config;
mod dashboard;
mod metrics;
mod p2p;
mod storage;

use config::NodeConfig;
use metrics::MetricsCollector;
use storage::StorageManager;

/// Shared application state
pub struct AppState {
    /// Node peer ID
    pub peer_id: String,
    /// Node configuration
    pub config: RwLock<NodeConfig>,
    /// Metrics collector
    pub metrics: Arc<MetricsCollector>,
    /// Storage manager
    pub storage: Arc<StorageManager>,
    /// Shutdown signal sender
    pub shutdown_signal: broadcast::Sender<()>,
    /// P2P state (None if P2P failed to initialize)
    pub p2p: Option<Arc<p2p::P2pState>>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize structured logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    println!("🌐 Starting ShadowMesh Node Runner...");
    println!();

    // Load configuration
    let config = match NodeConfig::load() {
        Ok(cfg) => {
            println!("✅ Configuration loaded");
            cfg
        }
        Err(e) => {
            println!("⚠️  Failed to load config: {}", e);
            println!("   Using default configuration");
            NodeConfig::default()
        }
    };

    // Validate configuration
    if let Err(errors) = config.validate() {
        eprintln!("❌ Configuration validation failed:");
        for err in errors {
            eprintln!("   - {}", err);
        }
        return Err("Invalid configuration".into());
    }

    config.print_summary();
    println!();

    // Create data directories
    let data_dir = config.storage.data_dir.clone();
    if let Err(e) = tokio::fs::create_dir_all(&data_dir).await {
        eprintln!("❌ Failed to create data directory: {}", e);
        return Err(e.into());
    }

    // Initialize storage manager
    let storage =
        match StorageManager::new(data_dir.clone(), config.storage.max_storage_bytes).await {
            Ok(s) => {
                let stats = s.get_stats().await;
                println!("✅ Storage initialized");
                println!(
                    "   📦 {} fragments, {} used",
                    stats.fragment_count,
                    metrics::BandwidthStats::format_bytes(stats.total_bytes)
                );
                Arc::new(s)
            }
            Err(e) => {
                eprintln!("❌ Failed to initialize storage: {}", e);
                return Err(e.into());
            }
        };

    // Initialize metrics collector
    let metrics = Arc::new(MetricsCollector::new());

    // Start background metrics recording
    metrics.clone().start_background_recording();

    // Create shutdown signal
    let (shutdown_tx, _shutdown_rx) = broadcast::channel::<()>(1);

    // Initialize the ShadowMesh protocol node and P2P event loop
    let (peer_id, p2p_state) = match shadowmesh_protocol::ShadowNode::new().await {
        Ok(mut node) => {
            let peer_id = node.peer_id().to_string();
            println!("✅ P2P node initialized");
            println!("   🆔 Peer ID: {}", &peer_id[..20]);

            // Start listening on configured addresses
            if let Err(e) = node.start().await {
                eprintln!("❌ Failed to bind P2P addresses: {}", e);
                (peer_id, None)
            } else {
                // Create shared P2P state and spawn the event loop
                let p2p_state = Arc::new(p2p::P2pState::new());
                let shutdown_rx = shutdown_tx.subscribe();

                let loop_state = p2p_state.clone();
                let loop_metrics = metrics.clone();
                tokio::spawn(async move {
                    p2p::run_event_loop(node, loop_state, loop_metrics, shutdown_rx).await;
                });

                println!("✅ P2P event loop started");
                (peer_id, Some(p2p_state))
            }
        }
        Err(e) => {
            println!("⚠️  Failed to initialize P2P node: {}", e);
            println!("   Running in dashboard-only mode");
            ("offline".to_string(), None)
        }
    };

    println!();

    // Create shared state
    let state = Arc::new(AppState {
        peer_id,
        config: RwLock::new(config.clone()),
        metrics,
        storage,
        shutdown_signal: shutdown_tx,
        p2p: p2p_state,
    });

    // Build CORS layer
    let cors = if config.dashboard.enable_cors {
        if config.dashboard.cors_origins.is_empty() {
            // No origins configured — deny all cross-origin requests
            CorsLayer::new()
        } else {
            let origins: Vec<_> = config
                .dashboard
                .cors_origins
                .iter()
                .filter_map(|o| o.parse().ok())
                .collect();
            CorsLayer::new()
                .allow_origin(origins)
                .allow_methods(Any)
                .allow_headers(Any)
        }
    } else {
        CorsLayer::new()
    };

    // Build the router
    let app = Router::new()
        // Dashboard
        .route("/", get(serve_dashboard))
        // API routes
        .nest("/api", api::api_routes())
        // Add state and middleware
        .with_state(state.clone())
        .layer(cors);

    // Bind to address
    let bind_addr = config.dashboard.bind_address();
    let listener = TcpListener::bind(&bind_addr).await?;

    println!("🚀 Node dashboard running at http://{}", bind_addr);
    println!("📊 API available at http://{}/api/status", bind_addr);
    println!();
    println!("Press Ctrl+C to stop the node");
    println!();

    // Run server with graceful shutdown
    let mut shutdown_rx = state.shutdown_signal.subscribe();

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    println!();
                    println!("🛑 Shutting down...");
                }
                _ = shutdown_rx.recv() => {
                    println!();
                    println!("🛑 Shutdown signal received...");
                }
            }
        })
        .await?;

    println!("✅ Node stopped gracefully");
    Ok(())
}

/// Serve the dashboard HTML
async fn serve_dashboard() -> Html<&'static str> {
    Html(include_str!("../assets/dashboard.html"))
}
