//! Integration tests for the ShadowMesh Protocol
//!
//! Tests verify the full protocol workflow including:
//! - Content fragmentation and reassembly
//! - Encryption and decryption flows
//! - DHT operations
//! - Replication management
//! - Peer discovery
//! - Bandwidth tracking
//! - ZK Relay
//! - Adaptive Routing with censorship detection

use shadowmesh_protocol::{
    hash_content,
    verify_content_hash,
    // Adaptive Routing types
    AdaptiveRouter,
    AdaptiveRoutingConfig,
    // Bandwidth types
    BandwidthTracker,
    BlindRequest,
    BlindResponse,
    CellType,
    CensorshipStatus,
    ContentDHTMetadata,
    ContentMetadata,
    ContentRecord,
    // Crypto types
    CryptoManager,
    // DHT types
    DHTManager,
    DiscoveryConfig,
    FailureType,
    // Fragment types
    FragmentManager,
    GeoRegion,
    KeyDerivation,
    OnionRouter,
    PathHealth,
    // Peer Discovery types
    PeerDiscovery,
    PeerInfo,
    PeerState,
    ProviderInfo,
    RateLimiter,
    RelayCell,
    RelayInfo,
    RelayPayload,
    ReplicationHealth,
    // Replication types
    ReplicationManager,
    ReplicationPriority,
    ReplicationStatus,
    RouteStrategy,
    TrafficPadder,
    // ZK Relay types
    ZkRelayClient,
    ZkRelayConfig,
    ZkRelayNode,
    KEY_SIZE,
};

// ============================================================================
// Fragment Manager Tests
// ============================================================================

#[test]
fn test_fragment_content() {
    let data = b"Hello, ShadowMesh! This is test content for fragmentation.";
    let manifest = FragmentManager::fragment_content(data, "test.txt".to_string());

    assert!(!manifest.content_hash.is_empty());
    assert!(!manifest.fragments.is_empty());
    assert_eq!(manifest.metadata.size, data.len() as u64);
    assert_eq!(manifest.metadata.name, "test.txt");
}

#[test]
fn test_create_fragment() {
    let data = b"Fragment data for testing".to_vec();
    let fragment = FragmentManager::create_fragment(data.clone(), 0, 5);

    assert_eq!(fragment.index, 0);
    assert_eq!(fragment.total_fragments, 5);
    assert_eq!(fragment.data, data);
    assert!(!fragment.hash.is_empty());
}

#[test]
fn test_fragment_and_reassemble() {
    // Create some test data
    let original_data = b"This is the original content that will be fragmented and reassembled.";

    // Create fragments
    let chunk_size = 20;
    let chunks: Vec<_> = original_data.chunks(chunk_size).collect();
    let total = chunks.len() as u32;

    let fragments: Vec<_> = chunks
        .into_iter()
        .enumerate()
        .map(|(i, chunk)| FragmentManager::create_fragment(chunk.to_vec(), i as u32, total))
        .collect();

    // Reassemble
    let reassembled = FragmentManager::reassemble_fragments(fragments);

    assert_eq!(reassembled, original_data.to_vec());
}

#[test]
fn test_verify_fragment() {
    let data = b"Data to verify".to_vec();
    let fragment = FragmentManager::create_fragment(data, 0, 1);

    // Should verify correctly
    assert!(FragmentManager::verify_fragment(&fragment));

    // Tampered fragment should fail
    let mut tampered = fragment.clone();
    tampered.data[0] ^= 0xFF;
    assert!(!FragmentManager::verify_fragment(&tampered));
}

#[test]
fn test_content_metadata() {
    let metadata = ContentMetadata {
        name: "test.bin".to_string(),
        size: 2048,
        mime_type: "application/octet-stream".to_string(),
    };

    assert_eq!(metadata.name, "test.bin");
    assert_eq!(metadata.size, 2048);
    assert_eq!(metadata.mime_type, "application/octet-stream");
}

// ============================================================================
// Crypto Tests
// ============================================================================

#[test]
fn test_crypto_manager_encryption_decryption() {
    let key = [42u8; 32];
    let crypto = CryptoManager::new(&key);

    let plaintext = b"Secret message for ShadowMesh encryption test";

    // Encrypt
    let encrypted = crypto.encrypt(plaintext).expect("Encryption failed");

    // Verify ciphertext is different from plaintext
    assert_ne!(encrypted.ciphertext, plaintext.to_vec());

    // Decrypt
    let decrypted = crypto.decrypt(&encrypted).expect("Decryption failed");

    // Verify roundtrip
    assert_eq!(decrypted, plaintext.to_vec());
}

#[test]
fn test_crypto_manager_random_key() {
    let (crypto, key) = CryptoManager::new_random().unwrap();

    let plaintext = b"Test message with random key";

    let encrypted = crypto.encrypt(plaintext).expect("Encryption failed");
    let decrypted = crypto.decrypt(&encrypted).expect("Decryption failed");

    assert_eq!(decrypted, plaintext.to_vec());
    assert_eq!(key.len(), 32);
}

#[test]
fn test_content_hashing() {
    let content = b"Test content for hashing";

    let hash1 = hash_content(content);
    let hash2 = hash_content(content);

    // Same content should produce same hash
    assert_eq!(hash1, hash2);

    // Different content should produce different hash
    let different_content = b"Different content";
    let hash3 = hash_content(different_content);
    assert_ne!(hash1, hash3);
}

#[test]
fn test_content_hash_verification() {
    let content = b"Content to verify";
    let hash = hash_content(content);

    // Verify correct hash
    assert!(verify_content_hash(content, &hash));

    // Verify incorrect hash fails
    let wrong_hash = "wrong_hash_value_here";
    assert!(!verify_content_hash(content, wrong_hash));
}

#[test]
fn test_key_derivation() {
    // Derive keys for different purposes
    let key1 = KeyDerivation::derive_key(b"secret", "purpose1");
    let key2 = KeyDerivation::derive_key(b"secret", "purpose2");

    // Different purposes should produce different keys
    assert_ne!(key1, key2);

    // Same purpose should produce same key
    let key1_again = KeyDerivation::derive_key(b"secret", "purpose1");
    assert_eq!(key1, key1_again);
}

