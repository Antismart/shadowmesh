# WebRTC Setup Guide

This guide explains how to set up and use WebRTC connections in ShadowMesh for direct browser-to-peer communication.

## Overview

ShadowMesh supports WebRTC transport, allowing browsers to connect directly to the P2P network without going through the HTTP gateway. This provides:

- **Lower latency**: Direct peer-to-peer connections
- **Better privacy**: No centralized gateway sees all traffic
- **NAT traversal**: Works behind firewalls using STUN/TURN

```
┌─────────────┐     WebRTC      ┌─────────────┐      TCP       ┌─────────────┐
│   Browser   │ ←────────────→  │   Server    │ ←───────────→  │   Server    │
│   Client    │   (+ Noise)     │    Node     │   (existing)   │    Node     │
└─────────────┘                 └─────────────┘                └─────────────┘
```

## Components

### 1. Signaling Server

The gateway includes a WebSocket signaling server for WebRTC peer discovery and SDP exchange.

**Endpoint**: `ws://gateway:8081/signaling/ws`

**Features**:
- Peer announcement and discovery
- SDP offer/answer relay
- ICE candidate exchange
- Heartbeat-based connection tracking

### 2. Browser SDK (WASM)

The `sdk-browser` crate provides a WASM-based client for browsers.

### 3. Server-Side WebRTC

Server nodes can accept WebRTC connections alongside TCP using libp2p's WebRTC transport.

## Configuration

### Gateway Signaling Server

In `gateway/config.toml`:

```toml
[signaling]
enabled = true
max_connections = 1000
session_timeout_secs = 300
heartbeat_interval_secs = 30
```

### Node Runner WebRTC

In `node-config.toml`:

```toml
[webrtc]
enabled = true
port = 4002

[webrtc.stun]
servers = [
    "stun:stun.l.google.com:19302",
    "stun:stun1.l.google.com:19302"
]

# Optional TURN server for symmetric NAT
[webrtc.turn]
url = "turn:turn.example.com:3478"
username = "${TURN_USER}"
credential = "${TURN_PASS}"
```

## Browser SDK Usage

### Installation

```bash
npm install @shadowmesh/browser
```

### Basic Usage

```javascript
import init, { ShadowMeshClient, ClientConfig } from '@shadowmesh/browser';

// Initialize WASM module
await init();

// Create configuration
const config = new ClientConfig('wss://gateway.shadowmesh.network/signaling/ws');
config.addStunServer('stun:stun.l.google.com:19302');
config.setMaxPeers(5);
config.setTimeout(30000);

// Create client
const client = new ShadowMeshClient(config);

// Connect to network
await client.connect();
console.log('Connected as:', client.peerId);

// Discover peers
const peers = await client.discoverPeers();
console.log('Available peers:', peers);

// Fetch content (once implemented)
// const content = await client.fetch('QmXxx...');

// Disconnect
client.disconnect();
```

### React Integration

```jsx
import { useEffect, useState } from 'react';
import init, { ShadowMeshClient, ClientConfig } from '@shadowmesh/browser';

function useShadowMesh(signalingUrl) {
  const [client, setClient] = useState(null);
  const [connected, setConnected] = useState(false);
  const [peerId, setPeerId] = useState(null);

  useEffect(() => {
    let shadowClient = null;

    async function connect() {
      await init();

      const config = new ClientConfig(signalingUrl);
      shadowClient = new ShadowMeshClient(config);

      await shadowClient.connect();
      setClient(shadowClient);
      setConnected(true);
      setPeerId(shadowClient.peerId);
    }

    connect().catch(console.error);

    return () => {
      if (shadowClient) {
        shadowClient.disconnect();
      }
    };
  }, [signalingUrl]);

  return { client, connected, peerId };
}

function App() {
  const { client, connected, peerId } = useShadowMesh(
    'wss://gateway.shadowmesh.network/signaling/ws'
  );

  return (
    <div>
      <p>Status: {connected ? 'Connected' : 'Disconnected'}</p>
      <p>Peer ID: {peerId}</p>
    </div>
  );
}
```

## Signaling Protocol

### Message Types

#### Announce
Register presence on the network.

```json
{
  "type": "announce",
  "peer_id": "abc123...",
  "multiaddrs": [],
  "transports": ["webrtc"],
  "metadata": {}
}
```

#### Discover
Request list of available peers.

