# ShadowMesh Protocol Specification v0.1

## Overview

ShadowMesh is a privacy-first decentralized content delivery network (CDN) that combines content-addressed storage with onion routing for enhanced privacy.

## Design Goals

1. **Privacy**: No single node should know both the content requester and the content source
2. **Decentralization**: No central points of failure or control
3. **Performance**: Competitive with traditional CDNs for common use cases
4. **Resilience**: Content remains available even when nodes go offline
5. **Simplicity**: Easy to implement and reason about

## Content Addressing

### Hash Function

- **Algorithm**: BLAKE3
- **Output**: 64-character hexadecimal string (256 bits)
- **Properties**: Fast (2.5 GB/s on modern CPUs), cryptographically secure

```
content_id = BLAKE3(content_bytes)
Example: "a7ffc6f8bf1ed76651c14756a061d662f580ff4de43b49fa82d80a4b80f8434a"
```

### Content Structure

Content is represented as a Merkle DAG:

```
ContentManifest {
    content_hash: String,      // BLAKE3 hash of complete content
    fragments: Vec<String>,    // Ordered list of fragment hashes
    metadata: ContentMetadata,
}

ContentMetadata {
    name: String,              // Original filename
    size: u64,                 // Total size in bytes
    mime_type: String,         // MIME type
}

ContentFragment {
    index: u32,                // Position in sequence
    total_fragments: u32,      // Total number of fragments
    data: Vec<u8>,             // Fragment data
    hash: String,              // BLAKE3 hash of this fragment
}
```

### Fragmentation

- **Chunk Size**: 256 KB (configurable)
- **Strategy**: Sequential chunking with hash verification
- **Benefits**: Parallel download, deduplication, partial verification

## Encryption

### Symmetric Encryption

- **Algorithm**: ChaCha20-Poly1305 (AEAD)
- **Key Size**: 256 bits (32 bytes)
- **Nonce Size**: 96 bits (12 bytes)
- **Properties**: Fast, constant-time, AEAD provides authenticity

```
EncryptedData {
    nonce: Vec<u8>,       // 12-byte random nonce
    ciphertext: Vec<u8>,  // Encrypted data with auth tag
}
```

### Key Derivation

- **Algorithm**: BLAKE3 in KDF mode
- **Context**: Purpose-specific context string
- **Output**: 256-bit derived key

```
derived_key = BLAKE3_KDF(master_key, context)
```

### Onion Encryption

Multi-layer encryption for privacy:

```
For route [Node1, Node2, Node3, Gateway]:

Layer 3: E(request, key_gateway)
Layer 2: E(Layer3, key_node3)  
Layer 1: E(Layer2, key_node2)
Layer 0: E(Layer1, key_node1)

Each node decrypts one layer and forwards to next hop.
```

## Network Layer

### Transport Protocol

- **Primary**: libp2p over TCP
- **Encryption**: Noise protocol (XX handshake pattern)
- **Multiplexing**: Yamux
- **Future**: QUIC, WebRTC

### Peer Identity

- **Key Type**: Ed25519
- **Peer ID**: Multihash of public key
- **Format**: Base58 encoded (e.g., "12D3KooW...")

### Discovery

1. **Bootstrap Nodes**: Hardcoded initial peers
2. **Kademlia DHT**: Distributed peer discovery
3. **mDNS**: Local network discovery (optional)

## DHT Operations

### Content Announcement

When a node has content available:

```
ContentRecord {
    cid: String,                    // Content ID
    providers: Vec<ProviderInfo>,   // List of providers
    metadata: ContentDHTMetadata,   // Content metadata
    created_at: u64,                // Unix timestamp
    ttl_seconds: u64,               // Time to live
}

ProviderInfo {
    peer_id: String,                // Provider's peer ID
    addresses: Vec<String>,         // Multiaddresses
    reputation: u8,                 // 0-100 score
    online: bool,                   // Current status
    last_seen: u64,                 // Last verification
}
```

### Content Lookup

```
1. Client computes CID of desired content
2. Query DHT: GET /providers/{cid}
3. DHT returns list of ProviderInfo
4. Client selects providers based on:
   - Reputation score
   - Latency
   - Geographic proximity
5. Client requests content from selected providers
```

## Routing Protocol

### Multi-Path Routing

