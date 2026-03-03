# Cross-Network P2P Discovery & NAT Traversal

How ShadowMesh nodes find and connect to each other across different networks.

## The Problem

Nodes on the same LAN discover each other instantly via mDNS. But nodes on different networks (home, office, mobile) are blocked by NAT routers that reject unsolicited inbound connections.

```
Network A (Home)                    Network B (Office)
┌─────────────────┐                ┌─────────────────┐
│  Node 1         │                │  Node 2         │
│  192.168.1.50   │                │  192.168.0.100  │
│                 │                │                 │
│  mDNS: works    │                │  mDNS: works    │
│  (finds LAN)    │                │  (finds LAN)    │
└────────┬────────┘                └────────┬────────┘
         │                                  │
         ▼                                  ▼
┌─────────────────┐                ┌─────────────────┐
│  NAT/Router     │                │  NAT/Router     │
│  Blocks inbound │      ???       │  Blocks inbound │
└────────┬────────┘                └────────┬────────┘
         │                                  │
         └──────────── INTERNET ────────────┘
                    Can't connect!
```

## The Solution: 3-Layer Approach

ShadowMesh uses three layers working together to solve this.

### Layer 1: Bootstrap Nodes (Discovery)

Bootstrap nodes are publicly reachable peers that act as entry points to the network. Every node connects to at least one bootstrap peer on startup to join the DHT and become discoverable.

Configure bootstrap nodes in `node-config.toml`:

```toml
[network]
bootstrap_nodes = [
  "/ip4/154.159.252.18/tcp/4001/p2p/12D3KooWBkrRBouo9g7eftTkUbRqEiwuvjoDGHecTb9ouJkMiGTd"
]
```

Bootstrap peers can also be discovered via DNS seed domains (TXT records containing multiaddrs) or the `SHADOWMESH_BOOTSTRAP_NODES` environment variable.

**Implementation**: `protocol/src/bootstrap.rs` — merges config entries, env var, and DNS seeds into a deduplicated peer list.

### Layer 2: Kademlia DHT (Peer Exchange)

Once connected to a bootstrap node, the Kademlia distributed hash table propagates peer information across the network. Nodes learn about each other without needing direct connections.

When a new peer connects, the Identify protocol exchanges listen addresses, and those addresses are added to the Kademlia routing table. This means joining one bootstrap node eventually leads to discovering the entire network.

**Implementation**: `protocol/src/node.rs` — `ShadowBehaviour.kademlia` is a `Kademlia<MemoryStore>` that bootstraps on startup and maintains the routing table as peers come and go.

### Layer 3: NAT Traversal (Connectivity)

Once peers know about each other via DHT, they still need to establish actual connections through NAT. ShadowMesh uses four mechanisms, tried in order:

#### 1. UPnP — Automatic Port Forwarding

If the router supports UPnP, the node automatically requests a port mapping. This makes the node directly reachable without manual configuration.

**Implementation**: `ShadowBehaviour.upnp` — `upnp::tokio::Behaviour` handles gateway discovery and mapping requests automatically.

#### 2. Direct Connection

If one peer has a public IP (or UPnP succeeded), direct TCP connections work immediately. The Identify protocol ensures peers know each other's external addresses.

#### 3. DCUtR — Hole Punching via Relay

When both peers are behind NAT, DCUtR (Direct Connection Upgrade through Relay) coordinates a simultaneous connection attempt through a relay node. Both peers open outbound connections at the same time, which their NAT routers allow. This works for ~70% of NAT combinations.

**Implementation**: `ShadowBehaviour.dcutr` — `dcutr::Behaviour` handles the coordination protocol automatically when a relayed connection exists.

#### 4. Circuit Relay — Full Relay (Fallback)

When hole punching fails, traffic flows through a relay node. This always works but adds latency since all data passes through the relay. ShadowMesh automatically reserves a relay slot when AutoNAT detects that the node is behind NAT.

**Implementation**: `ShadowBehaviour.relay_client` and `ShadowBehaviour.relay_server` — the relay client requests reservations; the relay server accepts them with configurable limits.

## Architecture Overview

