# Running a ShadowMesh Node

ShadowMesh is a decentralized CDN. There are no central servers. The network is made up entirely of people running nodes like you. Every node stores encrypted content fragments, serves them to peers, and replicates data across the mesh for redundancy.

## How the Network Works

```
    You (Node)  <──P2P──>  Friend (Node)  <──P2P──>  Another Node
        │                       │                         │
        └───────── DHT ─────────┴──────── DHT ────────────┘
                    (everyone finds everyone)
```

- Nodes discover each other automatically on local networks (mDNS) and across the internet (DHT)
- Content is encrypted, fragmented, and spread across multiple nodes
- No single node holds a complete file — pieces are distributed
- Onion routing means no node knows both who requested content and what the content is
- Content replicates 3x by default so the network stays healthy even when nodes go offline

## Quick Start (5 minutes)

### Step 1: Install Rust

**macOS / Linux:**
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
```

**Windows:** Download from https://rustup.rs

Verify: `rustc --version` (need 1.70+)

### Step 2: Clone and Build

```bash
git clone https://github.com/Antismart/shadowmesh.git
cd shadowmesh
cargo build --release -p node-runner -p shadowmesh-cli
```

First build takes a few minutes. Binaries land in `target/release/`.

### Step 3: Configure

```bash
cp node-runner/node-config.example.toml node-config.toml
```

Edit `node-config.toml`:

```toml
[identity]
name = "YourName"              # Pick a name others will see

[storage]
data_dir = ".shadowmesh/data"
max_storage_bytes = 5368709120  # 5 GB — adjust to what you can spare

[network]
max_peers = 50
enable_mdns = true              # Auto-discover peers on your WiFi/LAN
enable_dht = true               # Discover peers across the internet
replication_factor = 3

# To join an existing network, add a bootstrap node:
bootstrap_nodes = [
    # Get this from whoever invited you to the network
    # "/ip4/203.0.113.10/tcp/4001/p2p/12D3KooW..."
]

[dashboard]
enabled = true
host = "127.0.0.1"
port = 3030
```

### Step 4: Start

```bash
RUST_LOG=info ./target/release/node-runner
```

You'll see:

```
🌐 Starting ShadowMesh Node Runner...

✅ Configuration loaded
📋 Node Configuration:
   Name: YourName
   Storage: 5.00 GB
   Max Peers: 50
   Replication: 3x
   Dashboard: http://127.0.0.1:3030

✅ P2P node initialized
   🆔 Peer ID: 12D3KooW...
✅ P2P event loop started

🚀 Node dashboard running at http://127.0.0.1:3030
```

Your **Peer ID** (e.g. `12D3KooWGPAj...`) is your identity on the network. Share it with others so they can find you.

### Step 5: Check Your Dashboard

Open **http://127.0.0.1:3030** in your browser.

---

## Using the CLI

Set up an alias so you don't have to type the full path:

```bash
# Add to your .bashrc or .zshrc
alias smesh="./target/release/shadowmesh-cli"
```

### See Your Node

```bash
smesh status          # Name, peer ID, uptime, storage, peers
smesh health          # Health checks: API, network, storage
smesh peers           # Who you're connected to
smesh bandwidth       # How much data you're moving
smesh metrics         # Detailed performance numbers
```

### Upload Content

```bash
smesh upload photo.jpg
# Uploaded: photo.jpg
# CID:  a1b2c3d4e5...
# Size: 2.4 MB
```

The CID is the content's unique address. Share it with anyone on the network.

### Download Content

```bash
# Someone shares a CID with you
smesh download a1b2c3d4e5...
smesh download a1b2c3d4e5... -o ~/Downloads/photo.jpg
```

### Manage Storage

```bash
smesh storage         # How much space you're using
smesh list            # Everything stored on your node
smesh get <cid>       # Details about a specific piece of content
smesh pin <cid>       # Keep this content (don't garbage collect it)
smesh unpin <cid>     # Allow garbage collection again
smesh delete <cid>    # Remove content immediately
smesh gc              # Run garbage collection now
smesh gc --target-gb 2.0   # Free up space down to 2 GB
```

### Change Settings On the Fly

```bash
smesh config                          # See current config
smesh config-set --name "NewName"     # Change your node name
smesh config-set --max-peers 100      # Allow more connections
smesh config-set --storage-gb 20      # Increase storage limit
smesh config-set --bandwidth-mbps 50  # Cap bandwidth at 50 Mbps
```

### Other

```bash
smesh replication     # How well content is replicated
smesh ready           # Is the node fully connected to the mesh
smesh fetch <cid>     # Pull content from remote peers
smesh shutdown        # Graceful stop

