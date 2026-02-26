//! Prometheus Metrics for ShadowMesh Gateway
//!
//! Provides comprehensive metrics for monitoring gateway performance and health.

use lazy_static::lazy_static;
use prometheus::{
    self, Encoder, HistogramOpts, HistogramVec, IntCounter, IntCounterVec, IntGauge, Opts,
    Registry, TextEncoder,
};
use std::time::Instant;

/// Helper macro to create a metric, panicking on failure.
/// Metric registration is a hard startup requirement — if it fails,
/// the binary cannot serve correct metrics and must not start.
macro_rules! create_metric {
    ($constructor:expr, $name:expr) => {
        $constructor.unwrap_or_else(|e| {
            panic!("fatal: failed to register metric '{}': {}", $name, e)
        })
    };
}

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();

    // Request metrics
    pub static ref HTTP_REQUESTS_TOTAL: IntCounterVec = create_metric!(
        IntCounterVec::new(
            Opts::new("http_requests_total", "Total number of HTTP requests"),
            &["method", "endpoint", "status"]
        ),
        "http_requests_total"
    );

    pub static ref HTTP_REQUEST_DURATION_SECONDS: HistogramVec = create_metric!(
        HistogramVec::new(
            HistogramOpts::new(
                "http_request_duration_seconds",
                "HTTP request duration in seconds"
            ).buckets(vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]),
            &["method", "endpoint"]
        ),
        "http_request_duration_seconds"
    );

    pub static ref HTTP_REQUESTS_IN_FLIGHT: IntGauge = create_metric!(
        IntGauge::new(
            "http_requests_in_flight",
            "Number of HTTP requests currently being processed"
        ),
        "http_requests_in_flight"
    );

    // Bytes metrics
    pub static ref BYTES_SERVED_TOTAL: IntCounter = create_metric!(
        IntCounter::new("bytes_served_total", "Total bytes served to clients"),
        "bytes_served_total"
    );

    pub static ref BYTES_RECEIVED_TOTAL: IntCounter = create_metric!(
        IntCounter::new("bytes_received_total", "Total bytes received from clients"),
        "bytes_received_total"
    );

    // Cache metrics
    pub static ref CACHE_HITS_TOTAL: IntCounter = create_metric!(
        IntCounter::new("cache_hits_total", "Total number of cache hits"),
        "cache_hits_total"
    );

    pub static ref CACHE_MISSES_TOTAL: IntCounter = create_metric!(
        IntCounter::new("cache_misses_total", "Total number of cache misses"),
        "cache_misses_total"
    );

    pub static ref CACHE_SIZE_ENTRIES: IntGauge = create_metric!(
        IntGauge::new("cache_size_entries", "Current number of entries in cache"),
        "cache_size_entries"
    );

    // IPFS metrics
    pub static ref IPFS_OPERATIONS_TOTAL: IntCounterVec = create_metric!(
        IntCounterVec::new(
            Opts::new("ipfs_operations_total", "Total IPFS operations"),
            &["operation", "status"]
        ),
        "ipfs_operations_total"
    );

    pub static ref IPFS_OPERATION_DURATION_SECONDS: HistogramVec = create_metric!(
        HistogramVec::new(
            HistogramOpts::new(
                "ipfs_operation_duration_seconds",
                "IPFS operation duration in seconds"
            ).buckets(vec![0.1, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0]),
            &["operation"]
        ),
        "ipfs_operation_duration_seconds"
    );

    pub static ref IPFS_CONNECTED: IntGauge = create_metric!(
        IntGauge::new(
            "ipfs_connected",
            "Whether the gateway is connected to IPFS (1=yes, 0=no)"
        ),
        "ipfs_connected"
    );

    // Deployment metrics
    pub static ref DEPLOYMENTS_TOTAL: IntCounterVec = create_metric!(
        IntCounterVec::new(
            Opts::new("deployments_total", "Total deployments"),
            &["status"]
        ),
        "deployments_total"
    );

    pub static ref ACTIVE_DEPLOYMENTS: IntGauge = create_metric!(
        IntGauge::new("active_deployments", "Number of active deployments"),
        "active_deployments"
    );

    // Rate limiting metrics
    pub static ref RATE_LIMIT_EXCEEDED_TOTAL: IntCounter = create_metric!(
        IntCounter::new(
            "rate_limit_exceeded_total",
            "Total number of requests rejected due to rate limiting"
        ),
        "rate_limit_exceeded_total"
    );

    // Auth metrics
    pub static ref AUTH_FAILURES_TOTAL: IntCounterVec = create_metric!(
        IntCounterVec::new(
            Opts::new("auth_failures_total", "Total authentication failures"),
            &["reason"]
        ),
        "auth_failures_total"
    );

    // Circuit breaker metrics
    pub static ref CIRCUIT_BREAKER_STATE: IntGauge = create_metric!(
        IntGauge::new(
            "circuit_breaker_state",
            "Circuit breaker state (0=closed, 1=half-open, 2=open)"
        ),
        "circuit_breaker_state"
    );
}

