//! ShadowMesh Gateway

mod cache;
mod config;
mod dashboard;
mod deploy;
mod error;
mod middleware;
mod rate_limit;
mod upload;

use axum::{
    extract::{Path, State},
    middleware as axum_middleware,
    response::{Html, IntoResponse, Json, Response},
    routing::{delete, get, post},
    Router,
};
use std::sync::{Arc, RwLock};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use protocol::{StorageLayer, StorageConfig};
use tower_http::cors::{CorsLayer, Any};
use config::Config;
use cache::ContentCache;
use serde::Serialize;
use dashboard::Deployment;

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
pub struct AppState {
    pub storage: Option<Arc<StorageLayer>>,
    pub cache: Arc<ContentCache>,
    pub config: Arc<Config>,
    pub metrics: Arc<Metrics>,
    pub start_time: Instant,
    pub deployments: Arc<RwLock<Vec<Deployment>>>,
    pub github_auth: Arc<RwLock<Option<dashboard::GithubAuth>>>,
    pub github_oauth_state: Arc<RwLock<Option<String>>>,
}

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    let _ = dotenvy::from_filename("gateway/.env");
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
        deployments: Arc::new(RwLock::new(Vec::new())),
        github_auth: Arc::new(RwLock::new(None)),
        github_oauth_state: Arc::new(RwLock::new(None)),
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
        .route("/dashboard", get(dashboard::dashboard_handler))
        .route("/health", get(health_handler))
        .route("/metrics", get(metrics_handler))
        // Deploy endpoints (Vercel-like)
        .route("/api/deploy", post(deploy::deploy_zip))
        .route("/api/deploy/info", get(deploy::deploy_info))
        .route("/api/deploy/github", post(dashboard::deploy_from_github))
        .route("/api/deployments", get(dashboard::get_deployments))
    .route("/api/deployments/:cid", delete(dashboard::delete_deployment))
    .route("/api/deployments/:cid/redeploy", post(dashboard::redeploy_github))
    .route("/api/deployments/:cid/logs", get(dashboard::deployment_logs))
    .route("/api/github/login", get(dashboard::github_login))
    .route("/api/github/callback", get(dashboard::github_callback))
    .route("/api/github/status", get(dashboard::github_status))
    .route("/api/github/repos", get(dashboard::github_repos))
        // Upload endpoints (single files)
        .route("/api/upload", post(upload::upload_multipart))
        .route("/api/upload/json", post(upload::upload_json))
        .route("/api/upload/raw", post(upload::upload_raw))
        .route("/api/upload/batch", post(upload::upload_batch))
        // Content retrieval
        .route("/:cid", get(content_handler))
        .route("/ipfs/*path", get(ipfs_content_path_handler))
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
    println!("   Deploy: POST /api/deploy (ZIP, max 100 MB)");
    println!("   Upload: POST /api/upload (single file, max 50 MB)");
    if config.monitoring.metrics_enabled {
        println!("   Metrics: http://localhost:{}/metrics", config.server.port);
        println!("   Health: http://localhost:{}/health", config.server.port);
    }

    axum::serve(listener, app).await.unwrap();
}

