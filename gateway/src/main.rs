//! ShadowMesh Gateway

use gateway::*;

use axum::{
    extract::{Path, State},
    http::Uri,
    middleware as axum_middleware,
    response::{IntoResponse, Json, Response},
    routing::{delete, get, post},
    Router,
};
use tokio_util::sync::CancellationToken;
use protocol::{NamingManager, StorageConfig, StorageLayer};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tower_http::cors::{Any, CorsLayer};

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    let _ = dotenvy::from_filename("gateway/.env");

    // Load configuration first (needed for telemetry)
    let config = match config::Config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to load configuration: {}", e);
            config::Config::default()
        }
    };
    // Wrap in RwLock for hot-reload support
    let config_lock = Arc::new(tokio::sync::RwLock::new(config.clone()));

    // Initialize telemetry (replaces init_logging)
    if let Err(e) = telemetry::init_telemetry(&config.telemetry) {
        eprintln!("Warning: Failed to initialize telemetry: {}", e);
        // Fall back to basic logging
        init_logging();
    } else if config.telemetry.enabled {
        println!("✓ OpenTelemetry tracing enabled");
    }

    // Register Prometheus metrics
    metrics::register_metrics();

    // Validate GitHub OAuth environment variables
    validate_github_env();

    // Load authentication configuration
    let auth_config = Arc::new(auth::AuthConfig::from_env());

    println!("✓ Loaded configuration");

    // Warn about security configuration
    validate_security_config(&config);

    // Enforce production validation (fails in production mode if misconfigured)
    if let Err(e) = production::enforce_production_validation(&config) {
        eprintln!("FATAL: {}", e);
        std::process::exit(1);
    }

    // Initialize content cache
    let cache = Arc::new(cache::ContentCache::with_config(
        (config.cache.max_size_mb * 10) as usize, // Rough estimate: ~100KB per entry
        Duration::from_secs(config.cache.ttl_seconds),
    ));
    println!(
        "✓ Cache initialized: {} MB, TTL {}s",
        config.cache.max_size_mb, config.cache.ttl_seconds
    );

    // Create IPFS storage config
    let storage_config = StorageConfig {
        api_url: config.ipfs.api_url.clone(),
        timeout: Duration::from_secs(config.ipfs.timeout_seconds),
        retry_attempts: config.ipfs.retry_attempts,
    };

    let storage = match StorageLayer::with_config(storage_config).await {
        Ok(s) => {
            println!("✅ Connected to IPFS daemon at {}", config.ipfs.api_url);
            println!(
                "   Timeout: {}s, Retries: {}",
                config.ipfs.timeout_seconds, config.ipfs.retry_attempts
            );
            metrics::set_ipfs_connected(true);
            Some(Arc::new(s))
        }
        Err(e) => {
            println!("⚠️  IPFS daemon not available: {}", e);
            println!("   Running in demo mode");
            metrics::set_ipfs_connected(false);
            None
        }
    };

    // Create circuit breaker for IPFS operations (configurable)
    let ipfs_circuit_breaker = Arc::new(circuit_breaker::CircuitBreaker::new(
        "ipfs",
        config.circuit_breaker.failure_threshold,
        config.circuit_breaker.reset_timeout(),
    ));

    // Create audit logger for security events
    let audit_logger = Arc::new(audit::AuditLogger::new(10000)); // Keep last 10k events
    println!("✓ Audit logging enabled");

    // Initialize Redis client (optional)
    let redis = if let Some(redis_url) = config.redis.get_url() {
        match redis_client::RedisClient::new(&redis_url, config.redis.key_prefix.clone()).await {
            Ok(client) => {
                // Redact credentials from the URL before logging
                let display_url = if let Some(at_pos) = redis_url.find('@') {
                    if let Some(scheme_end) = redis_url.find("://") {
                        format!("{}://***@{}", &redis_url[..scheme_end], &redis_url[at_pos + 1..])
                    } else {
                        "redis://***@<redacted>".to_string()
                    }
                } else {
                    redis_url.clone()
                };
                println!("✅ Connected to Redis at {}", display_url);
                Some(Arc::new(client))
            }
            Err(e) => {
                println!("⚠️  Redis not available: {}", e);
                println!("   Running with in-memory state (data lost on restart)");
                None
            }
        }
    } else {
        println!("ℹ️  Redis not configured (using in-memory state)");
        None
    };

    // Load existing deployments from Redis if available
    let initial_deployments = if let Some(ref redis_client) = redis {
        match dashboard::Deployment::load_all_from_redis(redis_client).await {
            Ok(deployments) => {
                println!("✓ Loaded {} deployments from Redis", deployments.len());
                deployments
            }
            Err(e) => {
                tracing::warn!("Failed to load deployments from Redis: {}", e);
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };

    // Initialize naming manager and keypair
    let naming = Arc::new(RwLock::new(NamingManager::new()));
    let naming_key = if let Ok(seed_hex) = std::env::var("SHADOWMESH_NAMING_KEY") {
        let seed_hex = seed_hex.trim();
        if seed_hex.len() != 64 {
            eprintln!("FATAL: SHADOWMESH_NAMING_KEY must be 64 hex characters (32 bytes), got {}", seed_hex.len());
            std::process::exit(1);
        }
        let mut seed = [0u8; 32];
        for i in 0..32 {
            seed[i] = match u8::from_str_radix(&seed_hex[i * 2..i * 2 + 2], 16) {
                Ok(b) => b,
                Err(_) => {
                    eprintln!("FATAL: SHADOWMESH_NAMING_KEY contains invalid hex at position {}", i * 2);
                    std::process::exit(1);
                }
            };
        }
        match libp2p::identity::Keypair::ed25519_from_bytes(seed) {
            Ok(kp) => Arc::new(kp),
            Err(e) => {
                eprintln!("FATAL: Invalid Ed25519 seed in SHADOWMESH_NAMING_KEY: {}", e);
                std::process::exit(1);
            }
        }
    } else if production::is_production_mode() {
        eprintln!("FATAL: SHADOWMESH_NAMING_KEY is required in production mode");
        eprintln!("       Generate one with: openssl rand -hex 32");
        std::process::exit(1);
    } else {
        tracing::warn!("No SHADOWMESH_NAMING_KEY set — generating ephemeral naming keypair (names won't persist across restarts)");
        Arc::new(libp2p::identity::Keypair::generate_ed25519())
    };
    if config.naming.enabled {
        println!("✓ Decentralized naming layer enabled (cache: {} entries)", config.naming.cache_size);
    }

    // Initialize P2P mesh network (optional)
    let (p2p_shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(1);
    let p2p_state = if config.p2p.enabled {
        match protocol::ShadowNode::new().await {
            Ok(mut node) => {
                let peer_id = node.peer_id().to_string();
                println!("✓ P2P node initialized (Peer ID: {}...)", &peer_id[..peer_id.len().min(20)]);

                if let Err(e) = node.start().await {
                    eprintln!("  Failed to bind P2P addresses: {}", e);
                    None
                } else {
                    // Dial bootstrap peers and seed Kademlia routing table
                    for addr_str in &config.p2p.bootstrap_peers {
                        match addr_str.parse::<libp2p::Multiaddr>() {
                            Ok(addr) => {
                                if let Some(libp2p::multiaddr::Protocol::P2p(peer_id)) =
                                    addr.iter().last()
                                {
                                    node.swarm_mut()
                                        .behaviour_mut()
                                        .kademlia
                                        .add_address(&peer_id, addr.clone());
                                }
                                match node.dial(addr) {
                                    Ok(_) => {
                                        tracing::info!(
                                            "Dialing bootstrap peer: {}",
                                            addr_str
                                        )
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            "Failed to dial bootstrap {}: {}",
                                            addr_str,
                                            e
                                        )
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "Invalid bootstrap multiaddr '{}': {}",
                                    addr_str,
                                    e
                                );
                            }
                        }
                    }

                    let (command_tx, command_rx) =
                        tokio::sync::mpsc::channel::<p2p_commands::P2pCommand>(256);

                    let p2p_st = Arc::new(p2p::P2pState::new(command_tx));
                    let shutdown_rx = p2p_shutdown_tx.subscribe();

                    let loop_state = p2p_st.clone();
                    let loop_cache = cache.clone();
                    let loop_naming = naming.clone();
                    tokio::spawn(async move {
                        p2p::run_event_loop(
                            node,
                            loop_state,
                            loop_cache,
                            loop_naming,
                            command_rx,
                            shutdown_rx,
                        )
                        .await;
                    });

                    println!("✓ P2P event loop started");
                    Some(p2p_st)
                }
            }
            Err(e) => {
                println!("⚠  Failed to initialize P2P node: {}", e);
                println!("   Running without P2P content resolution");
                None
            }
        }
    } else {
        println!("ℹ️  P2P content resolution disabled");
        None
    };

    // Build node-runner health tracker (if any node-runners configured)
    let node_health_tracker = if !config.p2p.node_runners.is_empty() {
        let tracker = Arc::new(node_health::NodeHealthTracker::new(
            config.p2p.node_runners.clone(),
            &config.p2p.node_health,
        ));
        println!(
            "✓ Node health tracker: {} nodes, strategy={:?}",
            config.p2p.node_runners.len(),
            config.p2p.node_health.strategy,
        );
        Some(tracker)
    } else {
        None
    };

    let http_client = reqwest::Client::new();

    let state = AppState {
        storage,
        cache,
        config: config_lock.clone(),
        metrics: Arc::new(Metrics::default()),
        start_time: Instant::now(),
        deployments: Arc::new(RwLock::new(initial_deployments)),
        github_auth: Arc::new(RwLock::new(None)),
        github_oauth_states: Arc::new(RwLock::new(std::collections::HashMap::new())),
        ipfs_circuit_breaker,
        audit_logger,
        redis: redis.clone(),
        naming,
        naming_key,
        p2p: p2p_state,
        node_health_tracker: node_health_tracker.clone(),
        http_client,
    };

    // Clone audit logger before state is moved into the router
    let audit_for_auth = state.audit_logger.clone();

    // Initialize API key manager for key rotation
    let admin_key = std::env::var("SHADOWMESH_ADMIN_KEY").ok();
    let api_key_manager = Arc::new(api_keys::ApiKeyManager::new(redis, admin_key));
    let api_key_state = api_keys::ApiKeyState {
        manager: api_key_manager,
    };
    if std::env::var("SHADOWMESH_ADMIN_KEY").is_ok() {
        println!("✓ API key management enabled (admin key configured)");
    } else {
        println!("ℹ️  API key management disabled (set SHADOWMESH_ADMIN_KEY to enable)");
    }

    // Initialize WebRTC signaling server
    let signaling_config = signaling::SignalingConfig::default();
    let signaling_state: signaling::SignalingState =
        Arc::new(signaling::SignalingServer::new(signaling_config));
    println!("✓ WebRTC signaling server enabled at /signaling/ws");

    // Cancellation token for graceful shutdown of background tasks
    let shutdown_token = CancellationToken::new();

    // Start background cleanup task for stale signaling connections
    let signaling_cleanup = signaling_state.clone();
    let cleanup_token = shutdown_token.clone();
    let cleanup_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    signaling_cleanup.cleanup_stale().await;
                }
                _ = cleanup_token.cancelled() => {
                    tracing::info!("Signaling cleanup task shutting down");
                    break;
                }
            }
        }
    });
    // Monitor the cleanup task so panics are logged rather than silently lost
    tokio::spawn(async move {
        if let Err(e) = cleanup_handle.await {
            tracing::error!("Signaling cleanup task failed: {}", e);
        }
    });

    // Start background node-runner health probes
    if let Some(ref tracker) = node_health_tracker {
        let health_token = shutdown_token.clone();
        let health_tracker = tracker.clone();
        let interval_secs = config.p2p.node_health.health_check_interval_seconds;
        let health_handle = tokio::spawn(node_health::run_health_check_loop(
            health_tracker,
            Duration::from_secs(interval_secs),
            health_token,
        ));
        tokio::spawn(async move {
            if let Err(e) = health_handle.await {
                tracing::error!("Node health check task failed: {}", e);
            }
        });
        println!("✓ Node health probes started (interval: {}s)", interval_secs);
    }

    // Start config hot-reload task (watches config file + SIGHUP)
    {
        let reload_config = config_lock.clone();
        let config_paths = vec!["config.toml".to_string(), "gateway/config.toml".to_string()];
        tokio::spawn(config_watcher::start_config_reload_task(
            reload_config,
            config_paths,
        ));
        println!("✓ Config hot-reload enabled (file watch + SIGHUP)");
    }

    // Create rate limiter (distributed when Redis available)
    let rate_limiter = if config.rate_limit.enabled {
        let rate_config = rate_limit::RateLimitConfig {
            ip_requests_per_second: config.rate_limit.requests_per_second,
            key_requests_per_second: config.rate_limit.requests_per_second * 10,
            burst_size: config.rate_limit.burst_size,
            window: std::time::Duration::from_secs(1),
        };
        Some(distributed_rate_limit::DistributedRateLimiter::new(
            state.redis.clone(),
            rate_config,
        ))
    } else {
        None
    };

    let mut app = Router::new()
        .route("/health", get(health_handler))
        .route("/ready", get(ready_handler))
        .route("/metrics", get(metrics_handler))
        .route("/metrics/prometheus", get(prometheus_metrics_handler))
        // Dashboard UI
        .route("/dashboard", get(dashboard::dashboard_handler))
        // Deploy endpoints (Vercel-like)
        .route("/api/deploy", post(deploy::deploy_zip))
        .route("/api/deploy/info", get(deploy::deploy_info))
        .route("/api/deploy/github", post(dashboard::deploy_from_github))
        .route("/api/deployments", get(dashboard::get_deployments))
        .route(
            "/api/deployments/:cid",
            delete(dashboard::delete_deployment),
        )
        .route(
            "/api/deployments/:cid/redeploy",
            post(dashboard::redeploy_github),
        )
        .route(
            "/api/deployments/:cid/logs",
            get(dashboard::deployment_logs),
        )
        .route("/api/github/login", get(dashboard::github_login))
        .route("/api/github/callback", get(dashboard::github_callback))
        .route("/api/github/status", get(dashboard::github_status))
        .route("/api/github/repos", get(dashboard::github_repos))
        // Upload endpoints (single files)
        .route("/api/upload", post(upload::upload_multipart))
        .route("/api/upload/json", post(upload::upload_json))
        .route("/api/upload/raw", post(upload::upload_raw))
        .route("/api/upload/batch", post(upload::upload_batch))
        // Naming layer API
        .route("/api/names", get(name_list_handler))
        .route("/api/names/:name", get(name_resolve_handler))
        .route("/api/names/:name", post(name_register_handler))
        .route("/api/names/:name", delete(name_delete_handler))
        .route("/api/names/:name/assign", post(name_assign_handler))
        .route("/api/services/:service_type", get(service_discovery_handler))
        // Admin endpoints
        .route("/api/admin/reload", post(config_watcher::handler::reload_config_handler))
        // Content retrieval (CID paths only — SPA routes handled by fallback)
        .route("/:cid", get(content_or_spa_handler))
        .route("/:name/*path", get(shadow_or_content_subpath_handler))
        .route("/ipfs/*path", get(ipfs_content_path_handler))
        .with_state(state)
        // API key management routes (separate state)
        .nest("/api/keys", api_keys::api_keys_router(api_key_state))
        // WebRTC signaling server
        .nest("/signaling", signaling::signaling_router(signaling_state))
        // SPA fallback — serves React dashboard for all unmatched routes
        .fallback(spa::spa_handler);

    // Add middleware layers
    if config.security.cors_enabled {
        let allowed_origins = config.security.get_allowed_origins();
        let cors = if allowed_origins.contains(&"*".to_string()) {
            if production::is_production_mode() {
                tracing::error!("CORS wildcard (*) is not allowed in production mode — using restrictive CORS");
                CorsLayer::new()
            } else {
                tracing::warn!("CORS wildcard (*) enabled — this is insecure for production");
                CorsLayer::permissive()
            }
        } else {
            let origins: Vec<_> = allowed_origins
                .iter()
                .filter_map(|o| o.parse().ok())
                .collect();
            CorsLayer::new()
                .allow_origin(origins)
                .allow_methods(Any)
                .allow_headers(Any)
        };
        app = app.layer(cors);
    }

    // Add tracing middleware for OpenTelemetry (before other middleware)
    if config.telemetry.enabled {
        app = app.layer(axum_middleware::from_fn(
            telemetry::middleware::trace_request,
        ));
    }

    app = app
        .layer(axum_middleware::from_fn(middleware::request_logging))
        .layer(axum_middleware::from_fn(middleware::security_headers))
        .layer(axum_middleware::from_fn(middleware::request_id))
        .layer(axum_middleware::from_fn(middleware::max_request_size(
            config.security.max_request_size_mb,
        )));

    if let Some(limiter) = rate_limiter {
        let limiter_clone = limiter.clone();
        app = app.layer(axum_middleware::from_fn(move |req, next| {
            let limiter = limiter_clone.clone();
            async move { limiter.check_rate_limit(req, next).await }
        }));
    }

    // Add API key authentication middleware
    if auth_config.is_enabled() {
        let auth_config_clone = auth_config.clone();
        app = app.layer(axum_middleware::from_fn(move |req, next| {
            let auth = auth_config_clone.clone();
            let audit = audit_for_auth.clone();
            async move { auth::api_key_auth(auth, audit, req, next).await }
        }));
        println!("🔐 API authentication enabled");
    } else {
        println!("⚠️  API authentication DISABLED - all endpoints are public");
    }

    let bind_addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .unwrap_or_else(|e| {
            eprintln!("✗ Failed to bind to {}: {}", bind_addr, e);
            std::process::exit(1);
        });

    println!(
        "🌐 Gateway running at http://localhost:{}",
        config.server.port
    );
    println!("   Workers: {}", config.server.workers);
    println!(
        "   Cache: {} MB (TTL: {}s)",
        config.cache.max_size_mb, config.cache.ttl_seconds
    );
    if config.rate_limit.enabled {
        println!(
            "   Rate limit: {} req/s (burst: {})",
            config.rate_limit.requests_per_second, config.rate_limit.burst_size
        );
    }
    println!("   Deploy: POST /api/deploy (ZIP, max 100 MB)");
    println!("   Upload: POST /api/upload (single file, max 50 MB)");
    if config.monitoring.metrics_enabled {
        println!(
            "   Metrics: http://localhost:{}/metrics",
            config.server.port
        );
        println!("   Health: http://localhost:{}/health", config.server.port);
    }

    // Start server with graceful shutdown
    tracing::info!("Starting server with graceful shutdown support");

    let telemetry_enabled = config.telemetry.enabled;

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap_or_else(|e| {
            tracing::error!("Server error: {}", e);
        });

    // Cancel background tasks
    shutdown_token.cancel();

    // Shutdown P2P event loop
    let _ = p2p_shutdown_tx.send(());

    // Shutdown OpenTelemetry gracefully
    if telemetry_enabled {
        telemetry::shutdown_telemetry();
    }

    tracing::info!("Server shutdown complete");
}