/// Register all metrics with the registry
pub fn register_metrics() {
    let metrics: Vec<(&str, Box<dyn prometheus::core::Collector>)> = vec![
        ("http_requests_total", Box::new(HTTP_REQUESTS_TOTAL.clone())),
        ("http_request_duration_seconds", Box::new(HTTP_REQUEST_DURATION_SECONDS.clone())),
        ("http_requests_in_flight", Box::new(HTTP_REQUESTS_IN_FLIGHT.clone())),
        ("bytes_served_total", Box::new(BYTES_SERVED_TOTAL.clone())),
        ("bytes_received_total", Box::new(BYTES_RECEIVED_TOTAL.clone())),
        ("cache_hits_total", Box::new(CACHE_HITS_TOTAL.clone())),
        ("cache_misses_total", Box::new(CACHE_MISSES_TOTAL.clone())),
        ("cache_size_entries", Box::new(CACHE_SIZE_ENTRIES.clone())),
        ("ipfs_operations_total", Box::new(IPFS_OPERATIONS_TOTAL.clone())),
        ("ipfs_operation_duration_seconds", Box::new(IPFS_OPERATION_DURATION_SECONDS.clone())),
        ("ipfs_connected", Box::new(IPFS_CONNECTED.clone())),
        ("deployments_total", Box::new(DEPLOYMENTS_TOTAL.clone())),
        ("active_deployments", Box::new(ACTIVE_DEPLOYMENTS.clone())),
        ("rate_limit_exceeded_total", Box::new(RATE_LIMIT_EXCEEDED_TOTAL.clone())),
        ("auth_failures_total", Box::new(AUTH_FAILURES_TOTAL.clone())),
        ("circuit_breaker_state", Box::new(CIRCUIT_BREAKER_STATE.clone())),
    ];

    for (name, collector) in metrics {
        if let Err(e) = REGISTRY.register(collector) {
            tracing::error!("Failed to register metric '{}': {}", name, e);
        }
    }
}

/// Encode metrics in Prometheus text format
pub fn encode_metrics() -> String {
    let encoder = TextEncoder::new();
    let metric_families = REGISTRY.gather();
    let mut buffer = Vec::new();
    if let Err(e) = encoder.encode(&metric_families, &mut buffer) {
        tracing::error!("Failed to encode metrics: {}", e);
        return String::from("# Error encoding metrics");
    }
    String::from_utf8(buffer).unwrap_or_else(|e| {
        tracing::error!("Invalid UTF-8 in metrics output: {}", e);
        String::from("# Error: invalid UTF-8 in metrics")
    })
}

/// Helper for timing operations
pub struct Timer {
    start: Instant,
    histogram: &'static HistogramVec,
    labels: Vec<String>,
}

impl Timer {
    pub fn new(histogram: &'static HistogramVec, labels: Vec<String>) -> Self {
        Self {
            start: Instant::now(),
            histogram,
            labels,
        }
    }

    pub fn observe(self) {
        let duration = self.start.elapsed().as_secs_f64();
        let labels: Vec<&str> = self.labels.iter().map(|s| s.as_str()).collect();
        self.histogram.with_label_values(&labels).observe(duration);
    }
}

/// Record an HTTP request
pub fn record_request(method: &str, endpoint: &str, status: u16, duration_secs: f64) {
    HTTP_REQUESTS_TOTAL
        .with_label_values(&[method, endpoint, &status.to_string()])
        .inc();
    HTTP_REQUEST_DURATION_SECONDS
        .with_label_values(&[method, endpoint])
        .observe(duration_secs);
}

/// Record bytes served
pub fn record_bytes_served(bytes: u64) {
    BYTES_SERVED_TOTAL.inc_by(bytes);
}

/// Record cache hit
pub fn record_cache_hit() {
    CACHE_HITS_TOTAL.inc();
}

/// Record cache miss
pub fn record_cache_miss() {
    CACHE_MISSES_TOTAL.inc();
}

/// Update cache size
pub fn update_cache_size(entries: i64) {
    CACHE_SIZE_ENTRIES.set(entries);
}

/// Record IPFS operation
pub fn record_ipfs_operation(operation: &str, success: bool, duration_secs: f64) {
    let status = if success { "success" } else { "error" };
    IPFS_OPERATIONS_TOTAL
        .with_label_values(&[operation, status])
        .inc();
    IPFS_OPERATION_DURATION_SECONDS
        .with_label_values(&[operation])
        .observe(duration_secs);
}

/// Set IPFS connection status
pub fn set_ipfs_connected(connected: bool) {
    IPFS_CONNECTED.set(if connected { 1 } else { 0 });
}

/// Record deployment
pub fn record_deployment(success: bool) {
    let status = if success { "success" } else { "error" };
    DEPLOYMENTS_TOTAL.with_label_values(&[status]).inc();
    if success {
        ACTIVE_DEPLOYMENTS.inc();
    }
}

/// Record rate limit exceeded
pub fn record_rate_limit_exceeded() {
    RATE_LIMIT_EXCEEDED_TOTAL.inc();
}

/// Record auth failure
pub fn record_auth_failure(reason: &str) {
    AUTH_FAILURES_TOTAL.with_label_values(&[reason]).inc();
}

/// Update circuit breaker state metric
pub fn update_circuit_breaker_state(is_open: bool, is_half_open: bool) {
    let state = if is_open {
        2
    } else if is_half_open {
        1
    } else {
        0
    };
    CIRCUIT_BREAKER_STATE.set(state);
}
