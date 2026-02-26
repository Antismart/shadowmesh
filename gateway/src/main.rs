//! ShadowMesh Gateway

mod api_keys;
mod audit;
mod auth;
mod cache;
mod circuit_breaker;
mod config;
mod config_watcher;
mod dashboard;
mod deploy;
mod distributed_rate_limit;
mod error;
mod lock_utils;
mod metrics;
mod middleware;
mod production;
mod rate_limit;
mod redis_client;
mod p2p;
mod p2p_commands;
mod p2p_resolver;
mod node_resolver;
mod signaling;
mod spa;
mod telemetry;
mod upload;

use axum::{
    body::Body,
    extract::{Path, State},
    http::Uri,
    middleware as axum_middleware,
    response::{Html, IntoResponse, Json, Response},
    routing::{delete, get, post},
    Router,
};
use cache::ContentCache;
use config::Config;
use dashboard::Deployment;
use tokio_util::sync::CancellationToken;
use protocol::{NamingManager, StorageConfig, StorageLayer};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tower_http::cors::{Any, CorsLayer};

/// Metrics for monitoring
#[derive(Default)]
pub struct Metrics {
    requests_total: AtomicU64,
    requests_success: AtomicU64,
    requests_error: AtomicU64,
    bytes_served: AtomicU64,
}

