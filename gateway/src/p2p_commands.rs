//! Commands sent from HTTP handlers to the gateway P2P event loop.

use libp2p::PeerId;
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
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::NotFound => write!(f, "Content not found on peer network"),
            FetchError::ConnectionFailed(e) => write!(f, "Connection failed: {}", e),
            FetchError::PeerError(e) => write!(f, "Peer error: {}", e),
            FetchError::ChannelClosed => write!(f, "P2P event loop is shut down"),
            FetchError::Timeout => write!(f, "P2P resolution timed out"),
        }
    }
}

impl std::error::Error for FetchError {}
