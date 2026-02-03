//! Peer discovery for ShadowMesh
//!
//! Manages peer discovery, scoring, and selection for content routing.

use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Default peer score
pub const DEFAULT_PEER_SCORE: f64 = 50.0;

/// Maximum peer score
pub const MAX_PEER_SCORE: f64 = 100.0;

/// Minimum peer score
pub const MIN_PEER_SCORE: f64 = 0.0;

/// Score decay factor per hour
pub const SCORE_DECAY_FACTOR: f64 = 0.99;

/// Peer information
#[derive(Debug, Clone)]
pub struct PeerInfo {
    /// Peer ID
    pub peer_id: PeerId,
    /// Known addresses
    pub addresses: Vec<String>,
    /// Geographic region (if known)
    pub region: Option<String>,
    /// Last seen timestamp
    pub last_seen: Instant,
    /// Connection state
    pub state: PeerState,
    /// Peer score (0-100)
    pub score: f64,
    /// Content this peer provides
    pub provides: Vec<String>,
    /// Latency measurements (ms)
    pub latency_history: Vec<u32>,
    /// Bandwidth estimate (bytes/sec)
    pub bandwidth_estimate: Option<u64>,
    /// Failed connection attempts
    pub failed_attempts: u32,
    /// Successful transfers
    pub successful_transfers: u64,
    /// Failed transfers
    pub failed_transfers: u64,
}

impl PeerInfo {
    pub fn new(peer_id: PeerId) -> Self {
        Self {
            peer_id,
            addresses: Vec::new(),
            region: None,
            last_seen: Instant::now(),
            state: PeerState::Discovered,
            score: DEFAULT_PEER_SCORE,
            provides: Vec::new(),
            latency_history: Vec::new(),
            bandwidth_estimate: None,
            failed_attempts: 0,
            successful_transfers: 0,
            failed_transfers: 0,
        }
    }

    /// Calculate average latency
    pub fn average_latency(&self) -> Option<u32> {
        if self.latency_history.is_empty() {
            return None;
        }
        let sum: u32 = self.latency_history.iter().sum();
        Some(sum / self.latency_history.len() as u32)
    }

    /// Record a latency measurement
    pub fn record_latency(&mut self, latency_ms: u32) {
        self.latency_history.push(latency_ms);
        // Keep only last 100 measurements
        if self.latency_history.len() > 100 {
            self.latency_history.remove(0);
        }
    }

    /// Update score based on transfer result
    pub fn record_transfer(&mut self, success: bool, bytes: u64, duration_ms: u64) {
        if success {
            self.successful_transfers += 1;
            // Increase score for successful transfer
            self.score = (self.score + 2.0).min(MAX_PEER_SCORE);
            // Update bandwidth estimate
            if duration_ms > 0 {
                let bps = (bytes * 1000) / duration_ms;
                self.bandwidth_estimate = Some(bps);
            }
        } else {
            self.failed_transfers += 1;
            // Decrease score for failed transfer
            self.score = (self.score - 5.0).max(MIN_PEER_SCORE);
        }
    }

    /// Check if peer is considered reliable
    pub fn is_reliable(&self) -> bool {
        self.score >= 40.0 && self.failed_attempts < 5
    }

    /// Calculate success rate
    pub fn success_rate(&self) -> f64 {
        let total = self.successful_transfers + self.failed_transfers;
        if total == 0 {
            return 0.5; // Unknown, assume 50%
        }
        self.successful_transfers as f64 / total as f64
    }
}

/// Peer connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PeerState {
    Discovered,
    Connecting,
    Connected,
    Disconnected,
    Banned,
}

/// Peer discovery configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryConfig {
    /// Maximum number of peers to maintain
    pub max_peers: usize,
    /// Minimum peers to maintain
    pub min_peers: usize,
    /// How often to refresh peer list (seconds)
    pub refresh_interval_secs: u64,
    /// Peer timeout (seconds)
    pub peer_timeout_secs: u64,
    /// Enable mDNS local discovery
    pub enable_mdns: bool,
    /// Bootstrap peers
    pub bootstrap_peers: Vec<String>,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            max_peers: 50,
            min_peers: 5,
            refresh_interval_secs: 300,
            peer_timeout_secs: 600,
            enable_mdns: true,
            bootstrap_peers: Vec::new(),
        }
    }
}

