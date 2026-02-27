//! Rate Limiting for ShadowMesh Gateway
//!
//! Provides both IP-based and API-key-based rate limiting.
//! API keys get separate (typically higher) limits from anonymous IP requests.

use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{header, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Json, Response},
};
use serde::Serialize;
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::RwLock;

use crate::metrics;

/// Rate limiter configuration
#[derive(Clone, Debug)]
pub struct RateLimitConfig {
    /// Requests per second for anonymous (IP-based) clients
    pub ip_requests_per_second: u64,
    /// Requests per second for authenticated (API-key) clients
    pub key_requests_per_second: u64,
    /// Burst allowance (extra requests allowed in short bursts)
    pub burst_size: u32,
    /// Window duration for rate limit calculation
    pub window: Duration,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            ip_requests_per_second: 100,
            key_requests_per_second: 1000, // 10x for authenticated clients
            burst_size: 50,
            window: Duration::from_secs(1),
        }
    }
}

/// Client tracking data
struct ClientLimit {
    count: u64,
    window_start: Instant,
    burst_tokens: u32,
    last_request: Instant,
}

impl ClientLimit {
    fn new(burst_size: u32) -> Self {
        let now = Instant::now();
        Self {
            count: 0,
            window_start: now,
            burst_tokens: burst_size,
            last_request: now,
        }
    }
}

/// Enhanced rate limiter with IP and API key support
#[derive(Clone)]
pub struct RateLimiter {
    /// IP-based limits
    ip_limits: Arc<RwLock<HashMap<String, ClientLimit>>>,
    /// API-key-based limits
    key_limits: Arc<RwLock<HashMap<String, ClientLimit>>>,
    /// Configuration
    config: RateLimitConfig,
}

/// Rate limit error response
#[derive(Serialize)]
struct RateLimitError {
    error: String,
    code: String,
    retry_after_seconds: u64,
    limit_type: String,
}

impl RateLimiter {
    /// Create a new rate limiter with default config
    pub fn new(requests_per_second: u64) -> Self {
        Self::with_config(RateLimitConfig {
            ip_requests_per_second: requests_per_second,
            key_requests_per_second: requests_per_second * 10,
            ..Default::default()
        })
    }

