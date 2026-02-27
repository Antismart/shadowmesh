# 🌐 ShadowMesh

**A privacy-first decentralized CDN powered by libp2p**

[![CI](https://github.com/Antismart/shadowmesh/actions/workflows/ci.yml/badge.svg)](https://github.com/Antismart/shadowmesh/actions/workflows/ci.yml)
[![CD](https://github.com/Antismart/shadowmesh/actions/workflows/cd.yml/badge.svg)](https://github.com/Antismart/shadowmesh/actions/workflows/cd.yml)
[![Rust](https://img.shields.io/badge/rust-1.70+-orange.svg)](https://www.rust-lang.org)
[![TypeScript](https://img.shields.io/badge/typescript-5.0+-blue.svg)](https://www.typescriptlang.org)
[![WebRTC](https://img.shields.io/badge/webrtc-enabled-brightgreen.svg)](docs/webrtc-setup.md)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

ShadowMesh is a decentralized content delivery network that combines IPFS-style content addressing with onion routing for enhanced privacy. Content is fragmented, encrypted, and distributed across a peer-to-peer network, making it resilient and censorship-resistant.

## ✨ Features

- **🔐 Privacy-First**: Onion routing ensures no single node knows both the requester and the content
- **📦 Content Fragmentation**: Large files are split into encrypted chunks for parallel delivery
- **🌍 Decentralized**: No central servers - content is served by a network of peers
- **⚡ High Performance**: BLAKE3 hashing, ChaCha20-Poly1305 encryption, and multi-path routing
- **🔄 Automatic Replication**: Content is replicated across multiple nodes for reliability
- **📊 Bandwidth Tracking**: Built-in metrics and rate limiting for fair resource usage
- **🌐 WebRTC Support**: Browser-to-node P2P connections via WebRTC DataChannels
- **📱 Browser SDK**: WASM-based client for direct browser integration

## 🏗️ Architecture

```
┌──────────────────────────────────────────────────────────────────────┐
│                           ShadowMesh                                  │
├──────────────────────────────────────────────────────────────────────┤
│                                                                       │
│  ┌─────────────┐   ┌─────────────┐   ┌─────────────┐                │
│  │   Browser   │   │   Gateway   │   │ Node Runner │                │
│  │  (WASM SDK) │   │  (HTTP API) │   │  (P2P Node) │                │
│  └──────┬──────┘   └──────┬──────┘   └──────┬──────┘                │
│         │                 │                 │                        │
│         │    WebRTC      ┌┴─────────────────┤                        │
│         └───────────────►│   Signaling     │◄─── TCP/libp2p         │
│                          └─────────────────┘                         │
│                                  │                                   │
│  ┌───────────────────────────────┴───────────────────────────────┐  │
│  │                      Protocol Layer                            │  │
│  │  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐             │  │
│  │  │ Crypto  │ │   DHT   │ │ Routing │ │ WebRTC  │             │  │
│  │  └─────────┘ └─────────┘ └─────────┘ └─────────┘             │  │
│  │  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐             │  │
│  │  │Fragment │ │Replicate│ │Bandwidth│ │Signaling│             │  │
│  │  └─────────┘ └─────────┘ └─────────┘ └─────────┘             │  │
│  └───────────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────────────┘
```

## 🚀 Quick Start

### Prerequisites

- Rust 1.70+ (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
- Node.js 18+ (for SDK)
- IPFS daemon (optional, for storage backend)

### Installation

```bash
# Clone the repository
git clone https://github.com/Antismart/shadowmesh.git
cd shadowmesh

# Build all Rust components (gateway, node-runner, CLI)
cargo build --release

# (Optional) Build the TypeScript SDK
cd sdk && npm install && npm run build && cd ..
```

### Running a Node

```bash
# Start the gateway (HTTP API on port 8081 by default)
cargo run -p gateway

# Start a node runner (P2P node with dashboard on port 3030)
cargo run -p node-runner
```

### Using the SDK

```typescript
import { ShadowMeshClient, ContentStorage } from '@shadowmesh/sdk';

// Initialize client
const client = new ShadowMeshClient({
  gatewayUrl: 'http://localhost:3000',
});

// Deploy content
const content = new TextEncoder().encode('Hello, ShadowMesh!');
const { cid } = await client.deploy(content);
console.log(`Content deployed: ${cid}`);

// Retrieve content
const retrieved = await client.retrieve(cid);
console.log(new TextDecoder().decode(retrieved));
```

### Browser SDK (WebRTC)

```javascript
import init, { ShadowMeshClient, ClientConfig } from '@shadowmesh/browser';

// Initialize WASM
await init();

// Configure client
const config = new ClientConfig('wss://gateway.example.com/ws/signaling');
config.setGatewayUrl('https://gateway.example.com');
config.setMaxPeers(5);

// Create and connect
const client = new ShadowMeshClient(config);
await client.connect();

// Fetch content via P2P (with HTTP fallback)
const data = await client.fetch('bafybeigdyrzt5sfp7...');
console.log('Content:', new TextDecoder().decode(data));

// Disconnect
client.disconnect();
```

See [WebRTC Setup Guide](docs/webrtc-setup.md) for detailed configuration.

### CLI Usage

The CLI is a Rust binary. Build it with `cargo build --release -p shadowmesh-cli`, then use the binary at `target/release/shadowmesh-cli` (or set up an alias):

```bash
# Build the CLI
cargo build --release -p shadowmesh-cli

# Alias for convenience (add to .bashrc / .zshrc)
alias smesh="./target/release/shadowmesh-cli"

# Upload content
smesh upload ./myfile.txt

# Check node status
smesh status

# Download content
smesh download <cid> -o ./output.txt

# List connected peers
smesh peers
```

## 📦 Components

| Component | Description | Port |
|-----------|-------------|------|
| `protocol` | Core P2P protocol library | - |
| `gateway` | HTTP API server with signaling | 8081 |
| `node-runner` | Full P2P node with dashboard | 3030 |
| `cli` | Node management CLI (`shadowmesh-cli`) | - |
| `sdk` | TypeScript client library | - |
| `sdk-browser` | WASM browser SDK with WebRTC | - |
| `benchmarks` | Performance benchmarks | - |

## 🔧 Configuration

### Gateway Configuration

Edit `gateway/config.toml`:

```toml
[server]
host = "0.0.0.0"
port = 8081
workers = 4

[cache]
max_size_mb = 500
ttl_seconds = 3600

[rate_limit]
enabled = true
requests_per_second = 100
burst_size = 200

[security]
cors_enabled = true
allowed_origins = ["https://yourdomain.com"]  # no wildcards in production
max_request_size_mb = 10

[p2p]
enabled = true
tcp_port = 4001
bootstrap_peers = []
enable_mdns = true
```

### Node Runner Configuration

Create `node-config.toml` (or copy `node-runner/node-config.example.toml`):

```toml
[identity]
name = "my-node"

[storage]
data_dir = ".shadowmesh/data"
max_storage_bytes = 10737418240  # 10 GB

[network]
listen_addresses = ["/ip4/0.0.0.0/tcp/4001"]
bootstrap_nodes = []
max_peers = 50
enable_mdns = true
enable_dht = true
replication_factor = 3

[dashboard]
enabled = true
host = "127.0.0.1"
port = 3030
```

## Bootstrap Configuration

Nodes need at least one reachable peer to join the network. There are three ways to configure bootstrap peers:

### Config file (recommended)

For the **node-runner**, add bootstrap addresses to `node-config.toml` under `[network]`:

```toml
[network]
bootstrap_nodes = [
    "/ip4/203.0.113.10/tcp/4001/p2p/12D3KooWGPAjDTsHkY39arQUSFKTdSRaKQFDhqiND5nZR2iLdtr7"
]
```

For the **gateway**, add them to `gateway/config.toml` under `[p2p]`:

```toml
[p2p]
enabled = true
bootstrap_peers = [
    "/ip4/203.0.113.10/tcp/4001/p2p/12D3KooWGPAjDTsHkY39arQUSFKTdSRaKQFDhqiND5nZR2iLdtr7"
]
```

Note: the field name is `bootstrap_nodes` for node-runner and `bootstrap_peers` for gateway.

### Environment variable

Set `SHADOWMESH_BOOTSTRAP_NODES` with a comma-separated list of multiaddrs:

```bash
export SHADOWMESH_BOOTSTRAP_NODES="/ip4/203.0.113.10/tcp/4001/p2p/12D3KooW...,/ip4/198.51.100.5/tcp/4001/p2p/12D3KooW..."
```

This works for both node-runner and gateway and overrides the config file values.

### LAN discovery (automatic)

When `enable_mdns = true` (the default), nodes on the same local network discover each other automatically via mDNS. No bootstrap configuration is needed for LAN-only setups.

## Production Deployment Checklist

Before running ShadowMesh in production, verify these items:

- **Set API keys for authentication.** Set the `SHADOWMESH_API_KEYS` environment variable to restrict who can upload and manage content. Without this, your node's API is open to anyone who can reach it.
- **Configure bootstrap peers.** Add at least one stable bootstrap address to your config so nodes can rejoin the network after restarts. See the [Hosting Guide](docs/hosting-guide.md) for details.
- **Set proper CORS origins.** Replace wildcard (`*`) origins with specific domains in the gateway's `[security]` section. Override at runtime with `SHADOWMESH_SECURITY_ALLOWED_ORIGINS`.
- **Enable telemetry and monitoring.** Set `[telemetry] enabled = true` in the gateway config and point `otlp_endpoint` at your collector. Scrape Prometheus metrics from `/metrics/prometheus`.
- **Pin Docker image tags.** Use a specific image digest or version tag in your `docker-compose.yml` and Dockerfiles instead of `latest`.
- **Configure hot-reload (gateway).** The gateway watches its config file for changes and reloads automatically. You can also send `SIGHUP` or call `POST /api/admin/reload`. No restart required for rate-limit, cache, or CORS changes.

See the full [Hosting Guide](docs/hosting-guide.md) and [Node Runner Guide](docs/node-runner-guide.md) for detailed setup instructions.

## 📚 Documentation

- [Protocol Specification](docs/protocol-spec.md)
- [Architecture Guide](docs/architecture.md)
- [API Reference](docs/api-reference.md)
- [SDK Guide](docs/sdk-guide.md)
- [WebRTC Setup Guide](docs/webrtc-setup.md)
- [Deployment Guide](docs/deployment.md)
- [Monitoring Runbook](docs/runbooks/monitoring-alerting.md)
- [Contributing](CONTRIBUTING.md)

## 🧪 Testing

```bash
# Run all tests
cargo test --workspace

# Run specific component tests
cargo test -p protocol
cargo test -p gateway
cargo test -p node-runner

# Run with verbose output
cargo test --workspace -- --nocapture

# Run benchmarks
cargo bench -p benchmarks
```

## 📊 Benchmarks

| Operation | Throughput | Latency (p99) |
|-----------|------------|---------------|
| Content Hash (1MB) | 2.5 GB/s | < 1ms |
| Encrypt (1MB) | 1.2 GB/s | < 2ms |
| Fragment (1MB) | 3.0 GB/s | < 1ms |
| DHT Lookup | - | < 50ms |

## 🛣️ Roadmap

- [x] Core protocol implementation
- [x] Content fragmentation & encryption
- [x] DHT-based content discovery
- [x] Peer discovery & scoring
- [x] Bandwidth tracking & rate limiting
- [x] HTTP Gateway API
- [x] TypeScript SDK
- [x] Integration tests
- [x] WebRTC transport support
- [ ] Mobile SDK (React Native)
- [ ] Incentive layer with token rewards
- [ ] Browser extension
- [ ] IPFS pinning service integration

## 🤝 Contributing

We welcome contributions! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

```bash
# Fork the repo
# Create a feature branch
git checkout -b feature/amazing-feature

# Make your changes
# Run tests
cargo test --workspace

# Commit with conventional commits
git commit -m "feat: add amazing feature"

# Push and create PR
git push origin feature/amazing-feature
```

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 🙏 Acknowledgments

- [libp2p](https://libp2p.io/) - P2P networking stack
- [IPFS](https://ipfs.io/) - Content addressing inspiration
- [BLAKE3](https://github.com/BLAKE3-team/BLAKE3) - Fast cryptographic hashing
- [ChaCha20-Poly1305](https://datatracker.ietf.org/doc/html/rfc8439) - AEAD encryption

---

<p align="center">
  Built with ❤️ for a more private and decentralized web
</p>
