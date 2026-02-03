//! WebRTC Signaling Protocol for ShadowMesh
//!
//! Provides message types and handlers for WebRTC peer discovery and SDP exchange.
//! The signaling protocol enables browsers to discover and connect to P2P nodes.

use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Signaling message types exchanged between clients and signaling server
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SignalingMessage {
    /// Announce presence to the signaling server
    Announce(AnnounceMessage),

    /// Request peer discovery
    Discover(DiscoverMessage),

    /// Response with available peers
    Peers(PeersMessage),

    /// WebRTC offer (SDP)
    Offer(OfferMessage),

    /// WebRTC answer (SDP)
    Answer(AnswerMessage),

    /// ICE candidate for connection establishment
    IceCandidate(IceCandidateMessage),

    /// Heartbeat to maintain connection
    Heartbeat(HeartbeatMessage),

    /// Error response
    Error(ErrorMessage),

    /// Peer disconnected notification
    PeerDisconnected(PeerDisconnectedMessage),
}

/// Announce presence to the signaling server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnounceMessage {
    /// Local peer ID
    pub peer_id: String,

    /// List of multiaddresses the peer is listening on
    pub multiaddrs: Vec<String>,

    /// Supported transports (e.g., ["tcp", "webrtc"])
    #[serde(default)]
    pub transports: Vec<String>,

    /// Optional metadata (region, version, etc.)
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

/// Request peer discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverMessage {
    /// Requesting peer ID
    pub peer_id: String,

    /// Maximum number of peers to return
    #[serde(default = "default_max_peers")]
    pub max_peers: usize,

    /// Filter by transport type
    #[serde(default)]
    pub transport_filter: Option<String>,

    /// Filter by region
    #[serde(default)]
    pub region_filter: Option<String>,
}

/// Response with available peers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeersMessage {
    /// List of available peers
    pub peers: Vec<PeerInfo>,

    /// Total number of known peers
    pub total_peers: usize,
}

/// Information about a peer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    /// Peer ID
    pub peer_id: String,

    /// Multiaddresses
    pub multiaddrs: Vec<String>,

    /// Supported transports
    pub transports: Vec<String>,

    /// Optional metadata
    #[serde(default)]
    pub metadata: HashMap<String, String>,

    /// Last seen timestamp (Unix seconds)
    #[serde(default)]
    pub last_seen: u64,
}

/// WebRTC offer (SDP)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfferMessage {
    /// Sender peer ID
    pub from: String,

    /// Target peer ID
    pub to: String,

    /// SDP offer content
    pub sdp: String,

    /// Unique session ID for this connection attempt
    pub session_id: String,
}

/// WebRTC answer (SDP)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnswerMessage {
    /// Sender peer ID
    pub from: String,

    /// Target peer ID
    pub to: String,

    /// SDP answer content
    pub sdp: String,

    /// Session ID (matches the offer)
    pub session_id: String,
}

/// ICE candidate for connection establishment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IceCandidateMessage {
    /// Sender peer ID
    pub from: String,

    /// Target peer ID
    pub to: String,

    /// ICE candidate string
    pub candidate: String,

    /// SDP mid
    pub sdp_mid: Option<String>,

    /// SDP m-line index
    pub sdp_mline_index: Option<u16>,

    /// Session ID
    pub session_id: String,
}

/// Heartbeat to maintain connection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatMessage {
    /// Peer ID
    pub peer_id: String,

    /// Timestamp (Unix milliseconds)
    pub timestamp: u64,
}

/// Error response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorMessage {
    /// Error code
    pub code: SignalingErrorCode,

    /// Human-readable error message
    pub message: String,

    /// Optional request ID for correlation
    #[serde(default)]
    pub request_id: Option<String>,
}

/// Peer disconnected notification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerDisconnectedMessage {
    /// Disconnected peer ID
    pub peer_id: String,

    /// Reason for disconnection
    #[serde(default)]
    pub reason: Option<String>,
}

/// Signaling error codes
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SignalingErrorCode {
    /// Invalid message format
    InvalidMessage,
    /// Peer not found
    PeerNotFound,
    /// Rate limit exceeded
    RateLimitExceeded,
    /// Session expired
    SessionExpired,
    /// Internal server error
    InternalError,
    /// Unauthorized
    Unauthorized,
    /// Message too large
    MessageTooLarge,
}

fn default_max_peers() -> usize {
    10
}

/// Tracked peer state for signaling server
#[derive(Debug, Clone)]
pub struct TrackedPeer {
    /// Peer ID
    pub peer_id: String,

    /// Parsed peer ID (if valid)
    pub parsed_peer_id: Option<PeerId>,

    /// Multiaddresses
    pub multiaddrs: Vec<Multiaddr>,

    /// Supported transports
    pub transports: Vec<String>,

    /// Metadata
    pub metadata: HashMap<String, String>,

    /// Last heartbeat time
    pub last_heartbeat: Instant,

