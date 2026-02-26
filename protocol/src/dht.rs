//! Distributed Hash Table (DHT) operations for ShadowMesh
//!
//! Provides content announcement, discovery, and peer management using Kademlia DHT.

use libp2p::kad::{QueryId, RecordKey};
use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Content record stored in the DHT
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentRecord {
    /// Content identifier (CID)
    pub cid: String,
    /// Providers who have this content
    pub providers: Vec<ProviderInfo>,
    /// Content metadata
    pub metadata: ContentDHTMetadata,
    /// When this record was created
    pub created_at: u64,
    /// Time-to-live in seconds
    pub ttl_seconds: u64,
}

/// Information about a content provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    /// Peer ID of the provider
    pub peer_id: String,
    /// Multiaddresses where this peer can be reached
    pub addresses: Vec<String>,
    /// Reputation score (0-100)
    pub reputation: u8,
    /// Whether this provider is currently online
    pub online: bool,
    /// Last seen timestamp
    pub last_seen: u64,
}

/// Metadata for content in DHT
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentDHTMetadata {
    /// Size in bytes
    pub size: u64,
    /// MIME type
    pub mime_type: String,
    /// Number of fragments
    pub fragment_count: u32,
    /// Whether content is encrypted
    pub encrypted: bool,
}

/// DHT operation results
#[derive(Debug)]
pub enum DHTResult {
    /// Content was successfully announced
    Announced { cid: String, providers: usize },
    /// Content was found
    Found { record: ContentRecord },
    /// Content was not found
    NotFound { cid: String },
    /// Peers were discovered
    PeersDiscovered { peers: Vec<PeerId> },
    /// Operation failed
    Error { message: String },
}

/// DHT Manager for content operations
pub struct DHTManager {
    /// Local peer ID
    local_peer_id: PeerId,
    /// Local content cache (CID -> ContentRecord)
    local_records: HashMap<String, ContentRecord>,
    /// Known peers
    known_peers: HashMap<PeerId, PeerState>,
    /// Pending queries
    _pending_queries: HashMap<QueryId, QueryType>,
    /// Bootstrap nodes
    bootstrap_nodes: Vec<(PeerId, Multiaddr)>,
}

/// State of a known peer
#[derive(Debug, Clone)]
pub struct PeerState {
    pub addresses: Vec<Multiaddr>,
    pub reputation: u8,
    pub last_seen: Instant,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
}

/// Types of DHT queries
#[derive(Debug, Clone)]
pub enum QueryType {
    Announce(String), // CID being announced
    Lookup(String),   // CID being looked up
    FindPeers,
    Bootstrap,
}

impl DHTManager {
    /// Create a new DHT manager
    pub fn new(local_peer_id: PeerId) -> Self {
        Self {
            local_peer_id,
            local_records: HashMap::new(),
            known_peers: HashMap::new(),
            _pending_queries: HashMap::new(),
            bootstrap_nodes: Vec::new(),
        }
    }

    /// Add bootstrap nodes
    pub fn add_bootstrap_nodes(&mut self, nodes: Vec<(PeerId, Multiaddr)>) {
        self.bootstrap_nodes.extend(nodes);
    }

    /// Create a content record for announcement
    pub fn create_content_record(
        &self,
        cid: String,
        size: u64,
        mime_type: String,
        fragment_count: u32,
        encrypted: bool,
    ) -> ContentRecord {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        ContentRecord {
            cid: cid.clone(),
            providers: vec![ProviderInfo {
                peer_id: self.local_peer_id.to_string(),
                addresses: vec![], // Will be filled when announcing
                reputation: 100,   // Self gets max reputation
                online: true,
                last_seen: now,
            }],
            metadata: ContentDHTMetadata {
                size,
                mime_type,
                fragment_count,
                encrypted,
            },
            created_at: now,
            ttl_seconds: 86400, // 24 hours default
        }
    }

    /// Store a content record locally
    pub fn store_local(&mut self, record: ContentRecord) {
        self.local_records.insert(record.cid.clone(), record);
    }

    /// Get a local content record
    pub fn get_local(&self, cid: &str) -> Option<&ContentRecord> {
        self.local_records.get(cid)
    }

    /// List all local content records
    pub fn list_local_content(&self) -> Vec<&ContentRecord> {
        self.local_records.values().collect()
    }

    /// Register a peer
    pub fn register_peer(&mut self, peer_id: PeerId, addresses: Vec<Multiaddr>) {
        self.known_peers.insert(
            peer_id,
            PeerState {
                addresses,
                reputation: 50, // Start with neutral reputation
                last_seen: Instant::now(),
                bytes_sent: 0,
                bytes_received: 0,
                successful_requests: 0,
                failed_requests: 0,
            },
        );
    }