// ============================================================================
// Onion Router Tests
// ============================================================================

#[test]
fn test_onion_routing_single_hop() {
    let node_keys = vec![[10u8; 32]];
    let router = OnionRouter::new(&node_keys);

    let plaintext = b"Message for onion routing";

    let onion = router.wrap(plaintext).expect("Wrap failed");

    // Onion should be different from plaintext
    assert_ne!(onion, plaintext.to_vec());

    // Should be able to unwrap
    let unwrapped = router.unwrap_all(onion).expect("Unwrap failed");
    assert_eq!(unwrapped, plaintext.to_vec());
}

#[test]
fn test_onion_routing_multi_hop() {
    let node_keys = vec![[10u8; 32], [20u8; 32], [30u8; 32]];
    let router = OnionRouter::new(&node_keys);

    let plaintext = b"Multi-hop message";

    let onion = router.wrap(plaintext).expect("Wrap failed");

    // Each layer adds overhead
    assert!(onion.len() > plaintext.len());

    // Full unwrap should restore original
    let unwrapped = router.unwrap_all(onion).expect("Unwrap failed");
    assert_eq!(unwrapped, plaintext.to_vec());
}

// ============================================================================
// DHT Manager Tests
// ============================================================================

#[tokio::test]
async fn test_dht_manager_creation() {
    let peer_id = libp2p::identity::Keypair::generate_ed25519()
        .public()
        .to_peer_id();

    let manager = DHTManager::new(peer_id);

    // Should be able to create records
    let record = manager.create_content_record(
        "test_cid".to_string(),
        1024,
        "text/plain".to_string(),
        1,
        false,
    );

    assert_eq!(record.cid, "test_cid");
    assert_eq!(record.metadata.size, 1024);
}

#[test]
fn test_content_record_creation() {
    let record = ContentRecord {
        cid: "content_123".to_string(),
        providers: vec![],
        metadata: ContentDHTMetadata {
            size: 1024,
            mime_type: "application/octet-stream".to_string(),
            fragment_count: 5,
            encrypted: false,
        },
        created_at: 1234567890,
        ttl_seconds: 3600,
    };

    assert_eq!(record.cid, "content_123");
    assert_eq!(record.metadata.size, 1024);
    assert_eq!(record.metadata.fragment_count, 5);
}

#[test]
fn test_provider_info_creation() {
    let provider = ProviderInfo {
        peer_id: "peer123".to_string(),
        addresses: vec!["/ip4/127.0.0.1/tcp/4001".to_string()],
        reputation: 80,
        online: true,
        last_seen: 1234567890,
    };

    assert_eq!(provider.reputation, 80);
    assert!(provider.online);
}

// ============================================================================
// Replication Manager Tests
// ============================================================================

#[tokio::test]
async fn test_replication_manager_creation() {
    let manager = ReplicationManager::new();

    // Default replication factor should be 3
    assert_eq!(manager.replication_factor(), 3);
}

#[tokio::test]
async fn test_replication_manager_with_factor() {
    let manager = ReplicationManager::with_factor(5);

    assert_eq!(manager.replication_factor(), 5);
}

#[test]
fn test_replication_status() {
    let status = ReplicationStatus {
        cid: "test_cid".to_string(),
        replica_count: 2,
        target_replicas: 3,
        replica_holders: vec!["peer1".to_string()],
        healthy: false,
        last_checked: 1234567890,
    };

    assert_eq!(status.cid, "test_cid");
    assert_eq!(status.replica_count, 2);
    assert!(!status.healthy);
}

#[test]
fn test_replication_health() {
    let health = ReplicationHealth {
        total_content: 10,
        healthy_content: 8,
        under_replicated_content: 2,
        critical_content: 0,
        replication_factor: 3,
        pinned_content: 5,
    };

    assert_eq!(health.total_content, 10);
    assert_eq!(health.healthy_content, 8);
    assert_eq!(health.health_percentage(), 80.0);
}

#[test]
fn test_replication_priority_ordering() {
    let high = ReplicationPriority::High;
    let normal = ReplicationPriority::Normal;
    let low = ReplicationPriority::Low;
    let critical = ReplicationPriority::Critical;

    // Critical should be highest
    assert!(critical > high);
    assert!(high > normal);
    assert!(normal > low);
}

#[tokio::test]
async fn test_replication_pinning() {
    let mut manager = ReplicationManager::new();

    // Pin content
    manager.pin("content1");
    assert!(manager.is_pinned("content1"));

    // Unpin content
    manager.unpin("content1");
    assert!(!manager.is_pinned("content1"));
}

// ============================================================================
// Peer Discovery Tests
// ============================================================================

#[tokio::test]
async fn test_peer_discovery_creation() {
    let discovery = PeerDiscovery::new();

    let stats = discovery.get_stats();
    assert_eq!(stats.total_discovered, 0);
}

#[tokio::test]
async fn test_peer_discovery_with_config() {
    let config = DiscoveryConfig {
        max_peers: 100,
        min_peers: 10,
        refresh_interval_secs: 60,
        peer_timeout_secs: 300,
        enable_mdns: false,
        bootstrap_peers: vec!["peer1".to_string()],
    };
    let discovery = PeerDiscovery::with_config(config);

    let stats = discovery.get_stats();
    assert_eq!(stats.total_discovered, 0);
}

#[test]
fn test_peer_info_creation() {
    let peer_id = libp2p::identity::Keypair::generate_ed25519()
        .public()
        .to_peer_id();

    let peer = PeerInfo::new(peer_id);

    assert_eq!(peer.peer_id, peer_id);
    assert!(matches!(peer.state, PeerState::Discovered));
    assert!(peer.addresses.is_empty());
}

#[test]
fn test_peer_info_latency_tracking() {
    let peer_id = libp2p::identity::Keypair::generate_ed25519()
        .public()
        .to_peer_id();

    let mut peer = PeerInfo::new(peer_id);

    // Initially no latency
    assert!(peer.average_latency().is_none());

    // Record some latencies
    peer.record_latency(100);
    peer.record_latency(200);
    peer.record_latency(150);

    // Average should be computed
    assert_eq!(peer.average_latency(), Some(150));
}

