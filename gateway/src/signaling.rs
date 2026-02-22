//! WebSocket Signaling Server for WebRTC
//!
//! Provides peer discovery and SDP exchange for WebRTC connections.

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::{Json, Response},
};
use futures::{SinkExt, StreamExt};
use protocol::{
    generate_session_id, AnnounceMessage, SignalingErrorCode, SignalingMessage, TrackedPeer,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};

/// Signaling server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalingConfig {
    /// Maximum number of connected peers
    #[serde(default = "default_max_peers")]
    pub max_peers: usize,

    /// Session timeout in seconds
    #[serde(default = "default_session_timeout")]
    pub session_timeout_secs: u64,

    /// Heartbeat interval in seconds
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval_secs: u64,

    /// Maximum message size in bytes
    #[serde(default = "default_max_message_size")]
    pub max_message_size: usize,

    /// Enable peer discovery
    #[serde(default = "default_true")]
    pub enable_discovery: bool,
}

fn default_max_peers() -> usize {
    1000
}

fn default_session_timeout() -> u64 {
    300
}

fn default_heartbeat_interval() -> u64 {
    30
}

fn default_max_message_size() -> usize {
    65536
}

fn default_true() -> bool {
    true
}

impl Default for SignalingConfig {
    fn default() -> Self {
        Self {
            max_peers: default_max_peers(),
            session_timeout_secs: default_session_timeout(),
            heartbeat_interval_secs: default_heartbeat_interval(),
            max_message_size: default_max_message_size(),
            enable_discovery: true,
        }
    }
}

/// Connection entry for a WebSocket client
struct Connection {
    /// Sender to this connection
    tx: mpsc::Sender<String>,
    /// Peer ID (if announced)
    peer_id: Option<String>,
    /// Last activity time
    last_activity: Instant,
}

/// Signaling server state
pub struct SignalingServer {
    /// Configuration
    config: SignalingConfig,
    /// Active connections by connection ID
    connections: RwLock<HashMap<String, Connection>>,
    /// Tracked peers by peer ID
    peers: RwLock<HashMap<String, TrackedPeer>>,
    /// Peer ID to connection ID mapping
    peer_connections: RwLock<HashMap<String, String>>,
}

impl SignalingServer {
    /// Create a new signaling server
    pub fn new(config: SignalingConfig) -> Self {
        Self {
            config,
            connections: RwLock::new(HashMap::new()),
            peers: RwLock::new(HashMap::new()),
            peer_connections: RwLock::new(HashMap::new()),
        }
    }

