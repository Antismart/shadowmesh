//! Transport Configuration for ShadowMesh
//!
//! Provides configuration types for multi-transport support (TCP, WebRTC).

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Default TCP port for P2P connections
pub const DEFAULT_TCP_PORT: u16 = 4001;

/// Default WebRTC port (UDP)
pub const DEFAULT_WEBRTC_PORT: u16 = 4002;

/// Default STUN servers for NAT traversal.
///
/// Reliable public Google STUN servers are used as the primary choice.
pub const DEFAULT_STUN_SERVERS: &[&str] = &[
    "stun:stun.l.google.com:19302",
    "stun:stun1.l.google.com:19302",
    "stun:stun2.l.google.com:19302",
];

/// Transport configuration for ShadowNode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportConfig {
    /// Enable TCP transport (server-to-server)
    #[serde(default = "default_true")]
    pub enable_tcp: bool,

    /// TCP listen port
    #[serde(default = "default_tcp_port")]
    pub tcp_port: u16,

    /// Enable WebRTC transport (browser connections)
    #[serde(default)]
    pub enable_webrtc: bool,

    /// WebRTC listen port (UDP)
    #[serde(default = "default_webrtc_port")]
    pub webrtc_port: u16,

    /// STUN servers for NAT traversal
    #[serde(default = "default_stun_servers")]
    pub stun_servers: Vec<String>,

    /// TURN servers for relay fallback
    #[serde(default)]
    pub turn_servers: Vec<TurnServer>,

    /// Connection timeout
    #[serde(default = "default_connection_timeout")]
    pub connection_timeout_secs: u64,

    /// Maximum concurrent connections
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,
}

/// TURN server configuration for relay fallback
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnServer {
    /// TURN server URL (e.g., "turn:turn.example.com:3478")
    pub url: String,

    /// Username for TURN authentication
    pub username: String,

    /// Credential/password for TURN authentication
    pub credential: String,

    /// Transport protocol (udp, tcp, tls)
    #[serde(default = "default_turn_transport")]
    pub transport: TurnTransport,
}

/// TURN transport protocol
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TurnTransport {
    #[default]
    Udp,
    Tcp,
    Tls,
}

/// WebRTC-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebRtcConfig {
    /// Enable WebRTC transport
    #[serde(default)]
    pub enabled: bool,

    /// Listen port for WebRTC (UDP)
    #[serde(default = "default_webrtc_port")]
    pub port: u16,

    /// STUN servers
    #[serde(default = "default_stun_servers")]
    pub stun_servers: Vec<String>,

    /// TURN servers
    #[serde(default)]
    pub turn_servers: Vec<TurnServer>,

    /// ICE candidate gathering timeout
    #[serde(default = "default_ice_timeout")]
    pub ice_gathering_timeout_secs: u64,

    /// Maximum number of WebRTC connections
    #[serde(default = "default_max_webrtc_connections")]
    pub max_connections: usize,
}

/// Signaling configuration for WebRTC peer discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalingConfig {
    /// Enable signaling server
    #[serde(default)]
    pub enabled: bool,

    /// Signaling server URL (for clients)
    pub server_url: Option<String>,

    /// Maximum peers to track
    #[serde(default = "default_max_signaling_peers")]
    pub max_peers: usize,

    /// Session timeout
    #[serde(default = "default_signaling_timeout")]
    pub session_timeout_secs: u64,

    /// Heartbeat interval
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval_secs: u64,
}

// Default value functions
fn default_true() -> bool {
    true
}

fn default_tcp_port() -> u16 {
    DEFAULT_TCP_PORT
}

fn default_webrtc_port() -> u16 {
    DEFAULT_WEBRTC_PORT
}

fn default_stun_servers() -> Vec<String> {
    DEFAULT_STUN_SERVERS.iter().map(|s| s.to_string()).collect()
}

fn default_turn_transport() -> TurnTransport {
    TurnTransport::Udp
}

fn default_connection_timeout() -> u64 {
    30
}

fn default_max_connections() -> usize {
    100
}

fn default_ice_timeout() -> u64 {
    10
}

fn default_max_webrtc_connections() -> usize {
    50
}

fn default_max_signaling_peers() -> usize {
    1000
}

fn default_signaling_timeout() -> u64 {
    300
}

fn default_heartbeat_interval() -> u64 {
    30
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            enable_tcp: true,
            tcp_port: DEFAULT_TCP_PORT,
            enable_webrtc: false,
            webrtc_port: DEFAULT_WEBRTC_PORT,
            stun_servers: default_stun_servers(),
            turn_servers: Vec::new(),
            connection_timeout_secs: default_connection_timeout(),
            max_connections: default_max_connections(),
        }
    }
}

