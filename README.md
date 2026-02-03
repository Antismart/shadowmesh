# 🌐 ShadowMesh

**A privacy-first decentralized CDN powered by libp2p**

[![Rust](https://img.shields.io/badge/rust-1.70+-orange.svg)](https://www.rust-lang.org)
[![TypeScript](https://img.shields.io/badge/typescript-5.0+-blue.svg)](https://www.typescriptlang.org)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

ShadowMesh is a decentralized content delivery network that combines IPFS-style content addressing with onion routing for enhanced privacy. Content is fragmented, encrypted, and distributed across a peer-to-peer network, making it resilient and censorship-resistant.

## ✨ Features

- **🔐 Privacy-First**: Onion routing ensures no single node knows both the requester and the content
- **📦 Content Fragmentation**: Large files are split into encrypted chunks for parallel delivery
- **🌍 Decentralized**: No central servers - content is served by a network of peers
- **⚡ High Performance**: BLAKE3 hashing, ChaCha20-Poly1305 encryption, and multi-path routing
- **🔄 Automatic Replication**: Content is replicated across multiple nodes for reliability
- **📊 Bandwidth Tracking**: Built-in metrics and rate limiting for fair resource usage

## 🏗️ Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        ShadowMesh                                │
├─────────────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐             │
│  │   Gateway   │  │ Node Runner │  │     SDK     │             │
│  │  (HTTP API) │  │  (P2P Node) │  │ (TypeScript)│             │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘             │
│         │                │                │                     │
│         └────────────────┼────────────────┘                     │
│                          │                                      │
│  ┌───────────────────────┴───────────────────────┐             │
│  │              Protocol Layer                    │             │
│  │  ┌─────────┐ ┌─────────┐ ┌─────────┐         │             │
│  │  │ Crypto  │ │   DHT   │ │ Routing │         │             │
│  │  └─────────┘ └─────────┘ └─────────┘         │             │
│  │  ┌─────────┐ ┌─────────┐ ┌─────────┐         │             │
│  │  │Fragment │ │Replicate│ │Bandwidth│         │             │
│  │  └─────────┘ └─────────┘ └─────────┘         │             │
│  └───────────────────────────────────────────────┘             │
└─────────────────────────────────────────────────────────────────┘
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

# Build all components
cargo build --release

# Install SDK dependencies
cd sdk && npm install && npm run build && cd ..
```

### Running a Node

```bash
# Start the gateway (HTTP API on port 3000)
cargo run -p gateway

# Start a node runner (P2P node with dashboard)
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

### CLI Usage

```bash
# Deploy content
npx shadowmesh deploy ./myfile.txt

# Get content status
npx shadowmesh status <cid>

# Retrieve content
npx shadowmesh get <cid> --output ./output.txt

# View node statistics
npx shadowmesh stats
```

## 📦 Components

| Component | Description | Port |
|-----------|-------------|------|
| `protocol` | Core P2P protocol library | - |
| `gateway` | HTTP API server | 3000 |
| `node-runner` | Full P2P node with dashboard | 8080 |
| `sdk` | TypeScript client library | - |
| `benchmarks` | Performance benchmarks | - |

## 🔧 Configuration

### Gateway Configuration

Create `gateway.toml`:

```toml
[server]
host = "0.0.0.0"
port = 3000

[cache]
max_size_mb = 512
ttl_seconds = 3600

[rate_limit]
enabled = true
requests_per_second = 100

[cors]
enabled = true
origins = ["*"]
```

### Node Runner Configuration

Create `node-config.toml`:

```toml
[node]
data_dir = "./data"
max_storage_gb = 10

[network]
listen_addr = "/ip4/0.0.0.0/tcp/4001"
bootstrap_peers = []

[replication]
factor = 3
check_interval_secs = 300

[dashboard]
enabled = true
port = 8080
```

## 📚 Documentation

- [Protocol Specification](docs/protocol-spec.md)
- [Architecture Guide](docs/architecture.md)
- [API Reference](docs/api-reference.md)
- [SDK Guide](docs/sdk-guide.md)
- [Deployment Guide](docs/deployment.md)
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
