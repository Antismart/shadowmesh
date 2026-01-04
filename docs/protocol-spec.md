# ShadowMesh Protocol Specification v0.1

## Content Addressing
- Use IPLD/DAG for content structure
- Blake3 for hashing (faster than SHA-256)
- Each file → Merkle DAG with 256KB chunks

## Network Layer
- libp2p for P2P communication
- QUIC transport (UDP-based, faster than TCP)
- Kademlia DHT for peer discovery
- GossipSub for network announcements

## Routing Protocol
- Multi-path routing (3-5 hops default)
- Encrypted onion routing for privacy
- Adaptive path selection based on latency
- Fragment reassembly at client

## Incentive Layer (Future)
- Track bandwidth served per node
- Cryptographic proofs of delivery
- Token rewards (post-hackathon)