    /// Update peer stats after successful request
    pub fn record_success(&mut self, peer_id: &PeerId, bytes: u64) {
        if let Some(state) = self.known_peers.get_mut(peer_id) {
            state.last_seen = Instant::now();
            state.bytes_received += bytes;
            state.successful_requests += 1;
            // Increase reputation (max 100)
            state.reputation = (state.reputation + 1).min(100);
        }
    }

    /// Update peer stats after failed request
    pub fn record_failure(&mut self, peer_id: &PeerId) {
        if let Some(state) = self.known_peers.get_mut(peer_id) {
            state.failed_requests += 1;
            // Decrease reputation (min 0)
            state.reputation = state.reputation.saturating_sub(5);
        }
    }

    /// Get peers sorted by reputation
    pub fn get_best_peers(&self, limit: usize) -> Vec<(&PeerId, &PeerState)> {
        let mut peers: Vec<_> = self.known_peers.iter().collect();
        peers.sort_by(|a, b| b.1.reputation.cmp(&a.1.reputation));
        peers.truncate(limit);
        peers
    }

    /// Get total bandwidth stats
    pub fn get_bandwidth_stats(&self) -> BandwidthStats {
        let mut stats = BandwidthStats::default();
        for state in self.known_peers.values() {
            stats.total_bytes_sent += state.bytes_sent;
            stats.total_bytes_received += state.bytes_received;
            stats.total_requests += state.successful_requests + state.failed_requests;
            stats.successful_requests += state.successful_requests;
        }
        stats.peer_count = self.known_peers.len();
        stats
    }

    /// Serialize a content record for DHT storage
    pub fn serialize_record(record: &ContentRecord) -> Result<Vec<u8>, String> {
        serde_json::to_vec(record).map_err(|e| e.to_string())
    }

    /// Deserialize a content record from DHT
    pub fn deserialize_record(data: &[u8]) -> Result<ContentRecord, String> {
        serde_json::from_slice(data).map_err(|e| e.to_string())
    }

    /// Create a DHT key from CID
    pub fn cid_to_key(cid: &str) -> RecordKey {
        RecordKey::new(&format!("/shadowmesh/content/{}", cid).into_bytes())
    }

    /// Clean up stale peers (not seen in the last hour)
    pub fn cleanup_stale_peers(&mut self) {
        let cutoff = Instant::now() - Duration::from_secs(3600);
        self.known_peers.retain(|_, state| state.last_seen > cutoff);
    }

    /// Clean up expired content records
    pub fn cleanup_expired_records(&mut self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        self.local_records
            .retain(|_, record| record.created_at + record.ttl_seconds > now);
    }
}

/// Bandwidth statistics
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct BandwidthStats {
    pub total_bytes_sent: u64,
    pub total_bytes_received: u64,
    pub total_requests: u64,
    pub successful_requests: u64,
    pub peer_count: usize,
}

impl BandwidthStats {
    pub fn success_rate(&self) -> f64 {
        if self.total_requests == 0 {
            return 100.0;
        }
        (self.successful_requests as f64 / self.total_requests as f64) * 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p::identity;

    fn create_test_peer_id() -> PeerId {
        let keypair = identity::Keypair::generate_ed25519();
        PeerId::from(keypair.public())
    }

    #[test]
    fn test_create_content_record() {
        let peer_id = create_test_peer_id();
        let manager = DHTManager::new(peer_id);

        let record = manager.create_content_record(
            "Qmtest123".to_string(),
            1024,
            "text/plain".to_string(),
            1,
            false,
        );

        assert_eq!(record.cid, "Qmtest123");
        assert_eq!(record.metadata.size, 1024);
        assert_eq!(record.providers.len(), 1);
    }

    #[test]
    fn test_peer_reputation() {
        let local_peer = create_test_peer_id();
        let remote_peer = create_test_peer_id();
        let mut manager = DHTManager::new(local_peer);

        manager.register_peer(remote_peer, vec![]);
        assert_eq!(
            manager.known_peers.get(&remote_peer).unwrap().reputation,
            50
        );

        // Record successes
        for _ in 0..10 {
            manager.record_success(&remote_peer, 100);
        }
        assert_eq!(
            manager.known_peers.get(&remote_peer).unwrap().reputation,
            60
        );

        // Record failure
        manager.record_failure(&remote_peer);
        assert_eq!(
            manager.known_peers.get(&remote_peer).unwrap().reputation,
            55
        );
    }

    #[test]
    fn test_bandwidth_stats() {
        let local_peer = create_test_peer_id();
        let mut manager = DHTManager::new(local_peer);

        for i in 0..5 {
            let peer = create_test_peer_id();
            manager.register_peer(peer, vec![]);
            manager.record_success(&peer, 1000 * (i + 1));
        }

        let stats = manager.get_bandwidth_stats();
        assert_eq!(stats.peer_count, 5);
        assert_eq!(stats.total_bytes_received, 15000);
        assert_eq!(stats.success_rate(), 100.0);
    }
}
