//! Redis Client for ShadowMesh Gateway
//!
//! Provides connection management and common operations for:
//! - Deployment persistence
//! - Distributed rate limiting
//! - API key storage

use redis::{aio::ConnectionManager, AsyncCommands, Client, RedisError};
use serde::{de::DeserializeOwned, Serialize};
use std::time::Duration;

/// Redis client with connection pooling
#[derive(Clone)]
pub struct RedisClient {
    conn: ConnectionManager,
    key_prefix: String,
}

/// Error type for Redis operations
#[derive(Debug, thiserror::Error)]
pub enum RedisClientError {
    #[error("Redis connection error: {0}")]
    Connection(#[from] RedisError),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl RedisClient {
    /// Create a new Redis client
    pub async fn new(url: &str, key_prefix: String) -> Result<Self, RedisClientError> {
        let client = Client::open(url)?;
        let conn = ConnectionManager::new(client).await?;

        // Redact credentials before logging
        let display_url = if let Some(at_pos) = url.find('@') {
            if let Some(scheme_end) = url.find("://") {
                if scheme_end + 3 <= at_pos {
                    format!("{}://***@{}", &url[..scheme_end], &url[at_pos + 1..])
                } else {
                    "redis://***@<redacted>".to_string()
                }
            } else {
                "redis://***@<redacted>".to_string()
            }
        } else {
            url.to_string()
        };
        tracing::info!(url = %display_url, "Connected to Redis");

        Ok(Self { conn, key_prefix })
    }

    /// Get the full key with prefix
    fn prefixed_key(&self, key: &str) -> String {
        format!("{}{}", self.key_prefix, key)
    }

    /// Set a JSON value with optional TTL
    pub async fn set_json<T: Serialize>(
        &self,
        key: &str,
        value: &T,
        ttl: Option<Duration>,
    ) -> Result<(), RedisClientError> {
        let json = serde_json::to_string(value)?;
        let full_key = self.prefixed_key(key);

        let mut conn = self.conn.clone();
        if let Some(ttl) = ttl {
            let _: () = conn.set_ex(&full_key, &json, ttl.as_secs()).await?;
        } else {
            let _: () = conn.set(&full_key, &json).await?;
        }

        Ok(())
    }

    /// Get a JSON value
    pub async fn get_json<T: DeserializeOwned>(
        &self,
        key: &str,
    ) -> Result<Option<T>, RedisClientError> {
        let full_key = self.prefixed_key(key);
        let mut conn = self.conn.clone();

        let result: Option<String> = conn.get(&full_key).await?;

        match result {
            Some(json) => Ok(Some(serde_json::from_str(&json)?)),
            None => Ok(None),
        }
    }

    /// Delete a key
    pub async fn delete(&self, key: &str) -> Result<bool, RedisClientError> {
        let full_key = self.prefixed_key(key);
        let mut conn = self.conn.clone();

        let deleted: i64 = conn.del(&full_key).await?;
        Ok(deleted > 0)
    }

    /// Check if a key exists
    pub async fn exists(&self, key: &str) -> Result<bool, RedisClientError> {
        let full_key = self.prefixed_key(key);
        let mut conn = self.conn.clone();

        let exists: bool = conn.exists(&full_key).await?;
        Ok(exists)
    }

    /// Increment a counter with TTL (for rate limiting)
    /// Returns the new count after increment
    pub async fn incr_with_ttl(&self, key: &str, ttl_secs: u64) -> Result<i64, RedisClientError> {
        let full_key = self.prefixed_key(key);
        let mut conn = self.conn.clone();

        // Use a Lua script for atomic increment + expire
        let script = redis::Script::new(
            r#"
            local count = redis.call('INCR', KEYS[1])
            if count == 1 then
                redis.call('EXPIRE', KEYS[1], ARGV[1])
            end
            return count
            "#,
        );

        let count: i64 = script
            .key(&full_key)
            .arg(ttl_secs)
            .invoke_async(&mut conn)
            .await?;

        Ok(count)
    }

    /// Add to a sorted set with score (for deployment list ordering)
    pub async fn zadd(&self, key: &str, member: &str, score: f64) -> Result<(), RedisClientError> {
        let full_key = self.prefixed_key(key);
        let mut conn = self.conn.clone();

        let _: () = conn.zadd(&full_key, member, score).await?;
        Ok(())
    }

    /// Remove from a sorted set
    pub async fn zrem(&self, key: &str, member: &str) -> Result<(), RedisClientError> {
        let full_key = self.prefixed_key(key);
        let mut conn = self.conn.clone();

        let _: () = conn.zrem(&full_key, member).await?;
        Ok(())
    }

    /// Get members from sorted set in reverse order (newest first)
    pub async fn zrevrange(
        &self,
        key: &str,
        start: isize,
        stop: isize,
    ) -> Result<Vec<String>, RedisClientError> {
        let full_key = self.prefixed_key(key);
        let mut conn = self.conn.clone();

        let members: Vec<String> = conn.zrevrange(&full_key, start, stop).await?;
        Ok(members)
    }

    /// Add to a set
    pub async fn sadd(&self, key: &str, member: &str) -> Result<(), RedisClientError> {
        let full_key = self.prefixed_key(key);
        let mut conn = self.conn.clone();

        let _: () = conn.sadd(&full_key, member).await?;
        Ok(())
    }

    /// Remove from a set
    pub async fn srem(&self, key: &str, member: &str) -> Result<(), RedisClientError> {
        let full_key = self.prefixed_key(key);
        let mut conn = self.conn.clone();

        let _: () = conn.srem(&full_key, member).await?;
        Ok(())
    }

    /// Get all members of a set
    pub async fn smembers(&self, key: &str) -> Result<Vec<String>, RedisClientError> {
        let full_key = self.prefixed_key(key);
        let mut conn = self.conn.clone();

        let members: Vec<String> = conn.smembers(&full_key).await?;
        Ok(members)
    }

    /// Ping Redis to check connection health
    pub async fn ping(&self) -> Result<bool, RedisClientError> {
        let mut conn = self.conn.clone();
        let pong: String = redis::cmd("PING").query_async(&mut conn).await?;
        Ok(pong == "PONG")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests require a running Redis instance
    // Run with: docker run -d -p 6379:6379 redis:7-alpine

    #[tokio::test]
    #[ignore] // Requires Redis
    async fn test_redis_connection() {
        let client = RedisClient::new("redis://127.0.0.1:6379", "test:".to_string())
            .await
            .unwrap();
        assert!(client.ping().await.unwrap());
    }

    #[tokio::test]
    #[ignore] // Requires Redis
    async fn test_json_operations() {
        let client = RedisClient::new("redis://127.0.0.1:6379", "test:".to_string())
            .await
            .unwrap();

        #[derive(Debug, Serialize, serde::Deserialize, PartialEq)]
        struct TestData {
            name: String,
            value: i32,
        }

        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };

        // Set
        client.set_json("test_key", &data, None).await.unwrap();

        // Get
        let retrieved: Option<TestData> = client.get_json("test_key").await.unwrap();
        assert_eq!(retrieved, Some(data));

        // Delete
        assert!(client.delete("test_key").await.unwrap());
        let retrieved: Option<TestData> = client.get_json("test_key").await.unwrap();
        assert_eq!(retrieved, None);
    }

    #[tokio::test]
    #[ignore] // Requires Redis
    async fn test_incr_with_ttl() {
        let client = RedisClient::new("redis://127.0.0.1:6379", "test:".to_string())
            .await
            .unwrap();

        // Clean up first
        client.delete("counter_test").await.ok();

        // Increment
        let count1 = client.incr_with_ttl("counter_test", 60).await.unwrap();
        assert_eq!(count1, 1);

        let count2 = client.incr_with_ttl("counter_test", 60).await.unwrap();
        assert_eq!(count2, 2);

        // Clean up
        client.delete("counter_test").await.ok();
    }
}