/// Wait for shutdown signal (SIGTERM or Ctrl+C)
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::error!("Failed to listen for Ctrl+C: {}", e);
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(e) => {
                tracing::error!("Failed to install SIGTERM handler: {}", e);
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("Received Ctrl+C, initiating graceful shutdown...");
        },
        _ = terminate => {
            tracing::info!("Received SIGTERM, initiating graceful shutdown...");
        },
    }

    println!("\n🛑 Shutting down gracefully...");
}


/// Handles `/:cid` — if the path looks like a CID, serve IPFS content;
/// if it ends with `.shadow`, resolve the name; otherwise delegate to SPA.
async fn content_or_spa_handler(
    State(state): State<AppState>,
    Path(cid): Path<String>,
    uri: Uri,
) -> Response {
    // Check for .shadow domain resolution
    if cid.ends_with(".shadow") && state.config.read().await.naming.enabled {
        return resolve_shadow_name_and_serve(&state, &cid, "").await;
    }

    // Only treat paths starting with "Qm" (CIDv0) or "bafy" (CIDv1) as IPFS content,
    // and validate the CID contains only safe characters to prevent injection/XSS.
    if (cid.starts_with("Qm") || cid.starts_with("bafy"))
        && cid.len() <= 512
        && cid.chars().all(|c| c.is_ascii_alphanumeric())
    {
        return fetch_content(&state, cid, None).await;
    }

    // Everything else is a SPA route (e.g. /analytics, /settings, /login)
    spa::spa_handler(uri).await
}

