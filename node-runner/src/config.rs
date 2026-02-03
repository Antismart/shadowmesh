//! Node Runner Configuration
//!
//! File-based configuration with environment variable overrides.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

/// Default configuration file path
pub const DEFAULT_CONFIG_PATH: &str = "node-config.toml";

/// Default data directory
pub const DEFAULT_DATA_DIR: &str = ".shadowmesh";

/// Node runner configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeConfig {
    /// Node identity configuration
    #[serde(default)]
    pub identity: IdentityConfig,

    /// Storage configuration
    #[serde(default)]
    pub storage: StorageConfig,

    /// Network configuration
    #[serde(default)]
    pub network: NetworkConfig,

    /// Dashboard configuration
    #[serde(default)]
    pub dashboard: DashboardConfig,

    /// Performance configuration
    #[serde(default)]
    pub performance: PerformanceConfig,
}

/// Node identity configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityConfig {
    /// Path to identity key file (will be generated if not exists)
    #[serde(default = "default_identity_path")]
    pub key_file: PathBuf,

    /// Node display name
    #[serde(default = "default_node_name")]
    pub name: String,

    /// Node region (for geographic routing)
    pub region: Option<String>,
}

fn default_identity_path() -> PathBuf {
    PathBuf::from(DEFAULT_DATA_DIR).join("identity.key")
}

fn default_node_name() -> String {
    "ShadowMesh Node".to_string()
}

impl Default for IdentityConfig {
    fn default() -> Self {
        Self {
            key_file: default_identity_path(),
            name: default_node_name(),
            region: None,
        }
    }
}

/// Storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Data directory for fragments
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,

    /// Maximum storage capacity in bytes (default: 10 GB)
    #[serde(default = "default_max_storage")]
    pub max_storage_bytes: u64,

    /// Enable garbage collection for unpinned content
    #[serde(default = "default_true")]
    pub gc_enabled: bool,

    /// Garbage collection interval in seconds
    #[serde(default = "default_gc_interval")]
    pub gc_interval_secs: u64,

    /// Minimum free space to maintain (bytes)
    #[serde(default = "default_min_free_space")]
    pub min_free_space_bytes: u64,
}

fn default_data_dir() -> PathBuf {
    PathBuf::from(DEFAULT_DATA_DIR).join("data")
}

fn default_max_storage() -> u64 {
    10 * 1024 * 1024 * 1024 // 10 GB
}

fn default_gc_interval() -> u64 {
    3600 // 1 hour
}

fn default_min_free_space() -> u64 {
    1024 * 1024 * 1024 // 1 GB
}

fn default_true() -> bool {
    true
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            data_dir: default_data_dir(),
            max_storage_bytes: default_max_storage(),
            gc_enabled: true,
            gc_interval_secs: default_gc_interval(),
            min_free_space_bytes: default_min_free_space(),
        }
    }
}

impl StorageConfig {
    pub fn max_storage_gb(&self) -> f64 {
        self.max_storage_bytes as f64 / (1024.0 * 1024.0 * 1024.0)
    }

    pub fn gc_interval(&self) -> Duration {
        Duration::from_secs(self.gc_interval_secs)
    }
}

/// Network configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// P2P listen addresses
    #[serde(default = "default_listen_addresses")]
    pub listen_addresses: Vec<String>,

    /// Bootstrap nodes to connect to
    #[serde(default)]
    pub bootstrap_nodes: Vec<String>,

    /// Maximum inbound bandwidth (bytes/sec, None = unlimited)
    pub max_inbound_bandwidth: Option<u64>,

    /// Maximum outbound bandwidth (bytes/sec, None = unlimited)
    pub max_outbound_bandwidth: Option<u64>,

    /// Maximum number of peer connections
    #[serde(default = "default_max_peers")]
    pub max_peers: usize,

    /// Enable mDNS local peer discovery
    #[serde(default = "default_true")]
    pub enable_mdns: bool,

    /// Enable DHT for content discovery
    #[serde(default = "default_true")]
    pub enable_dht: bool,

    /// Replication factor for stored content
    #[serde(default = "default_replication_factor")]
    pub replication_factor: usize,
}

fn default_listen_addresses() -> Vec<String> {
    vec![
        "/ip4/0.0.0.0/tcp/4001".to_string(),
        "/ip6/::/tcp/4001".to_string(),
    ]
}

fn default_max_peers() -> usize {
    50
}

fn default_replication_factor() -> usize {
    3
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            listen_addresses: default_listen_addresses(),
            bootstrap_nodes: Vec::new(),
            max_inbound_bandwidth: None,
            max_outbound_bandwidth: None,
            max_peers: default_max_peers(),
            enable_mdns: true,
            enable_dht: true,
            replication_factor: default_replication_factor(),
        }
    }
}

