use config::{Config as ConfigLoader, ConfigError, File};
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub cache: CacheConfig,
    pub rate_limit: RateLimitConfig,
    pub ipfs: IpfsConfig,
    pub security: SecurityConfig,
    pub monitoring: MonitoringConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub workers: usize,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CacheConfig {
    pub max_size_mb: u64,
    pub ttl_seconds: u64,
}

impl CacheConfig {
    pub fn max_size_bytes(&self) -> u64 {
        self.max_size_mb * 1024 * 1024
    }

    pub fn ttl(&self) -> Duration {
        Duration::from_secs(self.ttl_seconds)
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct RateLimitConfig {
    pub enabled: bool,
    pub requests_per_second: u64,
    pub burst_size: u32,
}

#[derive(Debug, Deserialize, Clone)]
pub struct IpfsConfig {
    pub api_url: String,
    pub timeout_seconds: u64,
    pub retry_attempts: u32,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SecurityConfig {
    pub cors_enabled: bool,
    pub allowed_origins: Vec<String>,
    pub max_request_size_mb: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MonitoringConfig {
    pub metrics_enabled: bool,
    pub health_check_interval_seconds: u64,
}

impl Config {
    pub fn load() -> Result<Self, ConfigError> {
        let config = ConfigLoader::builder()
            .add_source(File::with_name("gateway/config").required(false))
            .add_source(config::Environment::with_prefix("SHADOWMESH"))
            .build()?;

        config.try_deserialize()
    }

    pub fn default() -> Self {
        Self {
            server: ServerConfig {
                host: "0.0.0.0".to_string(),
                port: 8080,
                workers: 4,
            },
            cache: CacheConfig {
                max_size_mb: 500,
                ttl_seconds: 3600,
            },
            rate_limit: RateLimitConfig {
                enabled: true,
                requests_per_second: 100,
                burst_size: 200,
            },
            ipfs: IpfsConfig {
                api_url: "http://127.0.0.1:5001".to_string(),
                timeout_seconds: 30,
                retry_attempts: 3,
            },
            security: SecurityConfig {
                cors_enabled: true,
                allowed_origins: vec!["*".to_string()],
                max_request_size_mb: 100,
            },
            monitoring: MonitoringConfig {
                metrics_enabled: true,
                health_check_interval_seconds: 30,
            },
        }
    }
}
