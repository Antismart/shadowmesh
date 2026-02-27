//! Upload handlers for the ShadowMesh gateway
//!
//! Provides endpoints for users to deploy content to the network.

use axum::{
    extract::{Multipart, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{audit, AppState};

/// Maximum upload size (50 MB)
const MAX_UPLOAD_SIZE: usize = 50 * 1024 * 1024;

/// Upload response returned after successful content deployment
#[derive(Debug, Serialize)]
pub struct UploadResponse {
    /// Whether the upload was successful
    pub success: bool,
    /// Content identifier (CID) for accessing the content
    pub cid: String,
    /// Gateway URL for HTTP access
    pub gateway_url: String,
    /// Native ShadowMesh URL
    pub shadow_url: String,
    /// Size of the uploaded content in bytes
    pub size: usize,
    /// Detected MIME type
    pub content_type: String,
    /// Optional filename if provided
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
}

/// Error response for failed uploads
#[derive(Debug, Serialize)]
pub struct UploadError {
    pub success: bool,
    pub error: String,
    pub code: String,
}

impl UploadError {
    fn new(error: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            success: false,
            error: error.into(),
            code: code.into(),
        }
    }
}

/// JSON upload request with base64-encoded data
#[derive(Debug, Deserialize)]
pub struct JsonUploadRequest {
    /// Base64-encoded content data
    pub data: String,
    /// Optional filename
    #[serde(default)]
    pub filename: Option<String>,
    /// Optional MIME type hint
    #[serde(default)]
    pub content_type: Option<String>,
}

/// Batch upload request for multiple files
#[derive(Debug, Deserialize)]
pub struct BatchUploadRequest {
    pub files: Vec<JsonUploadRequest>,
}

/// Batch upload response
#[derive(Debug, Serialize)]
pub struct BatchUploadResponse {
    pub success: bool,
    pub uploaded: Vec<UploadResponse>,
    pub failed: Vec<BatchUploadError>,
    pub total: usize,
    pub succeeded: usize,
}

#[derive(Debug, Serialize)]
pub struct BatchUploadError {
    pub index: usize,
    pub filename: Option<String>,
    pub error: String,
}

/// Handle multipart form file upload
///
/// Accepts multipart/form-data with a file field.
///
/// Example using curl:
/// ```bash
/// curl -X POST http://localhost:8080/api/upload \
///   -F "file=@/path/to/file.html"
/// ```
pub async fn upload_multipart(State(state): State<AppState>, mut multipart: Multipart) -> Response {
    // Check IPFS connection
    let Some(storage) = &state.storage else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(UploadError::new(
                "Storage backend not available",
                "STORAGE_UNAVAILABLE",
            )),
        )
            .into_response();
    };

    // Process multipart fields
    while let Ok(Some(mut field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();

        // We're looking for the "file" field
        if name != "file" {
            continue;
        }

        let filename = field.file_name().map(|s| s.to_string());
        let content_type_hint = field.content_type().map(|s| s.to_string());

        // Read the file data in chunks, enforcing size limit incrementally
        let mut data = Vec::new();
        loop {
            match field.chunk().await {
                Ok(Some(chunk)) => {
                    if data.len() + chunk.len() > MAX_UPLOAD_SIZE {
                        return (
                            StatusCode::PAYLOAD_TOO_LARGE,
                            Json(UploadError::new(
                                format!(
                                    "File too large. Maximum size is {} MB",
                                    MAX_UPLOAD_SIZE / 1024 / 1024
                                ),
                                "FILE_TOO_LARGE",
                            )),
                        )
                            .into_response();
                    }
                    data.extend_from_slice(&chunk);
                }
                Ok(None) => break,
                Err(e) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(UploadError::new(
                            format!("Failed to read upload: {}", e),
                            "READ_ERROR",
                        )),
                    )
                        .into_response();
                }
            }
        }

        // Detect content type
        let content_type = content_type_hint
            .or_else(|| infer::get(&data).map(|t| t.mime_type().to_string()))
            .unwrap_or_else(|| "application/octet-stream".to_string());

        // Store in IPFS
        let storage = Arc::clone(storage);
        let data_clone = data.clone();

        let result = tokio::task::spawn_blocking(move || {
            tokio::runtime::Handle::current()
                .block_on(async { storage.store_content(data_clone).await })
        })
        .await;

        match result {
            Ok(Ok(cid)) => {
                let port = state.config.read().await.server.port;
                let size = data.len();
                audit::log_file_upload(
                    &state.audit_logger,
                    "api",
                    &cid,
                    size as u64,
                    filename.as_deref(),
                    None,
                    None,
                )
                .await;
                return Json(UploadResponse {
                    success: true,
                    gateway_url: format!("http://localhost:{}/{}", port, cid),
                    shadow_url: format!("shadow://{}", cid),
                    cid,
                    size,
                    content_type,
                    filename,
                })
                .into_response();
            }
            Ok(Err(e)) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(UploadError::new(
                        format!("Failed to store content: {}", e),
                        "STORAGE_ERROR",
                    )),
                )
                    .into_response();
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(UploadError::new(
                        format!("Internal error: {}", e),
                        "INTERNAL_ERROR",
                    )),
                )
                    .into_response();
            }
        }
    }

    (
        StatusCode::BAD_REQUEST,
        Json(UploadError::new(
            "No file field found in multipart data",
            "MISSING_FILE",
        )),
    )
        .into_response()
}

