//! ShadowMesh Gateway

mod config;

use axum::{
    extract::{Path, State},
    response::{AppendHeaders, Html, IntoResponse},
    routing::get,
    Router,
};
use std::sync::Arc;
use protocol::StorageLayer;
use tower_http::cors::CorsLayer;
use config::GatewayConfig;

#[derive(Clone)]
struct AppState {
    storage: Option<Arc<StorageLayer>>,
    config: Arc<GatewayConfig>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    // Load configuration
    let config = match GatewayConfig::from_file("gateway/config.toml") {
        Ok(c) => {
            println!("✓ Loaded configuration from gateway/config.toml");
            Arc::new(c)
        }
        Err(e) => {
            eprintln!("✗ Failed to load configuration: {}", e);
            eprintln!("  Please ensure gateway/config.toml exists and is valid");
            std::process::exit(1);
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
    };

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/:cid", get(content_handler))
        .route("/:cid/*path", get(content_path_handler))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let bind_addr = config.server_address();
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .unwrap_or_else(|e| {
            eprintln!("✗ Failed to bind to {}: {}", bind_addr, e);
            std::process::exit(1);
        });

    println!("🌐 Gateway running at http://localhost:{}", config.server.port);
    println!("   Workers: {}", config.server.workers);
    println!("   Cache: {} MB (TTL: {}s)", config.cache.max_size_mb, config.cache.ttl_seconds);
    println!("   Rate limit: {} req/s", config.rate_limit.requests_per_second);

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