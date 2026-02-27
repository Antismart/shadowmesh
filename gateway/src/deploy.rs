//! Deploy handlers for the ShadowMesh gateway
//!
//! Provides Vercel-like deployment of static websites and projects.
//! Users can upload a ZIP file containing their website, and get back
//! a CID that serves the entire site with proper directory structure.

use axum::{
    extract::{Multipart, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use std::io::Cursor;
use std::sync::Arc;
use tempfile::TempDir;
use zip::ZipArchive;

use crate::{audit, lock_utils::write_lock, metrics, AppState};

/// Deploy response returned after successful website deployment
#[derive(Debug, Serialize)]
pub struct DeployResponse {
    /// Whether the deployment was successful
    pub success: bool,
    /// Root CID for the deployed website
    pub cid: String,
    /// Gateway URL for HTTP access (serves index.html at root)
    pub url: String,
    /// IPFS gateway URL
    pub ipfs_url: String,
    /// Native ShadowMesh URL
    pub shadow_url: String,
    /// Number of files deployed
    pub file_count: usize,
    /// Total size in bytes
    pub total_size: u64,
    /// List of deployed files
    pub files: Vec<DeployedFile>,
}

/// Information about a deployed file
#[derive(Debug, Serialize)]
pub struct DeployedFile {
    /// Path within the deployment
    pub path: String,
    /// IPFS CID of this file
    pub cid: String,
    /// Size in bytes
    pub size: u64,
}

/// Error response for failed deployments
#[derive(Debug, Serialize)]
pub struct DeployError {
    pub success: bool,
    pub error: String,
    pub code: String,
}

impl DeployError {
    fn new(error: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            success: false,
            error: error.into(),
            code: code.into(),
        }
    }
}

/// Deploy a static website from a ZIP file
///
/// The ZIP should contain website files (index.html, css/, js/, etc.)
/// The root of the ZIP becomes the root of the deployed site.
///
/// Example using curl:
/// ```bash
/// # Create a ZIP of your website
/// cd my-website && zip -r ../site.zip .
///
/// # Deploy to ShadowMesh
/// curl -X POST http://localhost:8080/api/deploy \
///   -F "file=@site.zip"
/// ```
pub async fn deploy_zip(State(state): State<AppState>, mut multipart: Multipart) -> Response {
    let deploy_cfg = state.config.read().await.clone();
    // Check IPFS connection
    let Some(storage) = &state.storage else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(DeployError::new(
                "Storage backend not available",
                "STORAGE_UNAVAILABLE",
            )),
        )
            .into_response();
    };

    // Process multipart fields
    while let Ok(Some(mut field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();

        if name != "file" {
            continue;
        }

        let filename = field.file_name().map(|s| s.to_string());

        // Read the ZIP data with streaming size enforcement.
        // This prevents buffering an unbounded upload into RAM.
        let max_deploy_size = deploy_cfg.deploy.max_size_bytes();
        let mut data = Vec::new();
        loop {
            match field.chunk().await {
                Ok(Some(chunk)) => {
                    if data.len() + chunk.len() > max_deploy_size {
                        return (
                            StatusCode::PAYLOAD_TOO_LARGE,
                            Json(DeployError::new(
                                format!(
                                    "Deployment too large. Maximum size is {} MB",
                                    deploy_cfg.deploy.max_size_mb
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
                        Json(DeployError::new(
                            format!("Failed to read upload: {}", e),
                            "READ_ERROR",
                        )),
                    )
                        .into_response();
                }
            }
        }

        // Verify it's a ZIP file
        if !is_zip(&data) {
            return (
                StatusCode::BAD_REQUEST,
                Json(DeployError::new(
                    "File must be a ZIP archive. Use: zip -r site.zip .",
                    "INVALID_FORMAT",
                )),
            )
                .into_response();
        }

        // Extract ZIP to temp directory
        let temp_dir = match extract_zip(&data) {
            Ok(dir) => dir,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(DeployError::new(
                        format!("Failed to extract ZIP: {}", e),
                        "EXTRACT_ERROR",
                    )),
                )
                    .into_response();
            }
        };

        // Check for index.html
        let index_path = temp_dir.path().join("index.html");
        if !index_path.exists() {
            // Check one level deep (in case ZIP has a root folder)
            let has_index = std::fs::read_dir(temp_dir.path())
                .ok()
                .and_then(|entries| {
                    entries
                        .filter_map(|e| e.ok())
                        .find(|e| e.path().is_dir() && e.path().join("index.html").exists())
                })
                .is_some();

            if !has_index {
                println!(
                    "⚠️  No index.html found in deployment (site may not serve correctly at root)"
                );
            }
        }

        // Upload directory to IPFS
        let storage = Arc::clone(storage);
        let temp_path = temp_dir.path().to_path_buf();
        let temp_dir_name = temp_path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
            .unwrap_or_default();

        let result = tokio::task::spawn_blocking(move || {
            tokio::runtime::Handle::current()
                .block_on(async { storage.store_directory(&temp_path).await })
        })
        .await;

        match result {
            Ok(Ok(upload_result)) => {
                let port = deploy_cfg.server.port;
                let cid = upload_result.root_cid.clone();

                // Strip the temp directory prefix from file paths
                let prefix = format!("{}/", temp_dir_name);
                let files: Vec<DeployedFile> = upload_result.files.iter()
                    .filter(|f| f.name != temp_dir_name) // Skip the root dir entry
                    .map(|f| DeployedFile {
                        path: f.name.strip_prefix(&prefix).unwrap_or(&f.name).to_string(),
                        cid: f.hash.clone(),
                        size: f.size,
                    })
                    .collect();

                println!("✅ Deployed {} files, root CID: {}", files.len(), cid);

                // Record deployment in state
                let deployment_name = filename
                    .as_ref()
                    .map(|f| f.trim_end_matches(".zip").to_string())
                    .unwrap_or_else(|| format!("deploy-{}", &cid[..8]));

                let mut deployment = crate::dashboard::Deployment::new(
                    deployment_name.clone(),
                    cid.clone(),
                    upload_result.total_size,
                    files.len(),
                );

                // Auto-assign .shadow domain
                deployment.domain = crate::auto_assign_domain(&state, &deployment_name, &cid).await;

                // Save to Redis if available
                if let Some(ref redis) = state.redis {
                    if let Err(e) = deployment.save_to_redis(redis).await {
                        tracing::warn!("Failed to save deployment to Redis: {}", e);
                    }
                }

                // Also keep in-memory for immediate access (cap to prevent unbounded growth)
                {
                    let mut deployments = write_lock(&state.deployments);
                    deployments.insert(0, deployment);
                    const MAX_IN_MEMORY_DEPLOYMENTS: usize = 1000;
                    deployments.truncate(MAX_IN_MEMORY_DEPLOYMENTS);
                }

                metrics::record_deployment(true);
                audit::log_deploy_success(
                    &state.audit_logger,
                    "api",
                    &cid,
                    upload_result.total_size,
                    None,
                    None,
                )
                .await;

                return Json(DeployResponse {
                    success: true,
                    url: format!("http://localhost:{}/{}", port, cid),
                    ipfs_url: format!("https://ipfs.io/ipfs/{}", cid),
                    shadow_url: format!("shadow://{}", cid),
                    cid,
                    file_count: files.len(),
                    total_size: upload_result.total_size,
                    files,
                })
                .into_response();
            }
            Ok(Err(e)) => {
                metrics::record_deployment(false);
                audit::log_deploy_failure(
                    &state.audit_logger,
                    "api",
                    &format!("Deploy error: {}", e),
                    None,
                    None,
                )
                .await;
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(DeployError::new(
                        format!("Failed to deploy: {}", e),
                        "DEPLOY_ERROR",
                    )),
                )
                    .into_response();
            }
            Err(e) => {
                metrics::record_deployment(false);
                audit::log_deploy_failure(
                    &state.audit_logger,
                    "api",
                    &format!("Internal error: {}", e),
                    None,
                    None,
                )
                .await;
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(DeployError::new(
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
        Json(DeployError::new(
            "No file field found in multipart data",
            "MISSING_FILE",
        )),
    )
        .into_response()
}

/// Deploy from a tarball (.tar.gz)
pub async fn deploy_tarball(State(_state): State<AppState>, _multipart: Multipart) -> Response {
    // For now, suggest using ZIP
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(DeployError::new(
            "Tarball deployment coming soon. Please use ZIP format for now.",
            "NOT_IMPLEMENTED",
        )),
    )
        .into_response()
}

/// Check if data is a ZIP file
fn is_zip(data: &[u8]) -> bool {
    // ZIP magic bytes: PK\x03\x04
    data.len() >= 4 && data[0] == 0x50 && data[1] == 0x4B && data[2] == 0x03 && data[3] == 0x04
}

/// Extract ZIP to a temporary directory with security validation
///
/// This function includes multiple layers of protection against path traversal attacks:
/// 1. Uses `enclosed_name()` which sanitizes paths
/// 2. Validates that resolved paths are within the temp directory
/// 3. Rejects any paths containing suspicious patterns
fn extract_zip(data: &[u8]) -> Result<TempDir, String> {
    let cursor = Cursor::new(data);
    let mut archive = ZipArchive::new(cursor).map_err(|e| format!("Invalid ZIP file: {}", e))?;

    let temp_dir = TempDir::new().map_err(|e| format!("Failed to create temp directory: {}", e))?;

    // Get canonical path of temp directory for validation
    let temp_dir_canonical = temp_dir
        .path()
        .canonicalize()
        .map_err(|e| format!("Failed to canonicalize temp directory: {}", e))?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("Failed to read ZIP entry: {}", e))?;

        // Get the file name - enclosed_name() sanitizes basic traversal attempts
        let file_path = match file.enclosed_name() {
            Some(path) => path.to_path_buf(),
            None => {
                tracing::warn!(
                    entry = %file.name(),
                    "Skipping ZIP entry with invalid path"
                );
                continue;
            }
        };

        // Additional security check: reject paths with suspicious patterns
        let path_str = file_path.to_string_lossy();
        if path_str.contains("..") || path_str.starts_with('/') || path_str.contains("\\..") {
            tracing::warn!(
                entry = %file.name(),
                sanitized = %path_str,
                "Rejecting ZIP entry with suspicious path pattern"
            );
            return Err(format!(
                "Security error: ZIP contains suspicious path '{}'. Path traversal attempts are not allowed.",
                file.name()
            ));
        }

        let outpath = temp_dir.path().join(&file_path);

        if file.name().ends_with('/') {
            // Create directory
            std::fs::create_dir_all(&outpath)
                .map_err(|e| format!("Failed to create directory: {}", e))?;
        } else {
            // Create parent directories if needed
            if let Some(parent) = outpath.parent() {
                if !parent.exists() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("Failed to create directory: {}", e))?;
                }
            }

            // Extract file
            let mut outfile = std::fs::File::create(&outpath)
                .map_err(|e| format!("Failed to create file: {}", e))?;
            std::io::copy(&mut file, &mut outfile)
                .map_err(|e| format!("Failed to extract file: {}", e))?;

            // SECURITY: Verify the extracted file is within the temp directory
            // This is a defense-in-depth check after extraction
            if let Ok(canonical_outpath) = outpath.canonicalize() {
                if !canonical_outpath.starts_with(&temp_dir_canonical) {
                    // Delete the file that escaped
                    let _ = std::fs::remove_file(&outpath);
                    tracing::error!(
                        entry = %file.name(),
                        resolved = %canonical_outpath.display(),
                        temp_dir = %temp_dir_canonical.display(),
                        "SECURITY: ZIP extraction path traversal attempt detected and blocked"
                    );
                    return Err(format!(
                        "Security error: ZIP entry '{}' attempted to escape extraction directory",
                        file.name()
                    ));
                }
            }
        }

        // Set permissions on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(mode) = file.unix_mode() {
                // Limit permissions - don't allow setuid/setgid/sticky bits
                let safe_mode = mode & 0o777;
                std::fs::set_permissions(&outpath, std::fs::Permissions::from_mode(safe_mode)).ok();
            }
        }
    }

    Ok(temp_dir)
}

