//! Circuit Breaker Pattern Implementation
//!
//! Prevents cascading failures by failing fast when a service is unhealthy.
//!
//! States:
//! - Closed: Normal operation, requests pass through
//! - Open: Service unhealthy, requests fail immediately
//! - HalfOpen: Testing if service recovered, limited requests allowed

use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use std::sync::RwLock;
use std::time::{Duration, Instant};

/// Circuit breaker states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CircuitState {
    /// Normal operation - requests pass through
    Closed = 0,
    /// Service unhealthy - requests fail immediately
    Open = 1,
    /// Testing recovery - limited requests allowed
    HalfOpen = 2,
}

impl From<u8> for CircuitState {
    fn from(val: u8) -> Self {
        match val {
            0 => CircuitState::Closed,
            1 => CircuitState::Open,
            2 => CircuitState::HalfOpen,
            _ => CircuitState::Closed,
        }
    }
}

/// Circuit breaker for protecting against cascading failures
pub struct CircuitBreaker {
    /// Current state (atomic for lock-free reads)
    state: AtomicU8,
    /// Consecutive failure count
    failure_count: AtomicU32,
    /// Number of failures before opening circuit
    failure_threshold: u32,
    /// Time to wait before testing recovery
    reset_timeout: Duration,
    /// Time of last failure (for reset timeout)
    last_failure_time: RwLock<Option<Instant>>,
    /// Time when circuit opened (for metrics)
    opened_at: RwLock<Option<Instant>>,
    /// Name for logging
    name: String,
}

impl CircuitBreaker {
    /// Create a new circuit breaker
    ///
    /// # Arguments
    /// * `name` - Identifier for logging
    /// * `failure_threshold` - Number of consecutive failures before opening
    /// * `reset_timeout` - Duration to wait before attempting recovery
    pub fn new(name: impl Into<String>, failure_threshold: u32, reset_timeout: Duration) -> Self {
        Self {
            state: AtomicU8::new(CircuitState::Closed as u8),
            failure_count: AtomicU32::new(0),
            failure_threshold,
            reset_timeout,
            last_failure_time: RwLock::new(None),
            opened_at: RwLock::new(None),
            name: name.into(),
        }
    }

    /// Create with default settings (5 failures, 30s reset)
    pub fn default_for(name: impl Into<String>) -> Self {
        Self::new(name, 5, Duration::from_secs(30))
    }

    /// Get current circuit state
    pub fn state(&self) -> CircuitState {
        self.state.load(Ordering::SeqCst).into()
    }