/// Handles `/:name/*path` — if the first segment is a .shadow name, resolve and serve
/// the subpath; otherwise fall through to CID+path or SPA.
async fn shadow_or_content_subpath_handler(
    State(state): State<AppState>,
    Path((name, path)): Path<(String, String)>,
    uri: Uri,
) -> Response {
    if name.ends_with(".shadow") && state.config.read().await.naming.enabled {
        return resolve_shadow_name_and_serve(&state, &name, &path).await;
    }

    // Fall through to CID-with-path behavior
    if (name.starts_with("Qm") || name.starts_with("bafy"))
        && name.len() <= 512
        && name.chars().all(|c| c.is_ascii_alphanumeric())
    {
        let base_prefix = Some(format!("/{}/", name));
        let full_path = format!("{}/{}", name, path);
        return fetch_content(&state, full_path, base_prefix).await;
    }

    // SPA fallback
    spa::spa_handler(uri).await
}

// ============================================
// Auto-assign .shadow domain on deploy
// ============================================


// ============================================
// Naming Layer Endpoints
// ============================================

/// Request body for name registration
#[derive(Deserialize)]
struct NameRegisterRequest {
    /// The name record as JSON (pre-signed by the client)
    record: protocol::NameRecord,
}

/// Response for name resolution
#[derive(Serialize)]
struct NameResolveResponse {
    found: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    record: Option<protocol::NameRecord>,
}