impl Default for WebRtcConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: DEFAULT_WEBRTC_PORT,
            stun_servers: default_stun_servers(),
            turn_servers: Vec::new(),
            ice_gathering_timeout_secs: default_ice_timeout(),
            max_connections: default_max_webrtc_connections(),
        }
    }
}

impl Default for SignalingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            server_url: None,
            max_peers: default_max_signaling_peers(),
            session_timeout_secs: default_signaling_timeout(),
            heartbeat_interval_secs: default_heartbeat_interval(),
        }
    }
}

impl TransportConfig {
    /// Create a TCP-only configuration
    pub fn tcp_only(port: u16) -> Self {
        Self {
            enable_tcp: true,
            tcp_port: port,
            enable_webrtc: false,
            ..Default::default()
        }
    }

    /// Create a WebRTC-only configuration
    pub fn webrtc_only(port: u16) -> Self {
        Self {
            enable_tcp: false,
            enable_webrtc: true,
            webrtc_port: port,
            ..Default::default()
        }
    }

    /// Create a dual-transport configuration (TCP + WebRTC)
    pub fn dual(tcp_port: u16, webrtc_port: u16) -> Self {
        Self {
            enable_tcp: true,
            tcp_port,
            enable_webrtc: true,
            webrtc_port,
            ..Default::default()
        }
    }

    /// Add a STUN server
    pub fn with_stun_server(mut self, server: impl Into<String>) -> Self {
        self.stun_servers.push(server.into());
        self
    }

    /// Add a TURN server
    pub fn with_turn_server(mut self, server: TurnServer) -> Self {
        self.turn_servers.push(server);
        self
    }

    /// Get connection timeout as Duration
    pub fn connection_timeout(&self) -> Duration {
        Duration::from_secs(self.connection_timeout_secs)
    }

    /// Get TCP multiaddr string
    pub fn tcp_multiaddr(&self) -> String {
        format!("/ip4/0.0.0.0/tcp/{}", self.tcp_port)
    }

    /// Get WebRTC multiaddr string
    pub fn webrtc_multiaddr(&self) -> String {
        format!("/ip4/0.0.0.0/udp/{}/webrtc-direct", self.webrtc_port)
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if !self.enable_tcp && !self.enable_webrtc {
            errors.push("At least one transport must be enabled".to_string());
        }

        if self.enable_tcp && self.tcp_port == 0 {
            errors.push("TCP port cannot be 0".to_string());
        }

        if self.enable_webrtc && self.webrtc_port == 0 {
            errors.push("WebRTC port cannot be 0".to_string());
        }

        if self.enable_tcp && self.enable_webrtc && self.tcp_port == self.webrtc_port {
            errors.push("TCP and WebRTC ports must be different".to_string());
        }

        if self.enable_webrtc && self.stun_servers.is_empty() && self.turn_servers.is_empty() {
            errors.push("WebRTC requires at least one STUN or TURN server".to_string());
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

impl TurnServer {
    /// Create a new TURN server configuration
    pub fn new(
        url: impl Into<String>,
        username: impl Into<String>,
        credential: impl Into<String>,
    ) -> Self {
        Self {
            url: url.into(),
            username: username.into(),
            credential: credential.into(),
            transport: TurnTransport::Udp,
        }
    }

    /// Set the transport protocol
    pub fn with_transport(mut self, transport: TurnTransport) -> Self {
        self.transport = transport;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = TransportConfig::default();
        assert!(config.enable_tcp);
        assert!(!config.enable_webrtc);
        assert_eq!(config.tcp_port, DEFAULT_TCP_PORT);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_dual_config() {
        let config = TransportConfig::dual(4001, 4002);
        assert!(config.enable_tcp);
        assert!(config.enable_webrtc);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validation_no_transport() {
        let config = TransportConfig {
            enable_tcp: false,
            enable_webrtc: false,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validation_same_ports() {
        let config = TransportConfig {
            enable_tcp: true,
            enable_webrtc: true,
            tcp_port: 4001,
            webrtc_port: 4001,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_webrtc_requires_stun() {
        let config = TransportConfig {
            enable_tcp: false,
            enable_webrtc: true,
            stun_servers: Vec::new(),
            turn_servers: Vec::new(),
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_multiaddr_generation() {
        let config = TransportConfig::dual(4001, 4002);
        assert_eq!(config.tcp_multiaddr(), "/ip4/0.0.0.0/tcp/4001");
        assert_eq!(
            config.webrtc_multiaddr(),
            "/ip4/0.0.0.0/udp/4002/webrtc-direct"
        );
    }
}
