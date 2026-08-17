# ShadowMesh Threat Model

**Status: v0.1 / pre-release. This document describes what ShadowMesh defends
against *today*, in the code as shipped — not what the design aspires to.** Where
a protection is partial or absent, it says so plainly. Overstating guarantees in
a privacy tool is worse than having fewer of them, so this document errs toward
under-claiming.

If you are evaluating ShadowMesh for a use case where being wrong gets someone
hurt, read the [Non-goals](#non-goals-what-shadowmesh-does-not-defend-against-yet)
and [Known weaknesses](#known-weaknesses) sections first.

---

## 1. What ShadowMesh is (for the purpose of this document)

ShadowMesh is a content-addressed, peer-to-peer content delivery network. Content
is chunked, hashed with BLAKE3, and served by a network of libp2p nodes; a
public HTTP gateway bridges ordinary browsers to that network. The core value
proposition this threat model is written around is:

> **A deploy target for static content that resists takedown of the content and
> single-host censorship of its delivery.**

Other subsystems in the repository (SSR process hosting, WASM edge functions,
KV/secrets state layer) are out of scope here; they are conventional server-side
features and are covered by ordinary web-app security, not by the P2P/privacy
model below.

---

## 2. Assets — what we are protecting

| Asset | Why it matters |
|---|---|
| **Content integrity** | A CID must return exactly the bytes that hash to it, or content addressing is meaningless. |
| **Content availability** | Published content should survive the loss or blocking of any single node. |
| **Requester–content unlinkability** | An observer should not be able to say "user X requested content Y." |
| **Transport confidentiality** | On-path observers should not read peer-to-peer traffic in clear. |
| **Tenant isolation (gateway)** | One deployment must not read or modify another's state (KV, secrets, blobs). |
| **Publisher availability of access** | New participants must be able to *reach* the network to use it at all. |

---

## 3. Adversary classes

We reason about four adversary tiers. A protection that holds against Tier 1 may
fail against Tier 3 — the matrix in §4 states which.

- **T0 — Malicious peer.** Runs one or more nodes, speaks the protocol, serves
  poisoned content, lies in the DHT, floods requests. No special network position.
- **T1 — On-path network observer / active MITM.** Sees and can modify traffic on
  links it controls (e.g. a hostile ISP, a compromised relay, a LAN attacker).
- **T2 — Gateway / node operator.** Controls a gateway or node that users actually
  route through, including the public gateway.
- **T3 — Network-level censor.** A national firewall or platform that can block IPs,
  fingerprint protocols by DPI, and poison DNS. Wants to prevent access, not
  necessarily to deanonymize.

---

## 4. Defense matrix (as shipped)

Legend: ✅ defended · 🟡 partial / caveated · ❌ not yet defended.

| Property | Status | Mechanism (as implemented) | Residual risk |
|---|---|---|---|
| **Content integrity vs. malicious peer** | ✅ (hex-BLAKE3 content) | Nodes bind a fetched manifest to the requested CID and re-hash the reassembled bytes before storing or re-announcing; per-fragment BLAKE3 is verified on the wire. Gateway and browser SDK verify at the requester. | Content addressed by an **IPFS-style `Qm`/`bafy` CID cannot be recomputed locally** and is currently served without content verification on those paths (logged, not rejected). Use hex-BLAKE3 CIDs for a verified path. |
| **Takedown resistance** | 🟡 | Content is replicated pull-style across peers and re-announced via DHT/gossip; any node holding a CID can serve it. No single host is authoritative for the bytes. | Availability depends on how many independent nodes actually pin the content. With few nodes, takedown resistance is theoretical. |
| **Availability under single-host loss** | 🟡 | Multi-node design; DHT provider records; replication loop. | See §5: the *live* network currently bootstraps through one host, so single-host loss breaks **new-node onboarding** even though existing peer-to-peer serving survives. |
| **Transport confidentiality** | ✅ | libp2p transport is authenticated and encrypted with **Noise** (`protocol/src/node.rs`). Browser DataChannels use an authenticated ephemeral X25519 ECDH handshake (Ed25519-bound) over DTLS. | Noise protects payload confidentiality; it does **not** hide *that* libp2p is being spoken (see traffic-analysis and censorship rows). |
| **Requester–content unlinkability (onion)** | 🟡 → ❌ end-to-end | The `zk_relay` onion handshake is now authenticated: each hop signs its CREATED response with its Ed25519 identity, verified against the expected per-hop PeerId over a transcript binding both ephemeral keys and the circuit id — so an on-path MITM (T1) can no longer splice into a circuit. | **The multi-hop *return* path does not yet work end to end**: exit nodes emit responses without per-hop onion encryption, so a full circuit cannot currently round-trip a response. Onion routing is therefore **not something to rely on today** for unlinkability. Direct fetch (no onion) is the working path and offers **no** unlinkability against T2. |
| **Traffic-analysis resistance** | ❌ | — | Onion cells are not padded to a constant size and shrink per hop, leaking circuit position; no cover traffic. Even once the return path works, timing/size correlation is not addressed. |
| **Censorship of network access (T3)** | ❌ | DNS TXT seed discovery and multi-peer bootstrap config *exist* in code. | Compiled default bootstrap list is empty; the live network is reached through **one documented IP**. One firewall rule blocks onboarding. This is the single biggest gap (see §5). |
| **Multi-tenant isolation (gateway)** | 🟡 | API keys now carry an identity; KV/secrets/blob/deploy handlers enforce namespace ownership (403 on cross-tenant); namespace-count caps bound growth. | Scoping is static per-key config; **legacy bare keys remain admin-level** for backward compatibility, so an operator must adopt scoped keys to actually get isolation. |
| **Node/API compromise** | 🟡 | The node API fails fast if bound to a non-loopback interface without `NODE_API_KEY`; the CLI authenticates with a bearer token. | Read endpoints still expose the content inventory and peer list; operators must set a key and firewall the port. |

---

## 5. The bootstrap-centralization problem (called out explicitly)

**Claim we do *not* make:** "ShadowMesh resists a national firewall."

The delivery of *content already in the mesh* is genuinely peer-to-peer and
survives loss of any single node. But **joining** the mesh currently depends on
reaching a single well-known bootstrap host on a bare IP. A censor at T3 blocks
that IP (or DPI-fingerprints the libp2p Noise handshake) and new nodes cannot
find peers. Existing, already-connected nodes keep working; new participants in
the censored region do not.

The code already contains the seams to fix this — DNS-seed discovery
(`protocol/src/bootstrap.rs`), env/config multi-peer bootstrap, rendezvous, and
relay support — they are simply not populated with diverse infrastructure yet.
Closing this gap is the top item on the roadmap below.

---

## 6. Non-goals — what ShadowMesh does NOT defend against (yet)

Stated bluntly so no one is surprised:

- **It is not an anonymity network on the level of Tor.** The onion layer is
  authenticated but not end-to-end functional, unpadded, and without cover
  traffic. Do not use it where deanonymization has serious consequences.
- **It does not resist a national firewall or DPI censor.** Bootstrap is
  centralized and the transport is fingerprintable.
- **It does not hide that you are using ShadowMesh** from an on-path observer.
- **It does not protect content the publisher wants kept secret** — content is
  public and integrity-protected, not confidential. (At-rest encryption exists
  in the SDK but is a separate, publisher-managed feature.)
- **It does not defend the gateway's server-side features** (SSR, WASM, state) with
  anything beyond ordinary web-app auth and sandboxing.

---

## 7. Known weaknesses (tracked)

These are open and tracked in the issue tracker; this section is the honest
"here's what's broken" list.

1. **IPFS-CID content is unverified** on fetch (only hex-BLAKE3 is re-hashed locally).
2. **Onion return path incomplete** — exit-node response encryption and the
   backward-decrypt path are unfinished; multi-hop circuits cannot round-trip.
3. **No traffic-analysis resistance** — variable/shrinking cell sizes, no padding
   or cover traffic.
4. **Centralized bootstrap** — single documented entry host; empty compiled defaults.
5. **Legacy bare API keys are admin-scoped** on the gateway until operators migrate
   to scoped keys.
6. **Fingerprintable transport** — Noise/libp2p is recognizable to DPI.

---

## 8. Roadmap to close the gaps

In rough priority order (this is also the project's real technical roadmap):

1. **Bootstrap diversity.** Ship multiple independent bootstrap nodes on distinct
   networks/ASNs; populate DNS-seed TXT records; document how operators add their
   own. Removes the single-firewall-rule kill switch.
2. **Rendezvous / pluggable ingress.** Snowflake-style volunteer proxies or
   domain-fronted rendezvous so onboarding does not depend on a fixed reachable IP.
3. **Transport indistinguishability.** A pluggable-transport layer so the wire does
   not obviously read as libp2p to a DPI censor.
4. **Complete the onion return path.** Per-hop response encryption at the exit and a
   correct backward-decrypt path, then constant-size cell padding.
5. **Traffic-analysis hardening.** Padding to fixed cell sizes at every hop; optional
   cover traffic.
6. **IPFS-CID verification.** Local multihash decoding so `Qm`/`bafy` content is
   verified, not just hex-BLAKE3.

---

## 9. Reporting security issues

Please report vulnerabilities privately to the maintainer rather than opening a
public issue. Content-integrity, tenant-isolation, and circuit-authentication bugs
are the highest priority.