/// Resolve a `.shadow` name via the naming layer
async fn name_resolve_handler(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Json<NameResolveResponse> {
    if !state.config.read().await.naming.enabled {
        return Json(NameResolveResponse {
            found: false,
            record: None,
        });
    }

    // 1. Check local cache first
    {
        let naming = lock_utils::read_lock(&state.naming);
        if let Some(record) = naming.resolve_local(&name) {
            return Json(NameResolveResponse {
                found: true,
                record: Some(record.clone()),
            });
        }
    }

    // 2. Try DHT lookup if P2P is available
    if let Some(record) = resolve_name_from_dht(&state, &name).await {
        return Json(NameResolveResponse {
            found: true,
            record: Some(record),
        });
    }

    Json(NameResolveResponse {
        found: false,
        record: None,
    })
}

/// Register or update a `.shadow` name (client sends a pre-signed record)
async fn name_register_handler(
    State(state): State<AppState>,
    Json(body): Json<NameRegisterRequest>,
) -> Response {
    if !state.config.read().await.naming.enabled {
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "Naming service disabled"})),
        )
            .into_response();
    }

    // Validate and verify the record
    if let Err(e) = protocol::naming::validate_record(&body.record) {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("Invalid record: {}", e)})),
        )
            .into_response();
    }

    match protocol::naming::verify_record(&body.record) {
        Ok(true) => {}
        Ok(false) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid signature"})),
            )
                .into_response();
        }
        Err(e) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("Verification error: {}", e)})),
            )
                .into_response();
        }
    }

    // Store the record (lock scoped to avoid holding across .await)
    let cache_result = {
        let mut naming = lock_utils::write_lock(&state.naming);
        naming.cache_record(body.record.clone())
    };

    match cache_result {
        Ok(()) => {
            // Publish to DHT + GossipSub via P2P
            if let Some(ref p2p) = state.p2p {
                if let Ok(bytes) = protocol::NamingManager::serialize_record(&body.record) {
                    let _ = p2p
                        .command_tx
                        .send(p2p_commands::P2pCommand::PublishName {
                            record_bytes: bytes,
                            name: body.record.name.clone(),
                        })
                        .await;
                }
            }
            Json(serde_json::json!({
                "success": true,
                "name": body.record.name,
                "name_hash": body.record.name_hash,
            }))
            .into_response()
        }
        Err(e) => (
            axum::http::StatusCode::CONFLICT,
            Json(serde_json::json!({"error": format!("{}", e)})),
        )
            .into_response(),
    }
}

