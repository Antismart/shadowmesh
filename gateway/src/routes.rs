//! Route handlers for the ShadowMesh gateway
//! 
//! Additional API routes and utilities.

use axum::{
    extract::State,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Upload request payload
#[derive(Debug, Deserialize)]
pub struct UploadRequest {
    pub data: String,  // Base64 encoded
    pub filename: Option<String>,
}

/// Upload response
#[derive(Debug, Serialize)]
pub struct UploadResponse {
    pub cid: String,
    pub gateway_url: String,
    pub native_url: String,
}

/// Fragment request for ShadowMesh protocol
#[derive(Debug, Deserialize)]
pub struct FragmentRequest {
    pub data: Vec<u8>,
}

/// Fragment response
#[derive(Debug, Serialize)]
pub struct FragmentResponse {
    pub content_hash: String,
    pub fragments: Vec<String>,
    pub metadata: FragmentMetadata,
}

#[derive(Debug, Serialize)]
pub struct FragmentMetadata {
    pub name: String,
    pub size: u64,
    pub mime_type: String,
}

/// Network statistics
#[derive(Debug, Serialize)]
pub struct NetworkStats {
    pub nodes: u32,
    pub bandwidth_served: u64,
    pub files_hosted: u32,
    pub uptime_seconds: u64,
}

/// Announce content to the network
#[derive(Debug, Deserialize)]
pub struct AnnounceRequest {
    pub manifest: serde_json::Value,
    pub privacy: String,
    pub redundancy: u32,
}

#[derive(Debug, Serialize)]
pub struct AnnounceResponse {
    pub success: bool,
    pub message: String,
}

/// Content status check
#[derive(Debug, Serialize)]
pub struct ContentStatus {
    pub available: bool,
    pub replicas: u32,
    pub last_seen: Option<u64>,
}

// Helper function to create a gateway URL
pub fn gateway_url(cid: &str) -> String {
    format!("http://localhost:8080/{}", cid)
}

// Helper function to create a native ShadowMesh URL
pub fn native_url(cid: &str) -> String {
    format!("shadow://{}", cid)
}