/// Deployment info endpoint - get info about deployment limits
#[derive(Debug, Serialize)]
pub struct DeployInfo {
    pub max_size_mb: u64,
    pub supported_formats: Vec<&'static str>,
    pub example_command: &'static str,
}

pub async fn deploy_info(State(state): State<AppState>) -> Json<DeployInfo> {
    Json(DeployInfo {
        max_size_mb: state.config.read().await.deploy.max_size_mb,
        supported_formats: vec!["zip"],
        example_command: "cd my-site && zip -r ../site.zip . && curl -X POST http://localhost:8080/api/deploy -F 'file=@../site.zip'",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // ── is_zip ──────────────────────────────────────────────────────

    #[test]
    fn is_zip_valid_magic_bytes() {
        // PK\x03\x04 followed by some payload
        let data = [0x50, 0x4B, 0x03, 0x04, 0x00, 0x00];
        assert!(is_zip(&data));
    }

    #[test]
    fn is_zip_exactly_four_bytes() {
        let data = [0x50, 0x4B, 0x03, 0x04];
        assert!(is_zip(&data));
    }

    #[test]
    fn is_zip_too_short() {
        assert!(!is_zip(&[0x50, 0x4B, 0x03]));
        assert!(!is_zip(&[]));
    }

    #[test]
    fn is_zip_wrong_magic() {
        // First byte wrong
        assert!(!is_zip(&[0x00, 0x4B, 0x03, 0x04]));
        // All zeroes
        assert!(!is_zip(&[0x00, 0x00, 0x00, 0x00]));
        // PNG magic
        assert!(!is_zip(&[0x89, 0x50, 0x4E, 0x47]));
    }

    #[test]
    fn is_zip_random_data() {
        let data = vec![0xFFu8; 1024];
        assert!(!is_zip(&data));
    }

    // ── extract_zip ─────────────────────────────────────────────────

    /// Helper: build a real ZIP archive in memory with the given entries.
    /// Each entry is (name, contents). Directory entries should end with '/'.
    fn build_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let buf = Vec::new();
        let cursor = Cursor::new(buf);
        let mut writer = zip::ZipWriter::new(cursor);
        let options =
            zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

        for (name, contents) in entries {
            if name.ends_with('/') {
                writer.add_directory(*name, options).unwrap();
            } else {
                writer.start_file(*name, options).unwrap();
                writer.write_all(contents).unwrap();
            }
        }

        let cursor = writer.finish().unwrap();
        cursor.into_inner()
    }

    #[test]
    fn extract_zip_single_file() {
        let zip_data = build_zip(&[("hello.txt", b"world")]);
        let temp_dir = extract_zip(&zip_data).expect("extraction should succeed");
        let content = std::fs::read_to_string(temp_dir.path().join("hello.txt")).unwrap();
        assert_eq!(content, "world");
    }

    #[test]
    fn extract_zip_nested_directories() {
        let zip_data = build_zip(&[
            ("css/", b""),
            ("css/style.css", b"body{}"),
            ("index.html", b"<html></html>"),
        ]);
        let temp_dir = extract_zip(&zip_data).expect("extraction should succeed");
        assert!(temp_dir.path().join("index.html").exists());
        assert!(temp_dir.path().join("css/style.css").exists());
        let css = std::fs::read_to_string(temp_dir.path().join("css/style.css")).unwrap();
        assert_eq!(css, "body{}");
    }

    #[test]
    fn extract_zip_invalid_data() {
        let result = extract_zip(b"this is not a zip file");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("Invalid ZIP"),
            "Expected 'Invalid ZIP' in error, got: {}",
            err
        );
    }

    #[test]
    fn extract_zip_empty_archive() {
        // A valid ZIP with zero entries
        let zip_data = build_zip(&[]);
        let temp_dir = extract_zip(&zip_data).expect("extraction should succeed");
        // The temp directory exists but has no files
        let entries: Vec<_> = std::fs::read_dir(temp_dir.path())
            .unwrap()
            .collect();
        assert!(entries.is_empty());
    }

    // ── DeployError ─────────────────────────────────────────────────

    #[test]
    fn deploy_error_new_sets_fields() {
        let err = DeployError::new("something broke", "BROKEN");
        assert!(!err.success);
        assert_eq!(err.error, "something broke");
        assert_eq!(err.code, "BROKEN");
    }

    #[test]
    fn deploy_error_serializes_to_json() {
        let err = DeployError::new("bad request", "BAD_REQ");
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["success"], false);
        assert_eq!(json["error"], "bad request");
        assert_eq!(json["code"], "BAD_REQ");
    }
}
