// Dashboard utilities and helpers
// Additional dashboard-related functionality can be added here

#![allow(dead_code)]

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct BandwidthDataPoint {
    pub timestamp: u64,
    pub bytes_per_second: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DashboardMetrics {
    pub bandwidth_history: Vec<BandwidthDataPoint>,
    pub connected_peers: u32,
    pub pending_requests: u32,
}

impl DashboardMetrics {
    pub fn new() -> Self {
        Self {
            bandwidth_history: Vec::new(),
            connected_peers: 0,
            pending_requests: 0,
        }
    }
}

impl Default for DashboardMetrics {
    fn default() -> Self {
        Self::new()
    }
}
