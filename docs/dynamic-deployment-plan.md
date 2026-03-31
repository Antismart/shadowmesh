# ShadowMesh Dynamic Deployment Plan

## Overview

Phased plan to add server-side rendering (SSR), API routes, and edge functions to ShadowMesh's static CDN.

## Phases

### Phase 1: Node.js Process Manager (4-6 weeks)
Ship first. Detect SSR frameworks, build in server mode, spawn Node.js processes, reverse proxy dynamic routes.

**New files:**
- `gateway/src/process_manager.rs` — Process lifecycle, port allocation, health checks, resource limits
- `gateway/src/reverse_proxy.rs` — HTTP reverse proxy to Node.js processes
- `gateway/src/framework_detect.rs` — Enhanced framework detection (static vs dynamic)

**Modified files:**
- `gateway/src/lib.rs` — Add `ProcessManager` to AppState
- `gateway/src/main.rs` — Initialize process manager, new routes, shutdown handling
- `gateway/src/dashboard.rs` — Conditional SSR build (skip static export injection), spawn process after deploy
- `gateway/src/config.rs` — New `[dynamic]` config section

**Dependencies:** `nix` (signal handling), `sysinfo` (process monitoring)

**Frameworks supported:**
| Framework | Start command | Health path |
|-----------|--------------|-------------|
| Next.js standalone | `node .next/standalone/server.js` | `/api/health` |
| Nuxt 3 server | `node .output/server/index.mjs` | `/_nuxt/health` |
| SvelteKit node | `node build/index.js` | `/health` |
| Remix | `remix-serve build/server/index.js` | `/health` |
| Express/Fastify | `npm start` | User-configured |

**Config:**
```toml
[dynamic]
enabled = false
port_range_start = 9000
port_range_end = 9999
max_processes = 50
memory_limit_mb = 512
health_check_interval_seconds = 10
startup_timeout_seconds = 30
node_binary = "node"
```

### Phase 2: WASM Edge Functions (6-8 weeks)
Embed Wasmtime for sandboxed execution. WASM modules stored on IPFS — content-addressed, verifiable, decentralized.

**New files:**
- `gateway/src/wasm_runtime.rs` — Wasmtime integration, AOT compilation, fuel metering
- `gateway/src/wasm_bridge.rs` — HTTP request/response ABI for WASM modules
- `gateway/src/route_manifest.rs` — Parse `_shadowmesh/routes.json`

**Dependencies:** `wasmtime`, `wasmtime-wasi` (feature-gated)

**Route manifest format:**
```json
{
  "version": 1,
  "routes": [
    { "path": "/api/*", "handler": "api.wasm", "method": ["GET", "POST"] }
  ],
  "static": ["/**"],
  "capabilities": { "api.wasm": ["net:connect", "kv:read"] }
}
```

**Security model:** Capability-based. Memory limits via Wasmtime. CPU limits via fuel metering. No ambient filesystem/network access.

### Phase 3: Framework Adapters (6-8 weeks)
npm packages that compile framework server output to WASM.

**New packages:**
- `@shadowmesh/adapter-nextjs`
- `@shadowmesh/adapter-nuxt`
- `@shadowmesh/adapter-sveltekit`
- `@shadowmesh/adapter-remix`
- `@shadowmesh/adapter-shared` (JS-to-WASM via javy)

### Phase 4: State Layer (4-6 weeks)
KV store, distributed SQLite, secrets, blob storage for edge functions.

**New files:**
- `gateway/src/kv_store.rs` — Redis-backed KV with per-deployment namespaces
- `gateway/src/edge_sqlite.rs` — WASM-compiled SQLite per deployment
- `gateway/src/secrets.rs` — ChaCha20Poly1305 encrypted secrets
- `gateway/src/blob_storage.rs` — IPFS read/write from WASM

## Migration

All phases are backward compatible. Existing static deployments are unaffected. Each phase is behind a config toggle (`dynamic.enabled`, `wasm.enabled`). New `Deployment` fields use `#[serde(default)]` for Redis compatibility.
