//! Bandwidth tracking and management for ShadowMesh
//!
//! Monitors bandwidth usage, enforces limits, and provides statistics.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// Default bandwidth limit (100 MB/s)
pub const DEFAULT_BANDWIDTH_LIMIT: u64 = 100 * 1024 * 1024;

/// Measurement window size
pub const MEASUREMENT_WINDOW_SECS: u64 = 60;

/// Bandwidth measurement sample
#[derive(Debug, Clone)]
struct BandwidthSample {
    bytes: u64,
    timestamp: Instant,
}

/// Direction of data transfer
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Direction {
    Inbound,
    Outbound,
}

/// Bandwidth statistics for a time period
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BandwidthStats {
    /// Total bytes transferred
    pub total_bytes: u64,
    /// Bytes in the current period
    pub period_bytes: u64,
    /// Average bytes per second
    pub avg_bps: f64,
    /// Peak bytes per second
    pub peak_bps: u64,
    /// Number of transfers
    pub transfer_count: u64,
}

/// Per-peer bandwidth tracking
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PeerBandwidth {
    pub peer_id: String,
    pub inbound_bytes: u64,
    pub outbound_bytes: u64,
    pub last_transfer: u64,
}

/// Bandwidth tracker for monitoring network usage
pub struct BandwidthTracker {
    /// Inbound samples
    inbound_samples: VecDeque<BandwidthSample>,
    /// Outbound samples
    outbound_samples: VecDeque<BandwidthSample>,
    /// Total inbound bytes
    total_inbound: u64,
    /// Total outbound bytes
    total_outbound: u64,
    /// Inbound bandwidth limit (bytes/sec)
    inbound_limit: Option<u64>,
    /// Outbound bandwidth limit (bytes/sec)
    outbound_limit: Option<u64>,
    /// Per-peer bandwidth
    peer_bandwidth: HashMap<String, PeerBandwidth>,
    /// Peak inbound rate
    peak_inbound: u64,
    /// Peak outbound rate
    peak_outbound: u64,
    /// Transfer counts
    inbound_transfers: u64,
    outbound_transfers: u64,
    /// Measurement window
    window: Duration,
}

impl BandwidthTracker {
    /// Create a new bandwidth tracker
    pub fn new() -> Self {
        Self {
            inbound_samples: VecDeque::new(),
            outbound_samples: VecDeque::new(),
            total_inbound: 0,
            total_outbound: 0,
            inbound_limit: None,
            outbound_limit: None,
            peer_bandwidth: HashMap::new(),
            peak_inbound: 0,
            peak_outbound: 0,
            inbound_transfers: 0,
            outbound_transfers: 0,
            window: Duration::from_secs(MEASUREMENT_WINDOW_SECS),
        }
    }

    /// Create with bandwidth limits
    pub fn with_limits(inbound: Option<u64>, outbound: Option<u64>) -> Self {
        let mut tracker = Self::new();
        tracker.inbound_limit = inbound;
        tracker.outbound_limit = outbound;
        tracker
    }

    /// Set inbound bandwidth limit
    pub fn set_inbound_limit(&mut self, limit: Option<u64>) {
        self.inbound_limit = limit;
    }

    /// Set outbound bandwidth limit
    pub fn set_outbound_limit(&mut self, limit: Option<u64>) {
        self.outbound_limit = limit;
    }

    /// Record inbound bytes
    pub fn record_inbound(&mut self, bytes: u64, peer_id: Option<&str>) {
        self.total_inbound += bytes;
        self.inbound_transfers += 1;
        self.inbound_samples.push_back(BandwidthSample {
            bytes,
            timestamp: Instant::now(),
        });

        if let Some(pid) = peer_id {
            self.record_peer_inbound(pid, bytes);
        }

        self.cleanup_old_samples();
        self.update_peak_rates();
    }

    /// Record outbound bytes
    pub fn record_outbound(&mut self, bytes: u64, peer_id: Option<&str>) {
        self.total_outbound += bytes;
        self.outbound_transfers += 1;
        self.outbound_samples.push_back(BandwidthSample {
            bytes,
            timestamp: Instant::now(),
        });

        if let Some(pid) = peer_id {
            self.record_peer_outbound(pid, bytes);
        }

        self.cleanup_old_samples();
        self.update_peak_rates();
    }

    /// Record bytes for a specific direction
    pub fn record(&mut self, direction: Direction, bytes: u64, peer_id: Option<&str>) {
        match direction {
            Direction::Inbound => self.record_inbound(bytes, peer_id),
            Direction::Outbound => self.record_outbound(bytes, peer_id),
        }
    }