#[test]
fn test_peer_info_transfer_tracking() {
    let peer_id = libp2p::identity::Keypair::generate_ed25519()
        .public()
        .to_peer_id();

    let mut peer = PeerInfo::new(peer_id);
    let initial_score = peer.score;

    // Successful transfer should increase score
    peer.record_transfer(true, 1024, 100);
    assert!(peer.score > initial_score);
    assert_eq!(peer.successful_transfers, 1);

    // Failed transfer should decrease score
    let after_success_score = peer.score;
    peer.record_transfer(false, 0, 0);
    assert!(peer.score < after_success_score);
    assert_eq!(peer.failed_transfers, 1);
}

#[test]
fn test_peer_state_transitions() {
    let state = PeerState::Discovered;
    assert!(matches!(state, PeerState::Discovered));

    let connected = PeerState::Connected;
    assert!(matches!(connected, PeerState::Connected));

    let disconnected = PeerState::Disconnected;
    assert!(matches!(disconnected, PeerState::Disconnected));
}

#[tokio::test]
async fn test_peer_discovery_add_peer() {
    let mut discovery = PeerDiscovery::new();

    let peer_id = libp2p::identity::Keypair::generate_ed25519()
        .public()
        .to_peer_id();

    discovery.add_peer(peer_id, vec!["/ip4/127.0.0.1/tcp/4001".to_string()]);

    let stats = discovery.get_stats();
    assert_eq!(stats.total_discovered, 1);
}

#[tokio::test]
async fn test_peer_connection_lifecycle() {
    let mut discovery = PeerDiscovery::new();

    let peer_id = libp2p::identity::Keypair::generate_ed25519()
        .public()
        .to_peer_id();

    // Add peer
    discovery.add_peer(peer_id, vec![]);
    assert_eq!(discovery.get_stats().currently_connected, 0);

    // Connect peer
    discovery.peer_connected(peer_id);
    assert_eq!(discovery.get_stats().currently_connected, 1);

    // Disconnect peer
    discovery.peer_disconnected(&peer_id);
    assert_eq!(discovery.get_stats().currently_connected, 0);
}

// ============================================================================
// Bandwidth Tracker Tests
// ============================================================================

#[test]
fn test_bandwidth_tracker_creation() {
    let tracker = BandwidthTracker::new();

    let inbound = tracker.inbound_stats();
    let outbound = tracker.outbound_stats();

    assert_eq!(inbound.total_bytes, 0);
    assert_eq!(outbound.total_bytes, 0);
}

#[test]
fn test_bandwidth_tracker_with_limits() {
    let tracker = BandwidthTracker::with_limits(
        Some(1024 * 1024), // 1MB inbound limit
        Some(512 * 1024),  // 512KB outbound limit
    );

    // Tracker should be created with limits
    assert_eq!(tracker.inbound_stats().total_bytes, 0);
}

#[test]
fn test_bandwidth_recording_inbound() {
    let mut tracker = BandwidthTracker::new();

    tracker.record_inbound(1024, None);
    tracker.record_inbound(512, None);

    let stats = tracker.inbound_stats();
    assert_eq!(stats.total_bytes, 1536);
}

#[test]
fn test_bandwidth_recording_outbound() {
    let mut tracker = BandwidthTracker::new();

    tracker.record_outbound(2048, None);
    tracker.record_outbound(1024, None);

    let stats = tracker.outbound_stats();
    assert_eq!(stats.total_bytes, 3072);
}

#[test]
fn test_bandwidth_bidirectional() {
    let mut tracker = BandwidthTracker::new();

    tracker.record_inbound(1000, None);
    tracker.record_outbound(2000, None);
    tracker.record_inbound(500, None);
    tracker.record_outbound(1500, None);

    assert_eq!(tracker.inbound_stats().total_bytes, 1500);
    assert_eq!(tracker.outbound_stats().total_bytes, 3500);
}

#[test]
fn test_bandwidth_per_peer_tracking() {
    let mut tracker = BandwidthTracker::new();

    tracker.record_inbound(1000, Some("peer1"));
    tracker.record_inbound(2000, Some("peer2"));
    tracker.record_outbound(500, Some("peer1"));

    // Total should include all
    assert_eq!(tracker.inbound_stats().total_bytes, 3000);
    assert_eq!(tracker.outbound_stats().total_bytes, 500);
}

// ============================================================================
// Rate Limiter Tests
// ============================================================================

#[test]
fn test_rate_limiter_creation() {
    let mut limiter = RateLimiter::new(100); // 100 bytes per second

    // Should have full bucket initially
    assert!(limiter.available() > 0);
}

#[test]
fn test_rate_limiter_consume() {
    let mut limiter = RateLimiter::new(1000); // 1000 bytes per second

    // Should be able to consume within limit
    assert!(limiter.try_consume(500));
    assert!(limiter.try_consume(500));

    // Now should be exhausted
    assert!(!limiter.try_consume(100));
}

#[test]
fn test_rate_limiter_burst() {
    let mut limiter = RateLimiter::with_burst(100, 1000); // 100 bps, 1000 byte burst

    // Should allow burst
    assert!(limiter.try_consume(1000));

    // Now should be exhausted
    assert!(!limiter.try_consume(100));
}

// ============================================================================
// Integration: Full Content Workflow
// ============================================================================

#[test]
fn test_full_content_encryption_workflow() {
    // 1. Create content
    let content = b"Full integration test content for ShadowMesh protocol";

    // 2. Hash content
    let content_hash = hash_content(content);
    assert!(!content_hash.is_empty());

    // 3. Create crypto manager and encrypt
    let (crypto, _key) = CryptoManager::new_random().unwrap();
    let encrypted = crypto.encrypt(content).expect("Encryption failed");

    // 4. Verify encrypted data is different
    assert_ne!(encrypted.ciphertext, content.to_vec());

    // 5. Decrypt and verify
    let decrypted = crypto.decrypt(&encrypted).expect("Decryption failed");
    assert_eq!(decrypted, content.to_vec());

    // 6. Verify hash still matches
    assert!(verify_content_hash(content, &content_hash));
}

