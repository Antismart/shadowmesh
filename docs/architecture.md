# ShadowMesh Architecture

## Overview

ShadowMesh is built as a modular, layered architecture that separates concerns between networking, content handling, and client interfaces.

## System Architecture

```
                                  ┌──────────────────┐
                                  │   Applications   │
                                  │  (Web, Mobile)   │
                                  └────────┬─────────┘
                                           │
                    ┌──────────────────────┼──────────────────────┐
                    │                      │                      │
              ┌─────┴─────┐          ┌─────┴─────┐         ┌─────┴─────┐
              │    SDK    │          │  Gateway  │         │    CLI    │
              │(TypeScript)│         │  (HTTP)   │         │  (Rust)   │
              └─────┬─────┘          └─────┬─────┘         └─────┬─────┘
                    │                      │                      │
                    └──────────────────────┼──────────────────────┘
                                           │
                              ┌────────────┴────────────┐
                              │     Protocol Layer      │
                              │                         │
                              │  ┌─────┐ ┌─────┐ ┌───┐ │
                              │  │Crypt│ │Frag │ │DHT│ │
                              │  └─────┘ └─────┘ └───┘ │
                              │  ┌─────┐ ┌─────┐ ┌───┐ │
                              │  │Route│ │Repli│ │BW │ │
                              │  └─────┘ └─────┘ └───┘ │
                              └────────────┬────────────┘
                                           │
                              ┌────────────┴────────────┐
                              │    Network Layer        │
                              │       (libp2p)          │
                              │                         │
                              │  ┌─────┐ ┌─────┐ ┌───┐ │
                              │  │Noise│ │Yamux│ │TCP│ │
                              │  └─────┘ └─────┘ └───┘ │
                              └─────────────────────────┘
```

## Component Details

### 1. Protocol Layer (`/protocol`)

The core library implementing all ShadowMesh functionality.

#### Crypto Module (`crypto.rs`)
```rust
// Key components:
- CryptoManager: Symmetric encryption with ChaCha20-Poly1305
- OnionRouter: Multi-layer encryption for anonymous routing
- KeyDerivation: BLAKE3-based key derivation
- hash_content(): Content addressing
```

**Security Model:**
- 256-bit keys (ChaCha20-Poly1305)
- 96-bit nonces (randomly generated)
- BLAKE3 for hashing (64-bit output as hex)
- Onion encryption: Each hop adds a layer

#### Fragment Module (`fragments.rs`)
```rust
// Key components:
- FragmentManager: Split/reassemble content
- ContentFragment: Individual chunk with hash
- ContentManifest: Merkle tree of fragments
```

**Chunking Strategy:**
- Default chunk size: 256KB
- Each chunk is independently hashed
- Manifest contains ordered list of fragment hashes
- Supports parallel download and verification

#### DHT Module (`dht.rs`)
```rust
// Key components:
- DHTManager: Kademlia DHT operations
- ContentRecord: Metadata stored in DHT
- ProviderInfo: Peer providing content
```

**DHT Operations:**
- `announce(cid)`: Announce content availability
- `lookup(cid)`: Find content providers
- `get_closest_peers()`: Find peers near a key

#### Replication Module (`replication.rs`)
```rust
// Key components:
- ReplicationManager: Track and maintain replicas
- ReplicationStatus: Per-content replica count
- ReplicationHealth: Network-wide health metrics
```

**Replication Strategy:**
- Target replication factor: 3 (configurable)
- Priority-based replication (Critical > High > Normal > Low)
- Automatic repair when nodes go offline

#### Peer Discovery Module (`peer_discovery.rs`)
```rust
// Key components:
- PeerDiscovery: Find and manage peers
- PeerInfo: Per-peer metadata and scoring
- PeerState: Connection state machine
```

**Peer Scoring:**
- Initial score: 50/100
- +2 for successful transfer
- -5 for failed transfer
- Score decay over time
- Ban threshold: score < 10 or 10+ failures

#### Bandwidth Module (`bandwidth.rs`)
```rust
// Key components:
- BandwidthTracker: Monitor network usage
- RateLimiter: Token bucket rate limiting
- BandwidthStats: Per-direction statistics
```

### 2. Gateway (`/gateway`)

HTTP API server for content operations.

**Endpoints:**
```
GET  /                    - Landing page
GET  /health              - Health check
GET  /metrics             - Prometheus metrics
GET  /:cid                - Retrieve content
POST /upload              - Upload content
GET  /status/:cid         - Content status
```

**Features:**
- In-memory content cache with TTL
- Rate limiting per IP
- CORS support
- Request/response compression

### 3. Node Runner (`/node-runner`)

Full P2P node with management dashboard.