async fn index_handler(State(state): State<AppState>) -> Html<String> {
    let port = state.config.server.port;
    let ipfs_status = if state.storage.is_some() { "Connected" } else { "Disconnected" };
    let status_color = if state.storage.is_some() { "#4CAF50" } else { "#f44336" };
    
    let html = format!(r#"
        <!DOCTYPE html>
        <html>
        <head>
            <title>ShadowMesh Gateway</title>
            <style>
                body {{
                    font-family: system-ui;
                    max-width: 900px;
                    margin: 50px auto;
                    padding: 20px;
                    line-height: 1.6;
                }}
                h1 {{ color: #333; }}
                h2 {{ color: #555; margin-top: 30px; }}
                h3 {{ color: #666; margin-top: 20px; }}
                code {{
                    background: #f4f4f4;
                    padding: 2px 6px;
                    border-radius: 3px;
                    font-size: 0.9em;
                }}
                pre {{
                    background: #2d2d2d;
                    color: #f8f8f2;
                    padding: 15px;
                    border-radius: 5px;
                    overflow-x: auto;
                    font-size: 0.85em;
                }}
                .section {{
                    background: #f9f9f9;
                    padding: 20px;
                    border-left: 4px solid #4CAF50;
                    margin: 20px 0;
                    border-radius: 0 5px 5px 0;
                }}
                .api-endpoint {{
                    background: #e8f5e9;
                    padding: 10px 15px;
                    margin: 10px 0;
                    border-radius: 5px;
                }}
                .method {{
                    display: inline-block;
                    padding: 3px 8px;
                    border-radius: 3px;
                    font-weight: bold;
                    font-size: 0.8em;
                    margin-right: 10px;
                }}
                .method-get {{ background: #61affe; color: white; }}
                .method-post {{ background: #49cc90; color: white; }}
                .config {{
                    background: #f0f0f0;
                    padding: 15px;
                    border-radius: 5px;
                    margin: 20px 0;
                }}
                table {{
                    width: 100%;
                    border-collapse: collapse;
                    margin: 15px 0;
                }}
                th, td {{
                    text-align: left;
                    padding: 10px;
                    border-bottom: 1px solid #ddd;
                }}
                th {{ background: #f5f5f5; }}
            </style>
        </head>
        <body>
            <h1>🌐 ShadowMesh Gateway</h1>
            <p>Deploy and access censorship-resistant content through a decentralized network.</p>

            <h2>� Deploy API (Vercel-like)</h2>
            
            <div class="section" style="border-left-color: #9c27b0;">
                <h3>Deploy a Website</h3>
                <div class="api-endpoint" style="background: #f3e5f5;">
                    <span class="method method-post">POST</span>
                    <code>/api/deploy</code>
                </div>
                <p>Deploy a complete static website from a ZIP file:</p>
                <pre># 1. Create a ZIP of your website
cd my-website
zip -r ../site.zip .

# 2. Deploy to ShadowMesh
curl -X POST http://localhost:{}/api/deploy -F "file=@../site.zip"</pre>
                <p><strong>Your site will be live at the returned URL with full directory structure!</strong></p>
            </div>

            <h2>�📤 Upload API (Single Files)</h2>
            
            <div class="section">
                <h3>Multipart Upload (Recommended)</h3>
                <div class="api-endpoint">
                    <span class="method method-post">POST</span>
                    <code>/api/upload</code>
                </div>
                <p>Upload a file using multipart form data:</p>
                <pre>curl -X POST http://localhost:{}/api/upload \
  -F "file=@./index.html"</pre>
            </div>

            <div class="section">
                <h3>JSON Upload (Base64)</h3>
                <div class="api-endpoint">
                    <span class="method method-post">POST</span>
                    <code>/api/upload/json</code>
                </div>
                <p>Upload base64-encoded content:</p>
                <pre>curl -X POST http://localhost:{}/api/upload/json \
  -H "Content-Type: application/json" \
  -d '{{"data": "PGh0bWw+SGVsbG88L2h0bWw+", "filename": "index.html"}}'</pre>
            </div>

            <div class="section">
                <h3>Raw Upload</h3>
                <div class="api-endpoint">
                    <span class="method method-post">POST</span>
                    <code>/api/upload/raw</code>
                </div>
                <p>Upload raw binary data:</p>
                <pre>curl -X POST http://localhost:{}/api/upload/raw \
  -H "Content-Type: text/html" \
  --data-binary @./index.html</pre>
            </div>

            <div class="section">
                <h3>Batch Upload</h3>
                <div class="api-endpoint">
                    <span class="method method-post">POST</span>
                    <code>/api/upload/batch</code>
                </div>
                <p>Upload multiple files at once:</p>
                <pre>curl -X POST http://localhost:{}/api/upload/batch \
  -H "Content-Type: application/json" \
  -d '{{"files": [{{"data": "...", "filename": "a.html"}}, {{"data": "..."}}]}}'</pre>
            </div>

            <h2>📥 Content Retrieval</h2>
            
            <div class="section">
                <div class="api-endpoint">
                    <span class="method method-get">GET</span>
                    <code>/{{cid}}</code> or <code>/ipfs/{{cid}}</code>
                </div>
                <p>Retrieve content by CID:</p>
                <pre>curl http://localhost:{}/QmExample...</pre>
            </div>

            <h2>📊 Response Format</h2>
            <div class="config">
                <p>Successful uploads return:</p>
                <pre>{{
  "success": true,
  "cid": "QmExample...",
  "gateway_url": "http://localhost:{}/QmExample...",
  "shadow_url": "shadow://QmExample...",
  "size": 1234,
  "content_type": "text/html",
  "filename": "index.html"
}}</pre>
            </div>

            <h2>⚙️ Configuration</h2>
            <div class="config">
                <table>
                    <tr><th>Setting</th><th>Value</th></tr>
                    <tr><td>Port</td><td>{}</td></tr>
                    <tr><td>Cache</td><td>{} MB (TTL: {}s)</td></tr>
                    <tr><td>Rate Limit</td><td>{} req/s (burst: {})</td></tr>
                    <tr><td>IPFS</td><td>{}</td></tr>
                    <tr><td>Max Deploy</td><td>100 MB (ZIP)</td></tr>
                    <tr><td>Max Upload</td><td>50 MB (single file)</td></tr>
                </table>
            </div>

            <p style="color: #666; margin-top: 40px;">
                IPFS Status: <span style="color: {};">● {}</span> |
                <a href="/health">Health</a> |
                <a href="/metrics">Metrics</a>
            </p>
        </body>
        </html>
    "#,
        port, port, port, port, port, port, port,
        port,
        state.config.cache.max_size_mb,
        state.config.cache.ttl_seconds,
        state.config.rate_limit.requests_per_second,
        state.config.rate_limit.burst_size,
        state.config.ipfs.api_url,
        status_color,
        ipfs_status
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

// Handler for /:cid/*path - access files within a directory CID
async fn content_with_path_handler(
    State(state): State<AppState>,
    Path((cid, path)): Path<(String, String)>,
) -> Response {
    let full_path = format!("{}/{}", cid, path);
    fetch_content(&state, full_path, Some(format!("/{}/", cid))).await
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

// Unified content fetching logic
async fn fetch_content(state: &AppState, cid: String, base_prefix: Option<String>) -> Response {
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
    
    let result = tokio::task::spawn_blocking(move || {
        tokio::runtime::Handle::current().block_on(async {
            storage.retrieve_content(&cid_clone).await
        })
    }).await;

    match result {
        Ok(Ok(data)) => {
            let content_type = content_type_from_path(&cid, &data);
            let data = rewrite_html_assets(&data, &content_type, base_prefix.as_deref());

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

fn content_type_from_path(path: &str, data: &[u8]) -> String {
    let lower = path.to_lowercase();
    if lower.ends_with(".html") || lower.ends_with(".htm") || looks_like_html(data) {
        return "text/html".to_string();
    }
    if lower.ends_with(".css") {
        return "text/css".to_string();
    }
    if lower.ends_with(".js") || lower.ends_with(".mjs") || lower.ends_with(".jsx") || lower.ends_with(".ts") {
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
        html = html.replace(&format!("{}=\"/", attr), &format!("{}=\"{}", attr, base_prefix));
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