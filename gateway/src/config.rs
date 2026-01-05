use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GatewayConfig {
    pub server: ServerConfig,
    pub cache: CacheConfig,
    pub rate_limit: RateLimitConfig,
    pub ipfs: IpfsConfig,
    pub security: SecurityConfig,
    pub monitoring: MonitoringConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub workers: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CacheConfig {
    pub max_size_mb: usize,
    pub ttl_seconds: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RateLimitConfig {
    pub enabled: bool,
    pub requests_per_second: u32,
    pub burst_size: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IpfsConfig {
    pub api_url: String,
    pub timeout_seconds: u64,
    pub retry_attempts: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SecurityConfig {
    pub cors_enabled: bool,
    pub allowed_origins: Vec<String>,
    pub max_request_size_mb: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MonitoringConfig {
    pub metrics_enabled: bool,
    pub health_check_interval_seconds: u64,
}

impl GatewayConfig {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let settings = config::Config::builder()
            .add_source(config::File::from(path.as_ref()))
            .build()?;

        let config: GatewayConfig = settings.try_deserialize()?;
        Ok(config)
    }

    pub fn default_config_path() -> &'static str {
        "gateway/config.toml"
    }

    pub fn server_address(&self) -> String {
        format!("{}:{}", self.server.host, self.server.port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_address() {
        let config = GatewayConfig {
            server: ServerConfig {
                host: "0.0.0.0".to_string(),
                port: 8081,
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
        };

        assert_eq!(config.server_address(), "0.0.0.0:8081");
    }
}
