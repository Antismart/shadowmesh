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

pub mod crypto;
pub mod fragments;
pub mod node;
pub mod routing;
pub mod storage;
pub mod dht;
pub mod replication;
pub mod peer_discovery;
pub mod bandwidth;
pub mod zk_relay;
pub mod adaptive_routing;

// Re-export crypto types
pub use crypto::{
    CryptoManager, CryptoError, EncryptedData,
    OnionRouter, KeyDerivation,
    hash_content, verify_content_hash,
    KEY_SIZE, NONCE_SIZE,
};

// Re-export fragment types
pub use fragments::{ContentFragment, ContentManifest, ContentMetadata, FragmentManager};

// Re-export routing types
pub use routing::RoutingLayer;

// Re-export node types
pub use node::ShadowNode;

// Re-export storage types
pub use storage::{StorageLayer, StorageConfig, DirectoryUploadResult, UploadedFile};

// Re-export DHT types
pub use dht::{DHTManager, DHTResult, ContentRecord, ProviderInfo, ContentDHTMetadata, BandwidthStats as DHTBandwidthStats};

// Re-export replication types
pub use replication::{ReplicationManager, ReplicationStatus, ReplicationHealth, ReplicationPriority};

// Re-export peer discovery types
pub use peer_discovery::{PeerDiscovery, PeerInfo, PeerState, DiscoveryConfig, DiscoveryStats};

// Re-export bandwidth types
pub use bandwidth::{BandwidthTracker, BandwidthStats, BandwidthSummary, RateLimiter, Direction};

// Re-export ZK relay types
pub use zk_relay::{
    ZkRelayClient, ZkRelayNode, ZkRelayConfig,
    RelayCell, RelayPayload, RelayAction, RelayError,
    Circuit, CircuitHop, CircuitId, HopId,
    CellType, BlindRequest, BlindResponse,
    RelayStats, TrafficPadder,
};

// Re-export adaptive routing types
pub use adaptive_routing::{
    AdaptiveRouter, AdaptiveRoutingConfig, ComputedRoute,
    RoutingStats as AdaptiveRoutingStats, RoutingError,
    RouteStrategy, CensorshipStatus, FailureType,
    GeoRegion, RelayInfo, PathHealth,
};