/// Peer discovery manager
pub struct PeerDiscovery {
    /// Configuration
    config: DiscoveryConfig,
    /// Known peers
    peers: HashMap<PeerId, PeerInfo>,
    /// Connected peers
    connected: Vec<PeerId>,
    /// Banned peers
    banned: Vec<PeerId>,
    /// Discovery statistics
    stats: DiscoveryStats,
}

/// Discovery statistics
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct DiscoveryStats {
    pub total_discovered: u64,
    pub currently_connected: usize,
    pub total_connections: u64,
    pub total_disconnections: u64,
    pub banned_peers: usize,
}

impl PeerDiscovery {
    /// Create a new peer discovery manager
    pub fn new() -> Self {
        Self::with_config(DiscoveryConfig::default())
    }

    /// Create with custom config
    pub fn with_config(config: DiscoveryConfig) -> Self {
        Self {
            config,
            peers: HashMap::new(),
            connected: Vec::new(),
            banned: Vec::new(),
            stats: DiscoveryStats::default(),
        }
    }

    /// Add a discovered peer
    pub fn add_peer(&mut self, peer_id: PeerId, addresses: Vec<String>) {
        if self.banned.contains(&peer_id) {
            return;
        }

        self.stats.total_discovered += 1;

        let peer = self
            .peers
            .entry(peer_id)
            .or_insert_with(|| PeerInfo::new(peer_id));
        peer.addresses = addresses;
        peer.last_seen = Instant::now();
        peer.state = PeerState::Discovered;
    }

    /// Mark peer as connected
    pub fn peer_connected(&mut self, peer_id: PeerId) {
        if let Some(peer) = self.peers.get_mut(&peer_id) {
            peer.state = PeerState::Connected;
            peer.last_seen = Instant::now();
            peer.failed_attempts = 0;
        }

        if !self.connected.contains(&peer_id) {
            self.connected.push(peer_id);
            self.stats.currently_connected = self.connected.len();
            self.stats.total_connections += 1;
        }
    }

    /// Mark peer as disconnected
    pub fn peer_disconnected(&mut self, peer_id: &PeerId) {
        if let Some(peer) = self.peers.get_mut(peer_id) {
            peer.state = PeerState::Disconnected;
        }

        self.connected.retain(|p| p != peer_id);
        self.stats.currently_connected = self.connected.len();
        self.stats.total_disconnections += 1;
    }

    /// Record a connection failure
    pub fn connection_failed(&mut self, peer_id: &PeerId) {
        if let Some(peer) = self.peers.get_mut(peer_id) {
            peer.failed_attempts += 1;
            peer.score = (peer.score - 10.0).max(MIN_PEER_SCORE);

            // Ban peer after too many failures
            if peer.failed_attempts >= 10 {
                self.ban_peer(peer_id);
            }
        }
    }

    /// Ban a peer
    pub fn ban_peer(&mut self, peer_id: &PeerId) {
        // First disconnect (removes from connected list)
        self.connected.retain(|p| p != peer_id);
        self.stats.currently_connected = self.connected.len();
        self.stats.total_disconnections += 1;

        // Then set banned state
        if let Some(peer) = self.peers.get_mut(peer_id) {
            peer.state = PeerState::Banned;
        }

        if !self.banned.contains(peer_id) {
            self.banned.push(*peer_id);
            self.stats.banned_peers = self.banned.len();
        }
    }

    /// Unban a peer
    pub fn unban_peer(&mut self, peer_id: &PeerId) {
        self.banned.retain(|p| p != peer_id);
        self.stats.banned_peers = self.banned.len();

        if let Some(peer) = self.peers.get_mut(peer_id) {
            peer.state = PeerState::Disconnected;
            peer.failed_attempts = 0;
            peer.score = DEFAULT_PEER_SCORE;
        }
    }