Content is retrieved via multiple paths for:
- Privacy (no single path knows everything)
- Performance (parallel download)
- Resilience (handle node failures)

### Path Selection

```
1. Get list of available relay nodes from DHT
2. Score nodes by:
   - Latency (lower is better)
   - Bandwidth capacity
   - Reputation score
   - Geographic diversity
3. Select 3-5 nodes for each path
4. Build onion-encrypted circuit
```

### Circuit Establishment

```
Client → Node1: Establish circuit
Node1 → Node2: Extend circuit
Node2 → Node3: Extend circuit
Node3 → Gateway: Extend circuit

Each extension is encrypted to the next hop only.
```

## Replication

### Replication Factor

- **Default**: 3 copies
- **Minimum**: 1 copy
- **Maximum**: 10 copies

### Priority Levels

```
Critical: 0 copies remaining - immediate replication
High:     1 copy remaining - urgent replication
Normal:   Below target - scheduled replication
Low:      At target - opportunistic replication
```

### Replication Strategy

```
1. Monitor fragment availability
2. When replica count drops below threshold:
   a. Find peers with capacity
   b. Select diverse peers (different regions/networks)
   c. Create replication tasks
   d. Execute in priority order
3. Verify replication success
4. Update DHT records
```

## Peer Scoring

### Score Calculation

```
Initial Score: 50/100

Positive Events:
  +2: Successful content transfer
  +1: Timely response to ping
  +0.5: Providing content to network

Negative Events:
  -5: Failed content transfer
  -3: Timeout on request
  -1: Slow response
  -10: Invalid content served

Score Bounds: [0, 100]
Ban Threshold: < 10 or 10+ consecutive failures
```

### Score Decay

```
score = score * 0.99 (applied hourly)
```

## Bandwidth Management

### Rate Limiting

Token bucket algorithm:
- Bucket size: Burst capacity
- Refill rate: Sustained bandwidth
- Per-peer limits: Fairness

```
RateLimiter {
    tokens: f64,
    max_tokens: f64,
    tokens_per_sec: f64,
    last_refill: Instant,
}
```

### Bandwidth Tracking

```
BandwidthStats {
    total_bytes: u64,
    period_bytes: u64,
    avg_bps: f64,
    peak_bps: u64,
    transfer_count: u64,
}
```

## Security Considerations

### Threat Model

| Threat | Mitigation |
|--------|------------|
| Content interception | End-to-end encryption |
| Traffic analysis | Onion routing, padding |
| Malicious nodes | Peer scoring, verification |
| Sybil attacks | Proof-of-work (planned) |
| Replay attacks | Nonces, timestamps |
| Eclipse attacks | Diverse peer selection |

### Content Verification

1. Fragment hash verification (BLAKE3)
2. Manifest hash verification
3. Signature verification (for signed content)

## Wire Formats

### Message Types

```
enum MessageType {
    ContentRequest,
    ContentResponse,
    PeerAnnounce,
    PeerQuery,
    Ping,
    Pong,
}
```

### Serialization

- **Format**: Protocol Buffers or JSON (configurable)
- **Compression**: Optional zstd compression

## Future Extensions

### Incentive Layer (v0.2)

- Token rewards for bandwidth provision
- Proof-of-delivery verification
- Staking for reputation

### WebRTC Transport (v0.2)

- Browser-to-browser communication
- No relay required for direct connections
- ICE/STUN/TURN support

### Content Pinning Service (v0.3)

- Persistent storage guarantees
- Payment for pinning
- SLA commitments

## Appendix

### Test Vectors

```
Content: "Hello, ShadowMesh!"
BLAKE3 Hash: "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"

Key: [0u8; 32]
Plaintext: "test"
Nonce: [0u8; 12]
Ciphertext: [178, 95, 124, ...] (varies by implementation)
```

### References

- [BLAKE3](https://github.com/BLAKE3-team/BLAKE3)
- [ChaCha20-Poly1305 RFC 8439](https://datatracker.ietf.org/doc/html/rfc8439)
- [libp2p Specifications](https://github.com/libp2p/specs)
- [Kademlia DHT](https://pdos.csail.mit.edu/~petar/papers/maymounkov-kademlia-lncs.pdf)
- [Tor Design](https://svn.torproject.org/svn/projects/design-paper/tor-design.pdf)
