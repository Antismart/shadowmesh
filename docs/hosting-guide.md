# ShadowMesh Network Setup Guide

This guide is for the person organizing a ShadowMesh test network. Your job is to get the first bootstrap node running so other node runners can find each other.

## How the Network Starts

ShadowMesh is fully decentralized — there are no central servers. But when starting a new network, nodes need a way to discover each other for the first time. That's where a **bootstrap node** comes in.

A bootstrap node is a regular node with a publicly reachable IP. It's the first address new nodes connect to. Once connected, they discover all other peers through the DHT and no longer depend on the bootstrap.

```
New Node ──connects to──> Bootstrap Node ──introduces──> Rest of Network
                              │
                    (just a regular node
                     with a public IP)
```

The bootstrap node is not special. It stores content, serves peers, and participates in the mesh like everyone else. It just happens to be reachable from the internet so new nodes can find it.

---

## Step 1: Get a Bootstrap Node Running

You need one machine with a **public IP and port 4001 open**. Options:

| Option | Cost | Uptime | Best For |
|--------|------|--------|----------|
| Home machine + port forwarding | Free | While your machine is on | Small tests |
| VPS (DigitalOcean, Hetzner, Vultr) | $5-10/mo | 24/7 | Real testing |
| Cloud VM (AWS, GCP) | Varies | 24/7 | Larger deployments |

### VPS Setup (recommended for testing)

Spin up a cheap VPS (Ubuntu 22.04+, 2 CPU, 2 GB RAM, 10 GB disk).

```bash
# Install dependencies
sudo apt update && sudo apt install -y build-essential pkg-config libssl-dev curl
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

# Clone and build
git clone https://github.com/Antismart/shadowmesh.git
cd shadowmesh
cargo build --release -p node-runner
```

### Configure the Bootstrap Node

```bash
cp node-runner/node-config.example.toml node-config.toml
```

Edit `node-config.toml`:

```toml
[identity]
name = "bootstrap-1"

[storage]
data_dir = ".shadowmesh/data"
max_storage_bytes = 10737418240  # 10 GB

[network]
listen_addresses = ["/ip4/0.0.0.0/tcp/4001"]
bootstrap_nodes = []             # First node -- nothing to bootstrap from
max_peers = 100                  # Higher limit since everyone connects here first
enable_mdns = false              # No local network on a VPS
enable_dht = true
replication_factor = 3

[dashboard]
enabled = true
host = "0.0.0.0"                # Accessible remotely (secure with firewall if needed)
port = 3030
```

Bootstrap addresses must include the `/p2p/<peer-id>` suffix. When the node starts, it adds each bootstrap peer to the Kademlia routing table and dials it. If a multiaddr is malformed (missing `/p2p/` or unparseable), the node logs a warning and skips it.

### Start It

```bash
RUST_LOG=info ./target/release/node-runner
```

Note the **Peer ID** from the startup log:

```
✅ P2P node initialized
   🆔 Peer ID: 12D3KooWGPAjDTsHkY39arQUSFKTdSRaKQFDhqiND5nZR2iLdtr7
```

### Your Bootstrap Address

Combine your VPS IP and peer ID:

```
/ip4/<VPS_IP>/tcp/4001/p2p/<PEER_ID>
```

Example:
```
/ip4/167.71.42.88/tcp/4001/p2p/12D3KooWGPAjDTsHkY39arQUSFKTdSRaKQFDhqiND5nZR2iLdtr7
```

**This is what you share with node runners.** They paste it into their config and they're on the network.

### Keep It Running (systemd)

```ini
# /etc/systemd/system/shadowmesh-node.service
[Unit]
Description=ShadowMesh Bootstrap Node
After=network.target

[Service]
Type=simple
User=ubuntu
WorkingDirectory=/home/ubuntu/shadowmesh
ExecStart=/home/ubuntu/shadowmesh/target/release/node-runner
Environment=RUST_LOG=info
Restart=always
RestartSec=5
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl enable shadowmesh-node
sudo systemctl start shadowmesh-node
sudo journalctl -u shadowmesh-node -f
```

