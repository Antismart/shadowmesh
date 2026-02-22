use axum::{
    body::Body,
    http::{header, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::time::Instant;

use crate::metrics;

/// Maximum request size check middleware
pub fn max_request_size(
    max_size_mb: u64,
) -> impl Fn(
    Request<Body>,
    Next,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>>
       + Clone {
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
                                format!("Request body too large. Max size: {} MB", max_size_mb),
                            )
                                .into_response();
                        }
                    }
                }
            }
            next.run(req).await
        })
    }
}

pub async fn security_headers(req: Request<Body>, next: Next) -> Response {
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
    let csp = if path.starts_with("/assets/")
        || path == "/"
        || path == "/metrics"
        || (!path.starts_with("/api/") && !path.starts_with("/ipfs/"))
    {
        // SPA dashboard pages and assets — allow inline styles (Tailwind), fonts, and GitHub avatars
        "default-src 'self'; style-src 'self' 'unsafe-inline'; script-src 'self'; img-src 'self' https://avatars.githubusercontent.com data:; font-src 'self' https://fonts.gstatic.com; connect-src 'self'"
    } else {
        "default-src 'self'"
    };

    if let Ok(value) = csp.parse() {
        headers.insert(header::CONTENT_SECURITY_POLICY, value);
    }

    // API version header for /api/ routes
    if path.starts_with("/api/") {
        if let Ok(value) = "1".parse() {
            headers.insert("X-API-Version", value);
        }
    }

    response
}

pub async fn request_id(mut req: Request<Body>, next: Next) -> Response {
    let request_id = uuid::Uuid::new_v4().to_string();
    req.extensions_mut().insert(request_id.clone());

    let mut response = next.run(req).await;
    if let Ok(value) = request_id.parse() {
        response.headers_mut().insert("X-Request-ID", value);
    }

    response
}

/// Request logging middleware - logs requests and records metrics
pub async fn request_logging(req: Request<Body>, next: Next) -> Response {
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
        .map(|part| {
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::get, Router};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    /// Trivial handler used by middleware tests.
    async fn ok_handler() -> &'static str {
        "ok"
    }

    // ── security_headers ────────────────────────────────────────────

    #[tokio::test]
    async fn security_headers_sets_x_content_type_options() {
        let app = Router::new()
            .route("/api/test", get(ok_handler))
            .layer(axum::middleware::from_fn(security_headers));

        let req = Request::builder()
            .uri("/api/test")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(
            resp.headers().get("x-content-type-options").unwrap(),
            "nosniff"
        );
    }

    #[tokio::test]
    async fn security_headers_sets_x_frame_options() {
        let app = Router::new()
            .route("/api/test", get(ok_handler))
            .layer(axum::middleware::from_fn(security_headers));

        let req = Request::builder()
            .uri("/api/test")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.headers().get("x-frame-options").unwrap(), "DENY");
    }

    #[tokio::test]
    async fn security_headers_sets_xss_protection() {
        let app = Router::new()
            .route("/api/test", get(ok_handler))
            .layer(axum::middleware::from_fn(security_headers));

        let req = Request::builder()
            .uri("/api/test")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(
            resp.headers().get("x-xss-protection").unwrap(),
            "1; mode=block"
        );
    }

    #[tokio::test]
    async fn security_headers_sets_hsts() {
        let app = Router::new()
            .route("/api/test", get(ok_handler))
            .layer(axum::middleware::from_fn(security_headers));

        let req = Request::builder()
            .uri("/api/test")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        let hsts = resp
            .headers()
            .get("strict-transport-security")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(hsts.contains("max-age="));
        assert!(hsts.contains("includeSubDomains"));
    }

    #[tokio::test]
    async fn security_headers_sets_api_version_for_api_routes() {
        let app = Router::new()
            .route("/api/deploy", get(ok_handler))
            .layer(axum::middleware::from_fn(security_headers));

        let req = Request::builder()
            .uri("/api/deploy")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.headers().get("x-api-version").unwrap(), "1");
    }

    #[tokio::test]
    async fn security_headers_no_api_version_for_non_api_routes() {
        let app = Router::new()
            .route("/metrics", get(ok_handler))
            .layer(axum::middleware::from_fn(security_headers));

        let req = Request::builder()
            .uri("/metrics")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert!(resp.headers().get("x-api-version").is_none());
    }

    #[tokio::test]
    async fn security_headers_csp_relaxed_for_dashboard() {
        let app = Router::new()
            .route("/", get(ok_handler))
            .layer(axum::middleware::from_fn(security_headers));

        let req = Request::builder()
            .uri("/")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        let csp = resp
            .headers()
            .get("content-security-policy")
            .unwrap()
            .to_str()
            .unwrap();
        // Dashboard CSP should allow unsafe-inline styles for Tailwind
        assert!(csp.contains("'unsafe-inline'"));
    }

    #[tokio::test]
    async fn security_headers_csp_strict_for_api() {
        let app = Router::new()
            .route("/api/data", get(ok_handler))
            .layer(axum::middleware::from_fn(security_headers));

        let req = Request::builder()
            .uri("/api/data")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        let csp = resp
            .headers()
            .get("content-security-policy")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(csp, "default-src 'self'");
    }

    #[tokio::test]
    async fn security_headers_preserves_response_body() {
        let app = Router::new()
            .route("/api/echo", get(ok_handler))
            .layer(axum::middleware::from_fn(security_headers));

        let req = Request::builder()
            .uri("/api/echo")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"ok");
    }

    // ── normalize_endpoint ──────────────────────────────────────────

    #[test]
    fn normalize_replaces_cid_starting_with_qm() {
        let result = normalize_endpoint("/api/deployments/QmXyz123456789012345678901234567890");
        assert_eq!(result, "/api/deployments/:cid");
    }

    #[test]
    fn normalize_replaces_cid_starting_with_bafy() {
        let result = normalize_endpoint("/ipfs/bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi");
        assert_eq!(result, "/ipfs/:cid");
    }

    #[test]
    fn normalize_replaces_uuid_long_segment() {
        // A standard UUID with dashes is 36 chars (> 30), so the length
        // check fires first and normalizes it as ":cid".
        let result = normalize_endpoint("/api/items/550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(result, "/api/items/:cid");
    }

    #[test]
    fn normalize_replaces_uuid_short_segment() {
        // A UUID without dashes is 32 chars (> 30), still caught by length.
        let result = normalize_endpoint("/api/items/550e8400e29b41d4a716446655440000");
        assert_eq!(result, "/api/items/:cid");
    }

    #[test]
    fn normalize_leaves_short_segments_alone() {
        let result = normalize_endpoint("/api/deploy");
        assert_eq!(result, "/api/deploy");
    }

    #[test]
    fn normalize_root_path() {
        let result = normalize_endpoint("/");
        assert_eq!(result, "/");
    }

    #[test]
    fn normalize_metrics_path() {
        let result = normalize_endpoint("/metrics");
        assert_eq!(result, "/metrics");
    }
}