/// Request body for simplified name assignment
#[derive(Deserialize)]
struct NameAssignRequest {
    cid: String,
}

/// List all names owned by this gateway (excludes revoked)
async fn name_list_handler(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    if !state.config.read().await.naming.enabled {
        return Json(serde_json::json!([]));
    }

    let naming = lock_utils::read_lock(&state.naming);
    let names: Vec<_> = naming
        .owned_names()
        .into_iter()
        .filter(|r| !r.records.is_empty()) // Exclude revoked
        .cloned()
        .collect();
    Json(serde_json::json!(names))
}

/// Assign a `.shadow` name to a CID (gateway signs on behalf of user)
async fn name_assign_handler(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(body): Json<NameAssignRequest>,
) -> Response {
    if !state.config.read().await.naming.enabled {
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "Naming service disabled"})),
        )
            .into_response();
    }

    let full_name = if name.ends_with(".shadow") {
        name.clone()
    } else {
        format!("{}.shadow", name)
    };

    if let Err(e) = protocol::validate_name(&full_name) {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("Invalid name: {}", e)})),
        )
            .into_response();
    }

    if body.cid.is_empty() || body.cid.len() > 512 {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "CID is required and must be at most 512 characters"})),
        )
            .into_response();
    }

    // Basic CID format validation: must be alphanumeric (base-encoded)
    if !body.cid.chars().all(|c| c.is_ascii_alphanumeric()) {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Invalid CID format"})),
        )
            .into_response();
    }

    let records = vec![protocol::NameRecordType::Content {
        cid: body.cid.clone(),
    }];

    // Register (lock scoped to avoid holding across .await)
    let register_result = {
        let mut naming = lock_utils::write_lock(&state.naming);
        naming.register_name(
            &full_name,
            records,
            &state.naming_key,
            protocol::naming::DEFAULT_NAME_TTL,
        )
    };

    match register_result {
        Ok(record) => {
            // Publish to DHT + GossipSub via P2P
            if let Some(ref p2p) = state.p2p {
                if let Ok(bytes) = protocol::NamingManager::serialize_record(&record) {
                    let _ = p2p
                        .command_tx
                        .send(p2p_commands::P2pCommand::PublishName {
                            record_bytes: bytes,
                            name: record.name.clone(),
                        })
                        .await;
                }
            }
            Json(serde_json::json!({
                "success": true,
                "name": record.name,
                "name_hash": record.name_hash,
                "cid": body.cid,
            }))
            .into_response()
        }
        Err(e) => {
            let status = match e {
                protocol::NamingError::NameTaken => axum::http::StatusCode::CONFLICT,
                _ => axum::http::StatusCode::BAD_REQUEST,
            };
            (status, Json(serde_json::json!({"error": format!("{}", e)})))
                .into_response()
        }
    }
}

