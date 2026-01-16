//! ShadowMesh Gateway

mod cache;
mod config;
mod error;
mod middleware;
mod rate_limit;

use axum::{
    extract::{Path, State},
    middleware as axum_middleware,
    response::{Html, IntoResponse, Json, Response},
    routing::get,
    Router,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use protocol::{StorageLayer, StorageConfig};
use tower_http::cors::{CorsLayer, Any};
use config::Config;
use cache::ContentCache;
use serde::Serialize;

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

#[derive(Clone)]
struct AppState {
    storage: Option<Arc<StorageLayer>>,
    cache: Arc<ContentCache>,
    config: Arc<Config>,
    metrics: Arc<Metrics>,
    start_time: Instant,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    // Load configuration
    let config = match Config::load() {
        Ok(c) => {
            println!("✓ Loaded configuration");
            Arc::new(c)
        }
        Err(e) => {
            println!("⚠️  Failed to load configuration: {}", e);
            println!("   Using default configuration");
            Arc::new(Config::default())
        }
    };

    // Initialize content cache
    let cache = Arc::new(ContentCache::with_config(
        (config.cache.max_size_mb * 10) as usize, // Rough estimate: ~100KB per entry
        Duration::from_secs(config.cache.ttl_seconds),
    ));
    println!("✓ Cache initialized: {} MB, TTL {}s", 
        config.cache.max_size_mb, 
        config.cache.ttl_seconds);

    // Create IPFS storage config
    let storage_config = StorageConfig {
        api_url: config.ipfs.api_url.clone(),
        timeout: Duration::from_secs(config.ipfs.timeout_seconds),
        retry_attempts: config.ipfs.retry_attempts,
    };

    let storage = match StorageLayer::with_config(storage_config).await {
        Ok(s) => {
            println!("✅ Connected to IPFS daemon at {}", config.ipfs.api_url);
            println!("   Timeout: {}s, Retries: {}", 
                config.ipfs.timeout_seconds, 
                config.ipfs.retry_attempts);
            Some(Arc::new(s))
        }
        Err(e) => {
            println!("⚠️  IPFS daemon not available: {}", e);
            println!("   Running in demo mode");
            None
        }
    };

    let state = AppState {
        storage,
        cache,
        config: config.clone(),
        metrics: Arc::new(Metrics::default()),
        start_time: Instant::now(),
    };

    // Create rate limiter
    let rate_limiter = if config.rate_limit.enabled {
        Some(rate_limit::RateLimiter::new(
            config.rate_limit.requests_per_second,
        ))
    } else {
        None
    };

    let mut app = Router::new()
        .route("/", get(index_handler))
        .route("/health", get(health_handler))
        .route("/metrics", get(metrics_handler))
        .route("/:cid", get(content_handler))
        .route("/:cid/*path", get(content_path_handler))
        .with_state(state);

    // Add middleware layers
    if config.security.cors_enabled {
        let cors = if config.security.allowed_origins.contains(&"*".to_string()) {
            CorsLayer::permissive()
        } else {
            let origins: Vec<_> = config.security.allowed_origins
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

    app = app
        .layer(axum_middleware::from_fn(middleware::security_headers))
        .layer(axum_middleware::from_fn(middleware::request_id))
        .layer(axum_middleware::from_fn(
            middleware::max_request_size(config.security.max_request_size_mb)
        ));

    if let Some(limiter) = rate_limiter {
        let limiter_clone = limiter.clone();
        app = app.layer(axum_middleware::from_fn(
            move |req, next| {
                let limiter = limiter_clone.clone();
                async move { limiter.check_rate_limit(req, next).await }
            },
        ));
    }

    let bind_addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .unwrap_or_else(|e| {
            eprintln!("✗ Failed to bind to {}: {}", bind_addr, e);
            std::process::exit(1);
        });

    println!("🌐 Gateway running at http://localhost:{}", config.server.port);
    println!("   Workers: {}", config.server.workers);
    println!("   Cache: {} MB (TTL: {}s)", config.cache.max_size_mb, config.cache.ttl_seconds);
    if config.rate_limit.enabled {
        println!("   Rate limit: {} req/s (burst: {})",
            config.rate_limit.requests_per_second,
            config.rate_limit.burst_size);
    }
    if config.monitoring.metrics_enabled {
        println!("   Metrics: http://localhost:{}/metrics", config.server.port);
        println!("   Health: http://localhost:{}/health", config.server.port);
    }

    axum::serve(listener, app).await.unwrap();
}

async fn index_handler(State(state): State<AppState>) -> Html<String> {
    let port = state.config.server.port;
    let html = format!(r#"
        <!DOCTYPE html>
        <html>
        <head>
            <title>ShadowMesh Gateway</title>
            <style>
                body {{
                    font-family: system-ui;
                    max-width: 800px;
                    margin: 100px auto;
                    padding: 20px;
                }}
                h1 {{ color: #333; }}
                code {{
                    background: #f4f4f4;
                    padding: 2px 6px;
                    border-radius: 3px;
                }}
                .example {{
                    background: #f9f9f9;
                    padding: 15px;
                    border-left: 4px solid #4CAF50;
                    margin: 20px 0;
                }}
                .config {{
                    background: #f0f0f0;
                    padding: 10px;
                    border-radius: 5px;
                    margin: 20px 0;
                    font-size: 0.9em;
                }}
            </style>
        </head>
        <body>
            <h1>🌐 ShadowMesh Gateway</h1>
            <p>Access censorship-resistant content through a decentralized network.</p>

            <div class="example">
                <h3>Usage:</h3>
                <p>Access: <code>http://localhost:{}/$CID</code></p>
            </div>

            <div class="config">
                <h3>Configuration:</h3>
                <ul>
                    <li>Port: {}</li>
                    <li>Cache: {} MB (TTL: {}s)</li>
                    <li>Rate Limit: {} req/s (burst: {})</li>
                    <li>IPFS: {}</li>
                </ul>
            </div>

            <p style="color: #666; margin-top: 40px;">
                Status: <span style="color: #4CAF50;">● Online</span>
            </p>
        </body>
        </html>
    "#,
        port,
        port,
        state.config.cache.max_size_mb,
        state.config.cache.ttl_seconds,
        state.config.rate_limit.requests_per_second,
        state.config.rate_limit.burst_size,
        state.config.ipfs.api_url
    );

    Html(html)
}

async fn content_handler(
    State(state): State<AppState>,
    Path(cid): Path<String>,
) -> Response {
    state.metrics.increment_requests();

    // Check cache first
    if let Some((data, content_type)) = state.cache.get(&cid) {
        state.metrics.increment_success();
        state.metrics.add_bytes(data.len() as u64);
        return (
            [
                (axum::http::header::CONTENT_TYPE, content_type),
                (axum::http::header::HeaderName::from_static("x-cache"), "HIT".to_string()),
            ],
            data
        ).into_response();
    }

    // Fetch from IPFS
    let Some(storage) = &state.storage else {
        state.metrics.increment_error();
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Html(r#"<html><body><h1>IPFS Not Connected</h1></body></html>"#)
        ).into_response();
    };

    let storage = Arc::clone(storage);
    let cid_clone = cid.clone();
    
    // Spawn blocking task for IPFS retrieval (handles non-Send stream)
    let result = tokio::task::spawn_blocking(move || {
        tokio::runtime::Handle::current().block_on(async {
            storage.retrieve_content(&cid_clone).await
        })
    }).await;

    match result {
        Ok(Ok(data)) => {
            let content_type = infer::get(&data)
                .map(|t| t.mime_type().to_string())
                .unwrap_or_else(|| "application/octet-stream".to_string());

            // Store in cache
            state.cache.set(cid, data.clone(), content_type.clone());
            
            state.metrics.increment_success();
            state.metrics.add_bytes(data.len() as u64);

            (
                [
                    (axum::http::header::CONTENT_TYPE, content_type),
                    (axum::http::header::HeaderName::from_static("x-cache"), "MISS".to_string()),
                ],
                data
            ).into_response()
        }
        Ok(Err(e)) => {
            state.metrics.increment_error();
            (
                axum::http::StatusCode::NOT_FOUND,
                Html(format!(r#"<html><body><h1>Not Found</h1><p>{}</p></body></html>"#, e))
            ).into_response()
        }
        Err(e) => {
            state.metrics.increment_error();
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Html(format!(r#"<html><body><h1>Error</h1><p>{}</p></body></html>"#, e))
            ).into_response()
        }
    }
}

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
                (axum::http::header::HeaderName::from_static("x-cache"), "HIT".to_string()),
            ],
            data
        ).into_response();
    }

    // Fetch from IPFS
    let Some(storage) = &state.storage else {
        state.metrics.increment_error();
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Html(r#"<html><body><h1>IPFS Not Connected</h1></body></html>"#)
        ).into_response();
    };

    let storage = Arc::clone(storage);
    let path_clone = full_path.clone();
    
    // Spawn blocking task for IPFS retrieval (handles non-Send stream)
    let result = tokio::task::spawn_blocking(move || {
        tokio::runtime::Handle::current().block_on(async {
            storage.retrieve_content(&path_clone).await
        })
    }).await;

    match result {
        Ok(Ok(data)) => {
            let content_type = infer::get(&data)
                .map(|t| t.mime_type().to_string())
                .unwrap_or_else(|| "application/octet-stream".to_string());

            // Store in cache
            state.cache.set(full_path, data.clone(), content_type.clone());
            
            state.metrics.increment_success();
            state.metrics.add_bytes(data.len() as u64);

            (
                [
                    (axum::http::header::CONTENT_TYPE, content_type),
                    (axum::http::header::HeaderName::from_static("x-cache"), "MISS".to_string()),
                ],
                data
            ).into_response()
        }
        Ok(Err(e)) => {
            state.metrics.increment_error();
            (
                axum::http::StatusCode::NOT_FOUND,
                Html(format!(r#"<html><body><h1>Not Found</h1><p>{}</p></body></html>"#, e))
            ).into_response()
        }
        Err(e) => {
            state.metrics.increment_error();
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Html(format!(r#"<html><body><h1>Error</h1><p>{}</p></body></html>"#, e))
            ).into_response()
        }
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
        status: if ipfs_connected { "healthy" } else { "degraded" },
        version: env!("CARGO_PKG_VERSION"),
        uptime_seconds: uptime,
        ipfs_connected,
    })
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