//! Per-node health tracking and load-balanced selection for node-runners.
//!
//! Each configured node-runner gets its own [`NodeHealth`] entry with EWMA
//! latency tracking and an independent circuit breaker.  The
//! [`NodeHealthTracker`] coordinates selection across all nodes using the
//! configured [`NodeSelectionStrategy`].

use crate::circuit_breaker::CircuitBreaker;
use crate::config::{NodeHealthConfig, NodeSelectionStrategy};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

// ── Per-node state ──────────────────────────────────────────────

/// Health state for a single node-runner.
pub struct NodeHealth {
    /// Exponentially-weighted moving average latency (ms).
    avg_latency_ms: f64,
    /// Timestamp of last successful request.
    last_success: Option<Instant>,
    /// Per-node circuit breaker.
    circuit_breaker: CircuitBreaker,
}

impl NodeHealth {
    fn new(url: &str, failure_threshold: u32, reset_timeout: Duration) -> Self {
        Self {
            avg_latency_ms: 0.0,
            last_success: None,
            circuit_breaker: CircuitBreaker::new(
                format!("node-{}", url),
                failure_threshold,
                reset_timeout,
            ),
        }
    }

    /// Record a successful request with its latency.
    pub fn record_success(&mut self, latency_ms: f64) {
        if self.avg_latency_ms == 0.0 {
            self.avg_latency_ms = latency_ms;
        } else {
            // EWMA: same 0.9/0.1 formula as protocol/src/adaptive_routing.rs
            self.avg_latency_ms = self.avg_latency_ms * 0.9 + latency_ms * 0.1;
        }
        self.last_success = Some(Instant::now());
        self.circuit_breaker.record_success();
    }

    /// Record a failed request.
    pub fn record_failure(&mut self) {
        self.circuit_breaker.record_failure();
    }

    /// Whether this node is available (circuit not open).
    pub fn is_available(&self) -> bool {
        self.circuit_breaker.allow_request()
    }

    /// Combined health score (higher is better, roughly 0.0–1.0).
    pub fn health_score(&self) -> f64 {
        if !self.is_available() {
            return 0.0;
        }

        // Latency factor: lower latency = higher score (0.5 at 1000ms).
        let latency_score = if self.avg_latency_ms > 0.0 {
            1000.0 / (1000.0 + self.avg_latency_ms)
        } else {
            0.5 // unknown latency
        };

        // Recency factor: more recent success = higher score.
        let recency_score = match self.last_success {
            Some(t) => {
                let secs = t.elapsed().as_secs_f64();
                1.0 / (1.0 + secs / 60.0) // decays over minutes
            }
            None => 0.3, // never succeeded
        };

        latency_score * 0.7 + recency_score * 0.3
    }
}

// ── Coordinator ─────────────────────────────────────────────────

/// Tracks health of all configured node-runners and orders them by strategy.
pub struct NodeHealthTracker {
    /// Per-URL health state.
    nodes: RwLock<HashMap<String, NodeHealth>>,
    /// Ordered list of node URLs (preserves config order).
    urls: Vec<String>,
    /// Selection strategy.
    strategy: NodeSelectionStrategy,
    /// Round-robin counter.
    round_robin_index: AtomicUsize,
    /// Circuit breaker parameters.
    cb_threshold: u32,
    cb_reset: Duration,
    /// HTTP client for background health probes.
    http_client: reqwest::Client,
}

impl NodeHealthTracker {
    pub fn new(urls: Vec<String>, config: &NodeHealthConfig) -> Self {
        Self {
            nodes: RwLock::new(HashMap::new()),
            urls,
            strategy: config.strategy,
            round_robin_index: AtomicUsize::new(0),
            cb_threshold: config.circuit_breaker_threshold,
            cb_reset: Duration::from_secs(config.circuit_breaker_reset_seconds),
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap_or_default(),
        }
    }

    /// Ensure every configured URL has a health entry.
    async fn ensure_nodes(&self) {
        let mut nodes = self.nodes.write().await;
        for url in &self.urls {
            if !nodes.contains_key(url) {
                nodes.insert(
                    url.clone(),
                    NodeHealth::new(url, self.cb_threshold, self.cb_reset),
                );
            }
        }
    }

