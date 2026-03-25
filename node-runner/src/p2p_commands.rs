//! Commands sent from API handlers to the P2P event loop.

use libp2p::PeerId;
use shadowmesh_protocol::adaptive_routing::FailureType;
use shadowmesh_protocol::zk_relay::{CircuitId, RelayCell};
use tokio::sync::oneshot;

/// Commands that API handlers can send to the P2P event loop.
pub enum P2pCommand {
    /// Request a fragment from a specific peer.
    FetchFragment {
        peer_id: PeerId,
        fragment_hash: String,
        reply: oneshot::Sender<Result<Vec<u8>, FetchError>>,
    },

    /// Request a manifest from a specific peer.
    FetchManifest {
        peer_id: PeerId,
        content_hash: String,
        reply: oneshot::Sender<Result<ManifestResult, FetchError>>,
    },

    /// Announce content availability to the DHT.
    AnnounceContent {
        content_hash: String,
        fragment_hashes: Vec<String>,
        total_size: u64,
        mime_type: String,
    },

    /// Look up providers for a CID in the DHT.
    FindProviders {
        content_hash: String,
        reply: oneshot::Sender<Result<Vec<PeerId>, FetchError>>,
    },

    /// Request content catalog from a specific peer.
    ListPeerContent {
        peer_id: PeerId,
        reply: oneshot::Sender<Result<Vec<String>, FetchError>>,
    },

    /// Broadcast a content announcement via GossipSub.
    BroadcastContentAnnouncement {
        cid: String,
        total_size: u64,
        fragment_count: u32,
        mime_type: String,
    },

    /// Report a censorship event from an external source (e.g. API handler).
    ///
    /// Feeds the failure into the adaptive router's censorship detection so that
    /// the path to the given peer can be marked as suspected/confirmed blocked.
    ReportCensorship {
        /// The peer whose path is experiencing censorship-like failures.
        peer_id: PeerId,
        /// The type of failure observed.
        failure_type: FailureType,
    },

    // ── ZK Relay commands ────────────────────────────────────────

    /// Build a ZK relay circuit through the specified relay peers.
    ///
    /// Initiates circuit construction using ECDH key exchange with each hop.
    /// The circuit is ready for use once all handshakes complete. Returns the
    /// circuit ID on success.
    BuildCircuit {
        /// Relay peers to route through (minimum 2, typically 3).
        peers: Vec<PeerId>,
        /// Channel to receive the circuit ID (or error) once building is complete.
        reply: oneshot::Sender<Result<CircuitId, FetchError>>,
    },

    /// Send a raw relay cell to a specific peer.
    ///
    /// Used for forwarding relay cells (CREATE, EXTEND, RELAY, DESTROY)
    /// through the circuit. The cell is serialized and sent via the existing
    /// content request-response protocol as an opaque wrapper.
    SendRelayCell {
        /// The relay cell to send.
        cell: RelayCell,
        /// The peer to send it to.
        target: PeerId,
    },

    /// Fetch content (fragment) via an established ZK relay circuit.
    ///
    /// Wraps the content request in onion-encrypted relay cells, sends it
    /// through the circuit, and returns the decrypted response. Falls back
    /// to direct fetching if the circuit is unavailable.
    FetchViaCircuit {
        /// The circuit to use for the fetch.
        circuit_id: CircuitId,
        /// BLAKE3 hash of the fragment to fetch.
        content_hash: String,
        /// Channel to receive the content bytes (or error).
        reply: oneshot::Sender<Result<Vec<u8>, FetchError>>,
    },
}

/// Result of a manifest fetch.
pub struct ManifestResult {
    pub content_hash: String,
    pub fragment_hashes: Vec<String>,
    pub total_size: u64,
    pub mime_type: String,
}

/// Errors during P2P content fetching.
#[derive(Debug)]
pub enum FetchError {
    /// Peer did not have the content.
    NotFound,
    /// The request timed out or the connection failed.
    ConnectionFailed(String),
    /// Peer returned an error.
    PeerError(String),
    /// Internal channel error (event loop shut down).
    ChannelClosed,
    /// ZK relay circuit error (circuit building/wrapping failed).
    RelayError(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::NotFound => write!(f, "Content not found on peer"),
            FetchError::ConnectionFailed(e) => write!(f, "Connection failed: {}", e),
            FetchError::PeerError(e) => write!(f, "Peer error: {}", e),
            FetchError::ChannelClosed => write!(f, "P2P event loop is shut down"),
            FetchError::RelayError(e) => write!(f, "ZK relay error: {}", e),
        }
    }
}

impl std::error::Error for FetchError {}
