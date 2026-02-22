use config::{Config as ConfigLoader, ConfigError, File};
use serde::Deserialize;
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub cache: CacheConfig,
    pub rate_limit: RateLimitConfig,
    pub ipfs: IpfsConfig,
    pub security: SecurityConfig,
    pub monitoring: MonitoringConfig,
    #[serde(default)]
    pub circuit_breaker: CircuitBreakerConfig,
    #[serde(default)]
    pub deploy: DeployConfig,
    #[serde(default)]
    pub telemetry: TelemetryConfig,
    #[serde(default)]
    pub redis: RedisConfig,
    #[serde(default)]
    pub naming: NamingConfig,
}

/// Validation errors for configuration
#[derive(Debug, thiserror::Error)]
#[allow(clippy::enum_variant_names)]
pub enum ConfigValidationError {
    #[error("Invalid port: {0}")]
    InvalidPort(u16),
    #[error("Invalid workers count: must be > 0")]
    InvalidWorkers,
    #[error("Invalid cache size: must be > 0")]
    InvalidCacheSize,
    #[error("Invalid rate limit: must be > 0")]
    InvalidRateLimit,
    #[error("Invalid IPFS URL: {0}")]
    InvalidIpfsUrl(String),
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

impl SecurityConfig {
    /// Load CORS origins from environment variable, falling back to config
    pub fn get_allowed_origins(&self) -> Vec<String> {
        // Check for environment variable override
        if let Ok(origins_env) = std::env::var("SHADOWMESH_SECURITY_ALLOWED_ORIGINS") {
            let origins: Vec<String> = origins_env
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if !origins.is_empty() {
                return origins;
            }
        }
        self.allowed_origins.clone()
    }

    /// Check if CORS is configured permissively
    pub fn is_cors_permissive(&self) -> bool {
        self.get_allowed_origins().contains(&"*".to_string())
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct MonitoringConfig {
    pub metrics_enabled: bool,
    pub health_check_interval_seconds: u64,
}

impl MonitoringConfig {
    pub fn health_check_interval(&self) -> Duration {
        Duration::from_secs(self.health_check_interval_seconds)
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of consecutive failures before opening circuit
    pub failure_threshold: u32,
    /// Seconds to wait before testing recovery
    pub reset_timeout_seconds: u64,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            reset_timeout_seconds: 30,
        }
    }
}

impl CircuitBreakerConfig {
    pub fn reset_timeout(&self) -> Duration {
        Duration::from_secs(self.reset_timeout_seconds)
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct DeployConfig {
    /// Maximum deployment size in MB
    pub max_size_mb: u64,
}

impl Default for DeployConfig {
    fn default() -> Self {
        Self { max_size_mb: 100 }
    }
}

impl DeployConfig {
    pub fn max_size_bytes(&self) -> usize {
        self.max_size_mb
            .saturating_mul(1024)
            .saturating_mul(1024)
            .min(usize::MAX as u64) as usize
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct TelemetryConfig {
    /// Enable OpenTelemetry tracing
    pub enabled: bool,
    /// Service name for tracing
    pub service_name: String,
    /// OTLP endpoint (e.g., http://localhost:4317)
    pub otlp_endpoint: Option<String>,
    /// Sampling rate (0.0 to 1.0)
    pub sample_rate: f64,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            service_name: "shadowmesh-gateway".to_string(),
            otlp_endpoint: None,
            sample_rate: 1.0,
        }
    }
}

impl TelemetryConfig {
    /// Get OTLP endpoint from config or environment
    pub fn get_otlp_endpoint(&self) -> Option<String> {
        std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
            .ok()
            .or_else(|| self.otlp_endpoint.clone())
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct RedisConfig {
    /// Redis URL (e.g., redis://127.0.0.1:6379)
    pub url: Option<String>,
    /// Key prefix for all Redis keys
    pub key_prefix: String,
}

impl Default for RedisConfig {
    fn default() -> Self {
        Self {
            url: None,
            key_prefix: "shadowmesh:".to_string(),
        }
    }
}

impl RedisConfig {
    /// Get Redis URL from config or environment
    pub fn get_url(&self) -> Option<String> {
        std::env::var("REDIS_URL").ok().or_else(|| self.url.clone())
    }
}

/// Configuration for the decentralized naming layer
#[derive(Debug, Deserialize, Clone)]
pub struct NamingConfig {
    /// Enable the naming service
    pub enabled: bool,
    /// Advertise this gateway in the naming layer
    pub advertise_gateway: bool,
    /// Maximum name record cache entries
    pub cache_size: usize,
}

impl Default for NamingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            advertise_gateway: true,
            cache_size: 10_000,
        }
    }
}

impl Config {
    /// Load configuration from file and environment variables
    ///
    /// Priority (highest to lowest):
    /// 1. Environment variables (SHADOWMESH_SERVER_PORT, etc.)
    /// 2. Config file specified by SHADOWMESH_CONFIG_PATH
    /// 3. gateway/config.toml
    /// 4. config.toml in current directory
    /// 5. Default values
    pub fn load() -> Result<Self, ConfigError> {
        Self::load_from_paths(&["config", "gateway/config"])
    }