    /// Select node URLs ordered by the configured strategy.
    /// Nodes whose circuit breaker is open are excluded.
    pub async fn select_nodes(&self) -> Vec<String> {
        self.ensure_nodes().await;
        let nodes = self.nodes.read().await;

        match self.strategy {
            NodeSelectionStrategy::Sequential => self
                .urls
                .iter()
                .filter(|u| nodes.get(*u).map_or(true, |n| n.is_available()))
                .cloned()
                .collect(),

            NodeSelectionStrategy::RoundRobin => {
                let available: Vec<String> = self
                    .urls
                    .iter()
                    .filter(|u| nodes.get(*u).map_or(true, |n| n.is_available()))
                    .cloned()
                    .collect();
                if available.is_empty() {
                    return Vec::new();
                }
                let idx =
                    self.round_robin_index.fetch_add(1, Ordering::Relaxed) % available.len();
                let mut result = Vec::with_capacity(available.len());
                for i in 0..available.len() {
                    result.push(available[(idx + i) % available.len()].clone());
                }
                result
            }

            NodeSelectionStrategy::LeastLatency => {
                let mut scored: Vec<(String, f64)> = self
                    .urls
                    .iter()
                    .filter(|u| nodes.get(*u).map_or(true, |n| n.is_available()))
                    .map(|u| {
                        let latency = nodes.get(u).map_or(0.0, |n| n.avg_latency_ms);
                        (u.clone(), latency)
                    })
                    .collect();
                // Sort by latency ascending; unknown (0) goes to the end.
                scored.sort_by(|a, b| match (a.1 == 0.0, b.1 == 0.0) {
                    (true, false) => std::cmp::Ordering::Greater,
                    (false, true) => std::cmp::Ordering::Less,
                    _ => a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal),
                });
                scored.into_iter().map(|(u, _)| u).collect()
            }

            NodeSelectionStrategy::Weighted => {
                let mut scored: Vec<(String, f64)> = self
                    .urls
                    .iter()
                    .filter(|u| nodes.get(*u).map_or(true, |n| n.is_available()))
                    .map(|u| {
                        let score = nodes.get(u).map_or(0.5, |n| n.health_score());
                        (u.clone(), score)
                    })
                    .collect();
                // Sort by health score descending.
                scored.sort_by(|a, b| {
                    b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
                });
                scored.into_iter().map(|(u, _)| u).collect()
            }
        }
    }

    /// Record a successful request to a node.
    pub async fn record_success(&self, url: &str, latency_ms: f64) {
        self.ensure_nodes().await;
        let mut nodes = self.nodes.write().await;
        if let Some(node) = nodes.get_mut(url) {
            node.record_success(latency_ms);
        }
    }

    /// Record a failed request to a node.
    pub async fn record_failure(&self, url: &str) {
        self.ensure_nodes().await;
        let mut nodes = self.nodes.write().await;
        if let Some(node) = nodes.get_mut(url) {
            node.record_failure();
        }
    }

    /// Probe all configured node-runners via `GET /api/health`.
    pub async fn probe_all_nodes(&self) {
        for url in &self.urls {
            let health_url = format!("{}/api/health", url.trim_end_matches('/'));
            let start = Instant::now();
            match self.http_client.get(&health_url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    let latency_ms = start.elapsed().as_secs_f64() * 1000.0;
                    self.record_success(url, latency_ms).await;
                    tracing::debug!(url = %url, latency_ms = %latency_ms, "Health probe OK");
                }
                Ok(resp) => {
                    self.record_failure(url).await;
                    tracing::debug!(url = %url, status = %resp.status(), "Health probe bad status");
                }
                Err(e) => {
                    self.record_failure(url).await;
                    tracing::debug!(url = %url, error = %e, "Health probe failed");
                }
            }
        }
    }
}

// ── Background loop ─────────────────────────────────────────────