/// Delete (revoke) a `.shadow` name owned by this gateway
async fn name_delete_handler(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Response {
    if !state.config.read().await.naming.enabled {
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "Naming service disabled"})),
        )
            .into_response();
    }

    let full_name = if name.ends_with(".shadow") {
        name.clone()
    } else {
        format!("{}.shadow", name)
    };

    // Revoke (lock scoped to avoid holding across .await)
    let revoke_result = {
        let mut naming = lock_utils::write_lock(&state.naming);
        if let Some(record) = naming.resolve_local(&full_name).cloned() {
            match naming.revoke_name_record(&record, &state.naming_key) {
                Ok(revoked) => {
                    naming.invalidate_cache(&full_name);
                    Ok(Some(revoked))
                }
                Err(e) => Err(e),
            }
        } else {
            Ok(None) // Not found
        }
    };

    match revoke_result {
        Ok(Some(revoked)) => {
            // Publish revoked record to DHT so peers learn about it
            if let Some(ref p2p) = state.p2p {
                if let Ok(bytes) = protocol::NamingManager::serialize_record(&revoked) {
                    let _ = p2p
                        .command_tx
                        .send(p2p_commands::P2pCommand::PublishName {
                            record_bytes: bytes,
                            name: full_name,
                        })
                        .await;
                }
            }
            Json(serde_json::json!({"success": true})).into_response()
        }
        Ok(None) => (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Name not found"})),
        )
            .into_response(),
        Err(e) => (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("{}", e)})),
        )
            .into_response(),
    }
}

// ============================================
// .shadow Name Resolution Helpers
// ============================================

/// Resolve a name record from DHT, validate, verify, and cache locally.
/// Returns None if P2P unavailable, name not found, or validation fails.
async fn resolve_name_from_dht(
    state: &AppState,
    name: &str,
) -> Option<protocol::NameRecord> {
    let p2p = state.p2p.as_ref()?;
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    p2p.command_tx
        .send(p2p_commands::P2pCommand::ResolveName {
            name: name.to_string(),
            reply: reply_tx,
        })
        .await
        .ok()?;

    let timeout_secs = state.config.read().await.p2p.resolve_timeout_seconds;
    let data = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        reply_rx,
    )
    .await
    .ok()?  // timeout
    .ok()?  // channel recv
    .ok()?  // FetchError
    ?;      // Option<Vec<u8>> → Vec<u8>

    let record = protocol::NamingManager::deserialize_record(&data).ok()?;
    if protocol::naming::validate_record(&record).is_err() {
        return None;
    }
    if !protocol::verify_record(&record).unwrap_or(false) {
        return None;
    }
    if record.is_revoked() {
        return None;
    }

    // Cache for future lookups
    {
        let mut naming = lock_utils::write_lock(&state.naming);
        let _ = naming.cache_record(record.clone());
    }

    Some(record)
}

