//! ShadowMesh Gateway

mod config;
mod error;
mod middleware;
mod rate_limit;

use axum::{
    extract::{Path, State},
    middleware as axum_middleware,
    response::{AppendHeaders, Html, IntoResponse, Json},
    routing::get,
    Router,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use protocol::StorageLayer;
use tower_http::cors::CorsLayer;
use config::Config;
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

    let storage = match StorageLayer::with_url(&config.ipfs.api_url).await {
        Ok(s) => {
            println!("✅ Connected to IPFS daemon at {}", config.ipfs.api_url);
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
        app = app.layer(CorsLayer::permissive());
    }

    app = app
        .layer(axum_middleware::from_fn(middleware::security_headers))
        .layer(axum_middleware::from_fn(middleware::request_id));

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
) -> impl IntoResponse {
    match &state.storage {
        Some(storage) => {
            match storage.retrieve_content(&cid).await {
                Ok(data) => {
                    let content_type = infer::get(&data)
                        .map(|t| t.mime_type())
                        .unwrap_or("application/octet-stream");

                    (
                        AppendHeaders([(axum::http::header::CONTENT_TYPE, content_type)]),
                        data
                    ).into_response()
                }
                Err(e) => {
                    (
                        axum::http::StatusCode::NOT_FOUND,
                        Html(format!(r#"<html><body><h1>Not Found</h1><p>{}</p></body></html>"#, e))
                    ).into_response()
                }
            }
        }
        None => {
            (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                Html(r#"<html><body><h1>IPFS Not Connected</h1></body></html>"#)
            ).into_response()
        }
    }
}

async fn content_path_handler(
    State(state): State<AppState>,
    Path((cid, path)): Path<(String, String)>,
) -> impl IntoResponse {
    let full_path = format!("{}/{}", cid, path);
    
    match &state.storage {
        Some(storage) => {
            match storage.retrieve_content(&full_path).await {
                Ok(data) => {
                    let content_type = infer::get(&data)
                        .map(|t| t.mime_type())
                        .unwrap_or("application/octet-stream");

                    (
                        AppendHeaders([(axum::http::header::CONTENT_TYPE, content_type)]),
                        data
                    ).into_response()
                }
                Err(e) => {
                    (
                        axum::http::StatusCode::NOT_FOUND,
                        Html(format!(r#"<html><body><h1>Not Found</h1><p>{}</p></body></html>"#, e))
                    ).into_response()
                }
            }
        }
        None => {
            (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                Html(r#"<html><body><h1>IPFS Not Connected</h1></body></html>"#)
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
    config: ConfigMetrics,
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

    Json(MetricsResponse {
        uptime_seconds: uptime,
        requests_total: state.metrics.requests_total.load(Ordering::Relaxed),
        requests_success: state.metrics.requests_success.load(Ordering::Relaxed),
        requests_error: state.metrics.requests_error.load(Ordering::Relaxed),
        bytes_served: state.metrics.bytes_served.load(Ordering::Relaxed),
        config: ConfigMetrics {
            cache_max_size_mb: state.config.cache.max_size_mb,
            cache_ttl_seconds: state.config.cache.ttl_seconds,
            rate_limit_enabled: state.config.rate_limit.enabled,
            rate_limit_rps: state.config.rate_limit.requests_per_second,
            ipfs_connected: state.storage.is_some(),
        },
    })
}