#[test]
fn test_fragment_and_encrypt_workflow() {
    let content = b"Content that will be fragmented and encrypted in ShadowMesh";

    // Create fragment
    let fragment = FragmentManager::create_fragment(content.to_vec(), 0, 1);

    // Encrypt fragment data
    let (crypto, _key) = CryptoManager::new_random().unwrap();
    let encrypted = crypto.encrypt(&fragment.data).expect("Encryption failed");

    // Decrypt
    let decrypted = crypto.decrypt(&encrypted).expect("Decryption failed");
    assert_eq!(decrypted, fragment.data);
}

#[tokio::test]
async fn test_peer_discovery_and_bandwidth_tracking() {
    let mut discovery = PeerDiscovery::new();
    let mut tracker = BandwidthTracker::new();

    // Register a peer
    let peer_id = libp2p::identity::Keypair::generate_ed25519()
        .public()
        .to_peer_id();

    discovery.add_peer(peer_id, vec!["/ip4/10.0.0.1/tcp/4001".to_string()]);

    // Simulate data transfer
    tracker.record_inbound(1024, None);
    tracker.record_outbound(2048, None);

    assert_eq!(tracker.inbound_stats().total_bytes, 1024);
    assert_eq!(tracker.outbound_stats().total_bytes, 2048);

    let disc_stats = discovery.get_stats();
    assert_eq!(disc_stats.total_discovered, 1);
}

// ============================================================================
// Edge Cases and Error Handling
// ============================================================================

#[test]
fn test_empty_content_encryption() {
    let (crypto, _key) = CryptoManager::new_random().unwrap();
    let empty_content = b"";

    let encrypted = crypto.encrypt(empty_content).expect("Should encrypt empty");
    let decrypted = crypto.decrypt(&encrypted).expect("Should decrypt empty");

    assert_eq!(decrypted, empty_content.to_vec());
}

#[test]
fn test_large_content_encryption() {
    let (crypto, _key) = CryptoManager::new_random().unwrap();
    let large_content = vec![0xAB; 1024 * 1024]; // 1MB

    let encrypted = crypto
        .encrypt(&large_content)
        .expect("Should encrypt large");
    let decrypted = crypto.decrypt(&encrypted).expect("Should decrypt large");

    assert_eq!(decrypted, large_content);
}

#[test]
fn test_hash_determinism() {
    let content = b"Deterministic hashing test";

    let hashes: Vec<String> = (0..10).map(|_| hash_content(content)).collect();

    // All hashes should be identical
    assert!(hashes.windows(2).all(|w| w[0] == w[1]));
}

#[test]
fn test_different_keys_produce_different_ciphertext() {
    let key1 = [1u8; 32];
    let key2 = [2u8; 32];

    let crypto1 = CryptoManager::new(&key1);
    let crypto2 = CryptoManager::new(&key2);

    let plaintext = b"Same plaintext, different keys";

    let encrypted1 = crypto1.encrypt(plaintext).expect("Encrypt 1 failed");
    let encrypted2 = crypto2.encrypt(plaintext).expect("Encrypt 2 failed");

    // Ciphertext should be different with different keys
    assert_ne!(encrypted1.ciphertext, encrypted2.ciphertext);
}

#[test]
fn test_large_fragment_manifest() {
    // Create a large file's worth of data
    let large_data = vec![0xCD; 1024 * 1024]; // 1MB
    let manifest = FragmentManager::fragment_content(&large_data, "large.bin".to_string());

    // Should have multiple fragments (256KB chunks)
    assert!(manifest.fragments.len() >= 4);
    assert_eq!(manifest.metadata.size, 1024 * 1024);
}

#[test]
fn test_peer_reliability_check() {
    let peer_id = libp2p::identity::Keypair::generate_ed25519()
        .public()
        .to_peer_id();

    let mut peer = PeerInfo::new(peer_id);

    // New peer starts reliable
    assert!(peer.is_reliable());

    // After many failures, becomes unreliable
    for _ in 0..10 {
        peer.record_transfer(false, 0, 0);
    }
    assert!(!peer.is_reliable());
}

#[test]
fn test_peer_success_rate() {
    let peer_id = libp2p::identity::Keypair::generate_ed25519()
        .public()
        .to_peer_id();

    let mut peer = PeerInfo::new(peer_id);

    // Initially unknown
    assert!((peer.success_rate() - 0.5).abs() < 0.001);

    // After transfers
    peer.record_transfer(true, 1000, 100);
    peer.record_transfer(true, 1000, 100);
    peer.record_transfer(false, 0, 0);

    // 2 successes, 1 failure = 66.7%
    let rate = peer.success_rate();
    assert!(rate > 0.6 && rate < 0.7);
}

// ============================================================================
// ZK Relay Integration Tests
// ============================================================================

fn create_test_peer() -> libp2p::PeerId {
    libp2p::identity::Keypair::generate_ed25519()
        .public()
        .to_peer_id()
}

fn create_test_peers_zk(n: usize) -> Vec<libp2p::PeerId> {
    (0..n).map(|_| create_test_peer()).collect()
}

#[test]
fn test_zk_relay_circuit_build() {
    let secret = [42u8; KEY_SIZE];
    let mut client = ZkRelayClient::new(secret);
    let peers = create_test_peers_zk(3);

    let circuit_id = client.build_circuit(&peers).unwrap();
    assert_eq!(client.active_circuits(), 1);

    // Verify entry node is first peer
    let entry = client.get_entry_node(&circuit_id).unwrap();
    assert_eq!(entry, peers[0]);
}

#[test]
fn test_zk_relay_multi_circuit() {
    let secret = [42u8; KEY_SIZE];
    let mut client = ZkRelayClient::new(secret);

    // Build multiple circuits
    for _ in 0..5 {
        let peers = create_test_peers_zk(3);
        client.build_circuit(&peers).unwrap();
    }

    assert_eq!(client.active_circuits(), 5);
}