# JSON output for scripting
smesh status --json
smesh list --json
```

---

## Connecting to Other Nodes

### Same WiFi / LAN (automatic)

If you and someone else are on the same network, mDNS handles everything. Just start your nodes — within 30 seconds:

```
INFO libp2p_mdns: discovered: 12D3KooWXYZ...
```

```bash
smesh peers
# PEER ID              ADDRESSES                    LATENCY
# 12D3KooWXYZ...      /ip4/192.168.1.42/tcp/4001   3 ms
```

No configuration needed.

### Different Networks (bootstrap)

When nodes are on different networks (different houses, different cities), they need a way to find each other. Any node with a reachable IP can be a bootstrap node.

**If you have a public IP or can port-forward:**

1. Forward port **4001 TCP** on your router to your machine
2. Find your public IP: `curl ifconfig.me`
3. Your bootstrap address is:
   ```
   /ip4/<YOUR_PUBLIC_IP>/tcp/4001/p2p/<YOUR_PEER_ID>
   ```
4. Share this with others

**If someone shared a bootstrap address with you:**

Add it to your `node-config.toml`:

```toml
[network]
bootstrap_nodes = [
    "/ip4/203.0.113.10/tcp/4001/p2p/12D3KooWGPAjDTsHkY39arQUSFKTdSRaKQFDhqiND5nZR2iLdtr7"
]
```

Restart your node. You'll connect to that peer and through them discover everyone else on the network.

### Testing the Connection

Once peers are connected:

```bash
# On Node A — upload something
smesh upload testfile.txt
# CID: abc123...

# On Node B — fetch it
smesh fetch abc123...
smesh download abc123...
```

If it works, you're on the mesh.

---

## Being a Bootstrap Node

A bootstrap node is just a regular node that others can reach from the internet. There's nothing special about it — it runs the same `node-runner` binary. The only requirement is a **reachable IP and open port**.

### Options

**Home connection with port forwarding:**
- Forward port 4001 on your router
- Works if your ISP gives you a public IP
- Free, but goes offline when your laptop sleeps

**VPS ($5-10/month):**
- DigitalOcean, Hetzner, Vultr, etc.
- Always online, stable IP
- Great for keeping the network connected

### Setup on a VPS

```bash
# On the VPS
sudo apt update && sudo apt install -y build-essential pkg-config libssl-dev
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

git clone https://github.com/Antismart/shadowmesh.git
cd shadowmesh
cargo build --release -p node-runner

cp node-runner/node-config.example.toml node-config.toml
```

Edit `node-config.toml`:

```toml
[identity]
name = "bootstrap-1"

[storage]
max_storage_bytes = 10737418240  # 10 GB

[network]
listen_addresses = ["/ip4/0.0.0.0/tcp/4001"]
max_peers = 100
enable_mdns = false   # No local network on a VPS
enable_dht = true

[dashboard]
host = "0.0.0.0"      # Accessible remotely (secure with firewall)
port = 3030
```

Run it:

```bash
RUST_LOG=info ./target/release/node-runner
```

Note the peer ID from the log. Your bootstrap address is:

```
/ip4/<VPS_IP>/tcp/4001/p2p/<PEER_ID>
```

Share this with everyone joining the network.

### Run as a Service (stays running after logout)

```ini
# /etc/systemd/system/shadowmesh-node.service
[Unit]
Description=ShadowMesh Node
After=network.target

[Service]
Type=simple
User=ubuntu
WorkingDirectory=/home/ubuntu/shadowmesh
ExecStart=/home/ubuntu/shadowmesh/target/release/node-runner
Environment=RUST_LOG=info
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl enable shadowmesh-node
sudo systemctl start shadowmesh-node
sudo journalctl -u shadowmesh-node -f   # Watch logs
```

---

## Optional: Running a Gateway

The gateway is **not required** for the network to work. Nodes talk to each other directly. The gateway exists for one reason: so regular web browsers can access content via HTTP URLs.

If you want people to access content at `https://yourdomain.com/<cid>` without running a node themselves:

```bash
cargo build --release -p gateway
RUST_LOG=info ./target/release/gateway
```

The gateway connects to the same P2P mesh as the nodes and serves content over HTTP on port 8081. It caches content, rate-limits requests, and provides a dashboard at `/dashboard`.

This is optional. The network works fine without it.

---

## Configuration Reference

### Storage

