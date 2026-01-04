//! ShadowMesh Gateway

use axum::{
    extract::{Path, State},
    response::{AppendHeaders, Html, IntoResponse},
    routing::get,
    Router,
};
use std::sync::Arc;
use protocol::StorageLayer;
use tower_http::cors::CorsLayer;

#[derive(Clone)]
struct AppState {
    storage: Option<Arc<StorageLayer>>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let storage = match StorageLayer::new().await {
        Ok(s) => {
            println!("✅ Connected to IPFS daemon");
            Some(Arc::new(s))
        }
        Err(e) => {
            println!("⚠️  IPFS daemon not available: {}", e);
            println!("   Running in demo mode");
            None
        }
    };

    let state = AppState { storage };

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/:cid", get(content_handler))
        .route("/:cid/*path", get(content_path_handler))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8081")
        .await
        .unwrap();

    println!("🌐 Gateway running at http://localhost:8081");
    
    axum::serve(listener, app).await.unwrap();
}

async fn index_handler() -> Html<&'static str> {
    Html(r#"
        <!DOCTYPE html>
        <html>
        <head>
            <title>ShadowMesh Gateway</title>
            <style>
                body { 
                    font-family: system-ui; 
                    max-width: 800px; 
                    margin: 100px auto;
                    padding: 20px;
                }
                h1 { color: #333; }
                code { 
                    background: #f4f4f4; 
                    padding: 2px 6px; 
                    border-radius: 3px;
                }
                .example {
                    background: #f9f9f9;
                    padding: 15px;
                    border-left: 4px solid #4CAF50;
                    margin: 20px 0;
                }
            </style>
        </head>
        <body>
            <h1>🌐 ShadowMesh Gateway</h1>
            <p>Access censorship-resistant content.</p>
            
            <div class="example">
                <h3>Usage:</h3>
                <p>Access: <code>http://localhost:8081/{CID}</code></p>
            </div>
            
            <p style="color: #666; margin-top: 40px;">
                Status: <span style="color: #4CAF50;">● Online</span>
            </p>
        </body>
        </html>
    "#)
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