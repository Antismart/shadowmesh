# Beyond the core: experimental features and roadmap

ShadowMesh's focus is one thing: **a deploy target for static content that can't
be taken down** (see the [README](README.md)). This document holds everything
that is *not* that — features that already exist in the repository but are
deliberately out of the core story, and things that are planned but not built.

Splitting these out is intentional. A single-sentence product is easier to
evaluate, trust, and adopt than a platform that tries to be five things at once.
The code below is real and stays in the tree; it is just not what ShadowMesh is
*for* right now.

Status labels are honest:

- **Working (experimental)** — implemented and tested, but not hardened or the focus.
- **Partial** — present but with known gaps; do not rely on it.
- **Planned** — designed or intended, not built.

---

## Already in the repo, but not the core product

### Dynamic (SSR) hosting — Working (experimental)

The gateway can build a project in server mode, spawn a Node.js child process on
an allocated localhost port, and reverse-proxy requests to it
(`gateway/src/process_manager.rs`, `gateway/src/reverse_proxy.rs`). Health checks,
auto-restart, and memory limits are implemented. This makes ShadowMesh a general
SSR host — which is precisely the scope creep the core README avoids. It works,
but it is a conventional server-side feature with none of the takedown-resistance
that defines the project.

### WASM edge functions — Working (experimental)

Per-route WASM modules run in a Wasmtime sandbox with fuel-based CPU metering and
a JSON-over-stdio bridge (`gateway/src/wasm_runtime.rs`, `adapters/wasm-sdk`).
Known limits before this is production-grade: the memory cap is not yet enforced
(only fuel is), and execution is not wall-clock bounded. Treat it as a demo.

### State layer: KV, secrets, blobs — Partial

A per-deployment KV store (Redis-backed), ChaCha20-Poly1305 encrypted secrets,
and IPFS-backed blob storage (`gateway/src/kv_store.rs`, `secrets.rs`,
`blob_storage.rs`). Tenant isolation is enforced for scoped API keys, but legacy
bare keys remain admin-scoped, and the secrets-delivery path into workloads is
not fully wired. Useful for experimentation, not for holding anything sensitive.

### Framework adapters — Partial

Adapters for Next.js, Nuxt, SvelteKit, and Remix
(`adapters/adapter-*`). **Important honesty note:** these currently *generate
route manifests* (`_shadowmesh/routes.json`) — they do **not** compile your JS/TS
route handlers to WASM. The referenced `.wasm` handlers must be provided
separately (today, hand-written with the Rust `shadowmesh-edge` SDK). Route
scanning and method detection work; JS→WASM compilation does not exist yet.

---

## Planned, not built

- **Mobile SDK (React Native)** — Planned.
- **Browser extension** — Planned.
- **IPFS pinning-service integration** — Planned.
- **Incentive layer / MESH token** — Planned. A tokenomics sketch lives in
  [docs/tokenomics.md](docs/tokenomics.md); no on-chain component is shipped, and
  it is explicitly not part of the near-term focus.
- **Per-deployment SQLite (WASM-compiled)** — Planned.
- **API rewrites / proxy to external backends** — Planned.
- **Custom multi-step build pipelines** — Planned.
- **Team management & RBAC** — Planned.

---

## The core roadmap lives elsewhere

The roadmap that actually matters — closing the gaps between what ShadowMesh
*claims* and what it *does* — is the gap-closing list in
[THREAT-MODEL.md](THREAT-MODEL.md#8-roadmap-to-close-the-gaps): bootstrap
diversity, rendezvous/pluggable ingress, transport indistinguishability,
completing the onion return path, traffic-analysis hardening, and IPFS-CID
verification. Those come before any of the features above.