    fn record_peer_inbound(&mut self, peer_id: &str, bytes: u64) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let entry = self.peer_bandwidth
            .entry(peer_id.to_string())
            .or_insert_with(|| PeerBandwidth {
                peer_id: peer_id.to_string(),
                ..Default::default()
            });
        entry.inbound_bytes += bytes;
        entry.last_transfer = now;
    }

    fn record_peer_outbound(&mut self, peer_id: &str, bytes: u64) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let entry = self.peer_bandwidth
            .entry(peer_id.to_string())
            .or_insert_with(|| PeerBandwidth {
                peer_id: peer_id.to_string(),
                ..Default::default()
            });
        entry.outbound_bytes += bytes;
        entry.last_transfer = now;
    }

    fn cleanup_old_samples(&mut self) {
        let cutoff = Instant::now() - self.window;

        while let Some(sample) = self.inbound_samples.front() {
            if sample.timestamp < cutoff {
                self.inbound_samples.pop_front();
            } else {
                break;
            }
        }

        while let Some(sample) = self.outbound_samples.front() {
            if sample.timestamp < cutoff {
                self.outbound_samples.pop_front();
            } else {
                break;
            }
        }
    }

    fn update_peak_rates(&mut self) {
        let inbound_rate = self.current_inbound_rate();
        let outbound_rate = self.current_outbound_rate();

        if inbound_rate > self.peak_inbound {
            self.peak_inbound = inbound_rate;
        }
        if outbound_rate > self.peak_outbound {
            self.peak_outbound = outbound_rate;
        }
    }

    /// Get current inbound rate (bytes/sec)
    pub fn current_inbound_rate(&self) -> u64 {
        self.calculate_rate(&self.inbound_samples)
    }

    /// Get current outbound rate (bytes/sec)
    pub fn current_outbound_rate(&self) -> u64 {
        self.calculate_rate(&self.outbound_samples)
    }

    fn calculate_rate(&self, samples: &VecDeque<BandwidthSample>) -> u64 {
        if samples.is_empty() {
            return 0;
        }

        let now = Instant::now();
        let window_start = now - self.window;

        let total_bytes: u64 = samples
            .iter()
            .filter(|s| s.timestamp >= window_start)
            .map(|s| s.bytes)
            .sum();

        // Calculate actual time span
        let oldest = samples.front().map(|s| s.timestamp).unwrap_or(now);
        let time_span = now.duration_since(oldest.max(window_start));
        let secs = time_span.as_secs_f64().max(1.0);

        (total_bytes as f64 / secs) as u64
    }

    /// Check if inbound transfer is allowed
    pub fn can_receive(&self, bytes: u64) -> bool {
        match self.inbound_limit {
            Some(limit) => self.current_inbound_rate() + bytes <= limit,
            None => true,
        }
    }

    /// Check if outbound transfer is allowed
    pub fn can_send(&self, bytes: u64) -> bool {
        match self.outbound_limit {
            Some(limit) => self.current_outbound_rate() + bytes <= limit,
            None => true,
        }
    }

    /// Get inbound statistics
    pub fn inbound_stats(&self) -> BandwidthStats {
        let period_bytes: u64 = self.inbound_samples.iter().map(|s| s.bytes).sum();
        BandwidthStats {
            total_bytes: self.total_inbound,
            period_bytes,
            avg_bps: self.current_inbound_rate() as f64,
            peak_bps: self.peak_inbound,
            transfer_count: self.inbound_transfers,
        }
    }

    /// Get outbound statistics
    pub fn outbound_stats(&self) -> BandwidthStats {
        let period_bytes: u64 = self.outbound_samples.iter().map(|s| s.bytes).sum();
        BandwidthStats {
            total_bytes: self.total_outbound,
            period_bytes,
            avg_bps: self.current_outbound_rate() as f64,
            peak_bps: self.peak_outbound,
            transfer_count: self.outbound_transfers,
        }
    }

    /// Get combined statistics
    pub fn combined_stats(&self) -> BandwidthSummary {
        BandwidthSummary {
            inbound: self.inbound_stats(),
            outbound: self.outbound_stats(),
            total_bytes: self.total_inbound + self.total_outbound,
            inbound_limit: self.inbound_limit,
            outbound_limit: self.outbound_limit,
        }
    }

    /// Get peer bandwidth statistics
    pub fn get_peer_stats(&self, peer_id: &str) -> Option<&PeerBandwidth> {
        self.peer_bandwidth.get(peer_id)
    }

    /// Get all peer bandwidth statistics
    pub fn get_all_peer_stats(&self) -> Vec<&PeerBandwidth> {
        self.peer_bandwidth.values().collect()
    }

    /// Get top bandwidth consumers
    pub fn get_top_peers(&self, count: usize, direction: Direction) -> Vec<&PeerBandwidth> {
        let mut peers: Vec<_> = self.peer_bandwidth.values().collect();
        match direction {
            Direction::Inbound => {
                peers.sort_by(|a, b| b.inbound_bytes.cmp(&a.inbound_bytes));
            }
            Direction::Outbound => {
                peers.sort_by(|a, b| b.outbound_bytes.cmp(&a.outbound_bytes));
            }
        }
        peers.into_iter().take(count).collect()
    }

    /// Reset statistics (keep limits)
    pub fn reset(&mut self) {
        self.inbound_samples.clear();
        self.outbound_samples.clear();
        self.total_inbound = 0;
        self.total_outbound = 0;
        self.peak_inbound = 0;
        self.peak_outbound = 0;
        self.inbound_transfers = 0;
        self.outbound_transfers = 0;
        self.peer_bandwidth.clear();
    }
}