/// Run the background health check loop.
///
/// Probes every configured node-runner at the given interval and updates
/// the tracker's per-node health state.  Exits when the cancellation
/// token fires.
pub async fn run_health_check_loop(
    tracker: Arc<NodeHealthTracker>,
    interval: Duration,
    cancel: CancellationToken,
) {
    let mut tick = tokio::time::interval(interval);
    // Skip the initial immediate tick so the first real probe runs after `interval`.
    tick.tick().await;

    loop {
        tokio::select! {
            _ = tick.tick() => {
                tracker.probe_all_nodes().await;
            }
            _ = cancel.cancelled() => {
                tracing::info!("Node health check loop shutting down");
                break;
            }
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::NodeHealthConfig;

    #[test]
    fn test_ewma_latency() {
        let mut node = NodeHealth::new("http://test", 3, Duration::from_secs(15));
        // First sample sets the average directly.
        node.record_success(100.0);
        assert!((node.avg_latency_ms - 100.0).abs() < 0.01);
        // Subsequent samples use EWMA: 100*0.9 + 200*0.1 = 110
        node.record_success(200.0);
        assert!((node.avg_latency_ms - 110.0).abs() < 0.01);
        // 110*0.9 + 200*0.1 = 119
        node.record_success(200.0);
        assert!((node.avg_latency_ms - 119.0).abs() < 0.01);
    }

    #[test]
    fn test_circuit_breaker_integration() {
        let mut node = NodeHealth::new("http://test", 3, Duration::from_secs(15));
        assert!(node.is_available());
        node.record_failure();
        node.record_failure();
        assert!(node.is_available()); // 2 < threshold of 3
        node.record_failure();
        assert!(!node.is_available()); // 3 >= threshold
    }

    #[test]
    fn test_health_score_known_beats_unknown() {
        let unknown = NodeHealth::new("http://a", 3, Duration::from_secs(15));
        let mut known = NodeHealth::new("http://b", 3, Duration::from_secs(15));
        known.record_success(50.0);
        assert!(known.health_score() > unknown.health_score());
    }

    #[tokio::test]
    async fn test_select_nodes_sequential() {
        let cfg = NodeHealthConfig {
            strategy: NodeSelectionStrategy::Sequential,
            ..Default::default()
        };
        let tracker = NodeHealthTracker::new(
            vec!["http://a".into(), "http://b".into(), "http://c".into()],
            &cfg,
        );
        let nodes = tracker.select_nodes().await;
        assert_eq!(nodes, vec!["http://a", "http://b", "http://c"]);
    }

    #[tokio::test]
    async fn test_select_nodes_skips_circuit_broken() {
        let cfg = NodeHealthConfig {
            strategy: NodeSelectionStrategy::Sequential,
            circuit_breaker_threshold: 2,
            ..Default::default()
        };
        let tracker = NodeHealthTracker::new(
            vec!["http://a".into(), "http://b".into()],
            &cfg,
        );
        // Break node A.
        tracker.record_failure("http://a").await;
        tracker.record_failure("http://a").await;

        let nodes = tracker.select_nodes().await;
        assert_eq!(nodes, vec!["http://b"]);
    }

    #[tokio::test]
    async fn test_select_nodes_round_robin() {
        let cfg = NodeHealthConfig {
            strategy: NodeSelectionStrategy::RoundRobin,
            ..Default::default()
        };
        let tracker = NodeHealthTracker::new(
            vec!["http://a".into(), "http://b".into(), "http://c".into()],
            &cfg,
        );
        let first = tracker.select_nodes().await;
        let second = tracker.select_nodes().await;
        assert_eq!(first[0], "http://a");
        assert_eq!(second[0], "http://b");
    }

    #[tokio::test]
    async fn test_select_nodes_least_latency() {
        let cfg = NodeHealthConfig {
            strategy: NodeSelectionStrategy::LeastLatency,
            ..Default::default()
        };
        let tracker = NodeHealthTracker::new(
            vec!["http://slow".into(), "http://fast".into()],
            &cfg,
        );
        tracker.record_success("http://slow", 500.0).await;
        tracker.record_success("http://fast", 50.0).await;

        let nodes = tracker.select_nodes().await;
        assert_eq!(nodes[0], "http://fast");
    }

    #[tokio::test]
    async fn test_all_nodes_circuit_broken_returns_empty() {
        let cfg = NodeHealthConfig {
            strategy: NodeSelectionStrategy::Sequential,
            circuit_breaker_threshold: 1,
            ..Default::default()
        };
        let tracker = NodeHealthTracker::new(
            vec!["http://a".into(), "http://b".into()],
            &cfg,
        );
        tracker.record_failure("http://a").await;
        tracker.record_failure("http://b").await;

        let nodes = tracker.select_nodes().await;
        assert!(nodes.is_empty());
    }
}
