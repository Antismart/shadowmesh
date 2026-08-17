# ShadowMesh

**A deploy target for static content that can't be taken down.**

[![CI](https://github.com/Antismart/shadowmesh/actions/workflows/ci.yml/badge.svg)](https://github.com/Antismart/shadowmesh/actions/workflows/ci.yml)
[![Rust](https://img.shields.io/badge/rust-1.70+-orange.svg)](https://www.rust-lang.org)
[![TypeScript](https://img.shields.io/badge/typescript-5.0+-blue.svg)](https://www.typescriptlang.org)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

ShadowMesh is a content-addressed, peer-to-peer CDN built on libp2p. You deploy a
static site, get back a [CID](https://docs.ipfs.tech/concepts/content-addressing/)
— a hash of its exact bytes — and it is then served by a network of independent
nodes. Because content is addressed by its hash, any node can serve it and the
hash guarantees you got the right bytes. No single host is authoritative, so no
single host can take the content down.

> **What this is and isn't.** ShadowMesh resists *content takedown* and
> *single-host censorship*. It is **not** an anonymity network and does **not**
> yet resist a national firewall. Read the **[Threat Model](THREAT-MODEL.md)**
> before relying on any privacy or censorship property — it states plainly what
> is defended today and what is not.

The repository also contains SSR hosting, WASM edge functions, and a state layer.
Those are real but deliberately **not** the focus — they live in
**[FUTURE.md](FUTURE.md)**. This README is about the one thing above.

---

## Why it can't be taken down

Deploy content once and it is reachable through multiple independent paths. Losing
or blocking any single one does not take your site offline, and the CID
guarantees integrity no matter which path you use:

| Access method | URL / path | Requires |
|---|---|---|
| **HTTP gateway** | `http://<gateway>:8081/content/<cid>` | Nothing (public) |
| **Any node** | `http://<any-node>:8081/content/<cid>` | Any running gateway |
| **`.shadow` name** | Resolve `myapp.shadow` via DHT | A connected node or SDK |
| **ENS** | `contenthash = shadow://<cid>` on a `.eth` name, via eth.limo | ENS name + eth.limo |
| **Local node** | `http://127.0.0.1:3030` | Running `node-runner` locally |
| **Browser (WebRTC)** | P2P fetch from peers, HTTP fallback | WASM SDK in browser |

Content integrity is enforced end to end: nodes verify a fetched manifest against
the requested CID and re-hash the reassembled bytes (BLAKE3) before storing or
re-serving them, so a malicious peer cannot poison a CID. (This holds for the
native hex-BLAKE3 content path; see the [Threat Model](THREAT-MODEL.md) for the
IPFS-CID caveat.)

**Live gateway:** the public gateway currently runs at
[http://62.171.189.140:8081/](http://62.171.189.140:8081/). It is one entry
point, not the network — see [Bootstrap & centralization](#bootstrap--centralization).

---

## Quick start

### Prerequisites

- Rust 1.70+ (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
- Node.js 18+ (only for the TypeScript SDK)
- An IPFS daemon (optional, for the storage backend)

### Build

```bash
git clone https://github.com/Antismart/shadowmesh.git
cd shadowmesh
cargo build --release          # gateway, node-runner, CLI
```

### Deploy a static site and fetch it

```bash
# Build the CLI
cargo build --release -p shadowmesh-cli
alias smesh="./target/release/shadowmesh-cli"

# Upload content — prints the CID
smesh upload ./index.html

# Fetch it back by CID, verified against the hash
smesh download <cid> -o ./out.html

# Or fetch over HTTP from any gateway
curl http://62.171.189.140:8081/content/<cid>
```

### Join the network as a node

Create `node-config.toml`:

```toml
[identity]
name = "your-node-name"

[storage]
data_dir = ".shadowmesh/data"
max_storage_bytes = 10737418240   # 10 GB

[network]
listen_addresses = ["/ip4/0.0.0.0/tcp/4001"]
bootstrap_nodes = [
  "/ip4/62.171.189.140/tcp/4001/p2p/12D3KooWCXDk6QR1zpAopogBPSFR977Az5gbrmcHjnFgY12UpBTb"
]
enable_dht = true

[dashboard]
enabled = true
host = "127.0.0.1"    # bind to loopback unless you set NODE_API_KEY
port = 3030
```

```bash
SHADOWMESH_CONFIG=node-config.toml cargo run -p node-runner
# Verify: curl http://127.0.0.1:3030/api/status
```

Your node dials the bootstrap peer, discovers others over the DHT, and starts
serving and replicating content.

### Use the SDK

```typescript
import { ShadowMeshClient } from '@shadowmesh/sdk';

const client = new ShadowMeshClient({ gatewayUrl: 'http://localhost:8081' });

const { cid } = await client.deploy(new TextEncoder().encode('Hello, ShadowMesh!'));
const bytes = await client.retrieve(cid);   // verified against the CID
```

---

## How it works

```
   deploy                fetch (any path)
     │                         │
     ▼                         ▼
┌──────────┐   CID     ┌───────────────┐      ┌─────────────┐
│  client  │──────────▶│    gateway    │◀────▶│  P2P nodes  │
│ (CLI/SDK)│           │  (HTTP + DHT) │ libp2p│ (node-runner)│
└──────────┘           └───────────────┘ Noise└─────────────┘
                              ▲                       │
                       WebRTc │                 DHT / GossipSub
                              │                 replication
                        ┌───────────┐
                        │  browser  │
                        │ (WASM SDK)│
                        └───────────┘
```

- **Content addressing** — files are chunked and hashed with BLAKE3; the CID *is*
  the hash, so integrity is verifiable by anyone.
- **P2P delivery** — nodes announce content to a Kademlia DHT and GossipSub, and
  pull-replicate it from each other, so any node holding a CID can serve it.
- **Encrypted transport** — peer connections are authenticated and encrypted with
  libp2p Noise; browsers connect over WebRTC DataChannels.

Core components:

| Component | Role | Port |
|---|---|---|
| `protocol` | Core P2P protocol library (DHT, fragments, crypto, routing) | — |
| `gateway` | HTTP API bridging browsers to the mesh | 8081 |
| `node-runner` | Full P2P node with dashboard | 3030 |
| `cli` | Node & content management (`shadowmesh-cli`) | — |
| `sdk` | TypeScript client | — |
| `sdk-browser` | WASM browser SDK with WebRTC | — |

(Server-side extras — SSR, WASM edge, state layer, framework adapters — are in
[FUTURE.md](FUTURE.md).)

---

## Bootstrap & centralization

**This is the honest limitation, stated up front.** Content *already in the mesh*
is served peer-to-peer and survives the loss of any single node. But **joining**
the mesh today depends on reaching a bootstrap peer, and the network currently
publishes one well-known host. A censor who blocks that IP prevents *new* nodes in
their region from finding peers; already-connected nodes keep working.

The mechanisms to fix this exist in the code (DNS-seed discovery, multi-peer
bootstrap, rendezvous, relay) and just need diverse infrastructure. Removing this
single point is the top item on the [roadmap](THREAT-MODEL.md#8-roadmap-to-close-the-gaps).

Configure bootstrap peers three ways (highest priority first):

```bash
# 1. Environment variable (comma-separated multiaddrs)
export SHADOWMESH_BOOTSTRAP_NODES="/ip4/203.0.113.10/tcp/4001/p2p/12D3KooW..."
```

```toml
# 2. Config file — node-config.toml [network] bootstrap_nodes = [...]
#                  gateway config.toml [p2p]  bootstrap_peers = [...]
```

3. **LAN** — with `enable_mdns = true` (default), nodes on the same network find
   each other automatically; no bootstrap needed.

---

## Running in production

Before exposing a node or gateway to the internet:

- **Set an API key.** `SHADOWMESH_API_KEYS` (gateway) / `NODE_API_KEY` (node).
  The node refuses to start if bound to a non-loopback interface without a key.
- **Scope your keys.** Use the `key:namespace1|namespace2` grammar so a key can
  only touch its own deployments; bare keys are admin-scoped.
- **Set CORS origins** — replace `*` with your domains in the gateway `[security]`
  section.
- **Put the gateway behind TLS** (a reverse proxy) and a domain name.

See the [Hosting Guide](docs/hosting-guide.md) and
[Node Runner Guide](docs/node-runner-guide.md) for full setup.

---

## Testing

```bash
cargo test --workspace
cargo test -p protocol       # or gateway, node-runner
```

---

## Documentation

- **[Threat Model](THREAT-MODEL.md)** — what is and isn't defended (read this first)
- **[Future & experimental features](FUTURE.md)** — SSR, WASM edge, state layer, roadmap
- [Protocol Specification](docs/protocol-spec.md)
- [Architecture Guide](docs/architecture.md)
- [API Reference](docs/api-reference.md)
- [SDK Guide](docs/sdk-guide.md)
- [Hosting Guide](docs/hosting-guide.md)
- [Contributing](CONTRIBUTING.md)

---

## Contributing

Contributions welcome — see [CONTRIBUTING.md](CONTRIBUTING.md). Please branch,
run `cargo test --workspace`, and use conventional commits.

## License

MIT — see [LICENSE](LICENSE).

## Acknowledgments

Built on [libp2p](https://libp2p.io/) (Kademlia DHT, GossipSub, Noise, Yamux,
Relay, AutoNAT, DCUtR), [IPFS/Kubo](https://ipfs.io/),
[BLAKE3](https://github.com/BLAKE3-team/BLAKE3),
[ChaCha20-Poly1305](https://datatracker.ietf.org/doc/html/rfc8439),
[x25519-dalek](https://github.com/dalek-cryptography/x25519-dalek),
[Tokio](https://tokio.rs/), [Axum](https://github.com/tokio-rs/axum), and
[Wasmtime](https://wasmtime.dev/).

---

<p align="center">Built for a web that stays online.</p>
