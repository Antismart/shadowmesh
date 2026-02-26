//! HTTP resolver for fetching content from node-runner instances.
//!
//! Tries each configured node-runner URL in order until one returns
//! the requested content. Used as a fallback between P2P and IPFS.

use std::time::Duration;

pub struct ResolvedContent {
    pub data: Vec<u8>,
    pub mime_type: String,
}

/// Try to resolve content from configured node-runner HTTP endpoints.
///
/// Iterates through `node_urls` sequentially, returning the first
/// successful response. Returns `None` if no node-runner has the content.
pub async fn resolve_from_nodes(
    http: &reqwest::Client,
    node_urls: &[String],
    cid: &str,
    timeout_secs: u64,
) -> Option<ResolvedContent> {
    for base_url in node_urls {
        let url = format!(
            "{}/api/storage/download/{}",
            base_url.trim_end_matches('/'),
            cid
        );

        match http
            .get(&url)
            .timeout(Duration::from_secs(timeout_secs))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                let mime = resp
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("application/octet-stream")
                    .to_string();

                if let Ok(data) = resp.bytes().await {
                    return Some(ResolvedContent {
                        data: data.to_vec(),
                        mime_type: mime,
                    });
                }
            }
            _ => continue,
        }
    }
    None
}
