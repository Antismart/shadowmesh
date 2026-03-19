#!/bin/bash
set -e

echo "=== Otter Gateway + IPFS Setup ==="
echo "Adds gateway and IPFS to an existing relay VPS"
echo ""

# ── 1. Install IPFS (Kubo) ──────────────────────────────────────────────

IPFS_VERSION="v0.32.1"

if ! command -v ipfs &> /dev/null; then
    echo "Installing IPFS Kubo ${IPFS_VERSION}..."
    ARCH=$(uname -m)
    case "$ARCH" in
        x86_64)  IPFS_ARCH="amd64" ;;
        aarch64) IPFS_ARCH="arm64" ;;
        *)       echo "Unsupported architecture: $ARCH"; exit 1 ;;
    esac

    cd /tmp
    wget -q "https://dist.ipfs.tech/kubo/${IPFS_VERSION}/kubo_${IPFS_VERSION}_linux-${IPFS_ARCH}.tar.gz"
    tar xzf "kubo_${IPFS_VERSION}_linux-${IPFS_ARCH}.tar.gz"
    cd kubo
    bash install.sh
    rm -rf /tmp/kubo /tmp/kubo_*.tar.gz
    echo "IPFS installed: $(ipfs --version)"
else
    echo "IPFS already installed: $(ipfs --version)"
fi

# Initialize IPFS if needed
if [ ! -d "/root/.ipfs" ]; then
    echo "Initializing IPFS with server profile..."
    IPFS_PATH=/root/.ipfs ipfs init --profile=server
fi

# Configure IPFS for production
echo "Configuring IPFS..."
IPFS_PATH=/root/.ipfs ipfs config Addresses.API "/ip4/127.0.0.1/tcp/5001"
IPFS_PATH=/root/.ipfs ipfs config Addresses.Gateway "/ip4/127.0.0.1/tcp/8080"
# Allow local gateway API access only (gateway binary connects via localhost)
IPFS_PATH=/root/.ipfs ipfs config --json API.HTTPHeaders.Access-Control-Allow-Origin '["http://127.0.0.1:5001"]'
# Increase storage to 50GB
IPFS_PATH=/root/.ipfs ipfs config Datastore.StorageMax "50GB"

# ── 2. Create IPFS systemd service ──────────────────────────────────────

cat > /etc/systemd/system/ipfs.service << 'EOF'
[Unit]
Description=IPFS Daemon
After=network.target

[Service]
Type=simple
User=root
Environment=IPFS_PATH=/root/.ipfs
ExecStart=/usr/local/bin/ipfs daemon --enable-gc
Restart=always
RestartSec=5
LimitNOFILE=65536

[Install]
WantedBy=multi-user.target
EOF

# ── 3. Build gateway ────────────────────────────────────────────────────

echo "Building gateway..."
cd /opt/otter
git pull
source "$HOME/.cargo/env"
cargo build --release -p gateway

# ── 4. Create gateway config ────────────────────────────────────────────

# Get the relay peer ID from the running relay node
RELAY_PEER_ID=$(journalctl -u otter-relay --no-pager 2>/dev/null | grep -oP 'Peer ID: \K[^ ]+' | tail -1 || true)
PUBLIC_IP=$(curl -s ifconfig.me)

cat > /opt/otter/gateway-config.toml << TOML
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

[ipfs]
api_url = "http://127.0.0.1:5001"
timeout_seconds = 30
retry_attempts = 3

[security]
cors_enabled = true
allowed_origins = ["*"]
max_request_size_mb = 50

[monitoring]
metrics_enabled = true
health_check_interval_seconds = 30

[circuit_breaker]
failure_threshold = 5
reset_timeout_seconds = 30

[deploy]
max_size_mb = 100

[redis]
enabled = false

[telemetry]
enabled = false
service_name = "otter-gateway"

[naming]
enabled = true

[p2p]
enabled = true
tcp_port = 4002
bootstrap_peers = ["/ip4/${PUBLIC_IP}/tcp/4001/p2p/${RELAY_PEER_ID}"]
enable_mdns = false
resolve_timeout_seconds = 15
announce_content = true
TOML

