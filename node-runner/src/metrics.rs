//! Node Metrics and Statistics
//!
//! Collects and exposes node performance metrics.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

/// Maximum history entries to keep
const MAX_HISTORY_ENTRIES: usize = 1440; // 24 hours at 1-minute intervals

/// Node metrics collector
pub struct MetricsCollector {
    /// Node start time
    start_time: Instant,
    /// Request counters
    requests: RequestMetrics,
    /// Bandwidth metrics
    bandwidth: BandwidthMetrics,
    /// P2P network metrics
    network: Arc<RwLock<NetworkMetrics>>,
    /// Historical data points
    history: Arc<RwLock<MetricsHistory>>,
}

/// Request metrics
#[derive(Debug, Default)]
pub struct RequestMetrics {
    pub total_requests: AtomicU64,
    pub successful_requests: AtomicU64,
    pub failed_requests: AtomicU64,
    pub cache_hits: AtomicU64,
    pub cache_misses: AtomicU64,
}

/// Bandwidth metrics
#[derive(Debug, Default)]
pub struct BandwidthMetrics {
    pub bytes_served: AtomicU64,
    pub bytes_received: AtomicU64,
    pub bytes_uploaded: AtomicU64,
}

/// Network metrics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NetworkMetrics {
    pub connected_peers: u32,
    pub discovered_peers: u32,
    pub active_connections: u32,
    pub pending_dials: u32,
    pub dht_records: u32,
    pub gossip_topics: u32,
}

/// Historical data point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataPoint {
    pub timestamp: u64,
    pub requests_per_min: u64,
    pub bandwidth_mbps: f64,
    pub connected_peers: u32,
    pub error_rate: f64,
}

/// Metrics history
#[derive(Debug, Default)]
pub struct MetricsHistory {
    pub points: VecDeque<DataPoint>,
    pub last_requests: u64,
    pub last_bandwidth: u64,
    pub last_recorded: Option<Instant>,
}

