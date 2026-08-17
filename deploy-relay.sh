#!/bin/bash
set -e

echo "=== Otter Relay Node Setup ==="

# Update system
apt update && apt upgrade -y

# Install build dependencies
apt install -y build-essential pkg-config libssl-dev git

# Install Rust
if ! command -v cargo &> /dev/null; then
    echo "Installing Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
else
    echo "Rust already installed"
    source "$HOME/.cargo/env"
fi

# Clone or update repo
if [ -d "/opt/otter" ]; then
    echo "Updating existing repo..."
    cd /opt/otter
    git pull
else
    echo "Cloning repo..."
    git clone https://github.com/Antismart/shadowmesh.git /opt/otter
    cd /opt/otter
fi

# Build release binary
echo "Building node-runner (this may take a few minutes)..."
cargo build --release -p node-runner

# Create config
cat > /opt/otter/node-config.toml << 'EOF'
[identity]
name = "Otter-Relay"
key_file = "/opt/otter/.data/identity.key"

[storage]
data_dir = "/opt/otter/.data/storage"

[network]
relay_mode = true
listen_addresses = ["/ip4/0.0.0.0/tcp/4001"]

[replication]
enabled = true
scan_interval_secs = 30

[dashboard]
host = "0.0.0.0"
port = 3030
EOF

# Create data directory
mkdir -p /opt/otter/.data/storage

# Generate a NODE_API_KEY (mandatory — the dashboard binds to 0.0.0.0 and the
# node refuses to start on a non-loopback bind without a key). Generate once and
# reuse on subsequent runs so the key is stable across redeploys.
API_KEY_ENV_FILE="/opt/otter/.data/api-key.env"
if [ ! -f "$API_KEY_ENV_FILE" ]; then
    echo "Generating NODE_API_KEY..."
    if command -v openssl &> /dev/null; then
        GENERATED_KEY=$(openssl rand -hex 32)
    else
        GENERATED_KEY=$(head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n')
    fi
    printf 'NODE_API_KEY=%s\n' "$GENERATED_KEY" > "$API_KEY_ENV_FILE"
    chmod 600 "$API_KEY_ENV_FILE"
    echo "NODE_API_KEY written to $API_KEY_ENV_FILE (keep this secret)"
else
    echo "Reusing existing NODE_API_KEY from $API_KEY_ENV_FILE"
fi

# Create systemd service
cat > /etc/systemd/system/otter-relay.service << 'EOF'
[Unit]
Description=Otter Relay Node
After=network.target

[Service]
Type=simple
User=root
WorkingDir=/opt/otter
Environment=SHADOWMESH_CONFIG=/opt/otter/node-config.toml
Environment=RUST_LOG=info
EnvironmentFile=/opt/otter/.data/api-key.env
ExecStart=/opt/otter/target/release/node-runner
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

# Open firewall ports
if command -v ufw &> /dev/null; then
    ufw allow 4001/tcp
    ufw allow 3030/tcp
    echo "Firewall rules added (ufw)"
fi

# Also handle iptables directly
iptables -I INPUT -p tcp --dport 4001 -j ACCEPT 2>/dev/null || true
iptables -I INPUT -p tcp --dport 3030 -j ACCEPT 2>/dev/null || true

# Enable and start service
systemctl daemon-reload
systemctl enable otter-relay
systemctl start otter-relay

echo ""
echo "=== Setup Complete ==="
echo ""
echo "Checking status..."
sleep 3
systemctl status otter-relay --no-pager -l | head -20
echo ""
echo "Dashboard: http://$(curl -s ifconfig.me):3030"
echo ""
echo "API authentication is REQUIRED. The key lives in $API_KEY_ENV_FILE."
echo "Use it with the CLI, e.g.:"
echo "  export \$(cat $API_KEY_ENV_FILE)"
echo "  shadowmesh-cli --node-url http://<host>:3030 status"
echo ""
echo "To check logs: journalctl -u otter-relay -f"
echo "To get the relay address: journalctl -u otter-relay | grep 'Peer ID'"
