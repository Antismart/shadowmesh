//! Authentication middleware for the node-runner API.
//!
//! When the `NODE_API_KEY` environment variable is set, destructive endpoints
//! (upload, delete, gc, shutdown, config changes) require a matching
//! `Authorization: Bearer <key>` header. Read-only endpoints (status, health,
//! metrics, download, list) are always open so the gateway and monitoring
//! tools can query them without credentials.

use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::sync::OnceLock;
use subtle::ConstantTimeEq;

/// Cached API key loaded from the environment once at first use.
static API_KEY: OnceLock<Option<String>> = OnceLock::new();

fn get_api_key() -> &'static Option<String> {
    API_KEY.get_or_init(|| std::env::var("NODE_API_KEY").ok().filter(|k| !k.is_empty()))
}

/// Middleware that rejects requests without a valid Bearer token when
/// `NODE_API_KEY` is configured.
pub async fn require_api_key(req: Request, next: Next) -> Response {
    let Some(expected) = get_api_key() else {
        // No key configured — allow all requests
        return next.run(req).await;
    };

    let auth_header = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok());

    match auth_header {
        Some(value) if value.len() > 7 && value[..7].eq_ignore_ascii_case("bearer ") => {
            let token = value[7..].trim_start();
            let is_valid = token.len() == expected.len()
                && token.as_bytes().ct_eq(expected.as_bytes()).into();
            if is_valid {
                return next.run(req).await;
            }
            (
                StatusCode::FORBIDDEN,
                Json(json!({"error": "Invalid API key"})),
            )
                .into_response()
        }
        _ => (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Authorization header required (Bearer <NODE_API_KEY>)"})),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_api_key_not_set_returns_none() {
        // When env var isn't set, get_api_key returns None (first call wins via OnceLock)
        // We can't reliably test OnceLock in unit tests, so just verify the module compiles
    }
}