#[test]
fn test_zk_relay_request_encryption() {
    let secret = [42u8; KEY_SIZE];
    let mut client = ZkRelayClient::new(secret);
    let peers = create_test_peers_zk(3);

    let circuit_id = client.build_circuit_sync(&peers).unwrap();
    let request = b"GET /content/abc123 HTTP/1.1\r\nHost: shadowmesh.network\r\n\r\n";

    let cell = client.wrap_request(&circuit_id, request).unwrap();

    // Cell should be encrypted
    assert_eq!(cell.circuit_id, circuit_id);
    assert_eq!(cell.cell_type, CellType::Relay);
    assert!(!cell.payload.is_empty());

    // Payload should not contain plaintext
    let payload_str = String::from_utf8_lossy(&cell.payload);
    assert!(!payload_str.contains("GET"));
    assert!(!payload_str.contains("shadowmesh"));
}

#[test]
fn test_zk_relay_blind_request_creation() {
    let request = BlindRequest::new(
        "bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi".to_string(),
    );

    assert!(!request.content_hash.is_empty());
    assert!(request.range.is_none());
    assert!(request.timestamp > 0);

    // With range
    let range_request = BlindRequest::with_range(
        "bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi".to_string(),
        0,
        1024 * 1024,
    );
    assert_eq!(range_request.range, Some((0, 1024 * 1024)));
}

#[test]
fn test_zk_relay_cell_serialization_roundtrip() {
    let circuit_id = [0xABu8; 32];
    let payload = b"encrypted payload data".to_vec();

    let cell = RelayCell::new(circuit_id, CellType::Relay, payload.clone());
    let serialized = cell.serialize();
    let deserialized = RelayCell::deserialize(&serialized).unwrap();

    assert_eq!(deserialized.circuit_id, circuit_id);
    assert_eq!(deserialized.cell_type, CellType::Relay);
    assert_eq!(deserialized.payload, payload);
}

#[test]
fn test_zk_relay_payload_integrity() {
    let hop_id = [0xCDu8; 16];
    let inner_data = b"secret content hash request".to_vec();

    let payload = RelayPayload::new(None, hop_id, inner_data.clone());

    // Should pass integrity check
    assert!(payload.verify());

    // Tampering should fail
    let mut tampered = payload.clone();
    tampered.inner_data[0] ^= 0xFF;
    assert!(!tampered.verify());
}

#[test]
fn test_zk_relay_node_stats() {
    let node_secret = [99u8; KEY_SIZE];
    let node = ZkRelayNode::new(node_secret);

    let stats = node.stats();
    assert_eq!(stats.cells_relayed, 0);
    assert_eq!(stats.bytes_relayed, 0);
    assert_eq!(stats.active_circuits, 0);
}

#[test]
fn test_zk_relay_traffic_padding() {
    let padder = TrafficPadder::with_size(512);

    // Small data gets padded
    let small = vec![1, 2, 3, 4, 5];
    let padded = padder.pad(&small);
    assert_eq!(padded.len(), 512);
    assert_eq!(&padded[..5], &small[..]);

    // Large data stays same size
    let large: Vec<u8> = (0..1000).map(|i| i as u8).collect();
    let padded_large = padder.pad(&large);
    assert_eq!(padded_large.len(), 1000);
}

#[test]
fn test_zk_relay_circuit_expiry() {
    use std::thread::sleep;
    use std::time::Duration;

    let secret = [42u8; KEY_SIZE];
    let config = ZkRelayConfig {
        circuit_lifetime: Duration::from_millis(50),
        ..Default::default()
    };
    let mut client = ZkRelayClient::with_config(secret, config);
    let peers = create_test_peers_zk(3);

    let circuit_id = client.build_circuit(&peers).unwrap();
    assert_eq!(client.active_circuits(), 1);

    // Wait for expiry
    sleep(Duration::from_millis(100));

    // Request should fail on expired circuit
    let result = client.wrap_request(&circuit_id, b"test");
    assert!(result.is_err());
}

#[test]
fn test_zk_relay_circuit_cleanup() {
    use std::thread::sleep;
    use std::time::Duration;

    let secret = [42u8; KEY_SIZE];
    let config = ZkRelayConfig {
        circuit_lifetime: Duration::from_millis(10),
        ..Default::default()
    };
    let mut client = ZkRelayClient::with_config(secret, config);

    // Build several circuits
    for _ in 0..5 {
        let peers = create_test_peers_zk(3);
        client.build_circuit(&peers).unwrap();
    }

    assert_eq!(client.active_circuits(), 5);

    // Wait for expiry
    sleep(Duration::from_millis(50));

    // Cleanup expired
    let cleaned = client.cleanup_expired();
    assert_eq!(cleaned, 5);
    assert_eq!(client.active_circuits(), 0);
}

#[test]
fn test_zk_relay_destroy_circuit() {
    let secret = [42u8; KEY_SIZE];
    let mut client = ZkRelayClient::new(secret);
    let peers = create_test_peers_zk(3);

    let circuit_id = client.build_circuit(&peers).unwrap();
    assert_eq!(client.active_circuits(), 1);

    // Destroy should return a destroy cell
    let destroy_cell = client.destroy_circuit(&circuit_id);
    assert!(destroy_cell.is_some());
    assert_eq!(destroy_cell.unwrap().cell_type, CellType::Destroy);
    assert_eq!(client.active_circuits(), 0);

    // Can't destroy again
    let destroy_again = client.destroy_circuit(&circuit_id);
    assert!(destroy_again.is_none());
}

#[test]
fn test_zk_relay_padding_cells() {
    let secret = [42u8; KEY_SIZE];
    let config = ZkRelayConfig {
        padding_enabled: true,
        ..Default::default()
    };
    let mut client = ZkRelayClient::with_config(secret, config);
    let peers = create_test_peers_zk(3);

    let circuit_id = client.build_circuit(&peers).unwrap();

    // Should generate padding cell
    let padding = client.create_padding_cell(&circuit_id);
    assert!(padding.is_some());
    let cell = padding.unwrap();
    assert_eq!(cell.cell_type, CellType::Padding);
    assert!(!cell.payload.is_empty());
}