/// Resolve a name record from local cache first, then DHT.
async fn resolve_name_record(
    state: &AppState,
    name: &str,
) -> Option<protocol::NameRecord> {
    // Local cache first
    {
        let naming = lock_utils::read_lock(&state.naming);
        if let Some(record) = naming.resolve_local(name) {
            return Some(record.clone());
        }
    }

    // DHT fallback
    resolve_name_from_dht(state, name).await
}

/// Maximum alias chain depth to prevent infinite loops.
const MAX_ALIAS_DEPTH: usize = 8;

/// Resolve a .shadow name (following aliases) and serve the content.
async fn resolve_shadow_name_and_serve(
    state: &AppState,
    name: &str,
    sub_path: &str,
) -> Response {
    let mut current_name = name.to_string();

    'resolve: for _ in 0..MAX_ALIAS_DEPTH {
        let record = match resolve_name_record(state, &current_name).await {
            Some(r) => r,
            None => {
                return (
                    axum::http::StatusCode::NOT_FOUND,
                    axum::response::Html(format!(
                        "<html><body><h1>Not Found</h1><p>{} could not be resolved</p></body></html>",
                        escape_html(&current_name)
                    )),
                )
                    .into_response();
            }
        };

        // Check for alias — follow the chain
        if let Some(alias_target) = record.records.iter().find_map(|r| match r {
            protocol::NameRecordType::Alias { target } => Some(target.clone()),
            _ => None,
        }) {
            current_name = alias_target;
            continue 'resolve;
        }

        // Check for content CID
        if let Some(cid) = record.content_cids().first() {
            let fetch_path = if sub_path.is_empty() {
                cid.to_string()
            } else {
                format!("{}/{}", cid, sub_path)
            };
            let base_prefix = Some(format!("/{}/", name));
            return fetch_content(state, fetch_path, base_prefix).await;
        }

        // Name exists but has no serveable records
        return (
            axum::http::StatusCode::NOT_FOUND,
            axum::response::Html(format!(
                "<html><body><h1>No Content</h1><p>{} has no content records</p></body></html>",
                escape_html(name)
            )),
        )
            .into_response();
    }

    // Alias depth exceeded
    (
        axum::http::StatusCode::LOOP_DETECTED,
        axum::response::Html(
            "<html><body><h1>Loop Detected</h1><p>Alias chain too deep</p></body></html>"
                .to_string(),
        ),
    )
        .into_response()
}

/// Discover services by type (gateway, signaling, stun, etc.)
async fn service_discovery_handler(
    State(state): State<AppState>,
    Path(service_type): Path<String>,
) -> Json<serde_json::Value> {
    if !state.config.read().await.naming.enabled {
        return Json(serde_json::json!({"entries": []}));
    }

    let well_known_name = match service_type.as_str() {
        "gateway" => protocol::WellKnownNames::GATEWAY,
        "signaling" => protocol::WellKnownNames::SIGNALING,
        "bootstrap" => protocol::WellKnownNames::BOOTSTRAP,
        "turn" => protocol::WellKnownNames::TURN,
        "stun" => protocol::WellKnownNames::STUN,
        _ => {
            return Json(serde_json::json!({
                "error": "Unknown service type",
                "valid_types": ["gateway", "signaling", "bootstrap", "turn", "stun"]
            }));
        }
    };

    let naming = lock_utils::read_lock(&state.naming);
    if let Some(registry) = naming.get_service_registry(well_known_name) {
        Json(serde_json::json!({
            "name": well_known_name,
            "entries": registry.entries,
            "updated_at": registry.updated_at,
        }))
    } else {
        Json(serde_json::json!({
            "name": well_known_name,
            "entries": [],
        }))
    }
}

// ============================================
// Health & Metrics Endpoints
// ============================================

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
    uptime_seconds: u64,
    ipfs_connected: bool,
}

async fn health_handler(State(state): State<AppState>) -> Json<HealthResponse> {
    let uptime = state.start_time.elapsed().as_secs();
    let ipfs_connected = state.storage.is_some();

    Json(HealthResponse {
        status: if ipfs_connected {
            "healthy"
        } else {
            "degraded"
        },
        version: env!("CARGO_PKG_VERSION"),
        uptime_seconds: uptime,
        ipfs_connected,
    })
}

#[derive(Serialize)]
struct ReadyResponse {
    ready: bool,
    checks: ReadyChecks,
}

#[derive(Serialize)]
struct ReadyChecks {
    ipfs: bool,
    redis: bool,
    circuit_breaker_closed: bool,
}