/// Handle JSON upload with base64-encoded data
///
/// Example using curl:
/// ```bash
/// curl -X POST http://localhost:8080/api/upload/json \
///   -H "Content-Type: application/json" \
///   -d '{"data": "PGh0bWw+...", "filename": "index.html"}'
/// ```
pub async fn upload_json(
    State(state): State<AppState>,
    Json(request): Json<JsonUploadRequest>,
) -> Response {
    // Check IPFS connection
    let Some(storage) = &state.storage else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(UploadError::new(
                "Storage backend not available",
                "STORAGE_UNAVAILABLE",
            )),
        )
            .into_response();
    };

    // Pre-check base64 string length to avoid allocating a huge decoded buffer.
    // base64 expands ~33%, so the encoded form of MAX_UPLOAD_SIZE is ~67 MB.
    if request.data.len() > MAX_UPLOAD_SIZE * 4 / 3 + 4 {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(UploadError::new(
                format!(
                    "Content too large. Maximum size is {} MB",
                    MAX_UPLOAD_SIZE / 1024 / 1024
                ),
                "FILE_TOO_LARGE",
            )),
        )
            .into_response();
    }

    // Decode base64 data
    let data = match BASE64.decode(&request.data) {
        Ok(bytes) => bytes,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(UploadError::new(
                    format!("Invalid base64 data: {}", e),
                    "INVALID_BASE64",
                )),
            )
                .into_response();
        }
    };

    // Check size limit
    if data.len() > MAX_UPLOAD_SIZE {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(UploadError::new(
                format!(
                    "Content too large. Maximum size is {} MB",
                    MAX_UPLOAD_SIZE / 1024 / 1024
                ),
                "FILE_TOO_LARGE",
            )),
        )
            .into_response();
    }

    // Detect content type
    let content_type = request
        .content_type
        .or_else(|| infer::get(&data).map(|t| t.mime_type().to_string()))
        .unwrap_or_else(|| "application/octet-stream".to_string());

    // Store in IPFS
    let storage = Arc::clone(storage);
    let data_clone = data.clone();

    let result = tokio::task::spawn_blocking(move || {
        tokio::runtime::Handle::current()
            .block_on(async { storage.store_content(data_clone).await })
    })
    .await;

    match result {
        Ok(Ok(cid)) => {
            let port = state.config.read().await.server.port;
            Json(UploadResponse {
                success: true,
                gateway_url: format!("http://localhost:{}/{}", port, cid),
                shadow_url: format!("shadow://{}", cid),
                cid,
                size: data.len(),
                content_type,
                filename: request.filename,
            })
            .into_response()
        }
        Ok(Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(UploadError::new(
                format!("Failed to store content: {}", e),
                "STORAGE_ERROR",
            )),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(UploadError::new(
                format!("Internal error: {}", e),
                "INTERNAL_ERROR",
            )),
        )
            .into_response(),
    }
}

/// Handle raw binary upload
///
/// Example using curl:
/// ```bash
/// curl -X POST http://localhost:8080/api/upload/raw \
///   -H "Content-Type: text/html" \
///   --data-binary @/path/to/file.html
/// ```
pub async fn upload_raw(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    // Check IPFS connection
    let Some(storage) = &state.storage else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(UploadError::new(
                "Storage backend not available",
                "STORAGE_UNAVAILABLE",
            )),
        )
            .into_response();
    };

    let data = body.to_vec();

    // Check size limit
    if data.len() > MAX_UPLOAD_SIZE {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(UploadError::new(
                format!(
                    "Content too large. Maximum size is {} MB",
                    MAX_UPLOAD_SIZE / 1024 / 1024
                ),
                "FILE_TOO_LARGE",
            )),
        )
            .into_response();
    }

    // Get content type from header or detect
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or_else(|| infer::get(&data).map(|t| t.mime_type().to_string()))
        .unwrap_or_else(|| "application/octet-stream".to_string());

    // Store in IPFS
    let storage = Arc::clone(storage);
    let data_clone = data.clone();

    let result = tokio::task::spawn_blocking(move || {
        tokio::runtime::Handle::current()
            .block_on(async { storage.store_content(data_clone).await })
    })
    .await;

    match result {
        Ok(Ok(cid)) => {
            let port = state.config.read().await.server.port;
            Json(UploadResponse {
                success: true,
                gateway_url: format!("http://localhost:{}/{}", port, cid),
                shadow_url: format!("shadow://{}", cid),
                cid,
                size: data.len(),
                content_type,
                filename: None,
            })
            .into_response()
        }
        Ok(Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(UploadError::new(
                format!("Failed to store content: {}", e),
                "STORAGE_ERROR",
            )),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(UploadError::new(
                format!("Internal error: {}", e),
                "INTERNAL_ERROR",
            )),
        )
            .into_response(),
    }
}

