//! ShadowMesh Node Runner

use axum::{response::Html, routing::get, Router};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, RwLock};
use tower_http::cors::{Any, CorsLayer};

use node_runner::*;
use node_runner::{api, bridge, config::NodeConfig, metrics, metrics::MetricsCollector, p2p, p2p_commands, replication, storage::StorageManager};

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
                // Create command channel for API → event loop communication
                let (command_tx, command_rx) =
                    tokio::sync::mpsc::channel::<p2p_commands::P2pCommand>(256);

                // Create shared P2P state and spawn the event loop
                let p2p_state = Arc::new(p2p::P2pState::new(command_tx));
                let shutdown_rx = shutdown_tx.subscribe();

                let loop_state = p2p_state.clone();
                let loop_metrics = metrics.clone();
                let loop_storage = storage.clone();
                tokio::spawn(async move {
                    p2p::run_event_loop(
                        node,
                        loop_state,
                        loop_metrics,
                        loop_storage,
                        command_rx,
                        shutdown_rx,
                    )
                    .await;
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

    // Initialize WebRTC bridge (browser ↔ node-runner)
    let bridge_state = if config.bridge.enabled {
        let bs = Arc::new(bridge::BridgeState::new(true));
        let loop_config = config.bridge.clone();
        let loop_peer_id = peer_id.clone();
        let loop_storage = storage.clone();
        let loop_metrics = metrics.clone();
        let loop_bridge = bs.clone();
        let bridge_shutdown = shutdown_tx.subscribe();
        tokio::spawn(async move {
            bridge::run_bridge(
                loop_config,
                loop_peer_id,
                loop_storage,
                loop_metrics,
                loop_bridge,
                bridge_shutdown,
            )
            .await;
        });
        println!("✅ WebRTC bridge started");
        Some(bs)
    } else {
        None
    };

    // Initialize background content replication
    let replication_state = if config.replication.enabled {
        if let Some(ref p2p) = p2p_state {
            let repl = Arc::new(replication::ReplicationState::new(
                config.network.replication_factor,
            ));
            let loop_config = config.replication.clone();
            let loop_peer_id = peer_id.clone();
            let loop_p2p = p2p.clone();
            let loop_storage = storage.clone();
            let loop_metrics = metrics.clone();
            let loop_repl = repl.clone();
            let repl_shutdown = shutdown_tx.subscribe();
            tokio::spawn(async move {
                replication::run_replication_loop(
                    loop_config,
                    loop_peer_id,
                    loop_p2p,
                    loop_storage,
                    loop_metrics,
                    loop_repl,
                    repl_shutdown,
                )
                .await;
            });
            println!("✅ Replication loop started");
            Some(repl)
        } else {
            println!("⚠️  Replication enabled but P2P unavailable — skipping");
            None
        }
    } else {
        None
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
        bridge: bridge_state,
        replication: replication_state,
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
