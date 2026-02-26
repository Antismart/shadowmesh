//! HTTP resolver for fetching content from node-runner instances.
//!
//! Tries each configured node-runner URL in order until one returns
//! the requested content. Used as a fallback between P2P and IPFS.
//!
//! Small content (<= 5 MB) is buffered for caching and HTML rewriting.
//! Large content (> 5 MB) is streamed directly to avoid memory pressure.

use crate::node_health::NodeHealthTracker;
use bytes::Bytes;
use futures::stream::Stream;
use std::time::{Duration, Instant};

/// Content larger than this is streamed instead of buffered.
const STREAM_THRESHOLD_BYTES: u64 = 5 * 1024 * 1024;

pub struct ResolvedContent {
    pub data: Vec<u8>,
    pub mime_type: String,
}

pub struct StreamingResolvedContent {
    pub stream: Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send + Unpin>,
    pub mime_type: String,
    pub content_length: u64,
}

/// Resolved content from a node-runner, either buffered or streaming.
pub enum NodeContent {
    Buffered(ResolvedContent),
    Streaming(StreamingResolvedContent),
}

/// Adaptive resolver: buffers small content, streams large content.
///
/// Uses the `Content-Length` header from the node-runner response to
/// decide whether to buffer or stream.
pub async fn resolve_from_nodes_adaptive(
    http: &reqwest::Client,
    node_urls: &[String],
    cid: &str,
    timeout_secs: u64,
) -> Option<NodeContent> {
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

                let content_length = resp
                    .headers()
                    .get("content-length")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(0);

                if content_length > STREAM_THRESHOLD_BYTES {
                    let stream = resp.bytes_stream();
                    return Some(NodeContent::Streaming(StreamingResolvedContent {
                        stream: Box::new(stream),
                        mime_type: mime,
                        content_length,
                    }));
                } else if let Ok(data) = resp.bytes().await {
                    return Some(NodeContent::Buffered(ResolvedContent {
                        data: data.to_vec(),
                        mime_type: mime,
                    }));
                }
            }
            _ => continue,
        }
    }
    None
}

/// Health-aware resolver: uses [`NodeHealthTracker`] to pick nodes by strategy,
/// records success/failure and latency per-node.
///
/// Falls through the ordered list exactly like `resolve_from_nodes_adaptive`
/// but records metrics into the tracker so subsequent calls benefit from
/// updated health state.
pub async fn resolve_from_nodes_with_tracking(
    http: &reqwest::Client,
    tracker: &NodeHealthTracker,
    cid: &str,
    timeout_secs: u64,
) -> Option<NodeContent> {
    let urls = tracker.select_nodes().await;

    for url in &urls {
        let download_url = format!(
            "{}/api/storage/download/{}",
            url.trim_end_matches('/'),
            cid
        );

        let start = Instant::now();

        match http
            .get(&download_url)
            .timeout(Duration::from_secs(timeout_secs))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                let latency_ms = start.elapsed().as_secs_f64() * 1000.0;
                tracker.record_success(url, latency_ms).await;

                let mime = resp
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("application/octet-stream")
                    .to_string();

                let content_length = resp
                    .headers()
                    .get("content-length")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(0);

                if content_length > STREAM_THRESHOLD_BYTES {
                    let stream = resp.bytes_stream();
                    return Some(NodeContent::Streaming(StreamingResolvedContent {
                        stream: Box::new(stream),
                        mime_type: mime,
                        content_length,
                    }));
                } else if let Ok(data) = resp.bytes().await {
                    return Some(NodeContent::Buffered(ResolvedContent {
                        data: data.to_vec(),
                        mime_type: mime,
                    }));
                }
            }
            _ => {
                tracker.record_failure(url).await;
                continue;
            }
        }
    }
    None
}
