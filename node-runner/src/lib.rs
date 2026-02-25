//! ShadowMesh Node Runner
//!
//! A complete node implementation for the ShadowMesh decentralized CDN.

use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

pub mod api;
pub mod bridge;
pub mod config;
pub mod dashboard;
pub mod metrics;
pub mod p2p;
pub mod p2p_commands;
pub mod replication;
pub mod storage;

use config::NodeConfig;
use metrics::MetricsCollector;
use storage::StorageManager;

/// Shared application state
pub struct AppState {
    /// Node peer ID
    pub peer_id: String,
    /// Node configuration
    pub config: RwLock<NodeConfig>,
    /// Metrics collector
    pub metrics: Arc<MetricsCollector>,
    /// Storage manager
    pub storage: Arc<StorageManager>,
    /// Shutdown signal sender
    pub shutdown_signal: broadcast::Sender<()>,
    /// P2P state (None if P2P failed to initialize)
    pub p2p: Option<Arc<p2p::P2pState>>,
    /// WebRTC bridge state (None if bridge is disabled)
    pub bridge: Option<Arc<bridge::BridgeState>>,
    /// Replication state (None if replication is disabled or P2P unavailable)
    pub replication: Option<Arc<replication::ReplicationState>>,
}