    /// Handle a new WebSocket connection
    pub async fn handle_connection(&self, socket: WebSocket, connection_id: String) {
        let (mut ws_sender, mut ws_receiver) = socket.split();

        // Create channel for sending messages to this connection
        let (tx, mut rx) = mpsc::channel::<String>(100);

        // Register connection
        {
            let mut connections = self.connections.write().await;
            connections.insert(
                connection_id.clone(),
                Connection {
                    tx,
                    peer_id: None,
                    last_activity: Instant::now(),
                },
            );
        }

        info!("WebSocket connection opened: {}", connection_id);

        // Spawn task to forward messages from channel to WebSocket
        let _conn_id_clone = connection_id.clone();
        let send_task = tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if ws_sender.send(Message::Text(msg)).await.is_err() {
                    break;
                }
            }
        });

        // Process incoming messages
        while let Some(result) = ws_receiver.next().await {
            match result {
                Ok(Message::Text(text)) => {
                    // Update last activity
                    if let Some(conn) = self.connections.write().await.get_mut(&connection_id) {
                        conn.last_activity = Instant::now();
                    }

                    // Process the message
                    if let Err(e) = self.handle_message(&connection_id, &text).await {
                        error!("Error handling message from {}: {}", connection_id, e);
                    }
                }
                Ok(Message::Close(_)) => {
                    info!("WebSocket connection closed: {}", connection_id);
                    break;
                }
                Ok(Message::Ping(_data)) => {
                    // Respond to ping with pong
                    if let Some(conn) = self.connections.read().await.get(&connection_id) {
                        let _ = conn.tx.send(String::new()).await; // Trigger a pong
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    error!("WebSocket error for {}: {}", connection_id, e);
                    break;
                }
            }
        }

        // Cleanup on disconnect
        self.handle_disconnect(&connection_id).await;

        // Cancel the send task
        send_task.abort();
    }

    /// Handle an incoming signaling message
    async fn handle_message(&self, connection_id: &str, text: &str) -> Result<(), String> {
        // Check message size
        if text.len() > self.config.max_message_size {
            self.send_error(
                connection_id,
                SignalingErrorCode::MessageTooLarge,
                "Message too large",
            )
            .await;
            return Err("Message too large".to_string());
        }

        // Parse the message
        let message: SignalingMessage = match serde_json::from_str(text) {
            Ok(msg) => msg,
            Err(e) => {
                self.send_error(
                    connection_id,
                    SignalingErrorCode::InvalidMessage,
                    &format!("Invalid JSON: {}", e),
                )
                .await;
                return Err(format!("Invalid JSON: {}", e));
            }
        };

        match &message {
            SignalingMessage::Announce(announce) => {
                self.handle_announce(connection_id, announce.clone()).await
            }
            SignalingMessage::Discover(discover) => {
                self.handle_discover(connection_id, discover.clone()).await
            }
            SignalingMessage::Offer(offer) => self.relay_to_peer(&offer.to, &message).await,
            SignalingMessage::Answer(answer) => self.relay_to_peer(&answer.to, &message).await,
            SignalingMessage::IceCandidate(candidate) => {
                self.relay_to_peer(&candidate.to, &message).await
            }
            SignalingMessage::Heartbeat(heartbeat) => {
                self.handle_heartbeat(connection_id, heartbeat.clone())
                    .await
            }
            _ => {
                debug!("Ignoring message type from {}", connection_id);
                Ok(())
            }
        }
    }

    /// Handle peer announcement
    async fn handle_announce(
        &self,
        connection_id: &str,
        announce: AnnounceMessage,
    ) -> Result<(), String> {
        let peer_id = announce.peer_id.clone();
        let tracked = TrackedPeer::from_announce(&announce, Some(connection_id.to_string()));

        // Atomically check peer limit and update all three maps together
        // to prevent desync between peers/peer_connections/connections.
        let total_peers = {
            let mut peers = self.peers.write().await;
            if peers.len() >= self.config.max_peers {
                // Release lock before sending error (which is async I/O)
                drop(peers);
                self.send_error(
                    connection_id,
                    SignalingErrorCode::RateLimitExceeded,
                    "Maximum peer limit reached",
                )
                .await;
                return Err("Max peers reached".to_string());
            }
            peers.insert(peer_id.clone(), tracked);
            let total = peers.len();

            // Update peer-connection mapping while peers lock is held
            let mut peer_connections = self.peer_connections.write().await;
            peer_connections.insert(peer_id.clone(), connection_id.to_string());

            // Update connection's peer ID
            let mut connections = self.connections.write().await;
            if let Some(conn) = connections.get_mut(connection_id) {
                conn.peer_id = Some(peer_id.clone());
            }

            total
        };

        info!(
            "Peer announced: {} via connection {}",
            peer_id, connection_id
        );

        // Send acknowledgment
        let response = SignalingMessage::Peers(protocol::PeersMessage {
            peers: vec![], // Empty for announce ack
            total_peers,
        });

        self.send_to_connection(connection_id, &response).await;

        Ok(())
    }

    /// Handle peer discovery request
    async fn handle_discover(
        &self,
        connection_id: &str,
        discover: protocol::DiscoverMessage,
    ) -> Result<(), String> {
        if !self.config.enable_discovery {
            self.send_error(
                connection_id,
                SignalingErrorCode::Unauthorized,
                "Peer discovery is disabled",
            )
            .await;
            return Err("Discovery disabled".to_string());
        }

        let peers = self.peers.read().await;
        let timeout = Duration::from_secs(self.config.session_timeout_secs);

        // Filter peers
        let available_peers: Vec<_> = peers
            .values()
            .filter(|p| {
                // Exclude the requesting peer
                if p.peer_id == discover.peer_id {
                    return false;
                }
                // Exclude stale peers
                if p.is_stale(timeout) {
                    return false;
                }
                // Apply transport filter
                if let Some(ref filter) = discover.transport_filter {
                    if !p.transports.contains(filter) {
                        return false;
                    }
                }
                // Apply region filter
                if let Some(ref region) = discover.region_filter {
                    if p.metadata.get("region") != Some(region) {
                        return false;
                    }
                }
                true
            })
            .map(|p| p.to_peer_info())
            .take(discover.max_peers)
            .collect();

        let total = peers.len();
        drop(peers);

        let response = SignalingMessage::Peers(protocol::PeersMessage {
            peers: available_peers,
            total_peers: total,
        });

        self.send_to_connection(connection_id, &response).await;

        Ok(())
    }

    /// Handle heartbeat
    async fn handle_heartbeat(
        &self,
        _connection_id: &str,
        heartbeat: protocol::HeartbeatMessage,
    ) -> Result<(), String> {
        // Update peer's last heartbeat
        if let Some(tracked) = self.peers.write().await.get_mut(&heartbeat.peer_id) {
            tracked.heartbeat();
        }

        Ok(())
    }

    /// Relay a message to a specific peer
    async fn relay_to_peer(
        &self,
        target_peer_id: &str,
        message: &SignalingMessage,
    ) -> Result<(), String> {
        // Find the connection for the target peer
        let connection_id = {
            let peer_connections = self.peer_connections.read().await;
            peer_connections.get(target_peer_id).cloned()
        };

        if let Some(conn_id) = connection_id {
            self.send_to_connection(&conn_id, message).await;
            Ok(())
        } else {
            // Peer not found - send error back to sender (we'd need sender info)
            warn!("Target peer not found: {}", target_peer_id);
            Err(format!("Peer not found: {}", target_peer_id))
        }
    }

    /// Send a message to a specific connection
    async fn send_to_connection(&self, connection_id: &str, message: &SignalingMessage) {
        if let Ok(json) = serde_json::to_string(message) {
            if let Some(conn) = self.connections.read().await.get(connection_id) {
                let _ = conn.tx.send(json).await;
            }
        }
    }

    /// Send an error to a connection
    async fn send_error(&self, connection_id: &str, code: SignalingErrorCode, message: &str) {
        let error = SignalingMessage::Error(protocol::ErrorMessage {
            code,
            message: message.to_string(),
            request_id: None,
        });
        self.send_to_connection(connection_id, &error).await;
    }

    /// Handle connection disconnect
    async fn handle_disconnect(&self, connection_id: &str) {
        // Atomically remove from all three maps to keep them in sync.
        // Lock order: peers -> peer_connections -> connections (same as handle_announce).
        let peer_id = {
            let mut connections = self.connections.write().await;
            let peer_id = connections
                .get(connection_id)
                .and_then(|c| c.peer_id.clone());
            connections.remove(connection_id);
            peer_id
        };

        if let Some(peer_id) = peer_id {
            {
                let mut peers = self.peers.write().await;
                peers.remove(&peer_id);
                let mut peer_connections = self.peer_connections.write().await;
                peer_connections.remove(&peer_id);
            }

            // Notify other peers about disconnect
            let disconnect_msg =
                SignalingMessage::PeerDisconnected(protocol::PeerDisconnectedMessage {
                    peer_id: peer_id.clone(),
                    reason: Some("Connection closed".to_string()),
                });

            let connections = self.connections.read().await;
            for (_, conn) in connections.iter() {
                if let Ok(json) = serde_json::to_string(&disconnect_msg) {
                    let _ = conn.tx.send(json).await;
                }
            }

            info!("Peer disconnected: {}", peer_id);
        }
    }

    /// Cleanup stale connections and peers
    pub async fn cleanup_stale(&self) {
        let timeout = Duration::from_secs(self.config.session_timeout_secs);
        let now = Instant::now();

        // Find stale connections
        let stale_connections: Vec<String> = {
            let connections = self.connections.read().await;
            connections
                .iter()
                .filter(|(_, conn)| now.duration_since(conn.last_activity) > timeout)
                .map(|(id, _)| id.clone())
                .collect()
        };

        // Handle disconnects
        for conn_id in stale_connections {
            warn!("Cleaning up stale connection: {}", conn_id);
            self.handle_disconnect(&conn_id).await;
        }
    }

    /// Get current stats (reads both maps under lock for consistency)
    pub async fn stats(&self) -> SignalingStats {
        let peers = self.peers.read().await;
        let connections = self.connections.read().await;
        SignalingStats {
            connected_peers: peers.len(),
            active_connections: connections.len(),
        }
    }
}