**Components:**
- Config management (TOML)
- Local fragment storage
- Metrics collection
- REST API for node management
- Web dashboard

**Dashboard Features:**
- Node status and uptime
- Peer connections
- Content catalog
- Bandwidth usage
- Storage utilization

### 4. SDK (`/sdk`)

TypeScript client library for applications.

**Modules:**
```typescript
// Client
ShadowMeshClient    - Main API client
ClientConfig        - Configuration options

// Types
ContentMetadata     - Content information
DeployResult        - Upload response
NetworkStats        - Network statistics

// Crypto
CryptoProvider      - Browser-compatible crypto
encrypt/decrypt     - Content encryption

// Storage
MemoryStorage       - In-memory storage
IndexedDBStorage    - Browser persistence

// Cache
ContentCache        - LRU cache with TTL
RequestDeduplicator - Prevent duplicate fetches
```

## Data Flow

### Content Upload

```
1. Client calls deploy(content)
2. Content is hashed (BLAKE3)
3. Content is encrypted (ChaCha20-Poly1305)
4. Encrypted content is fragmented (256KB chunks)
5. Each fragment is hashed
6. Manifest is created with fragment list
7. Fragments are uploaded to gateway
8. Gateway stores fragments locally
9. DHT announcement is made
10. CID is returned to client
```

### Content Retrieval

```
1. Client calls retrieve(cid)
2. Gateway receives request
3. Check local cache
4. If miss, query DHT for providers
5. Select best providers (by score/latency)
6. Download fragments in parallel
7. Verify fragment hashes
8. Reassemble fragments
9. Decrypt content
10. Cache and return to client
```

### Onion Routing (Privacy Mode)

```
1. Client builds route: [Node1, Node2, Node3, Gateway]
2. Content request is encrypted in layers:
   Layer 3: Encrypt(request, Gateway_key)
   Layer 2: Encrypt(Layer3, Node3_key)
   Layer 1: Encrypt(Layer2, Node2_key)
   Layer 0: Encrypt(Layer1, Node1_key)
3. Send to Node1
4. Node1 decrypts, forwards to Node2
5. Node2 decrypts, forwards to Node3
6. Node3 decrypts, forwards to Gateway
7. Gateway processes request
8. Response follows reverse path
```

## Security Considerations

### Threat Model

| Threat | Mitigation |
|--------|------------|
| Content interception | ChaCha20-Poly1305 encryption |
| Traffic analysis | Onion routing, fixed-size packets |
| Malicious nodes | Peer scoring, content verification |
| Sybil attacks | Proof-of-work for new peers (planned) |
| Replay attacks | Nonce in every encryption |
| Content tampering | BLAKE3 hash verification |

### Key Management

- Keys are never stored on disk in plaintext
- Master key derives per-purpose keys
- Ephemeral keys for each onion circuit
- Key rotation supported but not enforced

## Performance Optimizations

### Content Delivery

1. **Parallel Downloads**: Fragments fetched concurrently
2. **Local Caching**: Frequently accessed content cached
3. **Request Deduplication**: Multiple requests for same CID merged
4. **Preloading**: Predictive content fetching

### Network

1. **Connection Pooling**: Reuse peer connections
2. **Adaptive Routing**: Select paths by latency
3. **Bandwidth Throttling**: Fair resource sharing
4. **Peer Selection**: Score-based provider ranking

### Storage

1. **Deduplication**: Content-addressed storage
2. **LRU Eviction**: Automatic cache cleanup
3. **Lazy Loading**: Load fragments on demand
4. **Compression**: Optional content compression

## Scalability

### Horizontal Scaling

- Gateway can be load-balanced
- Multiple node-runners per deployment
- DHT naturally distributes load
- Replication improves availability

### Vertical Scaling

- Configurable cache sizes
- Adjustable replication factors
- Bandwidth limits per node
- Storage quotas

## Monitoring

### Metrics Exposed

```
# Gateway
shadowmesh_requests_total
shadowmesh_requests_success
shadowmesh_requests_error
shadowmesh_bytes_served
shadowmesh_cache_hits
shadowmesh_cache_misses

# Node Runner
shadowmesh_peers_connected
shadowmesh_content_stored
shadowmesh_bandwidth_in
shadowmesh_bandwidth_out
shadowmesh_storage_used
```

### Health Checks

- Gateway: `/health` endpoint
- Node Runner: Dashboard status
- DHT: Peer connectivity
- Replication: Health summary

## Future Architecture

### Planned Components

1. **Incentive Layer**: Token rewards for serving content
2. **Mobile SDK**: React Native bindings
3. **Browser Extension**: Direct P2P in browser
4. **IPFS Bridge**: Interoperability with IPFS
5. **CDN Integration**: Hybrid delivery mode
