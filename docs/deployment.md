# ShadowMesh Deployment Guide

This guide covers deploying ShadowMesh in various environments.

## Table of Contents

- [Prerequisites](#prerequisites)
- [Local Development](#local-development)
- [Docker Deployment](#docker-deployment)
- [Production Deployment](#production-deployment)
- [Configuration Reference](#configuration-reference)
- [Monitoring](#monitoring)
- [Troubleshooting](#troubleshooting)

## Prerequisites

### System Requirements

| Component | Minimum | Recommended |
|-----------|---------|-------------|
| CPU | 2 cores | 4+ cores |
| RAM | 2 GB | 8+ GB |
| Storage | 10 GB | 100+ GB |
| Network | 10 Mbps | 100+ Mbps |

### Software Requirements

- Rust 1.70+ (for building from source)
- Node.js 18+ (for SDK)
- Docker 20+ (for containerized deployment)

## Local Development

### Building from Source

```bash
# Clone repository
git clone https://github.com/Antismart/shadowmesh.git
cd shadowmesh

# Build all components
cargo build --release

# Binaries will be in target/release/
ls target/release/gateway target/release/node-runner target/release/protocol
```

### Running Locally

```bash
# Terminal 1: Start Gateway
cargo run -p gateway

# Terminal 2: Start Node Runner
cargo run -p node-runner

# Terminal 3: Test with curl
curl http://localhost:3000/health
```

### Development Configuration

Create `gateway.toml` for local development:

```toml
[server]
host = "127.0.0.1"
port = 3000

[cache]
max_size_mb = 256
ttl_seconds = 300

[rate_limit]
enabled = false

[cors]
enabled = true
origins = ["http://localhost:*"]

[logging]
level = "debug"
format = "pretty"
```

## Docker Deployment

### Dockerfile

Create `Dockerfile`:

```dockerfile
# Build stage
FROM rust:1.75-alpine AS builder

RUN apk add --no-cache musl-dev

WORKDIR /app
COPY . .

RUN cargo build --release -p gateway -p node-runner

# Runtime stage
FROM alpine:3.19

RUN apk add --no-cache ca-certificates

COPY --from=builder /app/target/release/gateway /usr/local/bin/
COPY --from=builder /app/target/release/node-runner /usr/local/bin/

EXPOSE 3000 8080

CMD ["gateway"]
```

### Docker Compose

Create `docker-compose.yml`:

```yaml
version: '3.8'

services:
  gateway:
    build:
      context: .
      dockerfile: Dockerfile
    command: gateway
    ports:
      - "3000:3000"
    environment:
      - SHADOWMESH_HOST=0.0.0.0
      - SHADOWMESH_PORT=3000
      - RUST_LOG=info
    volumes:
      - gateway-data:/data
    healthcheck:
      test: ["CMD", "wget", "-q", "--spider", "http://localhost:3000/health"]
      interval: 30s
      timeout: 10s
      retries: 3
    restart: unless-stopped

  node-runner:
    build:
      context: .
      dockerfile: Dockerfile
    command: node-runner
    ports:
      - "8080:8080"
      - "4001:4001"
    environment:
      - SHADOWMESH_DATA_DIR=/data
      - RUST_LOG=info
    volumes:
      - node-data:/data
    depends_on:
      - gateway
    restart: unless-stopped

volumes:
  gateway-data:
  node-data:
```

### Running with Docker

```bash
# Build and start
docker-compose up -d

# View logs
docker-compose logs -f

# Stop
docker-compose down

# Stop and remove volumes
docker-compose down -v
```

## Production Deployment

### System Setup

```bash
# Create system user
sudo useradd -r -s /bin/false shadowmesh

# Create directories
sudo mkdir -p /var/lib/shadowmesh/{gateway,node}
sudo mkdir -p /etc/shadowmesh
sudo mkdir -p /var/log/shadowmesh

# Set permissions
sudo chown -R shadowmesh:shadowmesh /var/lib/shadowmesh
sudo chown -R shadowmesh:shadowmesh /var/log/shadowmesh
```

### Systemd Services

#### Gateway Service

Create `/etc/systemd/system/shadowmesh-gateway.service`:

```ini
[Unit]
Description=ShadowMesh Gateway
After=network.target

[Service]
Type=simple
User=shadowmesh
Group=shadowmesh
WorkingDirectory=/var/lib/shadowmesh/gateway
ExecStart=/usr/local/bin/gateway
Restart=always
RestartSec=5

Environment=RUST_LOG=info
Environment=SHADOWMESH_CONFIG=/etc/shadowmesh/gateway.toml

# Security
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
ReadWritePaths=/var/lib/shadowmesh/gateway /var/log/shadowmesh

# Resource limits
LimitNOFILE=65535
MemoryMax=2G

[Install]
WantedBy=multi-user.target
```

#### Node Runner Service

Create `/etc/systemd/system/shadowmesh-node.service`:

```ini
[Unit]
Description=ShadowMesh Node Runner
After=network.target shadowmesh-gateway.service

[Service]
Type=simple
User=shadowmesh
Group=shadowmesh
WorkingDirectory=/var/lib/shadowmesh/node
ExecStart=/usr/local/bin/node-runner
Restart=always
RestartSec=5

Environment=RUST_LOG=info
Environment=SHADOWMESH_CONFIG=/etc/shadowmesh/node.toml

# Security
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
ReadWritePaths=/var/lib/shadowmesh/node /var/log/shadowmesh

# Resource limits
LimitNOFILE=65535
MemoryMax=4G

[Install]
WantedBy=multi-user.target
```

### Enable Services

```bash
# Reload systemd
sudo systemctl daemon-reload

# Enable services
sudo systemctl enable shadowmesh-gateway
sudo systemctl enable shadowmesh-node

# Start services
sudo systemctl start shadowmesh-gateway
sudo systemctl start shadowmesh-node

# Check status
sudo systemctl status shadowmesh-gateway
sudo systemctl status shadowmesh-node
```

### Nginx Reverse Proxy

Create `/etc/nginx/sites-available/shadowmesh`:

```nginx
upstream gateway {
    server 127.0.0.1:3000;
    keepalive 32;
}

upstream dashboard {
    server 127.0.0.1:8080;
}

server {
    listen 80;
    server_name shadowmesh.example.com;
    return 301 https://$server_name$request_uri;
}

server {
    listen 443 ssl http2;
    server_name shadowmesh.example.com;

    ssl_certificate /etc/letsencrypt/live/shadowmesh.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/shadowmesh.example.com/privkey.pem;
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers HIGH:!aNULL:!MD5;

    # Gateway API
    location / {
        proxy_pass http://gateway;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_set_header Connection "";
        
        # Timeouts
        proxy_connect_timeout 60s;
        proxy_send_timeout 60s;
        proxy_read_timeout 60s;
        
        # Buffer settings
        proxy_buffering on;
        proxy_buffer_size 4k;
        proxy_buffers 8 4k;
    }

    # Dashboard
    location /dashboard/ {
        proxy_pass http://dashboard/;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
    }

    # Health check (no auth)
    location /health {
        proxy_pass http://gateway/health;
        access_log off;
    }

    # Metrics (internal only)
    location /metrics {
        allow 10.0.0.0/8;
        allow 172.16.0.0/12;
        allow 192.168.0.0/16;
        deny all;
        proxy_pass http://gateway/metrics;
    }
}
```

### SSL/TLS Setup

```bash
# Install certbot
sudo apt install certbot python3-certbot-nginx

# Get certificate
sudo certbot --nginx -d shadowmesh.example.com

# Enable auto-renewal
sudo systemctl enable certbot.timer
```

### Firewall Configuration

```bash
# UFW
sudo ufw allow 80/tcp
sudo ufw allow 443/tcp
sudo ufw allow 4001/tcp  # P2P port
sudo ufw enable

# iptables
sudo iptables -A INPUT -p tcp --dport 80 -j ACCEPT
sudo iptables -A INPUT -p tcp --dport 443 -j ACCEPT
sudo iptables -A INPUT -p tcp --dport 4001 -j ACCEPT
```

## Configuration Reference

### Gateway Configuration

`/etc/shadowmesh/gateway.toml`:

```toml
[server]
# Bind address
host = "0.0.0.0"
port = 3000

# Maximum request body size
max_body_size = "10MB"

# Request timeout
timeout_seconds = 30

[cache]
# Enable caching
enabled = true

# Maximum cache size
max_size_mb = 512

# Cache TTL
ttl_seconds = 3600

# Eviction strategy: "lru" | "lfu"
eviction = "lru"

[rate_limit]
# Enable rate limiting
enabled = true

# Requests per second per IP
requests_per_second = 100

# Burst size
burst = 200

# Whitelist IPs
whitelist = ["10.0.0.0/8"]

[cors]
# Enable CORS
enabled = true

# Allowed origins
origins = ["https://app.example.com"]

# Allowed methods
methods = ["GET", "POST", "OPTIONS"]

# Allowed headers
headers = ["Content-Type", "Authorization"]

# Max age for preflight
max_age_seconds = 86400

[storage]
# Storage backend: "memory" | "disk" | "ipfs"
backend = "disk"

# Data directory
data_dir = "/var/lib/shadowmesh/gateway/data"

# IPFS API URL (if using IPFS backend)
ipfs_url = "http://localhost:5001"

[logging]
# Log level: "trace" | "debug" | "info" | "warn" | "error"
level = "info"

# Log format: "json" | "pretty"
format = "json"

# Log file
file = "/var/log/shadowmesh/gateway.log"
```

### Node Runner Configuration

`/etc/shadowmesh/node.toml`:

```toml
[node]
# Node data directory
data_dir = "/var/lib/shadowmesh/node"

# Maximum storage
max_storage_gb = 100

# Enable garbage collection
gc_enabled = true

# GC interval
gc_interval_hours = 24

[network]
# libp2p listen address
listen_addr = "/ip4/0.0.0.0/tcp/4001"

# External address (for NAT)
external_addr = "/ip4/203.0.113.1/tcp/4001"

# Bootstrap peers
bootstrap_peers = [
    "/ip4/104.131.131.82/tcp/4001/p2p/QmaCpDMGvV2BGHeYERUEnRQAwe3N8SzbUtfsmvsqQLuvuJ",
]

# Maximum connections
max_connections = 100

# Connection timeout
connection_timeout_seconds = 10

[replication]
# Target replication factor
factor = 3

# Minimum replication factor
min_factor = 1

# Replication check interval
check_interval_seconds = 300

# Enable automatic repair
auto_repair = true

[bandwidth]
# Inbound limit (bytes/sec, 0 = unlimited)
inbound_limit = 0

# Outbound limit
outbound_limit = 0

# Enable per-peer limits
per_peer_limits = true
per_peer_limit = 10485760  # 10MB/s

[dashboard]
# Enable web dashboard
enabled = true

# Dashboard bind address
host = "127.0.0.1"
port = 8080

# Enable authentication
auth_enabled = false
auth_username = "admin"
auth_password = "changeme"

[logging]
level = "info"
format = "json"
file = "/var/log/shadowmesh/node.log"
```

## Monitoring

### Prometheus Integration

Add to `prometheus.yml`:

```yaml
scrape_configs:
  - job_name: 'shadowmesh-gateway'
    static_configs:
      - targets: ['localhost:3000']
    metrics_path: '/metrics'
    scrape_interval: 15s

  - job_name: 'shadowmesh-node'
    static_configs:
      - targets: ['localhost:8080']
    metrics_path: '/api/metrics'
    scrape_interval: 15s
```

### Grafana Dashboard

Import the ShadowMesh dashboard from `docs/grafana-dashboard.json` or create panels for:

- Request rate (requests/sec)
- Error rate (%)
- Response latency (p50, p95, p99)
- Cache hit rate (%)
- Bandwidth (in/out)
- Peer connections
- Storage usage
- Replication health

### Alerting Rules

```yaml
groups:
  - name: shadowmesh
    rules:
      - alert: GatewayDown
        expr: up{job="shadowmesh-gateway"} == 0
        for: 1m
        labels:
          severity: critical
        annotations:
          summary: "ShadowMesh Gateway is down"

      - alert: HighErrorRate
        expr: rate(shadowmesh_requests_error[5m]) / rate(shadowmesh_requests_total[5m]) > 0.05
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "High error rate (> 5%)"

      - alert: LowPeerConnections
        expr: shadowmesh_peers_connected < 5
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Low peer connections (< 5)"

      - alert: StorageFull
        expr: shadowmesh_storage_used / shadowmesh_storage_max > 0.9
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Storage nearly full (> 90%)"
```

## Troubleshooting

### Common Issues

#### Gateway won't start

```bash
# Check logs
journalctl -u shadowmesh-gateway -f

# Check configuration
gateway --check-config

# Check port availability
ss -tlnp | grep 3000
```

#### Node can't connect to peers

```bash
# Check firewall
sudo ufw status
sudo iptables -L

# Check NAT traversal
# Ensure port 4001 is forwarded

# Check bootstrap peers
curl http://localhost:8080/api/peers
```

#### High memory usage

```bash
# Check current usage
ps aux | grep shadowmesh

# Reduce cache size in config
# [cache]
# max_size_mb = 256

# Restart service
sudo systemctl restart shadowmesh-gateway
```

#### Slow content retrieval

```bash
# Check cache hit rate
curl http://localhost:3000/metrics | grep cache

# Check peer latencies
curl http://localhost:8080/api/peers

# Enable debug logging temporarily
export RUST_LOG=debug
```

### Log Analysis

```bash
# View recent errors
journalctl -u shadowmesh-gateway --since "1 hour ago" | grep -i error

# Count requests by status
cat /var/log/shadowmesh/gateway.log | jq -r '.status' | sort | uniq -c

# Find slow requests
cat /var/log/shadowmesh/gateway.log | jq 'select(.duration_ms > 1000)'
```

### Health Checks

```bash
# Gateway health
curl -f http://localhost:3000/health || echo "Gateway unhealthy"

# Node health
curl -f http://localhost:8080/api/status || echo "Node unhealthy"

# Full system check
./scripts/healthcheck.sh
```

### Backup & Recovery

```bash
# Backup node data
tar -czf shadowmesh-backup-$(date +%Y%m%d).tar.gz /var/lib/shadowmesh

# Restore
sudo systemctl stop shadowmesh-node shadowmesh-gateway
tar -xzf shadowmesh-backup-*.tar.gz -C /
sudo systemctl start shadowmesh-gateway shadowmesh-node
```