impl Metrics {
    pub fn increment_requests(&self) {
        self.requests_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_success(&self) {
        self.requests_success.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_error(&self) {
        self.requests_error.fetch_add(1, Ordering::Relaxed);
    }

    pub fn add_bytes(&self, bytes: u64) {
        self.bytes_served.fetch_add(bytes, Ordering::Relaxed);
    }
}

/// Escape a string for safe embedding in HTML content.
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

#[derive(Clone)]
pub struct AppState {
    pub storage: Option<Arc<StorageLayer>>,
    pub cache: Arc<ContentCache>,
    pub config: Arc<Config>,
    pub metrics: Arc<Metrics>,
    pub start_time: Instant,
    pub deployments: Arc<RwLock<Vec<Deployment>>>,
    pub github_auth: Arc<RwLock<Option<dashboard::GithubAuth>>>,
    pub github_oauth_states: Arc<RwLock<std::collections::HashMap<String, Instant>>>,
    /// Circuit breaker for IPFS operations
    pub ipfs_circuit_breaker: Arc<circuit_breaker::CircuitBreaker>,
    /// Audit logger for security events
    pub audit_logger: Arc<audit::AuditLogger>,
    /// Redis client for persistence (optional)
    pub redis: Option<Arc<redis_client::RedisClient>>,
    /// Decentralized naming manager
    pub naming: Arc<RwLock<NamingManager>>,
    /// Ed25519 keypair for signing name records
    pub naming_key: Arc<libp2p::identity::Keypair>,
    /// P2P mesh state (None if P2P is disabled or failed to start)
    pub p2p: Option<Arc<p2p::P2pState>>,
}

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    let _ = dotenvy::from_filename("gateway/.env");

    // Load configuration first (needed for telemetry)
    let config = match Config::load() {
        Ok(c) => Arc::new(c),
        Err(e) => {
            eprintln!("Failed to load configuration: {}", e);
            Arc::new(Config::default())
        }
    };

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
    let cache = Arc::new(ContentCache::with_config(
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
                    // Dial bootstrap peers
                    for addr_str in &config.p2p.bootstrap_peers {
                        if let Ok(addr) = addr_str.parse::<libp2p::Multiaddr>() {
                            if let Err(e) = node.dial(addr) {
                                tracing::warn!("Failed to dial bootstrap peer {}: {}", addr_str, e);
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

    let state = AppState {
        storage,
        cache,
        config: config.clone(),
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
    if cid.ends_with(".shadow") && state.config.naming.enabled {
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
    if name.ends_with(".shadow") && state.config.naming.enabled {
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

#[allow(dead_code)]
async fn content_path_handler(
    State(state): State<AppState>,
    Path((cid, path)): Path<(String, String)>,
) -> Response {
    let full_path = format!("{}/{}", cid, path);
    state.metrics.increment_requests();

    // Check cache first
    if let Some((data, content_type)) = state.cache.get(&full_path) {
        state.metrics.increment_success();
        state.metrics.add_bytes(data.len() as u64);
        return (
            [
                (axum::http::header::CONTENT_TYPE, content_type),
                (
                    axum::http::header::HeaderName::from_static("x-cache"),
                    "HIT".to_string(),
                ),
            ],
            data,
        )
            .into_response();
    }

    // Fetch from IPFS
    let Some(storage) = &state.storage else {
        state.metrics.increment_error();
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Html(r#"<html><body><h1>IPFS Not Connected</h1></body></html>"#),
        )
            .into_response();
    };

    let storage = Arc::clone(storage);
    let path_clone = full_path.clone();

    // Spawn blocking task for IPFS retrieval (handles non-Send stream)
    let result = tokio::task::spawn_blocking(move || {
        tokio::runtime::Handle::current()
            .block_on(async { storage.retrieve_content(&path_clone).await })
    })
    .await;

    match result {
        Ok(Ok(data)) => {
            let content_type = infer::get(&data)
                .map(|t| t.mime_type().to_string())
                .unwrap_or_else(|| "application/octet-stream".to_string());

            // Store in cache
            state
                .cache
                .set(full_path, data.clone(), content_type.clone());

            state.metrics.increment_success();
            state.metrics.add_bytes(data.len() as u64);

            (
                [
                    (axum::http::header::CONTENT_TYPE, content_type),
                    (
                        axum::http::header::HeaderName::from_static("x-cache"),
                        "MISS".to_string(),
                    ),
                ],
                data,
            )
                .into_response()
        }
        Ok(Err(e)) => {
            state.metrics.increment_error();
            (
                axum::http::StatusCode::NOT_FOUND,
                Html(format!(
                    r#"<html><body><h1>Not Found</h1><p>{}</p></body></html>"#,
                    escape_html(&e.to_string())
                )),
            )
                .into_response()
        }
        Err(e) => {
            state.metrics.increment_error();
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Html(format!(
                    r#"<html><body><h1>Error</h1><p>{}</p></body></html>"#,
                    escape_html(&e.to_string())
                )),
            )
                .into_response()
        }
    }
}

// Handler for /:cid/*path - access files within a directory CID
#[allow(dead_code)]
async fn content_with_path_handler(
    State(state): State<AppState>,
    Path((cid, path)): Path<(String, String)>,
) -> Response {
    let full_path = format!("{}/{}", cid, path);
    fetch_content(&state, full_path, Some(format!("/{}/", cid))).await
}

// ============================================
// Auto-assign .shadow domain on deploy
// ============================================

/// Sanitize a deployment name into a valid .shadow domain label.
fn sanitize_domain_label(name: &str) -> String {
    let label: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
        .collect();
    // Trim leading/trailing hyphens and limit length
    let label = label.trim_matches('-');
    let label = if label.len() > 40 { &label[..40] } else { label };
    let label = label.trim_end_matches('-');
    if label.is_empty() {
        "site".to_string()
    } else {
        label.to_string()
    }
}

/// Best-effort publish of a name record to DHT + GossipSub (sync context).
fn publish_name_best_effort(state: &AppState, record: &protocol::NameRecord) {
    if let Some(ref p2p) = state.p2p {
        if let Ok(bytes) = protocol::NamingManager::serialize_record(record) {
            let _ = p2p
                .command_tx
                .try_send(p2p_commands::P2pCommand::PublishName {
                    record_bytes: bytes,
                    name: record.name.clone(),
                });
        }
    }
}

/// Try to auto-assign a `.shadow` domain for a deployment.
/// Returns the assigned domain name, or None if naming is disabled or registration fails.
pub fn auto_assign_domain(state: &AppState, deploy_name: &str, cid: &str) -> Option<String> {
    if !state.config.naming.enabled {
        return None;
    }

    let base_label = sanitize_domain_label(deploy_name);
    let records = vec![protocol::NameRecordType::Content {
        cid: cid.to_string(),
    }];

    let mut naming = lock_utils::write_lock(&state.naming);

    // Try the base name first
    let full_name = format!("{}.shadow", base_label);
    match naming.register_name(
        &full_name,
        records.clone(),
        &state.naming_key,
        protocol::naming::DEFAULT_NAME_TTL,
    ) {
        Ok(record) => {
            tracing::info!(domain = %full_name, cid = %cid, "Auto-assigned domain");
            publish_name_best_effort(state, &record);
            return Some(full_name);
        }
        Err(protocol::NamingError::NameTaken) => {
            // Name taken, try with random suffix
        }
        Err(e) => {
            tracing::warn!(name = %full_name, error = %e, "Failed to auto-assign domain");
            return None;
        }
    }

    // Try with random suffixes (up to 3 attempts)
    for _ in 0..3 {
        let suffix: String = (0..4)
            .map(|_| {
                let idx = rand::random::<u8>() % 36;
                if idx < 10 { (b'0' + idx) as char } else { (b'a' + idx - 10) as char }
            })
            .collect();
        let name_with_suffix = format!("{}-{}.shadow", base_label, suffix);
        match naming.register_name(
            &name_with_suffix,
            records.clone(),
            &state.naming_key,
            protocol::naming::DEFAULT_NAME_TTL,
        ) {
            Ok(record) => {
                tracing::info!(domain = %name_with_suffix, cid = %cid, "Auto-assigned domain (with suffix)");
                publish_name_best_effort(state, &record);
                return Some(name_with_suffix);
            }
            Err(_) => continue,
        }
    }

    tracing::warn!(name = %base_label, "Could not auto-assign domain after retries");
    None
}

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
    if !state.config.naming.enabled {
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
    if !state.config.naming.enabled {
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
    if !state.config.naming.enabled {
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
    if !state.config.naming.enabled {
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
    if !state.config.naming.enabled {
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

    let timeout_secs = state.config.p2p.resolve_timeout_seconds;
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
    if !state.config.naming.enabled {
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

// Handler for /ipfs/{*path} - handles /ipfs/cid and /ipfs/cid/path
async fn ipfs_content_path_handler(
    State(state): State<AppState>,
    Path(path): Path<String>,
) -> Response {
    // The path contains everything after /ipfs/
    // It could be just "cid" or "cid/subpath/..."
    let parts: Vec<&str> = path.splitn(2, '/').collect();
    let cid = parts[0];

    if parts.len() == 1 {
        // Just a CID, redirect to content_handler logic
        fetch_content(&state, cid.to_string(), Some(format!("/ipfs/{}/", cid))).await
    } else {
        // CID with path
        let full_path = path.clone();
        fetch_content(&state, full_path, Some(format!("/ipfs/{}/", cid))).await
    }
}

// Unified content fetching logic with circuit breaker protection
async fn fetch_content(state: &AppState, cid: String, base_prefix: Option<String>) -> Response {
    state.metrics.increment_requests();

    // Check cache first (bypasses circuit breaker)
    if let Some((data, content_type)) = state.cache.get(&cid) {
        state.metrics.increment_success();
        state.metrics.add_bytes(data.len() as u64);
        metrics::record_cache_hit();
        return (
            [
                (axum::http::header::CONTENT_TYPE, content_type),
                (
                    axum::http::header::HeaderName::from_static("x-cache"),
                    "HIT".to_string(),
                ),
            ],
            data,
        )
            .into_response();
    }

    metrics::record_cache_miss();

    // Try P2P network before IPFS
    if let Some(ref p2p) = state.p2p {
        let timeout_secs = state.config.p2p.resolve_timeout_seconds;
        match p2p_resolver::resolve_content(p2p, &cid, timeout_secs).await {
            Ok(resolved) => {
                let content_type = if resolved.mime_type.is_empty() {
                    content_type_from_path(&cid, &resolved.data)
                } else {
                    resolved.mime_type
                };
                let data =
                    rewrite_html_assets(&resolved.data, &content_type, base_prefix.as_deref());

                state
                    .cache
                    .set(cid.clone(), data.clone(), content_type.clone());
                metrics::update_cache_size(state.cache.stats().total_entries as i64);

                // Announce to DHT so other peers can discover us
                if state.config.p2p.announce_content {
                    let _ = p2p
                        .command_tx
                        .send(p2p_commands::P2pCommand::AnnounceContent {
                            content_hash: cid.clone(),
                            fragment_hashes: vec![cid],
                            total_size: data.len() as u64,
                            mime_type: content_type.clone(),
                        })
                        .await;
                }

                state.metrics.increment_success();
                state.metrics.add_bytes(data.len() as u64);
                metrics::record_bytes_served(data.len() as u64);

                return (
                    [
                        (axum::http::header::CONTENT_TYPE, content_type),
                        (
                            axum::http::header::HeaderName::from_static("x-cache"),
                            "MISS".to_string(),
                        ),
                        (
                            axum::http::header::HeaderName::from_static("x-source"),
                            "P2P".to_string(),
                        ),
                    ],
                    data,
                )
                    .into_response();
            }
            Err(e) => {
                tracing::debug!(cid = %cid, error = %e,
                    "P2P resolution failed, falling back to IPFS");
            }
        }
    }

    // Try configured node-runners via HTTP
    if !state.config.p2p.node_runners.is_empty() {
        let http = reqwest::Client::new();
        let timeout = state.config.p2p.resolve_timeout_seconds;
        if let Some(node_content) = node_resolver::resolve_from_nodes_adaptive(
            &http,
            &state.config.p2p.node_runners,
            &cid,
            timeout,
        )
        .await
        {
            match node_content {
                node_resolver::NodeContent::Buffered(resolved) => {
                    let content_type = if resolved.mime_type.is_empty() {
                        content_type_from_path(&cid, &resolved.data)
                    } else {
                        resolved.mime_type
                    };
                    let data = rewrite_html_assets(
                        &resolved.data,
                        &content_type,
                        base_prefix.as_deref(),
                    );

                    state
                        .cache
                        .set(cid.clone(), data.clone(), content_type.clone());
                    metrics::update_cache_size(state.cache.stats().total_entries as i64);

                    state.metrics.increment_success();
                    state.metrics.add_bytes(data.len() as u64);
                    metrics::record_bytes_served(data.len() as u64);

                    return (
                        [
                            (axum::http::header::CONTENT_TYPE, content_type),
                            (
                                axum::http::header::HeaderName::from_static("x-cache"),
                                "MISS".to_string(),
                            ),
                            (
                                axum::http::header::HeaderName::from_static("x-source"),
                                "NODE".to_string(),
                            ),
                        ],
                        data,
                    )
                        .into_response();
                }
                node_resolver::NodeContent::Streaming(streaming) => {
                    let content_type = if streaming.mime_type.is_empty() {
                        "application/octet-stream".to_string()
                    } else {
                        streaming.mime_type
                    };

                    state.metrics.increment_success();
                    state.metrics.add_bytes(streaming.content_length);
                    metrics::record_bytes_served(streaming.content_length);

                    let body = Body::from_stream(streaming.stream);

                    return axum::http::Response::builder()
                        .header(axum::http::header::CONTENT_TYPE, &content_type)
                        .header(
                            axum::http::header::CONTENT_LENGTH,
                            streaming.content_length.to_string(),
                        )
                        .header("x-cache", "MISS")
                        .header("x-source", "NODE-STREAM")
                        .body(body)
                        .unwrap()
                        .into_response();
                }
            }
        }
    }

    // Check circuit breaker before attempting IPFS operation
    if !state.ipfs_circuit_breaker.allow_request() {
        state.metrics.increment_error();
        tracing::warn!(cid = %cid, "IPFS circuit breaker is open - failing fast");
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "IPFS service temporarily unavailable",
                "code": "CIRCUIT_OPEN",
                "retry_after_seconds": 30
            })),
        )
            .into_response();
    }

    // Fetch from IPFS
    let Some(storage) = &state.storage else {
        state.metrics.increment_error();
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Html(r#"<html><body><h1>IPFS Not Connected</h1></body></html>"#),
        )
            .into_response();
    };

    let storage = Arc::clone(storage);
    let cid_clone = cid.clone();
    let start_time = std::time::Instant::now();

    let result = tokio::task::spawn_blocking(move || {
        tokio::runtime::Handle::current()
            .block_on(async { storage.retrieve_content(&cid_clone).await })
    })
    .await;

    let duration = start_time.elapsed();

    match result {
        Ok(Ok(data)) => {
            // Record success with circuit breaker and metrics
            state.ipfs_circuit_breaker.record_success();
            metrics::record_ipfs_operation("retrieve", true, duration.as_secs_f64());
            metrics::update_circuit_breaker_state(false, false);

            let content_type = content_type_from_path(&cid, &data);
            let data = rewrite_html_assets(&data, &content_type, base_prefix.as_deref());

            state.cache.set(cid.clone(), data.clone(), content_type.clone());
            metrics::update_cache_size(state.cache.stats().total_entries as i64);

            // Announce to P2P network after IPFS fetch
            if let Some(ref p2p) = state.p2p {
                if state.config.p2p.announce_content {
                    let _ = p2p
                        .command_tx
                        .send(p2p_commands::P2pCommand::AnnounceContent {
                            content_hash: cid.clone(),
                            fragment_hashes: vec![cid],
                            total_size: data.len() as u64,
                            mime_type: content_type.clone(),
                        })
                        .await;
                }
            }

            state.metrics.increment_success();
            state.metrics.add_bytes(data.len() as u64);
            metrics::record_bytes_served(data.len() as u64);

            (
                [
                    (axum::http::header::CONTENT_TYPE, content_type),
                    (
                        axum::http::header::HeaderName::from_static("x-cache"),
                        "MISS".to_string(),
                    ),
                ],
                data,
            )
                .into_response()
        }
        Ok(Err(e)) => {
            // Record failure with circuit breaker
            state.ipfs_circuit_breaker.record_failure();
            metrics::record_ipfs_operation("retrieve", false, duration.as_secs_f64());
            metrics::update_circuit_breaker_state(
                state.ipfs_circuit_breaker.is_open(),
                state.ipfs_circuit_breaker.state() == crate::circuit_breaker::CircuitState::HalfOpen,
            );
            state.metrics.increment_error();

            tracing::warn!(cid = %cid, error = %e, "IPFS retrieval failed");

            (
                axum::http::StatusCode::NOT_FOUND,
                Html(format!(
                    r#"<html><body><h1>Not Found</h1><p>{}</p></body></html>"#,
                    escape_html(&e.to_string())
                )),
            )
                .into_response()
        }
        Err(e) => {
            // Record failure with circuit breaker (task panic/cancellation)
            state.ipfs_circuit_breaker.record_failure();
            metrics::record_ipfs_operation("retrieve", false, duration.as_secs_f64());
            metrics::update_circuit_breaker_state(
                state.ipfs_circuit_breaker.is_open(),
                state.ipfs_circuit_breaker.state() == crate::circuit_breaker::CircuitState::HalfOpen,
            );
            state.metrics.increment_error();

            tracing::error!(cid = %cid, error = %e, "IPFS retrieval task failed");

            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Html(format!(
                    r#"<html><body><h1>Error</h1><p>{}</p></body></html>"#,
                    escape_html(&e.to_string())
                )),
            )
                .into_response()
        }
    }
}

fn content_type_from_path(path: &str, data: &[u8]) -> String {
    let lower = path.to_lowercase();
    if lower.ends_with(".html") || lower.ends_with(".htm") || looks_like_html(data) {
        return "text/html".to_string();
    }
    if lower.ends_with(".css") {
        return "text/css".to_string();
    }
    if lower.ends_with(".js")
        || lower.ends_with(".mjs")
        || lower.ends_with(".jsx")
        || lower.ends_with(".ts")
    {
        return "application/javascript".to_string();
    }
    if lower.ends_with(".json") || lower.ends_with(".map") {
        return "application/json".to_string();
    }
    if lower.ends_with(".svg") {
        return "image/svg+xml".to_string();
    }
    if lower.ends_with(".png") {
        return "image/png".to_string();
    }
    if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        return "image/jpeg".to_string();
    }
    if lower.ends_with(".gif") {
        return "image/gif".to_string();
    }
    if lower.ends_with(".webp") {
        return "image/webp".to_string();
    }
    if lower.ends_with(".ico") {
        return "image/x-icon".to_string();
    }

    infer::get(data)
        .map(|t| t.mime_type().to_string())
        .unwrap_or_else(|| "application/octet-stream".to_string())
}

fn looks_like_html(data: &[u8]) -> bool {
    let snippet = String::from_utf8_lossy(&data[..data.len().min(512)]).to_lowercase();
    snippet.contains("<!doctype html") || snippet.contains("<html")
}

fn rewrite_html_assets(data: &[u8], content_type: &str, base_prefix: Option<&str>) -> Vec<u8> {
    if content_type != "text/html" {
        return data.to_vec();
    }

    let Some(base_prefix) = base_prefix else {
        return data.to_vec();
    };

    let base_prefix = if base_prefix.ends_with('/') {
        base_prefix.to_string()
    } else {
        format!("{}/", base_prefix)
    };

    let mut html = String::from_utf8_lossy(data).to_string();
    let guard = "__IPFS_GUARD__";

    if !html.to_lowercase().contains("<base ") {
        if let Some(head_pos) = html.to_lowercase().find("<head") {
            if let Some(end_pos) = html[head_pos..].find('>') {
                let insert_pos = head_pos + end_pos + 1;
                let base_tag = format!("\n    <base href=\"{}\">", base_prefix);
                html.insert_str(insert_pos, &base_tag);
            }
        }
    }

    for attr in ["href", "src", "srcset"] {
        let guard_token = format!("{}=\"{}/", attr, guard);
        html = html.replace(&format!("{}=\"/ipfs/", attr), &guard_token);
        html = html.replace(
            &format!("{}=\"/", attr),
            &format!("{}=\"{}", attr, base_prefix),
        );
        html = html.replace(&guard_token, &format!("{}=\"/ipfs/", attr));
    }

    html.into_bytes()
}

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

    Json(MetricsResponse {
        uptime_seconds: uptime,
        requests_total: state.metrics.requests_total.load(Ordering::Relaxed),
        requests_success: state.metrics.requests_success.load(Ordering::Relaxed),
        requests_error: state.metrics.requests_error.load(Ordering::Relaxed),
        bytes_served: state.metrics.bytes_served.load(Ordering::Relaxed),
        cache: CacheMetrics {
            total_entries: cache_stats.total_entries,
            expired_entries: cache_stats.expired_entries,
            max_entries: cache_stats.max_entries,
        },
        config: ConfigMetrics {
            cache_max_size_mb: state.config.cache.max_size_mb,
            cache_ttl_seconds: state.config.cache.ttl_seconds,
            rate_limit_enabled: state.config.rate_limit.enabled,
            rate_limit_rps: state.config.rate_limit.requests_per_second,
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
fn validate_security_config(config: &Config) {
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