impl Default for BandwidthTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Combined bandwidth summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BandwidthSummary {
    pub inbound: BandwidthStats,
    pub outbound: BandwidthStats,
    pub total_bytes: u64,
    pub inbound_limit: Option<u64>,
    pub outbound_limit: Option<u64>,
}

impl BandwidthSummary {
    /// Get inbound utilization percentage
    pub fn inbound_utilization(&self) -> Option<f64> {
        self.inbound_limit.map(|limit| {
            if limit == 0 {
                0.0
            } else {
                (self.inbound.avg_bps / limit as f64) * 100.0
            }
        })
    }

    /// Get outbound utilization percentage
    pub fn outbound_utilization(&self) -> Option<f64> {
        self.outbound_limit.map(|limit| {
            if limit == 0 {
                0.0
            } else {
                (self.outbound.avg_bps / limit as f64) * 100.0
            }
        })
    }

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
}

/// Rate limiter for bandwidth throttling
pub struct RateLimiter {
    /// Tokens available
    tokens: f64,
    /// Maximum tokens (bucket size)
    max_tokens: f64,
    /// Tokens per second (refill rate)
    tokens_per_sec: f64,
    /// Last refill time
    last_refill: Instant,
}

impl RateLimiter {
    /// Create a new rate limiter
    pub fn new(bytes_per_sec: u64) -> Self {
        let max_tokens = bytes_per_sec as f64;
        Self {
            tokens: max_tokens,
            max_tokens,
            tokens_per_sec: bytes_per_sec as f64,
            last_refill: Instant::now(),
        }
    }

    /// Create with custom burst size
    pub fn with_burst(bytes_per_sec: u64, burst_bytes: u64) -> Self {
        Self {
            tokens: burst_bytes as f64,
            max_tokens: burst_bytes as f64,
            tokens_per_sec: bytes_per_sec as f64,
            last_refill: Instant::now(),
        }
    }

    /// Refill tokens based on elapsed time
    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.tokens_per_sec).min(self.max_tokens);
        self.last_refill = now;
    }

    /// Try to consume tokens for a transfer
    pub fn try_consume(&mut self, bytes: u64) -> bool {
        self.refill();
        let needed = bytes as f64;
        if self.tokens >= needed {
            self.tokens -= needed;
            true
        } else {
            false
        }
    }

    /// Get available tokens
    pub fn available(&mut self) -> u64 {
        self.refill();
        self.tokens as u64
    }

    /// Wait duration needed before bytes can be consumed
    pub fn wait_duration(&mut self, bytes: u64) -> Duration {
        self.refill();
        let needed = bytes as f64;
        if self.tokens >= needed {
            return Duration::ZERO;
        }

        let deficit = needed - self.tokens;
        let wait_secs = deficit / self.tokens_per_sec;
        Duration::from_secs_f64(wait_secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_bandwidth() {
        let mut tracker = BandwidthTracker::new();

        tracker.record_inbound(1024, Some("peer1"));
        tracker.record_outbound(2048, Some("peer1"));

        assert_eq!(tracker.inbound_stats().total_bytes, 1024);
        assert_eq!(tracker.outbound_stats().total_bytes, 2048);
    }

    #[test]
    fn test_peer_tracking() {
        let mut tracker = BandwidthTracker::new();

        tracker.record_inbound(1024, Some("peer1"));
        tracker.record_inbound(2048, Some("peer1"));
        tracker.record_outbound(512, Some("peer1"));

        let stats = tracker.get_peer_stats("peer1").unwrap();
        assert_eq!(stats.inbound_bytes, 3072);
        assert_eq!(stats.outbound_bytes, 512);
    }

    #[test]
    fn test_bandwidth_limits() {
        let mut tracker = BandwidthTracker::with_limits(Some(1024), Some(512));

        // Initially should be able to transfer
        assert!(tracker.can_receive(1024));
        assert!(tracker.can_send(512));
    }

    #[test]
    fn test_rate_limiter() {
        let mut limiter = RateLimiter::new(1024);

        // Should have full bucket initially
        assert!(limiter.try_consume(512));
        assert!(limiter.try_consume(512));
        // Bucket should be empty now
        assert!(!limiter.try_consume(100));
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(BandwidthSummary::format_bytes(512), "512 B");
        assert_eq!(BandwidthSummary::format_bytes(1024), "1.00 KB");
        assert_eq!(BandwidthSummary::format_bytes(1048576), "1.00 MB");
        assert_eq!(BandwidthSummary::format_bytes(1073741824), "1.00 GB");
    }
}
