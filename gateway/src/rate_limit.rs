use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode},
    middleware::Next,
    response::Response,
};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct RateLimiter {
    limits: Arc<RwLock<HashMap<String, ClientLimit>>>,
    requests_per_second: u64,
    window: Duration,
}

struct ClientLimit {
    count: u64,
    window_start: Instant,
}

impl RateLimiter {
    pub fn new(requests_per_second: u64) -> Self {
        Self {
            limits: Arc::new(RwLock::new(HashMap::new())),
            requests_per_second,
            window: Duration::from_secs(1),
        }
    }

    pub async fn check_rate_limit(
        &self,
        req: Request<Body>,
        next: Next,
    ) -> Result<Response, StatusCode> {
        // Extract client IP
        let client_ip = req
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ci| ci.0.ip().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        // Check rate limit
        let mut limits = self.limits.write().await;
        let now = Instant::now();

        let client_limit = limits.entry(client_ip.clone()).or_insert(ClientLimit {
            count: 0,
            window_start: now,
        });

        // Reset window if expired
        if now.duration_since(client_limit.window_start) > self.window {
            client_limit.count = 0;
            client_limit.window_start = now;
        }

        // Check limit
        if client_limit.count >= self.requests_per_second {
            tracing::warn!("Rate limit exceeded for {}", client_ip);
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }

        client_limit.count += 1;

        // Clean up old entries (simple cleanup)
        if limits.len() > 10000 {
            limits.retain(|_, limit| {
                now.duration_since(limit.window_start) < Duration::from_secs(60)
            });
        }

        Ok(next.run(req).await)
    }
}