```json
{
  "type": "discover",
  "peer_id": "abc123...",
  "max_peers": 10,
  "transport_filter": "webrtc"
}
```

#### Peers
Response with available peers.

```json
{
  "type": "peers",
  "peers": [
    {
      "peer_id": "def456...",
      "multiaddrs": ["/ip4/..."],
      "transports": ["webrtc"],
      "last_seen": 1234567890
    }
  ],
  "total_peers": 42
}
```

#### Offer
WebRTC SDP offer.

```json
{
  "type": "offer",
  "from": "abc123...",
  "to": "def456...",
  "sdp": "v=0\r\n...",
  "session_id": "sess123"
}
```

#### Answer
WebRTC SDP answer.

```json
{
  "type": "answer",
  "from": "def456...",
  "to": "abc123...",
  "sdp": "v=0\r\n...",
  "session_id": "sess123"
}
```

#### IceCandidate
ICE candidate for NAT traversal.

```json
{
  "type": "ice_candidate",
  "from": "abc123...",
  "to": "def456...",
  "candidate": "candidate:1 1 UDP...",
  "sdp_mid": "0",
  "sdp_mline_index": 0,
  "session_id": "sess123"
}
```

#### Heartbeat
Keep connection alive.

```json
{
  "type": "heartbeat",
  "peer_id": "abc123...",
  "timestamp": 1234567890
}
```

## NAT Traversal

### STUN Servers

STUN (Session Traversal Utilities for NAT) helps discover public IP addresses.

Default servers:
- `stun:stun.l.google.com:19302`
- `stun:stun1.l.google.com:19302`

### TURN Servers

For symmetric NATs, TURN (Traversal Using Relays around NAT) provides relay fallback.

To set up your own TURN server using coturn:

```bash
# Install coturn
apt install coturn

# Configure /etc/turnserver.conf
realm=shadowmesh.network
server-name=turn.shadowmesh.network
listening-port=3478
tls-listening-port=5349
fingerprint
lt-cred-mech
user=shadowmesh:your-secret-password
total-quota=100
stale-nonce=600

# Start service
systemctl enable coturn
systemctl start coturn
```

## Connection Flow

```
Browser                    Signaling Server               Peer Node
   │                              │                           │
   │──────── Announce ──────────→│                           │
   │                              │                           │
   │──────── Discover ──────────→│                           │
   │←──────── Peers ─────────────│                           │
   │                              │                           │
   │──────── Offer ─────────────→│──────── Offer ──────────→│
   │                              │                           │
   │                              │←─────── Answer ──────────│
   │←──────── Answer ────────────│                           │
   │                              │                           │
   │←────── ICE Candidate ───────│←───── ICE Candidate ─────│
   │──────── ICE Candidate ─────→│────── ICE Candidate ────→│
   │                              │                           │
   │═══════════════════ Direct WebRTC Connection ═══════════│
```

## Troubleshooting

### Connection Failed

1. Check signaling server is reachable
2. Verify STUN servers are accessible
3. Check browser console for WebRTC errors
4. Try adding TURN server for symmetric NAT

### No Peers Found

1. Verify other nodes are running with WebRTC enabled
2. Check transport filter matches peer capabilities
3. Increase `max_peers` in discovery request

### ICE Failed

1. Check firewall allows UDP traffic
2. Verify STUN/TURN configuration
3. Try different STUN servers
4. Add TURN server as fallback

### Performance Issues

1. Monitor peer count (too many connections = high overhead)
2. Check network latency to signaling server
3. Verify content is being cached locally

## Security Considerations

1. **Signaling Server Authentication**: Consider adding API key authentication for production
2. **Peer Verification**: Verify peer IDs match expected format
3. **Content Verification**: Always verify content hashes after retrieval
4. **Rate Limiting**: Signaling server implements rate limiting per peer

## Metrics

The signaling server exposes metrics at `/signaling/stats`:

```json
{
  "total_connections": 150,
  "active_peers": 142,
  "messages_relayed": 50000,
  "pending_sessions": 5
}
```

## Building from Source

### Browser SDK

```bash
# Install wasm-pack
cargo install wasm-pack

# Build WASM package
cd sdk-browser
wasm-pack build --target web --out-dir pkg

# The output in pkg/ can be published to npm
```

### Server with WebRTC

```bash
# Build with WebRTC support
cargo build --release -p node-runner

# Run with WebRTC enabled
./target/release/node-runner --webrtc-port 4002
```