async fn ready_handler(State(state): State<AppState>) -> Response {
    let ipfs_ok = state.storage.is_some();
    let redis_ok = match &state.redis {
        Some(redis) => redis.ping().await.unwrap_or(false),
        None => true, // Redis is optional; not configured is not a failure
    };
    let cb_ok = !state.ipfs_circuit_breaker.is_open();

    let all_ready = ipfs_ok && redis_ok && cb_ok;

    let response = ReadyResponse {
        ready: all_ready,
        checks: ReadyChecks {
            ipfs: ipfs_ok,
            redis: redis_ok,
            circuit_breaker_closed: cb_ok,
        },
    };

    if all_ready {
        (axum::http::StatusCode::OK, Json(response)).into_response()
    } else {
        (axum::http::StatusCode::SERVICE_UNAVAILABLE, Json(response)).into_response()
    }
}

#[derive(Serialize)]
struct MetricsResponse {
    uptime_seconds: u64,
    requests_total: u64,
    requests_success: u64,
    requests_error: u64,
    bytes_served: u64,
    cache: CacheMetrics,
    config: ConfigMetrics,
}

#[derive(Serialize)]
struct CacheMetrics {
    total_entries: usize,
    expired_entries: usize,
    max_entries: usize,
}

#[derive(Serialize)]
struct ConfigMetrics {
    cache_max_size_mb: u64,
    cache_ttl_seconds: u64,
    rate_limit_enabled: bool,
    rate_limit_rps: u64,
    ipfs_connected: bool,
}

async fn metrics_handler(State(state): State<AppState>) -> Json<MetricsResponse> {
    let uptime = state.start_time.elapsed().as_secs();
    let cache_stats = state.cache.stats();
    let cfg = state.config.read().await;

    Json(MetricsResponse {
        uptime_seconds: uptime,
        requests_total: state.metrics.requests_total(),
        requests_success: state.metrics.requests_success(),
        requests_error: state.metrics.requests_error(),
        bytes_served: state.metrics.bytes_served(),
        cache: CacheMetrics {
            total_entries: cache_stats.total_entries,
            expired_entries: cache_stats.expired_entries,
            max_entries: cache_stats.max_entries,
        },
        config: ConfigMetrics {
            cache_max_size_mb: cfg.cache.max_size_mb,
            cache_ttl_seconds: cfg.cache.ttl_seconds,
            rate_limit_enabled: cfg.rate_limit.enabled,
            rate_limit_rps: cfg.rate_limit.requests_per_second,
            ipfs_connected: state.storage.is_some(),
        },
    })
}

// ============================================
// Prometheus Metrics Endpoint
// ============================================

async fn prometheus_metrics_handler() -> impl IntoResponse {
    let body = metrics::encode_metrics();
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
}

// ============================================
// Logging Initialization
// ============================================

fn init_logging() {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    // Check if we should use JSON logging (for production)
    let use_json = std::env::var("SHADOWMESH_LOG_FORMAT")
        .map(|v| v.to_lowercase() == "json")
        .unwrap_or(false);

    if use_json {
        // JSON format for production/log aggregation
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer().json())
            .init();
    } else {
        // Pretty format for development
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer().pretty())
            .init();
    }
}

// ============================================
// Environment Validation Functions
// ============================================

/// Validate GitHub OAuth environment variables
fn validate_github_env() {
    let client_id = std::env::var("GITHUB_CLIENT_ID").ok();
    let client_secret = std::env::var("GITHUB_CLIENT_SECRET").ok();

    match (&client_id, &client_secret) {
        (Some(id), Some(secret)) if !id.is_empty() && !secret.is_empty() => {
            println!("✓ GitHub OAuth configured");
        }
        (Some(_), None) | (None, Some(_)) => {
            println!("⚠️  GitHub OAuth partially configured - both GITHUB_CLIENT_ID and GITHUB_CLIENT_SECRET required");
            println!("   Dashboard GitHub integration will be disabled");
        }
        _ => {
            println!("ℹ️  GitHub OAuth not configured");
            println!("   Set GITHUB_CLIENT_ID and GITHUB_CLIENT_SECRET for GitHub integration");
        }
    }
}

/// Validate security configuration and warn about insecure settings
fn validate_security_config(config: &config::Config) {
    // Warn about permissive CORS
    if config.security.cors_enabled && config.security.allowed_origins.contains(&"*".to_string()) {
        println!("⚠️  SECURITY WARNING: CORS is set to allow all origins (*)");
        println!("   This is NOT recommended for production!");
        println!("   Set SHADOWMESH_SECURITY_ALLOWED_ORIGINS to specific origins");
    }

    // Warn about missing API keys
    let api_keys = std::env::var("SHADOWMESH_API_KEYS").unwrap_or_default();
    if api_keys.is_empty() {
        println!("⚠️  SECURITY WARNING: No API keys configured");
        println!("   All mutation endpoints are publicly accessible!");
        println!("   Set SHADOWMESH_API_KEYS for production");
    }
}