    /// Create a rate limiter with custom configuration
    pub fn with_config(config: RateLimitConfig) -> Self {
        Self {
            ip_limits: Arc::new(RwLock::new(HashMap::new())),
            key_limits: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// Extract API key from request if present
    fn extract_api_key(req: &Request<Body>) -> Option<String> {
        req.headers()
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer ").map(|s| s.to_string()))
    }

    /// Extract client IP from request.
    ///
    /// Uses the direct connection IP to prevent X-Forwarded-For spoofing.
    /// If running behind a trusted reverse proxy, configure the proxy to
    /// set a verified header and update this function accordingly.
    fn extract_client_ip(req: &Request<Body>) -> String {
        req.extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ci| ci.0.ip().to_string())
            .unwrap_or_else(|| "unknown".to_string())
    }

    /// Check rate limit for a client identifier
    async fn check_limit(
        limits: &RwLock<HashMap<String, ClientLimit>>,
        identifier: &str,
        max_requests: u64,
        burst_size: u32,
        window: Duration,
    ) -> Result<(), (u64, String)> {
        let mut limits = limits.write().await;
        let now = Instant::now();

        let client_limit = limits
            .entry(identifier.to_string())
            .or_insert_with(|| ClientLimit::new(burst_size));

        // Replenish burst tokens based on time elapsed
        let elapsed = now.duration_since(client_limit.last_request);
        let tokens_to_add = (elapsed.as_millis() as u32 * burst_size) / 1000;
        client_limit.burst_tokens = (client_limit.burst_tokens + tokens_to_add).min(burst_size);
        client_limit.last_request = now;

        // Reset window if expired
        if now.duration_since(client_limit.window_start) > window {
            client_limit.count = 0;
            client_limit.window_start = now;
        }

        // Check limit
        if client_limit.count >= max_requests {
            // Try to use burst tokens
            if client_limit.burst_tokens > 0 {
                client_limit.burst_tokens -= 1;
                client_limit.count += 1;
                return Ok(());
            }

            // Calculate retry time
            let remaining = window
                .as_secs()
                .saturating_sub(now.duration_since(client_limit.window_start).as_secs());
            return Err((remaining.max(1), identifier.to_string()));
        }

        client_limit.count += 1;
        Ok(())
    }

    /// Clean up old entries from a limits map
    async fn cleanup_old_entries(limits: &RwLock<HashMap<String, ClientLimit>>) {
        let mut limits = limits.write().await;
        let now = Instant::now();

        // Always evict stale entries when map grows beyond a modest threshold
        if limits.len() > 1000 {
            limits.retain(|_, limit| {
                now.duration_since(limit.window_start) < Duration::from_secs(60)
            });
        }
    }

    /// Check rate limit middleware
    pub async fn check_rate_limit(
        &self,
        req: Request<Body>,
        next: Next,
    ) -> Result<Response, StatusCode> {
        let client_ip = Self::extract_client_ip(&req);
        let api_key = Self::extract_api_key(&req);

        // Determine which limit to apply
        let (identifier, limit_type, max_requests) = if let Some(ref key) = api_key {
            // Use a hash of the full key to avoid storing actual keys
            let key_hash = {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                key.hash(&mut h);
                format!("key:{:016x}", h.finish())
            };
            (key_hash, "api_key", self.config.key_requests_per_second)
        } else {
            (
                format!("ip:{}", client_ip),
                "ip",
                self.config.ip_requests_per_second,
            )
        };

        // Choose the appropriate limits map
        let limits = if api_key.is_some() {
            &self.key_limits
        } else {
            &self.ip_limits
        };

        // Check the rate limit
        match Self::check_limit(
            limits,
            &identifier,
            max_requests,
            self.config.burst_size,
            self.config.window,
        )
        .await
        {
            Ok(()) => {
                // Periodically cleanup old entries
                if rand::random::<u8>() < 25 {
                    // ~10% chance
                    let ip_limits = self.ip_limits.clone();
                    let key_limits = self.key_limits.clone();
                    tokio::spawn(async move {
                        Self::cleanup_old_entries(&ip_limits).await;
                        Self::cleanup_old_entries(&key_limits).await;
                    });
                }

                Ok(next.run(req).await)
            }
            Err((retry_after, id)) => {
                metrics::record_rate_limit_exceeded();

                tracing::warn!(
                    identifier = %id,
                    limit_type = %limit_type,
                    max_requests = %max_requests,
                    "Rate limit exceeded"
                );

                let _response = (
                    StatusCode::TOO_MANY_REQUESTS,
                    [(header::RETRY_AFTER, retry_after.to_string())],
                    Json(RateLimitError {
                        error: "Rate limit exceeded".to_string(),
                        code: "RATE_LIMIT_EXCEEDED".to_string(),
                        retry_after_seconds: retry_after,
                        limit_type: limit_type.to_string(),
                    }),
                )
                    .into_response();

                Err(StatusCode::TOO_MANY_REQUESTS)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rate_limiter_creation() {
        let limiter = RateLimiter::new(100);
        assert_eq!(limiter.config.ip_requests_per_second, 100);
        assert_eq!(limiter.config.key_requests_per_second, 1000);
    }

    #[tokio::test]
    async fn test_rate_limit_check() {
        let limits = RwLock::new(HashMap::new());
        let config = RateLimitConfig::default();

        // First request should succeed
        let result =
            RateLimiter::check_limit(&limits, "test-client", 5, config.burst_size, config.window)
                .await;
        assert!(result.is_ok());

        // Should allow up to limit
        for _ in 0..4 {
            let result = RateLimiter::check_limit(
                &limits,
                "test-client",
                5,
                config.burst_size,
                config.window,
            )
            .await;
            assert!(result.is_ok());
        }
    }

    #[tokio::test]
    async fn test_different_clients_have_separate_limits() {
        let limits = RwLock::new(HashMap::new());

        // Client A uses all their requests
        for _ in 0..5 {
            let _ =
                RateLimiter::check_limit(&limits, "client-a", 5, 0, Duration::from_secs(1)).await;
        }

        // Client A should be rate limited
        let result =
            RateLimiter::check_limit(&limits, "client-a", 5, 0, Duration::from_secs(1)).await;
        assert!(result.is_err());

        // Client B should still be allowed
        let result =
            RateLimiter::check_limit(&limits, "client-b", 5, 0, Duration::from_secs(1)).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_extract_api_key() {
        let req = Request::builder()
            .header(header::AUTHORIZATION, "Bearer test-key-123")
            .body(Body::empty())
            .unwrap();

        let key = RateLimiter::extract_api_key(&req);
        assert_eq!(key, Some("test-key-123".to_string()));

        // Without Bearer prefix
        let req2 = Request::builder()
            .header(header::AUTHORIZATION, "Basic xyz")
            .body(Body::empty())
            .unwrap();

        let key2 = RateLimiter::extract_api_key(&req2);
        assert_eq!(key2, None);
    }
}