    /// Get peer info
    pub fn get_peer(&self, peer_id: &PeerId) -> Option<&PeerInfo> {
        self.peers.get(peer_id)
    }

    /// Get mutable peer info
    pub fn get_peer_mut(&mut self, peer_id: &PeerId) -> Option<&mut PeerInfo> {
        self.peers.get_mut(peer_id)
    }

    /// Get all connected peers
    pub fn get_connected_peers(&self) -> Vec<&PeerInfo> {
        self.connected
            .iter()
            .filter_map(|id| self.peers.get(id))
            .collect()
    }

    /// Get best peers for content retrieval (sorted by score)
    pub fn get_best_peers(&self, count: usize) -> Vec<&PeerInfo> {
        let mut connected: Vec<_> = self.get_connected_peers();
        connected.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        connected.into_iter().take(count).collect()
    }

    /// Find peers that provide specific content
    pub fn find_providers(&self, cid: &str) -> Vec<&PeerInfo> {
        self.peers
            .values()
            .filter(|p| p.provides.contains(&cid.to_string()) && p.state == PeerState::Connected)
            .collect()
    }

    /// Add content to peer's provides list
    pub fn add_provider(&mut self, peer_id: &PeerId, cid: String) {
        if let Some(peer) = self.peers.get_mut(peer_id) {
            if !peer.provides.contains(&cid) {
                peer.provides.push(cid);
            }
        }
    }

    /// Select peers for content distribution
    pub fn select_distribution_peers(&self, count: usize, exclude: &[PeerId]) -> Vec<PeerId> {
        let mut candidates: Vec<_> = self
            .connected
            .iter()
            .filter(|id| !exclude.contains(id))
            .filter_map(|id| self.peers.get(id))
            .filter(|p| p.is_reliable())
            .collect();

        // Sort by combination of score and latency
        candidates.sort_by(|a, b| {
            let score_a = a.score - (a.average_latency().unwrap_or(100) as f64 * 0.1);
            let score_b = b.score - (b.average_latency().unwrap_or(100) as f64 * 0.1);
            score_b.partial_cmp(&score_a).unwrap()
        });

        candidates
            .into_iter()
            .take(count)
            .map(|p| p.peer_id)
            .collect()
    }

    /// Clean up stale peers
    pub fn cleanup_stale_peers(&mut self) {
        let timeout = Duration::from_secs(self.config.peer_timeout_secs);
        let now = Instant::now();

        // Collect stale peer IDs
        let stale: Vec<PeerId> = self
            .peers
            .iter()
            .filter(|(_, p)| {
                p.state == PeerState::Disconnected && now.duration_since(p.last_seen) > timeout
            })
            .map(|(id, _)| *id)
            .collect();

        // Remove stale peers
        for id in stale {
            self.peers.remove(&id);
        }
    }

    /// Apply score decay
    pub fn apply_score_decay(&mut self) {
        for peer in self.peers.values_mut() {
            // Decay toward default score
            let diff = peer.score - DEFAULT_PEER_SCORE;
            peer.score = DEFAULT_PEER_SCORE + (diff * SCORE_DECAY_FACTOR);
        }
    }

    /// Check if we need more peers
    pub fn needs_more_peers(&self) -> bool {
        self.connected.len() < self.config.min_peers
    }

    /// Check if we have too many peers
    pub fn too_many_peers(&self) -> bool {
        self.connected.len() > self.config.max_peers
    }

    /// Get peers to disconnect (lowest scoring)
    pub fn get_peers_to_disconnect(&self, count: usize) -> Vec<PeerId> {
        let mut connected: Vec<_> = self.get_connected_peers();
        connected.sort_by(|a, b| a.score.partial_cmp(&b.score).unwrap());
        connected
            .into_iter()
            .take(count)
            .map(|p| p.peer_id)
            .collect()
    }

    /// Get discovery statistics
    pub fn get_stats(&self) -> &DiscoveryStats {
        &self.stats
    }

    /// Get all known peer IDs
    pub fn get_all_peer_ids(&self) -> Vec<PeerId> {
        self.peers.keys().cloned().collect()
    }
}