/// Handle batch upload of multiple files
///
/// Example using curl:
/// ```bash
/// curl -X POST http://localhost:8080/api/upload/batch \
///   -H "Content-Type: application/json" \
///   -d '{"files": [{"data": "...", "filename": "a.html"}, {"data": "..."}]}'
/// ```
pub async fn upload_batch(
    State(state): State<AppState>,
    Json(request): Json<BatchUploadRequest>,
) -> Response {
    // Check IPFS connection
    let Some(storage) = &state.storage else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(UploadError::new(
                "Storage backend not available",
                "STORAGE_UNAVAILABLE",
            )),
        )
            .into_response();
    };

    const MAX_BATCH_FILES: usize = 100;
    const MAX_BATCH_TOTAL_SIZE: usize = 200 * 1024 * 1024; // 200 MB aggregate

    if request.files.len() > MAX_BATCH_FILES {
        return (
            StatusCode::BAD_REQUEST,
            Json(UploadError::new(
                format!("Too many files in batch (max {})", MAX_BATCH_FILES),
                "BATCH_TOO_LARGE",
            )),
        )
            .into_response();
    }

    let total = request.files.len();
    let mut uploaded = Vec::new();
    let mut failed = Vec::new();
    let mut aggregate_size: usize = 0;

    for (index, file) in request.files.into_iter().enumerate() {
        // Check base64 length before decoding to avoid huge allocations
        // (base64 expands ~33%, so encoded size of MAX_UPLOAD_SIZE is ~67MB)
        if file.data.len() > MAX_UPLOAD_SIZE * 4 / 3 + 4 {
            failed.push(BatchUploadError {
                index,
                filename: file.filename,
                error: format!("File too large (max {} MB)", MAX_UPLOAD_SIZE / 1024 / 1024),
            });
            continue;
        }

        // Decode base64 data
        let data = match BASE64.decode(&file.data) {
            Ok(bytes) => bytes,
            Err(e) => {
                failed.push(BatchUploadError {
                    index,
                    filename: file.filename,
                    error: format!("Invalid base64: {}", e),
                });
                continue;
            }
        };

        // Check individual size limit
        if data.len() > MAX_UPLOAD_SIZE {
            failed.push(BatchUploadError {
                index,
                filename: file.filename,
                error: format!("File too large (max {} MB)", MAX_UPLOAD_SIZE / 1024 / 1024),
            });
            continue;
        }

        // Check aggregate size limit
        aggregate_size += data.len();
        if aggregate_size > MAX_BATCH_TOTAL_SIZE {
            failed.push(BatchUploadError {
                index,
                filename: file.filename,
                error: format!(
                    "Batch total size exceeds limit ({} MB)",
                    MAX_BATCH_TOTAL_SIZE / 1024 / 1024
                ),
            });
            continue;
        }

        // Detect content type
        let content_type = file
            .content_type
            .or_else(|| infer::get(&data).map(|t| t.mime_type().to_string()))
            .unwrap_or_else(|| "application/octet-stream".to_string());

        // Store in IPFS
        let storage = Arc::clone(storage);
        let data_len = data.len();
        let filename = file.filename.clone();

        let result = tokio::task::spawn_blocking(move || {
            tokio::runtime::Handle::current().block_on(async { storage.store_content(data).await })
        })
        .await;

        match result {
            Ok(Ok(cid)) => {
                let port = state.config.read().await.server.port;
                uploaded.push(UploadResponse {
                    success: true,
                    gateway_url: format!("http://localhost:{}/{}", port, cid),
                    shadow_url: format!("shadow://{}", cid),
                    cid,
                    size: data_len,
                    content_type,
                    filename,
                });
            }
            Ok(Err(e)) => {
                failed.push(BatchUploadError {
                    index,
                    filename: file.filename,
                    error: format!("Storage error: {}", e),
                });
            }
            Err(e) => {
                failed.push(BatchUploadError {
                    index,
                    filename: file.filename,
                    error: format!("Internal error: {}", e),
                });
            }
        }
    }

    let succeeded = uploaded.len();
    Json(BatchUploadResponse {
        success: failed.is_empty(),
        uploaded,
        failed,
        total,
        succeeded,
    })
    .into_response()
}
