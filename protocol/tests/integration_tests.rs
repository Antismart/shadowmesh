//! Integration tests for the ShadowMesh Protocol
//! 
//! Tests verify the full protocol workflow including:
//! - Content fragmentation and reassembly
//! - Encryption and decryption flows
//! - DHT operations
//! - Replication management
//! - Peer discovery
//! - Bandwidth tracking

use shadowmesh_protocol::{
    // Fragment types
    FragmentManager, ContentMetadata,
    // Crypto types
    CryptoManager, OnionRouter, KeyDerivation,
    hash_content, verify_content_hash,
    // DHT types
    DHTManager, ContentRecord, ProviderInfo, ContentDHTMetadata,
    // Replication types
    ReplicationManager, ReplicationStatus, ReplicationHealth, ReplicationPriority,
    // Peer Discovery types
    PeerDiscovery, PeerInfo, PeerState, DiscoveryConfig,
    // Bandwidth types
    BandwidthTracker, RateLimiter,
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
        .map(|(i, chunk)| {
            FragmentManager::create_fragment(chunk.to_vec(), i as u32, total)
        })
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
    let (crypto, key) = CryptoManager::new_random();
    
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
    let node_keys = vec![
        [10u8; 32],
        [20u8; 32],
        [30u8; 32],
    ];
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
    let (crypto, _key) = CryptoManager::new_random();
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
    let fragment = FragmentManager::create_fragment(
        content.to_vec(),
        0,
        1,
    );
    
    // Encrypt fragment data
    let (crypto, _key) = CryptoManager::new_random();
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
    let (crypto, _key) = CryptoManager::new_random();
    let empty_content = b"";
    
    let encrypted = crypto.encrypt(empty_content).expect("Should encrypt empty");
    let decrypted = crypto.decrypt(&encrypted).expect("Should decrypt empty");
    
    assert_eq!(decrypted, empty_content.to_vec());
}

#[test]
fn test_large_content_encryption() {
    let (crypto, _key) = CryptoManager::new_random();
    let large_content = vec![0xAB; 1024 * 1024]; // 1MB
    
    let encrypted = crypto.encrypt(&large_content).expect("Should encrypt large");
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