    /// Check if request is allowed
    ///
    /// Returns true if the circuit allows the request to proceed.
    /// Automatically transitions from Open to HalfOpen after timeout.
    pub fn allow_request(&self) -> bool {
        let state = self.state();

        match state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // Check if reset timeout has elapsed
                if let Some(last_failure) = *crate::lock_utils::read_lock(&self.last_failure_time) {
                    if last_failure.elapsed() >= self.reset_timeout {
                        // Transition to half-open
                        self.transition_to(CircuitState::HalfOpen);
                        tracing::info!(
                            circuit = %self.name,
                            "Circuit transitioning to half-open state"
                        );
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => true, // Allow test request
        }
    }

    /// Record a successful operation
    pub fn record_success(&self) {
        let state = self.state();

        match state {
            CircuitState::HalfOpen => {
                // Success in half-open means service recovered
                self.transition_to(CircuitState::Closed);
                self.failure_count.store(0, Ordering::SeqCst);
                tracing::info!(
                    circuit = %self.name,
                    "Circuit closed - service recovered"
                );
            }
            CircuitState::Closed => {
                // Reset failure count on success
                self.failure_count.store(0, Ordering::SeqCst);
            }
            CircuitState::Open => {
                // Shouldn't happen, but handle gracefully
            }
        }
    }

    /// Record a failed operation
    pub fn record_failure(&self) {
        let state = self.state();
        let new_count = self.failure_count.fetch_add(1, Ordering::SeqCst) + 1;

        // Update last failure time
        *crate::lock_utils::write_lock(&self.last_failure_time) = Some(Instant::now());

        match state {
            CircuitState::Closed => {
                if new_count >= self.failure_threshold {
                    self.open_circuit();
                }
            }
            CircuitState::HalfOpen => {
                // Failure in half-open means service still unhealthy
                self.open_circuit();
            }
            CircuitState::Open => {
                // Already open, nothing to do
            }
        }
    }

    /// Open the circuit
    fn open_circuit(&self) {
        self.transition_to(CircuitState::Open);
        *crate::lock_utils::write_lock(&self.opened_at) = Some(Instant::now());
        tracing::warn!(
            circuit = %self.name,
            failure_count = %self.failure_count.load(Ordering::SeqCst),
            threshold = %self.failure_threshold,
            reset_timeout_secs = %self.reset_timeout.as_secs(),
            "Circuit opened - failing fast"
        );
    }

    /// Transition to a new state
    fn transition_to(&self, new_state: CircuitState) {
        self.state.store(new_state as u8, Ordering::SeqCst);
    }

    /// Get failure count
    pub fn failure_count(&self) -> u32 {
        self.failure_count.load(Ordering::SeqCst)
    }

    /// Get time since circuit opened (if open)
    pub fn time_in_open_state(&self) -> Option<Duration> {
        if self.state() == CircuitState::Open {
            crate::lock_utils::read_lock(&self.opened_at).map(|t| t.elapsed())
        } else {
            None
        }
    }

    /// Check if circuit is open (service unhealthy)
    pub fn is_open(&self) -> bool {
        self.state() == CircuitState::Open
    }

    /// Force close the circuit (for manual recovery)
    pub fn force_close(&self) {
        self.transition_to(CircuitState::Closed);
        self.failure_count.store(0, Ordering::SeqCst);
        tracing::info!(
            circuit = %self.name,
            "Circuit force-closed"
        );
    }
}

/// Error returned when circuit is open
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct CircuitOpenError {
    pub circuit_name: String,
    pub time_until_retry: Duration,
}

impl std::fmt::Display for CircuitOpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Circuit '{}' is open. Retry after {:?}",
            self.circuit_name, self.time_until_retry
        )
    }
}

impl std::error::Error for CircuitOpenError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_circuit_starts_closed() {
        let cb = CircuitBreaker::new("test", 3, Duration::from_secs(10));
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.allow_request());
    }

    #[test]
    fn test_circuit_opens_after_threshold() {
        let cb = CircuitBreaker::new("test", 3, Duration::from_secs(10));

        // Record failures
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Closed);
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Closed);
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);

        // Should not allow requests when open
        assert!(!cb.allow_request());
    }

    #[test]
    fn test_success_resets_failure_count() {
        let cb = CircuitBreaker::new("test", 3, Duration::from_secs(10));

        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.failure_count(), 2);

        cb.record_success();
        assert_eq!(cb.failure_count(), 0);
    }

    #[test]
    fn test_half_open_success_closes() {
        let cb = CircuitBreaker::new("test", 1, Duration::from_millis(10));

        cb.record_failure(); // Opens circuit
        assert_eq!(cb.state(), CircuitState::Open);

        // Wait for reset timeout
        std::thread::sleep(Duration::from_millis(20));

        // Should allow request and transition to half-open
        assert!(cb.allow_request());
        assert_eq!(cb.state(), CircuitState::HalfOpen);

        // Success should close circuit
        cb.record_success();
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn test_half_open_failure_reopens() {
        let cb = CircuitBreaker::new("test", 1, Duration::from_millis(10));

        cb.record_failure(); // Opens circuit
        std::thread::sleep(Duration::from_millis(20));
        cb.allow_request(); // Transitions to half-open

        // Failure should reopen circuit
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
    }
}