#[test]
fn test_zk_relay_padding_disabled() {
    let secret = [42u8; KEY_SIZE];
    let config = ZkRelayConfig {
        padding_enabled: false,
        ..Default::default()
    };
    let mut client = ZkRelayClient::with_config(secret, config);
    let peers = create_test_peers_zk(3);

    let circuit_id = client.build_circuit(&peers).unwrap();

    // Should not generate padding
    let padding = client.create_padding_cell(&circuit_id);
    assert!(padding.is_none());
}

#[test]
fn test_zk_relay_minimum_hops() {
    let secret = [42u8; KEY_SIZE];
    let mut client = ZkRelayClient::new(secret);

    // Single hop should fail
    let single_peer = create_test_peers_zk(1);
    let result = client.build_circuit(&single_peer);
    assert!(result.is_err());

    // Two hops should work
    let two_peers = create_test_peers_zk(2);
    let result = client.build_circuit(&two_peers);
    assert!(result.is_ok());
}

#[test]
fn test_zk_relay_blind_response() {
    let request = BlindRequest::new("test_hash".to_string());

    let response = BlindResponse {
        request_id: request.request_id,
        encrypted_content: vec![1, 2, 3, 4, 5],
        content_hash: "test_hash".to_string(),
        success: true,
        error: None,
    };

    assert!(response.success);
    assert!(response.error.is_none());
    assert_eq!(response.request_id, request.request_id);
}

#[test]
fn test_zk_relay_node_circuit_limit() {
    let node_secret = [99u8; KEY_SIZE];
    let config = ZkRelayConfig {
        max_circuits: 100,
        ..Default::default()
    };
    let node = ZkRelayNode::with_config(node_secret, config);

    // Node should start with 0 circuits
    assert_eq!(node.active_circuits(), 0);
}

#[test]
fn test_zk_relay_config_defaults() {
    let config = ZkRelayConfig::default();

    assert_eq!(config.default_hops, 3);
    assert!(config.padding_enabled);
    assert!(config.circuit_lifetime.as_secs() > 0);
    assert!(config.max_circuits > 0);
}

#[test]
fn test_zk_relay_full_workflow() {
    // This test simulates the full ZK relay workflow:
    // 1. Client builds circuit
    // 2. Client wraps request
    // 3. (In real world: request travels through relays)
    // 4. Client can destroy circuit

    let secret = [0xABu8; KEY_SIZE];
    let mut client = ZkRelayClient::new(secret);
    let peers = create_test_peers_zk(4);

    // 1. Build circuit through 4 relays
    let circuit_id = client.build_circuit_sync(&peers).unwrap();
    assert_eq!(client.active_circuits(), 1);

    // 2. Create blind request
    let blind_request = BlindRequest::new(
        "bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi".to_string(),
    );
    let request_bytes = bincode::serialize(&blind_request).unwrap();

    // 3. Wrap request for circuit
    let cell = client.wrap_request(&circuit_id, &request_bytes).unwrap();

    // 4. Verify cell is properly formed
    assert_eq!(cell.circuit_id, circuit_id);
    assert_eq!(cell.cell_type, CellType::Relay);
    assert!(cell.timestamp > 0);

    // 5. Entry node should be first peer
    let entry = client.get_entry_node(&circuit_id).unwrap();
    assert_eq!(entry, peers[0]);

    // 6. Cleanup
    client.destroy_circuit(&circuit_id);
    assert_eq!(client.active_circuits(), 0);
}

// ============================================================================
// Adaptive Routing Integration Tests
// ============================================================================

fn create_relay_info(
    peer_id: libp2p::PeerId,
    country: &str,
    is_guard: bool,
    is_exit: bool,
) -> RelayInfo {
    let mut info = RelayInfo::with_geo(peer_id, country, None);
    info.is_guard = is_guard;
    info.is_exit = is_exit;
    info.reputation = 0.8;
    info.avg_latency_ms = 50;
    info.bandwidth_bps = 10_000_000;
    info
}

#[test]
fn test_adaptive_router_creation() {
    let router = AdaptiveRouter::new();
    assert_eq!(router.relay_count(), 0);
    assert_eq!(router.active_route_count(), 0);
}

#[test]
fn test_adaptive_router_with_config() {
    let config = AdaptiveRoutingConfig {
        strategy: RouteStrategy::HighPrivacy,
        min_hops: 4,
        max_hops: 6,
        require_geo_diversity: true,
        auto_failover: true,
        ..Default::default()
    };
    let router = AdaptiveRouter::with_config(config);
    assert_eq!(router.relay_count(), 0);
}

#[test]
fn test_relay_registration_and_retrieval() {
    let router = AdaptiveRouter::new();

    let peer_id = libp2p::identity::Keypair::generate_ed25519()
        .public()
        .to_peer_id();
    let info = create_relay_info(peer_id, "US", true, false);

    router.register_relay(info.clone());
    assert_eq!(router.relay_count(), 1);

    let retrieved = router.get_relay(&peer_id);
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().peer_id, peer_id);
}

#[test]
fn test_geo_region_diversity() {
    // Test that geographic regions are properly assigned
    assert_eq!(GeoRegion::from_country_code("US"), GeoRegion::NorthAmerica);
    assert_eq!(GeoRegion::from_country_code("CA"), GeoRegion::NorthAmerica);
    assert_eq!(GeoRegion::from_country_code("DE"), GeoRegion::Europe);
    assert_eq!(GeoRegion::from_country_code("FR"), GeoRegion::Europe);
    assert_eq!(GeoRegion::from_country_code("JP"), GeoRegion::Asia);
    assert_eq!(GeoRegion::from_country_code("CN"), GeoRegion::Asia);
    assert_eq!(GeoRegion::from_country_code("AU"), GeoRegion::Oceania);
    assert_eq!(GeoRegion::from_country_code("BR"), GeoRegion::SouthAmerica);
    assert_eq!(GeoRegion::from_country_code("ZA"), GeoRegion::Africa);
    assert_eq!(GeoRegion::from_country_code("AE"), GeoRegion::MiddleEast);
}