    /// Load configuration from specific paths
    pub fn load_from_paths(paths: &[&str]) -> Result<Self, ConfigError> {
        let mut builder = ConfigLoader::builder();

        // Add config files (optional)
        for path in paths {
            builder = builder.add_source(File::with_name(path).required(false));
        }

        // Check for custom config path via environment
        if let Ok(custom_path) = std::env::var("SHADOWMESH_CONFIG_PATH") {
            if Path::new(&custom_path).exists() {
                builder = builder.add_source(File::with_name(&custom_path).required(true));
            }
        }

        // Add environment variables with prefix
        builder = builder.add_source(
            config::Environment::with_prefix("SHADOWMESH")
                .separator("_")
                .try_parsing(true),
        );

        let config = builder.build()?;
        config.try_deserialize()
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), ConfigValidationError> {
        // Validate port
        if self.server.port == 0 {
            return Err(ConfigValidationError::InvalidPort(self.server.port));
        }

        // Validate workers
        if self.server.workers == 0 {
            return Err(ConfigValidationError::InvalidWorkers);
        }

        // Validate cache size
        if self.cache.max_size_mb == 0 {
            return Err(ConfigValidationError::InvalidCacheSize);
        }

        // Validate rate limit
        if self.rate_limit.enabled && self.rate_limit.requests_per_second == 0 {
            return Err(ConfigValidationError::InvalidRateLimit);
        }

        // Validate IPFS URL
        if !self.ipfs.api_url.starts_with("http://") && !self.ipfs.api_url.starts_with("https://") {
            return Err(ConfigValidationError::InvalidIpfsUrl(
                self.ipfs.api_url.clone(),
            ));
        }

        Ok(())
    }

    /// Load and validate configuration
    pub fn load_and_validate() -> Result<Self, Box<dyn std::error::Error>> {
        let config = Self::load()?;
        config.validate()?;
        Ok(config)
    }

    /// Print configuration summary
    pub fn print_summary(&self) {
        println!("📋 Configuration Summary:");
        println!(
            "   Server: {}:{} ({} workers)",
            self.server.host, self.server.port, self.server.workers
        );
        println!(
            "   Cache: {} MB (TTL: {}s)",
            self.cache.max_size_mb, self.cache.ttl_seconds
        );
        if self.rate_limit.enabled {
            println!(
                "   Rate Limit: {} req/s (burst: {})",
                self.rate_limit.requests_per_second, self.rate_limit.burst_size
            );
        } else {
            println!("   Rate Limit: disabled");
        }
        println!(
            "   IPFS: {} (timeout: {}s, retries: {})",
            self.ipfs.api_url, self.ipfs.timeout_seconds, self.ipfs.retry_attempts
        );
        println!(
            "   Security: CORS={}, Max Request={}MB",
            self.security.cors_enabled, self.security.max_request_size_mb
        );
        if self.monitoring.metrics_enabled {
            println!(
                "   Monitoring: enabled (health interval: {}s)",
                self.monitoring.health_check_interval_seconds
            );
        }
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
                cors_enabled: false,
                allowed_origins: vec![],
                max_request_size_mb: 100,
            },
            monitoring: MonitoringConfig {
                metrics_enabled: true,
                health_check_interval_seconds: 30,
            },
            circuit_breaker: CircuitBreakerConfig::default(),
            deploy: DeployConfig::default(),
            telemetry: TelemetryConfig::default(),
            redis: RedisConfig::default(),
            naming: NamingConfig::default(),
        }
    }
}
