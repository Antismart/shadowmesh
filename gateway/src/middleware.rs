use axum::{
    body::Body,
    http::{Request, header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::time::Instant;

use crate::metrics;

/// Maximum request size check middleware
pub fn max_request_size(max_size_mb: u64) -> impl Fn(Request<Body>, Next) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> + Clone {
    move |req: Request<Body>, next: Next| {
        let max_bytes = max_size_mb * 1024 * 1024;
        Box::pin(async move {
            // Check Content-Length header if present
            if let Some(content_length) = req.headers().get(header::CONTENT_LENGTH) {
                if let Ok(len_str) = content_length.to_str() {
                    if let Ok(len) = len_str.parse::<u64>() {
                        if len > max_bytes {
                            return (
                                StatusCode::PAYLOAD_TOO_LARGE,
                                format!("Request body too large. Max size: {} MB", max_size_mb)
                            ).into_response();
                        }
                    }
                }
            }
            next.run(req).await
        })
    }
}

pub async fn security_headers(
    req: Request<Body>,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();
    let mut response = next.run(req).await;

    let headers = response.headers_mut();

    // Security headers - using if-let to handle parse errors gracefully
    if let Ok(value) = "nosniff".parse() {
        headers.insert(header::X_CONTENT_TYPE_OPTIONS, value);
    }
    if let Ok(value) = "DENY".parse() {
        headers.insert(header::X_FRAME_OPTIONS, value);
    }
    if let Ok(value) = "1; mode=block".parse() {
        headers.insert("X-XSS-Protection", value);
    }
    if let Ok(value) = "max-age=31536000; includeSubDomains".parse() {
        headers.insert(header::STRICT_TRANSPORT_SECURITY, value);
    }
    let csp = if path == "/dashboard" || path == "/metrics" {
        "default-src 'self'; style-src 'self' 'unsafe-inline'; script-src 'self' 'unsafe-inline'"
    } else {
        "default-src 'self'"
    };

    if let Ok(value) = csp.parse() {
        headers.insert(header::CONTENT_SECURITY_POLICY, value);
    }

    response
}

pub async fn request_id(
    mut req: Request<Body>,
    next: Next,
) -> Response {
    let request_id = uuid::Uuid::new_v4().to_string();
    req.extensions_mut().insert(request_id.clone());

    let mut response = next.run(req).await;
    if let Ok(value) = request_id.parse() {
        response.headers_mut().insert("X-Request-ID", value);
    }

    response
}

/// Request logging middleware - logs requests and records metrics
pub async fn request_logging(
    req: Request<Body>,
    next: Next,
) -> Response {
    let start = Instant::now();
    let method = req.method().to_string();
    let uri = req.uri().path().to_string();
    let request_id = req.extensions().get::<String>().cloned();

    // Track in-flight requests
    metrics::HTTP_REQUESTS_IN_FLIGHT.inc();

    let response = next.run(req).await;

    metrics::HTTP_REQUESTS_IN_FLIGHT.dec();

    let duration = start.elapsed();
    let status = response.status().as_u16();

    // Normalize endpoint for metrics (avoid high cardinality)
    let endpoint = normalize_endpoint(&uri);

    // Record metrics
    metrics::record_request(&method, &endpoint, status, duration.as_secs_f64());

    // Log the request
    tracing::info!(
        request_id = ?request_id,
        method = %method,
        path = %uri,
        status = %status,
        duration_ms = %duration.as_millis(),
        "request completed"
    );

    response
}

/// Normalize endpoint paths to reduce cardinality in metrics
/// e.g., /api/deployments/QmXxx -> /api/deployments/:cid
fn normalize_endpoint(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').collect();

    let normalized: Vec<&str> = parts
        .iter()
        .enumerate()
        .map(|(i, part)| {
            // Check if this looks like a CID or UUID
            if part.starts_with("Qm") || part.starts_with("bafy") || part.len() > 30 {
                ":cid"
            } else if uuid::Uuid::parse_str(part).is_ok() {
                ":id"
            } else {
                part
            }
        })
        .collect();

    normalized.join("/")
}