echo "Gateway config written to /opt/otter/gateway-config.toml"

# ── 5. Create gateway systemd service ───────────────────────────────────

cat > /etc/systemd/system/otter-gateway.service << 'EOF'
[Unit]
Description=Otter Gateway (HTTP API + Content Serving)
After=network.target ipfs.service
Requires=ipfs.service

[Service]
Type=simple
User=root
WorkingDir=/opt/otter
Environment=RUST_LOG=info
Environment=SHADOWMESH_SERVER_HOST=0.0.0.0
Environment=SHADOWMESH_SERVER_PORT=8081
# Uncomment and set these for production:
# Environment=SHADOWMESH_API_KEYS=your-key-here
# Environment=GITHUB_CLIENT_ID=your-client-id
# Environment=GITHUB_CLIENT_SECRET=your-client-secret
# Environment=SHADOWMESH_SECURITY_ALLOWED_ORIGINS=https://your-domain.com
ExecStart=/opt/otter/target/release/gateway --config /opt/otter/gateway-config.toml
Restart=always
RestartSec=5
LimitNOFILE=65536

[Install]
WantedBy=multi-user.target
EOF

# ── 6. Open firewall ports ──────────────────────────────────────────────

if command -v ufw &> /dev/null; then
    ufw allow 8081/tcp    # Gateway API
    ufw allow 4002/tcp    # Gateway P2P (separate from relay 4001)
    echo "Firewall rules added (ufw)"
fi

iptables -I INPUT -p tcp --dport 8081 -j ACCEPT 2>/dev/null || true
iptables -I INPUT -p tcp --dport 4002 -j ACCEPT 2>/dev/null || true

# ── 7. Start everything ────────────────────────────────────────────────

systemctl daemon-reload

echo "Starting IPFS..."
systemctl enable ipfs
systemctl start ipfs

# Wait for IPFS to be ready
echo "Waiting for IPFS daemon..."
for i in $(seq 1 30); do
    if IPFS_PATH=/root/.ipfs ipfs id &>/dev/null; then
        echo "IPFS ready"
        break
    fi
    sleep 1
done

echo "Starting gateway..."
systemctl enable otter-gateway
systemctl start otter-gateway

# ── 8. Verify ───────────────────────────────────────────────────────────

echo ""
echo "Waiting for services to start..."
sleep 5

echo ""
echo "=== Service Status ==="
echo ""
echo "IPFS:"
systemctl is-active ipfs && IPFS_PATH=/root/.ipfs ipfs id 2>/dev/null | head -1 || echo "  not running"
echo ""
echo "Relay (existing):"
systemctl is-active otter-relay 2>/dev/null || echo "  not running"
echo ""
echo "Gateway:"
systemctl is-active otter-gateway 2>/dev/null || echo "  not running"
echo ""

# Health check
if curl -sf http://localhost:8081/health > /dev/null 2>&1; then
    echo "Gateway health check: OK"
else
    echo "Gateway health check: FAILED (check logs: journalctl -u otter-gateway -f)"
fi

echo ""
echo "=== Setup Complete ==="
echo ""
echo "Services running on this VPS:"
echo "  Relay:    port 4001  (P2P bootstrap)"
echo "  IPFS:     port 5001  (local API only)"
echo "  Gateway:  port 8081  (HTTP API + content)"
echo ""
echo "Access:"
echo "  Health:     http://${PUBLIC_IP}:8081/health"
echo "  Metrics:    http://${PUBLIC_IP}:8081/metrics"
echo "  Dashboard:  http://${PUBLIC_IP}:8081/"
echo "  Deploy:     http://${PUBLIC_IP}:8081/api/deploy"
echo ""
echo "Deploy the dashboard as content:"
echo "  GATEWAY=http://${PUBLIC_IP}:8081 ./scripts/deploy-dashboard.sh"
echo ""
echo "Useful commands:"
echo "  journalctl -u otter-gateway -f    # Gateway logs"
echo "  journalctl -u ipfs -f             # IPFS logs"
echo "  journalctl -u otter-relay -f      # Relay logs"
echo "  systemctl restart otter-gateway   # Restart gateway"
echo "  ipfs pin ls                       # List pinned content"
