//! ShadowMesh Protocol Library
//!
//! Provides core functionality for the ShadowMesh decentralized CDN network.
//!
//! # Modules
//!
//! - `crypto` - Encryption, decryption, and key management
//! - `fragments` - Content chunking and reassembly
//! - `routing` - Multi-hop onion routing
//! - `node` - P2P network node implementation
//! - `storage` - IPFS/Filecoin integration
//! - `dht` - Distributed hash table for content discovery
//! - `replication` - Content replication and redundancy
//! - `peer_discovery` - Peer discovery and scoring
//! - `bandwidth` - Bandwidth tracking and rate limiting
//! - `zk_relay` - Zero-knowledge relay for plausible deniability
//! - `adaptive_routing` - Censorship detection and route-around
//! - `transport` - Multi-transport configuration (TCP, WebRTC)
//! - `naming` - Decentralized naming layer (DHT-native name resolution)
//! - `bootstrap` - DNS-free bootstrap configuration

pub mod adaptive_routing;
pub mod bandwidth;
pub mod bootstrap;
pub mod crypto;
pub mod dht;
pub mod ens_bridge;
pub mod fragments;
pub mod lock_utils;
pub mod naming;
pub mod node;
pub mod peer_discovery;
pub mod replication;
pub mod routing;
pub mod signaling;
pub mod storage;
pub mod transport;
pub mod zk_relay;

// Re-export crypto types
pub use crypto::{
    hash_content, verify_content_hash, CryptoError, CryptoManager, EncryptedData, KeyDerivation,
    OnionRouter, KEY_SIZE, NONCE_SIZE,
};

// Re-export fragment types
pub use fragments::{ContentFragment, ContentManifest, ContentMetadata, FragmentManager};

// Re-export routing types
pub use routing::RoutingLayer;

// Re-export node types
pub use node::ShadowNode;

// Re-export storage types
pub use storage::{DirectoryUploadResult, StorageConfig, StorageLayer, UploadedFile};

// Re-export DHT types
pub use dht::{
    BandwidthStats as DHTBandwidthStats, ContentDHTMetadata, ContentRecord, DHTManager, DHTResult,
    ProviderInfo,
};

// Re-export replication types
pub use replication::{
    ReplicationHealth, ReplicationManager, ReplicationPriority, ReplicationStatus,
};

// Re-export peer discovery types
pub use peer_discovery::{DiscoveryConfig, DiscoveryStats, PeerDiscovery, PeerInfo, PeerState};

// Re-export bandwidth types
pub use bandwidth::{BandwidthStats, BandwidthSummary, BandwidthTracker, Direction, RateLimiter};

// Re-export ZK relay types
pub use zk_relay::{
    BlindRequest, BlindResponse, CellType, Circuit, CircuitHop, CircuitId, HopId, RelayAction,
    RelayCell, RelayError, RelayPayload, RelayStats, TrafficPadder, ZkRelayClient, ZkRelayConfig,
    ZkRelayNode,
};

// Re-export adaptive routing types
pub use adaptive_routing::{
    AdaptiveRouter, AdaptiveRoutingConfig, CensorshipStatus, ComputedRoute, FailureType, GeoRegion,
    PathHealth, RelayInfo, RouteStrategy, RoutingError, RoutingStats as AdaptiveRoutingStats,
};

// Re-export transport types
pub use transport::{
    SignalingConfig, TransportConfig, TurnServer, TurnTransport, WebRtcConfig,
    DEFAULT_STUN_SERVERS, DEFAULT_TCP_PORT, DEFAULT_WEBRTC_PORT,
};

// Re-export signaling types
pub use signaling::{
    generate_session_id, AnnounceMessage, AnswerMessage, DiscoverMessage, ErrorMessage,
    HeartbeatMessage, IceCandidateMessage, OfferMessage, PeerDisconnectedMessage,
    PeerInfo as SignalingPeerInfo, PeersMessage, PendingSession, SessionState, SignalingErrorCode,
    SignalingMessage, TrackedPeer,
};

// Re-export naming types
pub use naming::{
    validate_name, verify_record, NameRecord, NameRecordType, NameResolver, NamingError,
    NamingManager, ResolveResult, ServiceRegistry, ServiceRegistryEntry, ServiceType,
    WellKnownNames, NAMING_GOSSIP_TOPIC,
};

// Re-export bootstrap types
pub use bootstrap::{
    all_stun_servers, is_valid_bootstrap_multiaddr, BOOTSTRAP_GOSSIP_TOPIC,
    FALLBACK_STUN_SERVERS, OFFICIAL_BOOTSTRAP_NODES, SHADOWMESH_STUN_SERVERS,
};