impl MetricsCollector {
    /// Create a new metrics collector
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
            requests: RequestMetrics::default(),
            bandwidth: BandwidthMetrics::default(),
            network: Arc::new(RwLock::new(NetworkMetrics::default())),
            history: Arc::new(RwLock::new(MetricsHistory::default())),
        }
    }

    /// Get node uptime in seconds
    pub fn uptime_secs(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }

    /// Get formatted uptime string
    pub fn uptime_formatted(&self) -> String {
        let secs = self.uptime_secs();
        let days = secs / 86400;
        let hours = (secs % 86400) / 3600;
        let mins = (secs % 3600) / 60;

        if days > 0 {
            format!("{}d {}h {}m", days, hours, mins)
        } else if hours > 0 {
            format!("{}h {}m", hours, mins)
        } else {
            format!("{}m", mins)
        }
    }

    /// Record a request
    pub fn record_request(&self, success: bool) {
        self.requests.total_requests.fetch_add(1, Ordering::Relaxed);
        if success {
            self.requests
                .successful_requests
                .fetch_add(1, Ordering::Relaxed);
        } else {
            self.requests
                .failed_requests
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Record cache hit/miss
    pub fn record_cache(&self, hit: bool) {
        if hit {
            self.requests.cache_hits.fetch_add(1, Ordering::Relaxed);
        } else {
            self.requests.cache_misses.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Record bandwidth usage
    pub fn record_bandwidth(&self, served: u64, received: u64, uploaded: u64) {
        self.bandwidth
            .bytes_served
            .fetch_add(served, Ordering::Relaxed);
        self.bandwidth
            .bytes_received
            .fetch_add(received, Ordering::Relaxed);
        self.bandwidth
            .bytes_uploaded
            .fetch_add(uploaded, Ordering::Relaxed);
    }

    /// Record bytes served
    pub fn record_served(&self, bytes: u64) {
        self.bandwidth
            .bytes_served
            .fetch_add(bytes, Ordering::Relaxed);
    }

    /// Record bytes received
    pub fn record_received(&self, bytes: u64) {
        self.bandwidth
            .bytes_received
            .fetch_add(bytes, Ordering::Relaxed);
    }

    /// Update network metrics
    pub async fn update_network(&self, metrics: NetworkMetrics) {
        let mut network = self.network.write().await;
        *network = metrics;
    }

    /// Update connected peers count
    pub async fn set_connected_peers(&self, count: u32) {
        let mut network = self.network.write().await;
        network.connected_peers = count;
    }

    /// Get request statistics
    pub fn get_request_stats(&self) -> RequestStats {
        let total = self.requests.total_requests.load(Ordering::Relaxed);
        let successful = self.requests.successful_requests.load(Ordering::Relaxed);
        let failed = self.requests.failed_requests.load(Ordering::Relaxed);
        let cache_hits = self.requests.cache_hits.load(Ordering::Relaxed);
        let cache_misses = self.requests.cache_misses.load(Ordering::Relaxed);

        RequestStats {
            total,
            successful,
            failed,
            cache_hits,
            cache_misses,
            success_rate: if total > 0 {
                (successful as f64 / total as f64) * 100.0
            } else {
                100.0
            },
            cache_hit_rate: if cache_hits + cache_misses > 0 {
                (cache_hits as f64 / (cache_hits + cache_misses) as f64) * 100.0
            } else {
                0.0
            },
        }
    }

    /// Get bandwidth statistics
    pub fn get_bandwidth_stats(&self) -> BandwidthStats {
        let served = self.bandwidth.bytes_served.load(Ordering::Relaxed);
        let received = self.bandwidth.bytes_received.load(Ordering::Relaxed);
        let uploaded = self.bandwidth.bytes_uploaded.load(Ordering::Relaxed);
        let uptime = self.uptime_secs().max(1);

        BandwidthStats {
            total_served: served,
            total_received: received,
            total_uploaded: uploaded,
            avg_served_bps: served / uptime,
            avg_received_bps: received / uptime,
            avg_uploaded_bps: uploaded / uptime,
        }
    }

    /// Get network statistics
    pub async fn get_network_stats(&self) -> NetworkMetrics {
        self.network.read().await.clone()
    }

    /// Record a history data point (call every minute)
    pub async fn record_history_point(&self) {
        let mut history = self.history.write().await;
        let now = Instant::now();

        // Calculate rates since last recording
        let current_requests = self.requests.total_requests.load(Ordering::Relaxed);
        let current_bandwidth = self.bandwidth.bytes_served.load(Ordering::Relaxed);
        let failed = self.requests.failed_requests.load(Ordering::Relaxed);
        let network = self.network.read().await.clone();

        let (requests_per_min, bandwidth_mbps, error_rate) =
            if let Some(last) = history.last_recorded {
                let elapsed_secs = now.duration_since(last).as_secs_f64().max(1.0);
                let req_diff = current_requests.saturating_sub(history.last_requests);
                let bw_diff = current_bandwidth.saturating_sub(history.last_bandwidth);

                let rpm = (req_diff as f64 / elapsed_secs * 60.0) as u64;
                let mbps = (bw_diff as f64 / elapsed_secs) / (1024.0 * 1024.0);
                let err = if current_requests > 0 {
                    (failed as f64 / current_requests as f64) * 100.0
                } else {
                    0.0
                };

                (rpm, mbps, err)
            } else {
                (0, 0.0, 0.0)
            };

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let point = DataPoint {
            timestamp,
            requests_per_min,
            bandwidth_mbps,
            connected_peers: network.connected_peers,
            error_rate,
        };

        history.points.push_back(point);
        if history.points.len() > MAX_HISTORY_ENTRIES {
            history.points.pop_front();
        }

        history.last_requests = current_requests;
        history.last_bandwidth = current_bandwidth;
        history.last_recorded = Some(now);
    }

    /// Get history data points
    pub async fn get_history(&self, limit: usize) -> Vec<DataPoint> {
        let history = self.history.read().await;
        history
            .points
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    /// Get complete node metrics snapshot
    pub async fn get_snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            uptime_secs: self.uptime_secs(),
            uptime_formatted: self.uptime_formatted(),
            requests: self.get_request_stats(),
            bandwidth: self.get_bandwidth_stats(),
            network: self.get_network_stats().await,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }

    /// Start background metrics recording
    pub fn start_background_recording(self: Arc<Self>) {
        let metrics = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                metrics.record_history_point().await;
            }
        });
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Request statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestStats {
    pub total: u64,
    pub successful: u64,
    pub failed: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub success_rate: f64,
    pub cache_hit_rate: f64,
}

/// Bandwidth statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BandwidthStats {
    pub total_served: u64,
    pub total_received: u64,
    pub total_uploaded: u64,
    pub avg_served_bps: u64,
    pub avg_received_bps: u64,
    pub avg_uploaded_bps: u64,
}

impl BandwidthStats {
    /// Format bytes as human readable
    pub fn format_bytes(bytes: u64) -> String {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        const GB: u64 = MB * 1024;
        const TB: u64 = GB * 1024;

        if bytes >= TB {
            format!("{:.2} TB", bytes as f64 / TB as f64)
        } else if bytes >= GB {
            format!("{:.2} GB", bytes as f64 / GB as f64)
        } else if bytes >= MB {
            format!("{:.2} MB", bytes as f64 / MB as f64)
        } else if bytes >= KB {
            format!("{:.2} KB", bytes as f64 / KB as f64)
        } else {
            format!("{} B", bytes)
        }
    }

    pub fn served_formatted(&self) -> String {
        Self::format_bytes(self.total_served)
    }

    pub fn received_formatted(&self) -> String {
        Self::format_bytes(self.total_received)
    }
}

/// Complete metrics snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    pub uptime_secs: u64,
    pub uptime_formatted: String,
    pub requests: RequestStats,
    pub bandwidth: BandwidthStats,
    pub network: NetworkMetrics,
    pub timestamp: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uptime_formatting() {
        let collector = MetricsCollector::new();
        // Just check it doesn't panic
        let _ = collector.uptime_formatted();
    }

    #[test]
    fn test_request_recording() {
        let collector = MetricsCollector::new();

        collector.record_request(true);
        collector.record_request(true);
        collector.record_request(false);

        let stats = collector.get_request_stats();
        assert_eq!(stats.total, 3);
        assert_eq!(stats.successful, 2);
        assert_eq!(stats.failed, 1);
    }

    #[test]
    fn test_cache_recording() {
        let collector = MetricsCollector::new();

        collector.record_cache(true);
        collector.record_cache(true);
        collector.record_cache(false);

        let stats = collector.get_request_stats();
        assert_eq!(stats.cache_hits, 2);
        assert_eq!(stats.cache_misses, 1);
    }

    #[test]
    fn test_bandwidth_formatting() {
        assert_eq!(BandwidthStats::format_bytes(500), "500 B");
        assert_eq!(BandwidthStats::format_bytes(1024), "1.00 KB");
        assert_eq!(BandwidthStats::format_bytes(1048576), "1.00 MB");
        assert_eq!(BandwidthStats::format_bytes(1073741824), "1.00 GB");
    }
}
