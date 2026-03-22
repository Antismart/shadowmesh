//! Pin/unpin handlers for the ShadowMesh gateway
//!
//! Allows callers to pin or unpin content on the local IPFS node so it is
//! retained (or released) across garbage-collection cycles.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

use crate::{audit, cid_validation, AppState};

// ── Response types ──────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct PinResponse {
    pub success: bool,
    pub cid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PinError {
    pub success: bool,
    pub error: String,
    pub code: String,
}

impl PinError {
    fn new(error: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            success: false,
            error: error.into(),
            code: code.into(),
        }
    }
}

// ── Handlers ────────────────────────────────────────────────────────────

/// POST /api/pin/:cid — pin content on the local IPFS node.
pub async fn pin_content(State(state): State<AppState>, Path(cid): Path<String>) -> Response {
    let Some(storage) = &state.storage else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(PinError::new("Storage backend not available", "STORAGE_UNAVAILABLE")),
        )
            .into_response();
    };

    if !cid_validation::validate_cid(&cid) {
        return (
            StatusCode::BAD_REQUEST,
            Json(PinError::new("Invalid CID format", "INVALID_CID")),
        )
            .into_response();
    }

    match storage.pin_content(&cid).await {
        Ok(()) => {
            state.audit_logger.log(
                audit::AuditEvent::new(
                    audit::AuditAction::FileUpload,
                    audit::AuditOutcome::Success,
                    "system",
                )
                .with_details(format!("Pinned content: {}", cid)),
            ).await;

            (
                StatusCode::OK,
                Json(PinResponse {
                    success: true,
                    cid,
                    message: Some("Content pinned successfully".into()),
                }),
            )
                .into_response()
        }
        Err(e) => {
            tracing::warn!(cid = %cid, error = %e, "Failed to pin content");
            (
                StatusCode::BAD_REQUEST,
                Json(PinError::new(e.to_string(), "PIN_FAILED")),
            )
                .into_response()
        }
    }
}

/// DELETE /api/pin/:cid — unpin content from the local IPFS node.
pub async fn unpin_content(State(state): State<AppState>, Path(cid): Path<String>) -> Response {
    let Some(storage) = &state.storage else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(PinError::new("Storage backend not available", "STORAGE_UNAVAILABLE")),
        )
            .into_response();
    };

    if !cid_validation::validate_cid(&cid) {
        return (
            StatusCode::BAD_REQUEST,
            Json(PinError::new("Invalid CID format", "INVALID_CID")),
        )
            .into_response();
    }

    match storage.unpin_content(&cid).await {
        Ok(()) => {
            state.audit_logger.log(
                audit::AuditEvent::new(
                    audit::AuditAction::DeployDelete,
                    audit::AuditOutcome::Success,
                    "system",
                )
                .with_details(format!("Unpinned content: {}", cid)),
            ).await;

            (
                StatusCode::OK,
                Json(PinResponse {
                    success: true,
                    cid,
                    message: Some("Content unpinned successfully".into()),
                }),
            )
                .into_response()
        }
        Err(e) => {
            tracing::warn!(cid = %cid, error = %e, "Failed to unpin content");
            (
                StatusCode::BAD_REQUEST,
                Json(PinError::new(e.to_string(), "UNPIN_FAILED")),
            )
                .into_response()
        }
    }
}

/// GET /api/pin/:cid — check whether content is pinned.
pub async fn pin_status(State(state): State<AppState>, Path(cid): Path<String>) -> Response {
    let Some(storage) = &state.storage else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(PinError::new("Storage backend not available", "STORAGE_UNAVAILABLE")),
        )
            .into_response();
    };

    match storage.is_pinned(&cid).await {
        Ok(pinned) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "cid": cid,
                "pinned": pinned,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(PinError::new(e.to_string(), "PIN_STATUS_FAILED")),
        )
            .into_response(),
    }
}