/// Dashboard configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardConfig {
    /// Enable web dashboard
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Dashboard listen address
    #[serde(default = "default_dashboard_host")]
    pub host: String,

    /// Dashboard port
    #[serde(default = "default_dashboard_port")]
    pub port: u16,

    /// Enable CORS for API
    #[serde(default = "default_true")]
    pub enable_cors: bool,

    /// Allowed CORS origins (empty = allow all)
    #[serde(default)]
    pub cors_origins: Vec<String>,
}

fn default_dashboard_host() -> String {
    "127.0.0.1".to_string()
}

fn default_dashboard_port() -> u16 {
    3030
}

impl Default for DashboardConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            host: default_dashboard_host(),
            port: default_dashboard_port(),
            enable_cors: true,
            cors_origins: Vec::new(),
        }
    }
}

impl DashboardConfig {
    pub fn bind_address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// Performance configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceConfig {
    /// Number of worker threads (0 = auto)
    #[serde(default)]
    pub worker_threads: usize,

    /// Maximum concurrent requests
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_requests: usize,

    /// Request timeout in seconds
    #[serde(default = "default_request_timeout")]
    pub request_timeout_secs: u64,

    /// Enable request caching
    #[serde(default = "default_true")]
    pub enable_cache: bool,

    /// Cache size in entries
    #[serde(default = "default_cache_size")]
    pub cache_size: usize,
}

fn default_max_concurrent() -> usize {
    100
}

fn default_request_timeout() -> u64 {
    30
}

fn default_cache_size() -> usize {
    1000
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            worker_threads: 0,
            max_concurrent_requests: default_max_concurrent(),
            request_timeout_secs: default_request_timeout(),
            enable_cache: true,
            cache_size: default_cache_size(),
        }
    }
}

impl PerformanceConfig {
    pub fn request_timeout(&self) -> Duration {
        Duration::from_secs(self.request_timeout_secs)
    }
}

impl NodeConfig {
    /// Load configuration from file, environment, and defaults
    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        let config_path =
            std::env::var("SHADOWMESH_CONFIG").unwrap_or_else(|_| DEFAULT_CONFIG_PATH.to_string());

        let mut builder = config::Config::builder()
            .set_default("identity.name", "ShadowMesh Node")?
            .set_default("storage.max_storage_bytes", default_max_storage() as i64)?
            .set_default("network.max_peers", default_max_peers() as i64)?
            .set_default("dashboard.port", default_dashboard_port() as i64)?;

        // Try to load from file
        if std::path::Path::new(&config_path).exists() {
            builder = builder.add_source(config::File::with_name(&config_path));
        }

        // Override with environment variables
        builder = builder.add_source(
            config::Environment::with_prefix("SHADOWMESH")
                .separator("__")
                .try_parsing(true),
        );

        let config = builder.build()?;
        let node_config: NodeConfig = config.try_deserialize()?;

        Ok(node_config)
    }

    /// Save configuration to file
    pub fn save(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let toml_string = toml::to_string_pretty(self)?;
        std::fs::write(path, toml_string)?;
        Ok(())
    }

    /// Create default configuration file
    pub fn create_default_config(path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let config = NodeConfig::default();
        config.save(path)
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if self.storage.max_storage_bytes < 1024 * 1024 * 100 {
            errors.push("Storage must be at least 100 MB".to_string());
        }

        if self.network.max_peers == 0 {
            errors.push("Max peers must be at least 1".to_string());
        }

        if self.network.replication_factor == 0 || self.network.replication_factor > 10 {
            errors.push("Replication factor must be between 1 and 10".to_string());
        }

        if self.dashboard.port == 0 {
            errors.push("Dashboard port must be specified".to_string());
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Print configuration summary
    pub fn print_summary(&self) {
        println!("📋 Node Configuration:");
        println!("   Name: {}", self.identity.name);
        println!("   Storage: {:.2} GB", self.storage.max_storage_gb());
        println!("   Max Peers: {}", self.network.max_peers);
        println!("   Replication: {}x", self.network.replication_factor);
        println!("   Dashboard: http://{}", self.dashboard.bind_address());
        if let Some(region) = &self.identity.region {
            println!("   Region: {}", region);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = NodeConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_storage_config() {
        let storage = StorageConfig::default();
        assert_eq!(storage.max_storage_gb(), 10.0);
    }

    #[test]
    fn test_dashboard_bind_address() {
        let dashboard = DashboardConfig::default();
        assert_eq!(dashboard.bind_address(), "127.0.0.1:3030");
    }
}
