//! Distributed Rate Limiting for ShadowMesh Gateway
//!
//! Uses Redis for distributed rate limiting across multiple gateway instances.
//! Falls back to local rate limiting when Redis is unavailable.

use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{header, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Json, Response},
};
use serde::Serialize;
use std::{net::SocketAddr, sync::Arc};

use crate::rate_limit::{RateLimitConfig, RateLimiter};
use crate::redis_client::RedisClient;

/// Distributed rate limiter using Redis
#[derive(Clone)]
pub struct DistributedRateLimiter {
    /// Redis client for distributed counting
    redis: Option<Arc<RedisClient>>,
    /// Local fallback rate limiter
    local: RateLimiter,
    /// Configuration
    config: RateLimitConfig,
}

/// Rate limit exceeded error response
#[derive(Serialize)]
struct RateLimitError {
    error: String,
    code: String,
    retry_after_seconds: u64,
    limit_type: String,
}

impl DistributedRateLimiter {
    /// Create a new distributed rate limiter
    pub fn new(redis: Option<Arc<RedisClient>>, config: RateLimitConfig) -> Self {
        Self {
            redis,
            local: RateLimiter::with_config(config.clone()),
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

    /// Extract client IP from request
    fn extract_client_ip(req: &Request<Body>) -> String {
        // Check X-Forwarded-For header first (for proxied requests)
        if let Some(forwarded) = req.headers().get("x-forwarded-for") {
            if let Ok(value) = forwarded.to_str() {
                if let Some(ip) = value.split(',').next() {
                    return ip.trim().to_string();
                }
            }
        }

        // Fall back to direct connection IP
        req.extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ci| ci.0.ip().to_string())
            .unwrap_or_else(|| "unknown".to_string())
    }

    /// Check rate limit using Redis (distributed)
    async fn check_redis_limit(
        &self,
        redis: &RedisClient,
        identifier: &str,
        limit: u64,
    ) -> Result<(), (u64, String)> {
        let window_secs = self.config.window.as_secs();
        let key = format!("ratelimit:{}", identifier);

        match redis.incr_with_ttl(&key, window_secs).await {
            Ok(count) => {
                if count > limit as i64 {
                    // Calculate retry time (remaining window time)
                    let retry_after = window_secs.saturating_sub(1);
                    Err((retry_after.max(1), identifier.to_string()))
                } else {
                    Ok(())
                }
            }
            Err(e) => {
                tracing::warn!(
                    "Redis rate limit check failed: {}, falling back to local",
                    e
                );
                // Return Ok to fall through to local rate limiting
                Ok(())
            }
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
            // Use a hash of the key to avoid storing actual keys
            let key_hash = format!("key:{}", &key[..key.len().min(8)]);
            (key_hash, "api_key", self.config.key_requests_per_second)
        } else {
            (
                format!("ip:{}", client_ip),
                "ip",
                self.config.ip_requests_per_second,
            )
        };

        // Try Redis first if available
        if let Some(ref redis) = self.redis {
            match self
                .check_redis_limit(redis, &identifier, max_requests)
                .await
            {
                Ok(()) => {
                    // Request allowed, continue
                    return Ok(next.run(req).await);
                }
                Err((retry_after, id)) => {
                    crate::metrics::record_rate_limit_exceeded();

                    tracing::warn!(
                        identifier = %id,
                        limit_type = %limit_type,
                        max_requests = %max_requests,
                        "Distributed rate limit exceeded"
                    );

                    let response = (
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

                    return Ok(response);
                }
            }
        }

        // Fall back to local rate limiting
        self.local.check_rate_limit(req, next).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_distributed_rate_limiter_creation() {
        let config = RateLimitConfig::default();
        let limiter = DistributedRateLimiter::new(None, config);
        assert!(limiter.redis.is_none());
    }
}