#[test]
fn test_route_computation_basic() {
    let config = AdaptiveRoutingConfig {
        min_hops: 2,
        max_hops: 3,
        require_geo_diversity: false,
        ..Default::default()
    };
    let router = AdaptiveRouter::with_config(config);

    // Register relays from different regions
    for country in &["US", "DE", "JP", "AU"] {
        let peer_id = libp2p::identity::Keypair::generate_ed25519()
            .public()
            .to_peer_id();
        let info = create_relay_info(peer_id, country, true, true);
        router.register_relay(info);
    }

    let route = router.compute_route(None).unwrap();
    assert!(route.relays.len() >= 2);
    assert!(route.health_score > 0.0);
    assert!(!route.is_backup);
}

#[test]
fn test_route_computation_insufficient_relays() {
    let config = AdaptiveRoutingConfig {
        min_hops: 5,
        ..Default::default()
    };
    let router = AdaptiveRouter::with_config(config);

    // Only 2 relays
    for country in &["US", "DE"] {
        let peer_id = libp2p::identity::Keypair::generate_ed25519()
            .public()
            .to_peer_id();
        let info = create_relay_info(peer_id, country, true, true);
        router.register_relay(info);
    }

    let result = router.compute_route(None);
    assert!(result.is_err());
}

#[test]
fn test_censorship_detection_patterns() {
    // Test failure types that indicate censorship
    assert!(FailureType::ConnectionReset.is_censorship_indicator());
    assert!(FailureType::ContentTampering.is_censorship_indicator());
    assert!(FailureType::DnsFailure.is_censorship_indicator());
    assert!(FailureType::TlsFailure.is_censorship_indicator());
    assert!(FailureType::Timeout.is_censorship_indicator());

    // These should NOT indicate censorship
    assert!(!FailureType::ConnectionRefused.is_censorship_indicator());
    assert!(!FailureType::HttpError(404).is_censorship_indicator());
    assert!(!FailureType::NetworkError.is_censorship_indicator());
}

#[test]
fn test_path_health_success_tracking() {
    let from = libp2p::identity::Keypair::generate_ed25519()
        .public()
        .to_peer_id();
    let to = libp2p::identity::Keypair::generate_ed25519()
        .public()
        .to_peer_id();

    let mut health = PathHealth::new(from, to);

    // Record successes
    health.record_success(50, 1000);
    health.record_success(60, 2000);
    health.record_success(55, 1500);

    assert_eq!(health.consecutive_failures, 0);
    assert!(health.last_success.is_some());
    assert!(!health.should_avoid());
}

#[test]
fn test_path_health_failure_tracking() {
    let from = libp2p::identity::Keypair::generate_ed25519()
        .public()
        .to_peer_id();
    let to = libp2p::identity::Keypair::generate_ed25519()
        .public()
        .to_peer_id();

    let mut health = PathHealth::new(from, to);

    // Record failures
    health.record_failure(FailureType::Timeout);
    assert_eq!(health.consecutive_failures, 1);

    health.record_failure(FailureType::ConnectionReset);
    assert_eq!(health.consecutive_failures, 2);

    // Success resets counter
    health.record_success(50, 1000);
    assert_eq!(health.consecutive_failures, 0);
}

#[test]
fn test_censorship_status_progression() {
    let from = libp2p::identity::Keypair::generate_ed25519()
        .public()
        .to_peer_id();
    let to = libp2p::identity::Keypair::generate_ed25519()
        .public()
        .to_peer_id();

    let mut health = PathHealth::new(from, to);

    // Initially unknown
    assert_eq!(health.censorship_status, CensorshipStatus::Unknown);

    // Record many censorship-indicative failures
    for _ in 0..10 {
        health.record_failure(FailureType::ConnectionReset);
    }

    // Should be confirmed censorship
    assert_eq!(health.censorship_status, CensorshipStatus::Confirmed);
    assert!(health.should_avoid());
}

#[test]
fn test_blocked_path_management() {
    let router = AdaptiveRouter::new();

    let from = libp2p::identity::Keypair::generate_ed25519()
        .public()
        .to_peer_id();
    let to = libp2p::identity::Keypair::generate_ed25519()
        .public()
        .to_peer_id();

    // Initially empty
    assert!(router.blocked_paths().is_empty());

    // Block a path (simulating confirmed censorship)
    router.block_path(from, to);

    assert_eq!(router.blocked_paths().len(), 1);

    // Unblock
    router.unblock_path(from, to);
    assert!(router.blocked_paths().is_empty());
}

#[test]
fn test_routing_success_recording() {
    let config = AdaptiveRoutingConfig {
        min_hops: 2,
        require_geo_diversity: false,
        ..Default::default()
    };
    let router = AdaptiveRouter::with_config(config);

    for country in &["US", "DE", "JP"] {
        let peer_id = libp2p::identity::Keypair::generate_ed25519()
            .public()
            .to_peer_id();
        let info = create_relay_info(peer_id, country, true, true);
        router.register_relay(info);
    }

    let route = router.compute_route(None).unwrap();
    router.record_success(&route.id, 100, 5000);

    let stats = router.stats();
    assert_eq!(stats.successful_deliveries, 1);
}

#[test]
fn test_routing_failure_and_failover() {
    let config = AdaptiveRoutingConfig {
        min_hops: 2,
        require_geo_diversity: false,
        auto_failover: true,
        ..Default::default()
    };
    let router = AdaptiveRouter::with_config(config);

    // Register plenty of relays for backup routes
    for country in &["US", "DE", "JP", "AU", "GB", "FR", "CA", "NL"] {
        let peer_id = libp2p::identity::Keypair::generate_ed25519()
            .public()
            .to_peer_id();
        let info = create_relay_info(peer_id, country, true, true);
        router.register_relay(info);
    }

    let route = router.compute_route(None).unwrap();

    // Record failure
    let backup = router.record_failure(&route.id, 0, FailureType::ConnectionReset);

    let stats = router.stats();
    assert!(stats.failed_deliveries > 0 || backup.is_some());
}