```
┌──────────────────────────────────────────────────────────────┐
│                    BOOTSTRAP LAYER                            │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐          │
│  │ Bootstrap 1 │  │ Bootstrap 2 │  │ Bootstrap 3 │          │
│  │ (Public IP) │  │ (Public IP) │  │ (Public IP) │          │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘          │
└─────────┼────────────────┼────────────────┼──────────────────┘
          │                │                │
          ▼                ▼                ▼
┌──────────────────────────────────────────────────────────────┐
│                    KADEMLIA DHT                               │
│                                                               │
│  Node A ──────────── Peer Exchange ──────────── Node B       │
│    │                                              │           │
│    └──────── Both learn about each other ────────┘           │
└──────────────────────────────────────────────────────────────┘
          │                                        │
          ▼                                        ▼
┌──────────────────────────────────────────────────────────────┐
│                    NAT TRAVERSAL                              │
│                                                               │
│  Step 1: UPnP port mapping (automatic, if router supports)   │
│                         │                                     │
│                         ▼ (if unavailable)                    │
│  Step 2: Direct TCP connection (works if one peer is public) │
│                         │                                     │
│                         ▼ (if fails)                          │
│  Step 3: DCUtR hole punching via relay coordination           │
│          (works ~70% of NAT combinations)                     │
│                         │                                     │
│                         ▼ (if fails)                          │
│  Step 4: Full relay (always works, higher latency)            │
│          Node A <-> Relay <-> Node B                          │
└──────────────────────────────────────────────────────────────┘
```

## AutoNAT Detection

AutoNAT probes determine whether the node is publicly reachable. Other peers attempt to dial back on the node's reported addresses. Based on the result:

- **Public** — Node is directly reachable. No relay needed.
- **Private** — Node is behind NAT. Automatically requests relay reservations from bootstrap peers.
- **Unknown** — Not enough peers have probed yet. Treated as private until confirmed.

**Implementation**: `ShadowBehaviour.autonat` — on `StatusChanged` to `Private`, the event handler iterates over bootstrap peers and calls `swarm.listen_on()` with relay circuit addresses.

## Relay Bootstrap Mode

Any node with a public IP can act as a relay for NAT-ed peers. Enable relay mode in config:

```toml
[network]
relay_mode = true
```

Relay mode configures generous limits:

| Setting | Default | Relay Mode |
|---------|---------|------------|
| Max reservations | 128 | 512 |
| Reservations per peer | 4 | 8 |
| Max circuits | 16 | 256 |
| Circuits per peer | 4 | 8 |
| Max circuit bytes | 64 KiB | 1 MiB |

On startup, relay nodes print their full multiaddr for sharing:

```
RELAY BOOTSTRAP MODE
   Share these addresses with other nodes:
   /ip4/203.0.113.10/tcp/4001/p2p/12D3KooW...
```

## ShadowBehaviour Protocols

All protocols are composed into a single `ShadowBehaviour` struct using libp2p's `#[derive(NetworkBehaviour)]`:

| Protocol | Struct Field | Purpose |
|----------|-------------|---------|
| Kademlia | `kademlia` | DHT for content and peer discovery |
| GossipSub | `gossipsub` | Pub/sub messaging (naming, content, bootstrap) |
| mDNS | `mdns` | Automatic LAN peer discovery |
| Identify | `identify` | Peer address and protocol exchange |
| Relay Server | `relay_server` | Accept relay reservations from other peers |
| Relay Client | `relay_client` | Use relays for NAT traversal |
| AutoNAT | `autonat` | External address reachability probing |
| DCUtR | `dcutr` | Direct connection upgrade through relay |
| UPnP | `upnp` | Automatic router port forwarding |
| Content Req/Resp | `content_req_resp` | P2P content fragment serving |
| Rendezvous Client | `rendezvous_client` | Namespace-based peer discovery |

## Connection Flow

```
1. Node starts
   -> Loads persistent identity from disk (or generates new key)
   -> Listens on configured addresses

2. Connects to bootstrap peers
   -> Adds them to Kademlia routing table
   -> Triggers Kademlia bootstrap query

3. Identify exchange
   -> Learns peer listen addresses
   -> Adds addresses to Kademlia

4. AutoNAT probes run
   -> Other peers try to dial back
   -> Determines Public or Private status

5. If Private (behind NAT):
   -> Requests relay reservation from bootstrap peers
   -> Becomes reachable via relay circuit address

6. When connecting to another NAT-ed peer:
   a. Establish relayed connection
   b. DCUtR attempts hole punch for direct connection
   c. If hole punch succeeds: direct low-latency link
   d. If hole punch fails: continue using relay

7. mDNS runs continuously
   -> Discovers peers on same LAN instantly
   -> No bootstrap needed for local connections
```

## Key Files

| File | Description |
|------|-------------|
| `protocol/src/node.rs` | ShadowNode and ShadowBehaviour definition, transport setup |
| `protocol/src/bootstrap.rs` | Bootstrap peer resolution (config + env + DNS seeds) |
| `node-runner/src/p2p.rs` | P2P event loop with all event handlers |
| `node-runner/src/config.rs` | Network configuration (listen addresses, bootstrap, relay mode) |
| `gateway/src/p2p.rs` | Gateway P2P event loop |
