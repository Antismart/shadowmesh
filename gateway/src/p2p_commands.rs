//! Commands sent from HTTP handlers to the gateway P2P event loop.

use libp2p::PeerId;
use protocol::adaptive_routing::FailureType;
use protocol::zk_relay::{CircuitId, RelayCell};
use tokio::sync::oneshot;

/// Commands that HTTP handlers can send to the P2P event loop.
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

    /// Publish a signed NameRecord to the DHT and broadcast via GossipSub.
    PublishName {
        /// Serialized NameRecord (JSON bytes)
        record_bytes: Vec<u8>,
        /// The .shadow name (used to derive the DHT key)
        name: String,
    },

    /// Resolve a .shadow name via DHT lookup.
    ResolveName {
        /// The .shadow name to resolve
        name: String,
        /// Returns raw DHT record bytes, or None if not found
        reply: oneshot::Sender<Result<Option<Vec<u8>>, FetchError>>,
    },

    /// Report a censorship event from an external source (e.g. HTTP handler).
    ///
    /// Feeds the failure into the adaptive router's censorship detection so that
    /// the path to the given peer can be marked as suspected/confirmed blocked.
    ReportCensorship {
        /// The peer whose path is experiencing censorship-like failures.
        peer_id: PeerId,
        /// The type of failure observed.
        failure_type: FailureType,
    },

    /// Build a ZK relay circuit through the specified peers.
    BuildCircuit {
        /// Ordered list of peers to form the circuit hops.
        peers: Vec<PeerId>,
        /// Reply channel returning the circuit ID on success.
        reply: oneshot::Sender<Result<protocol::zk_relay::CircuitId, FetchError>>,
    },

    /// Send a relay cell to a target peer (for circuit forwarding).
    SendRelayCell {
        /// The relay cell to send.
        cell: RelayCell,
        /// The peer to send the cell to.
        target: PeerId,
    },

    /// Fetch content (fragment) via an established ZK relay circuit.
    ///
    /// Wraps the content request in onion-encrypted relay cells, sends it
    /// through the circuit, and returns the decrypted response.
    FetchViaCircuit {
        /// The circuit to use for the fetch.
        circuit_id: CircuitId,
        /// BLAKE3 hash of the content to fetch.
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
    /// P2P resolution timed out.
    Timeout,
    /// ZK relay circuit error (circuit building/wrapping failed).
    RelayError(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::NotFound => write!(f, "Content not found on peer network"),
            FetchError::ConnectionFailed(e) => write!(f, "Connection failed: {}", e),
            FetchError::PeerError(e) => write!(f, "Peer error: {}", e),
            FetchError::ChannelClosed => write!(f, "P2P event loop is shut down"),
            FetchError::Timeout => write!(f, "P2P resolution timed out"),
            FetchError::RelayError(e) => write!(f, "ZK relay error: {}", e),
        }
    }
}

impl std::error::Error for FetchError {}