impl Default for PeerDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

/// Region-based peer selection for geographic routing
pub struct RegionalPeerSelector {
    /// Peers grouped by region
    regions: HashMap<String, Vec<PeerId>>,
    /// Local region
    local_region: Option<String>,
}

impl RegionalPeerSelector {
    pub fn new() -> Self {
        Self {
            regions: HashMap::new(),
            local_region: None,
        }
    }

    pub fn set_local_region(&mut self, region: String) {
        self.local_region = Some(region);
    }

    pub fn add_peer_to_region(&mut self, peer_id: PeerId, region: String) {
        self.regions.entry(region).or_default().push(peer_id);
    }

    /// Get peers in same region (for low latency)
    pub fn get_local_peers(&self) -> Vec<PeerId> {
        self.local_region
            .as_ref()
            .and_then(|r| self.regions.get(r))
            .cloned()
            .unwrap_or_default()
    }

    /// Get peers in other regions (for redundancy)
    pub fn get_remote_peers(&self) -> Vec<PeerId> {
        let local = self.local_region.as_ref();
        self.regions
            .iter()
            .filter(|(region, _)| Some(*region) != local)
            .flat_map(|(_, peers)| peers.clone())
            .collect()
    }

    /// Select peers for optimal distribution (mix of local and remote)
    pub fn select_distributed(&self, count: usize) -> Vec<PeerId> {
        let local = self.get_local_peers();
        let remote = self.get_remote_peers();

        // Prefer 2/3 local, 1/3 remote for latency optimization
        let local_count = (count * 2) / 3;
        let remote_count = count - local_count;

        let mut result: Vec<PeerId> = local.into_iter().take(local_count).collect();
        result.extend(remote.into_iter().take(remote_count));
        result
    }
}

impl Default for RegionalPeerSelector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p::identity;

    fn create_test_peer() -> PeerId {
        let keypair = identity::Keypair::generate_ed25519();
        PeerId::from(keypair.public())
    }

    #[test]
    fn test_add_and_connect_peer() {
        let mut discovery = PeerDiscovery::new();
        let peer_id = create_test_peer();

        discovery.add_peer(peer_id, vec!["/ip4/127.0.0.1/tcp/4001".to_string()]);
        assert!(discovery.get_peer(&peer_id).is_some());

        discovery.peer_connected(peer_id);
        assert_eq!(discovery.get_connected_peers().len(), 1);
    }

    #[test]
    fn test_peer_scoring() {
        let mut discovery = PeerDiscovery::new();
        let peer_id = create_test_peer();

        discovery.add_peer(peer_id, vec![]);
        discovery.peer_connected(peer_id);

        // Record successful transfer
        if let Some(peer) = discovery.get_peer_mut(&peer_id) {
            peer.record_transfer(true, 1024, 100);
            assert!(peer.score > DEFAULT_PEER_SCORE);
        }
    }

    #[test]
    fn test_best_peers_selection() {
        let mut discovery = PeerDiscovery::new();

        for i in 0..5 {
            let peer_id = create_test_peer();
            discovery.add_peer(peer_id, vec![]);
            discovery.peer_connected(peer_id);

            // Vary scores
            if let Some(peer) = discovery.get_peer_mut(&peer_id) {
                peer.score = (i as f64) * 20.0;
            }
        }

        let best = discovery.get_best_peers(3);
        assert_eq!(best.len(), 3);
        // Should be sorted by score descending
        assert!(best[0].score >= best[1].score);
        assert!(best[1].score >= best[2].score);
    }

    #[test]
    fn test_ban_peer() {
        let mut discovery = PeerDiscovery::new();
        let peer_id = create_test_peer();

        discovery.add_peer(peer_id, vec![]);
        discovery.peer_connected(peer_id);
        discovery.ban_peer(&peer_id);

        let peer = discovery.get_peer(&peer_id).unwrap();
        assert_eq!(peer.state, PeerState::Banned);
        assert_eq!(discovery.get_connected_peers().len(), 0);
    }
}