| Setting | Default | Description |
|---------|---------|-------------|
| `data_dir` | `.shadowmesh/data` | Where fragments are stored |
| `max_storage_bytes` | 10 GB | Maximum disk usage |
| `gc_enabled` | true | Auto garbage collection |
| `gc_interval_secs` | 3600 | GC runs every hour |
| `min_free_space_bytes` | 1 GB | GC target free space |

### Network

| Setting | Default | Description |
|---------|---------|-------------|
| `listen_addresses` | `/ip4/0.0.0.0/tcp/4001` | P2P listen addresses |
| `bootstrap_nodes` | `[]` | Known peers to connect to first |
| `max_peers` | 50 | Maximum connections |
| `enable_mdns` | true | Auto-discover on local network |
| `enable_dht` | true | Discover across the internet |
| `replication_factor` | 3 | How many copies of each content |
| `max_inbound_bandwidth` | unlimited | Inbound bandwidth cap (bytes/sec) |
| `max_outbound_bandwidth` | unlimited | Outbound bandwidth cap (bytes/sec) |

### Dashboard

| Setting | Default | Description |
|---------|---------|-------------|
| `enabled` | true | Show web dashboard |
| `host` | 127.0.0.1 | Bind address (localhost only) |
| `port` | 3030 | Dashboard port |

### Performance

| Setting | Default | Description |
|---------|---------|-------------|
| `worker_threads` | 0 (auto) | Worker threads (0 = use all CPUs) |
| `max_concurrent_requests` | 100 | Max parallel requests |
| `request_timeout_secs` | 30 | Request timeout |

---

## Ports

| Port | Protocol | Purpose | Open to internet? |
|------|----------|---------|-------------------|
| 4001 | TCP | P2P connections | Yes, if you want incoming peers |
| 3030 | TCP | Dashboard & API | No — localhost only by default |

---

## Running as a Background Service

### macOS

Create `~/Library/LaunchAgents/com.shadowmesh.node.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.shadowmesh.node</string>
    <key>ProgramArguments</key>
    <array>
        <string>/path/to/shadowmesh/target/release/node-runner</string>
    </array>
    <key>WorkingDirectory</key>
    <string>/path/to/shadowmesh</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>RUST_LOG</key>
        <string>info</string>
    </dict>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/tmp/shadowmesh-node.log</string>
    <key>StandardErrorPath</key>
    <string>/tmp/shadowmesh-node.log</string>
</dict>
</plist>
```

```bash
launchctl load ~/Library/LaunchAgents/com.shadowmesh.node.plist
```

### Linux

See the systemd section under "Being a Bootstrap Node" above.

---

## Troubleshooting

**"No known peers" warning**
```
WARN libp2p_kad: Failed to trigger bootstrap: No known peers.
```
Normal if you haven't configured bootstrap nodes. You'll still discover peers on your local network via mDNS. To connect to the wider network, add a bootstrap node to your config.

**Port 4001 already in use**
Something else (probably IPFS) is on that port. Change it:
```toml
[network]
listen_addresses = ["/ip4/0.0.0.0/tcp/4002"]
```

**Can't find peers across the internet**
Both nodes behind NAT with no port forwarding = neither can reach the other. At least one node needs to be reachable (public IP, or port 4001 forwarded on router). That node acts as the bridge.

**Dashboard won't load**
Use `http://127.0.0.1:3030` not `http://localhost:3030` — localhost may resolve to IPv6 which the dashboard doesn't bind to by default.

**High memory usage**
Lower your limits:
```toml
[storage]
max_storage_bytes = 1073741824  # 1 GB

[performance]
cache_size = 100
max_concurrent_requests = 20
```

---

## FAQ

**Do I need a server?**
No. Run it on your laptop. The network is designed for nodes that come and go. Content replicates so nothing is lost when you go offline.

**Can I see what's stored on my node?**
You can see CIDs and metadata (`smesh list`), but the actual content is encrypted. You're storing fragments, not complete files.

**How much bandwidth does it use?**
Check with `smesh bandwidth`. Set caps if needed:
```toml
[network]
max_inbound_bandwidth = 10485760   # 10 MB/s
max_outbound_bandwidth = 10485760  # 10 MB/s
```

**Can I run this on a Raspberry Pi?**
Yes. Rust compiles for ARM. Use `cargo build --release -p node-runner` on the Pi. Lower the storage and peer limits for the hardware.

**Is my IP address visible?**
To peers you're directly connected to, yes — same as any P2P network (BitTorrent, IPFS). ShadowMesh's onion routing protects which content you're requesting or serving from being linked back to you.

**What happens when I turn off my laptop?**
Your node disconnects. Content that was stored on your node is also on other nodes (replication factor 3 by default). When you come back online, your node rejoins the mesh and resumes serving content.
