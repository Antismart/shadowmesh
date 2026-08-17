# Changelog

All notable changes to ShadowMesh are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

_Nothing yet._

## [0.1.0] — 2026-08-17

First tagged release. ShadowMesh is a content-addressed, peer-to-peer CDN: you
deploy static content, get a BLAKE3 CID, and it is served by a network of
independent nodes so no single host can take it down. See the
[Threat Model](THREAT-MODEL.md) for exactly what is and isn't defended.

### Added

- **Core protocol** (`protocol`) — content chunking and BLAKE3 hashing, libp2p
  transport (Kademlia DHT, GossipSub, Noise, Yamux, Relay, AutoNAT), pull-based
  replication, `.shadow` naming, and ENS `shadow://` contenthash resolution.
- **HTTP gateway** (`gateway`) — serves content by CID with a P2P/DHT fallback
  resolver; signaling for browser peers.
- **P2P node** (`node-runner`) — full libp2p node with storage, replication, and a
  local dashboard.
- **Clients** — TypeScript SDK (`@shadowmesh/sdk`), WASM browser SDK
  (`@shadowmesh/browser`) with WebRTC DataChannels, and the `shadowmesh-cli`.
- **Multi-path access** — content is reachable via the HTTP gateway, any node, a
  `.shadow` name, an ENS contenthash, a local node, or browser WebRTC P2P.
- **THREAT-MODEL.md** — an honest, code-grounded statement of the adversaries
  ShadowMesh does and does not defend against, with a gap-closing roadmap.
- **FUTURE.md** — experimental features and roadmap kept separate from the core:
  SSR hosting, WASM edge functions, the KV/secrets/blob state layer, and the
  framework adapters (all labelled `working-experimental` / `partial` / `planned`).

### Security

- **Content-integrity verification (#57).** Nodes now bind a fetched manifest to
  the requested CID and re-hash the reassembled bytes (hex-BLAKE3) before storing
  or re-announcing, rejecting mismatches; the Node SDK verifies every fetch and
  fragment. Prevents a malicious peer from poisoning a CID. Adds inbound
  fragment/content size caps.
- **Authenticated node API (#58).** The node fails fast if bound to a non-loopback
  interface without `NODE_API_KEY`; the CLI authenticates via `--api-key` /
  `NODE_API_KEY`.
- **Gateway multi-tenant isolation (#59).** API keys carry an identity;
  KV/secrets/blob/deploy handlers enforce namespace ownership (403 on
  cross-tenant), with namespace-count caps.
- **Authenticated onion handshake (#60).** The `zk_relay` CREATED response is
  Ed25519-signed and verified against the expected per-hop `PeerId` over a
  transcript binding both ephemeral keys and the circuit id; adds replay window,
  per-peer and global circuit caps, and circuit-id collision rejection. (Closes
  the handshake MITM; the multi-hop return path remains a known limitation.)
- **WebRTC channel confidentiality (#61).** The DataChannel key is derived from an
  authenticated ephemeral X25519 ECDH handshake (Ed25519-bound to peer identity)
  plus HKDF-SHA256, instead of from publicly observable peer ids.

### Changed

- **Unified content hashing (#62).** Standardized on hex-BLAKE3 across the
  TypeScript and Rust clients; removed the silent SHA-256 fallback and the path
  that fabricated a CID and reported success on upload failure (now fails loudly).
- **README refocused** on a single positioning — "a deploy target for static
  content that can't be taken down" — with breadth moved to [FUTURE.md](FUTURE.md).

### Known limitations

These are documented in [THREAT-MODEL.md](THREAT-MODEL.md) and tracked in the
issue tracker:

- IPFS-style `Qm`/`bafy` CIDs cannot be recomputed locally and are not yet
  content-verified on fetch.
- The onion multi-hop **return path** is not end-to-end functional; onion
  unlinkability should not be relied on yet.
- **Bootstrap is centralized** on a single documented host — a network-level
  censor can block new-node onboarding. Diversifying this is the top roadmap item.
- No traffic-analysis resistance (unpadded cells, no cover traffic).

[Unreleased]: https://github.com/Antismart/shadowmesh/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/Antismart/shadowmesh/releases/tag/v0.1.0