    /// Connection ID (for WebSocket)
    pub connection_id: Option<String>,
}

impl TrackedPeer {
    /// Create a new tracked peer from an announce message
    pub fn from_announce(msg: &AnnounceMessage, connection_id: Option<String>) -> Self {
        let parsed_peer_id = msg.peer_id.parse().ok();
        let multiaddrs = msg
            .multiaddrs
            .iter()
            .filter_map(|a| a.parse().ok())
            .collect();

        Self {
            peer_id: msg.peer_id.clone(),
            parsed_peer_id,
            multiaddrs,
            transports: msg.transports.clone(),
            metadata: msg.metadata.clone(),
            last_heartbeat: Instant::now(),
            connection_id,
        }
    }

    /// Update the last heartbeat time
    pub fn heartbeat(&mut self) {
        self.last_heartbeat = Instant::now();
    }

    /// Check if peer is considered stale
    pub fn is_stale(&self, timeout: Duration) -> bool {
        self.last_heartbeat.elapsed() > timeout
    }

    /// Convert to PeerInfo for responses
    pub fn to_peer_info(&self) -> PeerInfo {
        let last_seen = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        PeerInfo {
            peer_id: self.peer_id.clone(),
            multiaddrs: self.multiaddrs.iter().map(|a| a.to_string()).collect(),
            transports: self.transports.clone(),
            metadata: self.metadata.clone(),
            last_seen,
        }
    }
}

/// Pending WebRTC session
#[derive(Debug, Clone)]
pub struct PendingSession {
    /// Session ID
    pub session_id: String,

    /// Initiating peer
    pub from_peer: String,

    /// Target peer
    pub to_peer: String,

    /// Offer SDP (if received)
    pub offer_sdp: Option<String>,

    /// Answer SDP (if received)
    pub answer_sdp: Option<String>,

    /// ICE candidates from initiator
    pub from_candidates: Vec<IceCandidateMessage>,

    /// ICE candidates from responder
    pub to_candidates: Vec<IceCandidateMessage>,

    /// Session creation time
    pub created_at: Instant,

    /// Session state
    pub state: SessionState,
}

/// WebRTC session state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// Waiting for offer
    AwaitingOffer,
    /// Offer sent, waiting for answer
    AwaitingAnswer,
    /// Answer received, exchanging ICE candidates
    ExchangingCandidates,
    /// Connection established
    Connected,
    /// Session failed or timed out
    Failed,
}

impl PendingSession {
    /// Create a new pending session
    pub fn new(session_id: String, from_peer: String, to_peer: String) -> Self {
        Self {
            session_id,
            from_peer,
            to_peer,
            offer_sdp: None,
            answer_sdp: None,
            from_candidates: Vec::new(),
            to_candidates: Vec::new(),
            created_at: Instant::now(),
            state: SessionState::AwaitingOffer,
        }
    }

    /// Check if session has timed out
    pub fn is_expired(&self, timeout: Duration) -> bool {
        self.created_at.elapsed() > timeout
    }
}

/// Generate a unique session ID
pub fn generate_session_id() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: [u8; 16] = rng.gen();
    hex::encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signaling_message_serialize() {
        let msg = SignalingMessage::Announce(AnnounceMessage {
            peer_id: "12D3KooW...".to_string(),
            multiaddrs: vec!["/ip4/127.0.0.1/tcp/4001".to_string()],
            transports: vec!["tcp".to_string(), "webrtc".to_string()],
            metadata: HashMap::new(),
        });

        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("announce"));

        let parsed: SignalingMessage = serde_json::from_str(&json).unwrap();
        if let SignalingMessage::Announce(announce) = parsed {
            assert_eq!(announce.peer_id, "12D3KooW...");
        } else {
            panic!("Expected Announce message");
        }
    }

    #[test]
    fn test_offer_answer_messages() {
        let offer = SignalingMessage::Offer(OfferMessage {
            from: "peer1".to_string(),
            to: "peer2".to_string(),
            sdp: "v=0\r\n...".to_string(),
            session_id: "abc123".to_string(),
        });

        let json = serde_json::to_string(&offer).unwrap();
        assert!(json.contains("offer"));
        assert!(json.contains("peer1"));
    }

    #[test]
    fn test_tracked_peer() {
        let announce = AnnounceMessage {
            peer_id: "test-peer".to_string(),
            multiaddrs: vec!["/ip4/127.0.0.1/tcp/4001".to_string()],
            transports: vec!["tcp".to_string()],
            metadata: HashMap::new(),
        };

        let tracked = TrackedPeer::from_announce(&announce, Some("conn-1".to_string()));
        assert_eq!(tracked.peer_id, "test-peer");
        assert!(!tracked.is_stale(Duration::from_secs(60)));
    }

    #[test]
    fn test_session_id_generation() {
        let id1 = generate_session_id();
        let id2 = generate_session_id();
        assert_ne!(id1, id2);
        assert_eq!(id1.len(), 32); // 16 bytes = 32 hex chars
    }
}