/// Signaling server statistics
#[derive(Debug, Clone, Serialize)]
pub struct SignalingStats {
    pub connected_peers: usize,
    pub active_connections: usize,
}

/// Signaling server state wrapper for Axum
pub type SignalingState = Arc<SignalingServer>;

/// WebSocket upgrade handler
pub async fn ws_handler(ws: WebSocketUpgrade, State(signaling): State<SignalingState>) -> Response {
    let connection_id = generate_session_id();
    let max_size = signaling.config.max_message_size;
    ws.max_frame_size(max_size)
        .max_message_size(max_size)
        .on_upgrade(move |socket| async move {
            signaling.handle_connection(socket, connection_id).await;
        })
}

/// Stats endpoint handler
pub async fn stats_handler(State(signaling): State<SignalingState>) -> Json<SignalingStats> {
    Json(signaling.stats().await)
}

/// Create signaling router
pub fn signaling_router(signaling: SignalingState) -> axum::Router {
    axum::Router::new()
        .route("/ws", axum::routing::get(ws_handler))
        .route("/stats", axum::routing::get(stats_handler))
        .with_state(signaling)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_signaling_server_creation() {
        let config = SignalingConfig::default();
        let server = SignalingServer::new(config);
        let stats = server.stats().await;
        assert_eq!(stats.connected_peers, 0);
        assert_eq!(stats.active_connections, 0);
    }

    #[test]
    fn test_signaling_config_defaults() {
        let config = SignalingConfig::default();
        assert_eq!(config.max_peers, 1000);
        assert_eq!(config.session_timeout_secs, 300);
        assert!(config.enable_discovery);
    }
}