---

## Step 2: Share the Bootstrap Address

Send your bootstrap address to node runners. They add it to their `node-config.toml`:

```toml
[network]
bootstrap_nodes = [
    "/ip4/167.71.42.88/tcp/4001/p2p/12D3KooWGPAjDTsHkY39arQUSFKTdSRaKQFDhqiND5nZR2iLdtr7"
]
```

That's it. When they start their node, they connect to your bootstrap, discover other peers via DHT, and they're part of the mesh.

Point them to the [Node Runner Guide](node-runner-guide.md) for setup instructions.

---

## Step 3: Verify the Network

Once a few nodes are running:

```bash
# On the bootstrap node
./target/release/shadowmesh-cli peers
# Should show connected peers with their addresses and latency

./target/release/shadowmesh-cli status
# connected_peers should be > 0
```

### Test Content Flow

```bash
# Node A uploads
./target/release/shadowmesh-cli upload testfile.txt
# CID: abc123...

# Node B (different machine) fetches
./target/release/shadowmesh-cli fetch abc123...
./target/release/shadowmesh-cli download abc123...
# Should get the file
```

If node B can download content that node A uploaded, the mesh is working.

---

## Optional: Adding a Gateway

The gateway is **not required for the network**. Nodes communicate directly via P2P. The gateway exists only so regular web browsers can access content via HTTP URLs (browsers can't speak libp2p).

Add a gateway if you want a URL like `https://yourdomain.com/<cid>` that anyone with a browser can open.

### Build and Run

```bash
cargo build --release -p gateway
RUST_LOG=info ./target/release/gateway
```

The gateway:
- Connects to the same P2P mesh as the nodes
- Serves content over HTTP on port 8081
- Caches frequently accessed content
- Rate-limits requests
- Provides a web dashboard at `/dashboard`
- Provides Prometheus metrics at `/metrics/prometheus`

### Gateway Configuration

Edit `gateway/config.toml`:

```toml
[server]
host = "0.0.0.0"
port = 8081

[cache]
max_size_mb = 500
ttl_seconds = 3600

[rate_limit]
enabled = true
requests_per_second = 100

[p2p]
enabled = true
tcp_port = 4001
enable_mdns = false
bootstrap_peers = [
    # Point to your bootstrap node
    "/ip4/167.71.42.88/tcp/4001/p2p/12D3KooWGPAjDTsHkY39..."
]
```

### Environment Variables

```bash
# Recommended
export RUST_LOG=info
export SHADOWMESH_NAMING_KEY=$(openssl rand -hex 32)

# Bootstrap peers (alternative to config file)
export SHADOWMESH_BOOTSTRAP_NODES="/ip4/<BOOTSTRAP_IP>/tcp/4001/p2p/<PEER_ID>"

# For production (lock down the API)
export SHADOWMESH_API_KEYS="your-secret-key"
export SHADOWMESH_ADMIN_KEY="your-admin-key"

# CORS origins (comma-separated, overrides config file)
export SHADOWMESH_SECURITY_ALLOWED_ORIGINS="https://yourdomain.com,https://app.yourdomain.com"

# Optional extras
export REDIS_URL="redis://127.0.0.1:6379"      # Persistent state across restarts
export GITHUB_CLIENT_ID="..."                    # GitHub OAuth for dashboard deploys
export GITHUB_CLIENT_SECRET="..."
```

### Put It Behind a Domain (Nginx)

```nginx
server {
    listen 443 ssl http2;
    server_name mesh.yourdomain.com;

    ssl_certificate /etc/letsencrypt/live/mesh.yourdomain.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/mesh.yourdomain.com/privkey.pem;

    location / {
        proxy_pass http://127.0.0.1:8081;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_for_addr;
        proxy_set_header X-Forwarded-Proto $scheme;
    }

    location /signaling/ws {
        proxy_pass http://127.0.0.1:8081;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
    }
}
```

Now `https://mesh.yourdomain.com/<cid>` serves content from the decentralized network to any browser.

### Docker Option

```bash
docker-compose up -d
```

This starts the gateway + an IPFS node (as an optional storage backend). The gateway works fine without IPFS -- it falls back to demo mode with local-only content.

**Tip:** Pin your Docker images to a specific tag or digest in production instead of using `latest`.

### Configuration Hot-Reload

The gateway watches its config file for changes and reloads automatically -- no restart required. This applies to rate limits, cache TTL, CORS origins, and other runtime settings.

You can also trigger a reload manually:

```bash
# Via signal (Unix only)
kill -HUP <gateway-pid>

# Via admin API
curl -X POST http://localhost:8081/api/admin/reload
```

The gateway logs what changed on each reload:

```
INFO  Rate limit updated old=100 new=200
INFO  Configuration reloaded successfully
```

### Content Resolution Fallback Chain

When a browser requests content from the gateway, it tries multiple sources in order:

1. **Local cache** -- in-memory LRU cache of recently served content
2. **P2P network** -- DHT provider lookup, then fetch manifest and fragments from peers
3. **Node-runners via HTTP** -- queries configured `node_runners` URLs with health-aware load balancing
4. **IPFS** -- falls back to the IPFS API if all other sources fail

The response includes an `X-Source` header (`CACHE`, `P2P`, `NODE`, `NODE-STREAM`, or `IPFS`) so you can see which layer served the content.

---

## Multiple Bootstrap Nodes

For resilience, run 2-3 bootstrap nodes. If one goes down, new nodes can still join via the others.

Each bootstrap node should know about the others:

```toml
# On bootstrap-2
[network]
bootstrap_nodes = [
    "/ip4/<BOOTSTRAP_1_IP>/tcp/4001/p2p/<BOOTSTRAP_1_PEER_ID>"
]
```

Share all bootstrap addresses with node runners:

```toml
# Node runner config
[network]
bootstrap_nodes = [
    "/ip4/<BOOTSTRAP_1_IP>/tcp/4001/p2p/<BOOTSTRAP_1_PEER_ID>",
    "/ip4/<BOOTSTRAP_2_IP>/tcp/4001/p2p/<BOOTSTRAP_2_PEER_ID>"
]
```

---

## Monitoring the Network

### From any node

Use the CLI binary (`alias smesh="./target/release/shadowmesh-cli"`):

```bash
smesh status          # Uptime, peers, storage
smesh peers           # All connected peers
smesh bandwidth       # Data flowing through this node
smesh replication     # Content replication health
smesh metrics         # Detailed numbers
```

### Gateway endpoints (if running)

| Endpoint | Purpose |
|----------|---------|
| `/health` | `{"status":"healthy"}` or `{"status":"degraded"}` |
| `/metrics` | JSON metrics |
| `/metrics/prometheus` | Prometheus scrape target |
| `/dashboard` | Web UI with deploy tracking and metrics |

### What to Watch

- **Peer count**: Nodes should discover each other within 30-60 seconds
- **Replication health**: Content should replicate to 3 nodes
- **Bandwidth**: Even split across nodes means healthy distribution
- **Storage**: No single node should be overloaded

---

## What You Don't Need

| Thing | Required? | Why |
|-------|-----------|-----|
| Central server | No | The mesh IS the network |
| IPFS daemon | No | Nice to have, gateway runs without it |
| Redis | No | In-memory state works fine for testing |
| Domain name | No | Only if you run a gateway for browser access |
| Docker | No | Build from source works everywhere |
| Kubernetes | No | For large-scale production only |

The minimum viable network is **two nodes that can reach each other**. Everything else is optional.
