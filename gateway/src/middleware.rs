use axum::{
    body::Body,
    http::{Request, header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

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

    // Security headers
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        "nosniff".parse().unwrap(),
    );
    headers.insert(
        header::X_FRAME_OPTIONS,
        "DENY".parse().unwrap(),
    );
    headers.insert(
        "X-XSS-Protection",
        "1; mode=block".parse().unwrap(),
    );
    headers.insert(
        header::STRICT_TRANSPORT_SECURITY,
        "max-age=31536000; includeSubDomains".parse().unwrap(),
    );
    let csp = if path == "/dashboard" || path == "/metrics" {
        "default-src 'self'; style-src 'self' 'unsafe-inline'; script-src 'self' 'unsafe-inline'"
    } else {
        "default-src 'self'"
    };

    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        csp.parse().unwrap(),
    );

    response
}

pub async fn request_id(
    mut req: Request<Body>,
    next: Next,
) -> Response {
    let request_id = uuid::Uuid::new_v4().to_string();
    req.extensions_mut().insert(request_id.clone());

    let mut response = next.run(req).await;
    response.headers_mut().insert(
        "X-Request-ID",
        request_id.parse().unwrap(),
    );

    response
}