#[test]
fn test_routing_strategies() {
    // Low latency should prefer fewer hops
    let low_latency_config = AdaptiveRoutingConfig {
        strategy: RouteStrategy::LowLatency,
        min_hops: 2,
        max_hops: 5,
        require_geo_diversity: false,
        ..Default::default()
    };

    // High privacy should prefer more hops
    let high_privacy_config = AdaptiveRoutingConfig {
        strategy: RouteStrategy::HighPrivacy,
        min_hops: 2,
        max_hops: 5,
        require_geo_diversity: false,
        ..Default::default()
    };

    let fast_router = AdaptiveRouter::with_config(low_latency_config);
    let private_router = AdaptiveRouter::with_config(high_privacy_config);

    for country in &["US", "DE", "JP", "AU", "GB"] {
        let peer_id1 = libp2p::identity::Keypair::generate_ed25519()
            .public()
            .to_peer_id();
        let peer_id2 = libp2p::identity::Keypair::generate_ed25519()
            .public()
            .to_peer_id();

        fast_router.register_relay(create_relay_info(peer_id1, country, true, true));
        private_router.register_relay(create_relay_info(peer_id2, country, true, true));
    }

    let fast_route = fast_router.compute_route(None).unwrap();
    let private_route = private_router.compute_route(None).unwrap();

    // High privacy should have more or equal hops
    assert!(private_route.relays.len() >= fast_route.relays.len());
}

#[test]
fn test_relay_info_health_check() {
    let peer_id = libp2p::identity::Keypair::generate_ed25519()
        .public()
        .to_peer_id();

    let mut info = RelayInfo::new(peer_id);
    info.reputation = 0.8;
    info.load = 0.3;
    assert!(info.is_healthy());

    // Low reputation = unhealthy
    info.reputation = 0.1;
    assert!(!info.is_healthy());

    // High load = unhealthy
    info.reputation = 0.8;
    info.load = 0.95;
    assert!(!info.is_healthy());
}

#[test]
fn test_relay_fitness_scoring() {
    let peer_id = libp2p::identity::Keypair::generate_ed25519()
        .public()
        .to_peer_id();

    let mut info = RelayInfo::new(peer_id);
    info.avg_latency_ms = 50;
    info.load = 0.2;
    info.reputation = 0.9;

    let score = info.fitness_score(true);
    assert!(score > 0.5);

    // Higher load should reduce score
    info.load = 0.9;
    let lower_score = info.fitness_score(true);
    assert!(lower_score < score);
}

#[test]
fn test_adaptive_routing_full_workflow() {
    // Complete workflow: setup -> compute -> use -> fail -> failover

    let config = AdaptiveRoutingConfig {
        min_hops: 3,
        max_hops: 4,
        require_geo_diversity: false,
        auto_failover: true,
        ..Default::default()
    };
    let router = AdaptiveRouter::with_config(config);

    // 1. Register diverse relay network
    let countries = ["US", "DE", "JP", "AU", "GB", "FR", "CA", "NL", "SG", "BR"];
    let mut peer_ids = Vec::new();
    for country in &countries {
        let peer_id = libp2p::identity::Keypair::generate_ed25519()
            .public()
            .to_peer_id();
        peer_ids.push(peer_id);
        router.register_relay(create_relay_info(peer_id, country, true, true));
    }

    assert_eq!(router.relay_count(), 10);

    // 2. Compute initial route
    let route = router.compute_route(None).unwrap();
    assert!(route.relays.len() >= 3);
    assert!(route.health_score > 0.0);
    assert_eq!(router.active_route_count(), 1);

    // 3. Simulate successful delivery
    router.record_success(&route.id, 150, 10000);
    let stats = router.stats();
    assert_eq!(stats.successful_deliveries, 1);
    assert_eq!(stats.routes_computed, 1);

    // 4. Simulate failure with censorship indicator
    router.record_failure(&route.id, 0, FailureType::ConnectionReset);
    let stats = router.stats();
    assert_eq!(stats.failed_deliveries, 1);

    // 5. Check that failover was triggered
    assert!(stats.failovers_triggered > 0);
}

#[test]
fn test_avoid_regions_config() {
    use std::collections::HashSet;

    let mut avoid_regions = HashSet::new();
    avoid_regions.insert(GeoRegion::Asia);
    avoid_regions.insert(GeoRegion::MiddleEast);

    let config = AdaptiveRoutingConfig {
        min_hops: 2,
        max_hops: 3,
        require_geo_diversity: false,
        avoid_regions,
        ..Default::default()
    };
    let router = AdaptiveRouter::with_config(config);

    // Register relays including ones in avoided regions
    let regions = [
        ("US", GeoRegion::NorthAmerica),
        ("DE", GeoRegion::Europe),
        ("JP", GeoRegion::Asia), // Should be avoided
        ("AU", GeoRegion::Oceania),
        ("AE", GeoRegion::MiddleEast), // Should be avoided
    ];

    for (country, _region) in &regions {
        let peer_id = libp2p::identity::Keypair::generate_ed25519()
            .public()
            .to_peer_id();
        router.register_relay(create_relay_info(peer_id, country, true, true));
    }

    let route = router.compute_route(None).unwrap();

    // Route should not include Asia or Middle East
    for region in &route.regions {
        assert!(!matches!(region, GeoRegion::Asia | GeoRegion::MiddleEast));
    }
}

#[test]
fn test_path_health_rate_calculations() {
    let from = libp2p::identity::Keypair::generate_ed25519()
        .public()
        .to_peer_id();
    let to = libp2p::identity::Keypair::generate_ed25519()
        .public()
        .to_peer_id();

    let mut health = PathHealth::new(from, to);

    // 7 successes, 3 failures = 30% failure rate
    for _ in 0..7 {
        health.record_success(50, 1000);
    }
    for _ in 0..3 {
        health.record_failure(FailureType::Timeout);
    }

    let failure_rate = health.failure_rate();
    assert!(failure_rate > 0.25 && failure_rate < 0.35);
}

#[test]
fn test_routing_stats() {
    let router = AdaptiveRouter::new();
    let stats = router.stats();

    assert_eq!(stats.routes_computed, 0);
    assert_eq!(stats.successful_deliveries, 0);
    assert_eq!(stats.failed_deliveries, 0);
    assert_eq!(stats.failovers_triggered, 0);
    assert_eq!(stats.censorship_events, 0);
}
