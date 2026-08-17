# ShadowMesh v0.1.0

**A deploy target for static content that can't be taken down.**

This is the first tagged release of ShadowMesh — a content-addressed,
peer-to-peer CDN built on libp2p. You deploy static content, get back a BLAKE3
CID, and it is served by a network of independent nodes. Because content is
addressed by its hash, any node can serve it and the hash guarantees you got the
right bytes — so no single host can take the content down.

## Highlights

- **Content addressing you can trust.** Files are chunked and hashed with BLAKE3;
  nodes verify fetched content against the requested CID before serving it, so a
  malicious peer cannot poison a hash.
- **Multiple independent access paths.** HTTP gateway, any node, `.shadow` naming,
  ENS `shadow://` contenthash, a local node, or browser-to-peer WebRTC — losing
  any one does not take your content offline.
- **Encrypted P2P transport.** libp2p Noise between nodes; an authenticated
  ephemeral X25519 handshake for browser DataChannels.
- **Clients for everywhere.** A Rust CLI, a TypeScript SDK, and a WASM browser SDK.

## Read this first: what this is and isn't

ShadowMesh resists **content takedown** and **single-host censorship**. It is
**not** an anonymity network and does **not** yet resist a national firewall. The
[**Threat Model**](THREAT-MODEL.md) states plainly what is defended today and what
is not — please read it before relying on any privacy or censorship property.

Notable current limitations (all tracked):

- **Bootstrap is centralized** on one documented host; a censor blocking that IP
  can stop new nodes in their region from joining. Fixing this is the top roadmap
  item.
- The **onion multi-hop return path** is not end-to-end functional yet; do not
  rely on onion unlinkability.
- IPFS-style `Qm`/`bafy` CIDs are not yet locally content-verified.

## Security hardening in this release

This release closes the findings from an internal soundness review:

| Area | What changed |
|---|---|
| Content integrity (#57) | Fetched content re-verified against its CID before storing/serving |
| Node API auth (#58) | Auth required on non-loopback binds; CLI bearer auth |
| Tenant isolation (#59) | Per-key identity; namespace ownership enforced on gateway state |
| Onion handshake (#60) | Ed25519-signed, verified against the expected per-hop identity |
| WebRTC key (#61) | Authenticated ephemeral ECDH; not derivable from public peer ids |
| CID consistency (#62) | Unified on hex-BLAKE3; removed fabricated-CID and hash-downgrade paths |

See the [CHANGELOG](CHANGELOG.md) for the full list.

## Install

```bash
git clone https://github.com/Antismart/shadowmesh.git
cd shadowmesh
cargo build --release          # gateway, node-runner, CLI
```

Deploy and fetch a file:

```bash
cargo build --release -p shadowmesh-cli
./target/release/shadowmesh-cli upload ./index.html      # prints a CID
./target/release/shadowmesh-cli download <cid> -o out.html
```

See the [README](README.md) for joining the network as a node and the
[Hosting Guide](docs/hosting-guide.md) for production setup.

## What's next

The roadmap is the gap-closing list in the
[Threat Model](THREAT-MODEL.md#8-roadmap-to-close-the-gaps): bootstrap diversity,
rendezvous / pluggable ingress, transport indistinguishability, completing the
onion return path, traffic-analysis hardening, and IPFS-CID verification.

---

**Full changelog:** [CHANGELOG.md](CHANGELOG.md) ·
**Threat model:** [THREAT-MODEL.md](THREAT-MODEL.md)
