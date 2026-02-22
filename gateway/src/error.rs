use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

#[allow(dead_code)]
#[derive(Error, Debug)]
pub enum GatewayError {
    #[error("IPFS error: {0}")]
    IpfsError(String),

    #[error("Invalid CID: {0}")]
    InvalidCid(String),

    #[error("Content not found: {0}")]
    NotFound(String),

    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    #[error("Request timeout")]
    Timeout,

    #[error("Service unavailable: {0}")]
    ServiceUnavailable(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl IntoResponse for GatewayError {
    fn into_response(self) -> Response {
        let (status, error_message) = match self {
            GatewayError::IpfsError(msg) => (StatusCode::BAD_GATEWAY, msg),
            GatewayError::InvalidCid(msg) => (StatusCode::BAD_REQUEST, msg),
            GatewayError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            GatewayError::RateLimitExceeded => (
                StatusCode::TOO_MANY_REQUESTS,
                "Rate limit exceeded".to_string(),
            ),
            GatewayError::Timeout => (StatusCode::GATEWAY_TIMEOUT, "Request timeout".to_string()),
            GatewayError::ServiceUnavailable(msg) => (StatusCode::SERVICE_UNAVAILABLE, msg),
            GatewayError::Internal(msg) => {
                tracing::error!("Internal error: {}", msg);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
        };

        let body = Json(json!({
            "error": error_message,
            "status": status.as_u16(),
        }));

        (status, body).into_response()
    }
}
