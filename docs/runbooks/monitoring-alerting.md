# ShadowMesh Monitoring and Alerting Runbook

This runbook covers monitoring setup, alerting rules, and incident response procedures for ShadowMesh deployments.

## Overview

ShadowMesh provides built-in metrics via Prometheus format at `/metrics` endpoint on both the gateway and node-runner components.

## Metrics Endpoints

| Component | Endpoint | Port |
|-----------|----------|------|
| Gateway | `/metrics` | 3000 |
| Node Runner | `/metrics` | 8080 |

## Key Metrics

### Gateway Metrics

```
# Request metrics
shadowmesh_gateway_requests_total{method, path, status}
shadowmesh_gateway_request_duration_seconds{method, path}
shadowmesh_gateway_request_size_bytes{method, path}
shadowmesh_gateway_response_size_bytes{method, path}

# Cache metrics
shadowmesh_cache_hits_total
shadowmesh_cache_misses_total
shadowmesh_cache_size_bytes
shadowmesh_cache_evictions_total

# Rate limiting
shadowmesh_rate_limit_exceeded_total{client_id}
shadowmesh_rate_limit_remaining{client_id}

# Circuit breaker
shadowmesh_circuit_breaker_state{backend}  # 0=closed, 1=half-open, 2=open
shadowmesh_circuit_breaker_failures_total{backend}

# WebRTC signaling
shadowmesh_signaling_connections_total
shadowmesh_signaling_active_sessions
shadowmesh_signaling_messages_total{type}
```

### Node Runner Metrics

```
# P2P network
shadowmesh_peers_connected
shadowmesh_peers_discovered_total
shadowmesh_peer_connection_failures_total

# Content metrics
shadowmesh_content_requests_total{type}
shadowmesh_content_served_bytes_total
shadowmesh_content_received_bytes_total

# Storage
shadowmesh_storage_used_bytes
shadowmesh_storage_available_bytes
shadowmesh_fragments_stored

# Bandwidth
shadowmesh_bandwidth_inbound_bytes_per_second
shadowmesh_bandwidth_outbound_bytes_per_second

# Replication
shadowmesh_replication_factor{cid}
shadowmesh_replication_pending
```

## Prometheus Configuration

### prometheus.yml

```yaml
global:
  scrape_interval: 15s
  evaluation_interval: 15s

alerting:
  alertmanagers:
    - static_configs:
        - targets:
          - alertmanager:9093

rule_files:
  - /etc/prometheus/rules/*.yml

scrape_configs:
  - job_name: 'shadowmesh-gateway'
    static_configs:
      - targets: ['gateway:3000']
    metrics_path: /metrics

  - job_name: 'shadowmesh-nodes'
    static_configs:
      - targets:
        - 'node1:8080'
        - 'node2:8080'
        - 'node3:8080'
    metrics_path: /metrics

  - job_name: 'shadowmesh-nodes-k8s'
    kubernetes_sd_configs:
      - role: pod
    relabel_configs:
      - source_labels: [__meta_kubernetes_pod_label_app]
        regex: shadowmesh-node
        action: keep
      - source_labels: [__meta_kubernetes_pod_container_port_number]
        regex: "8080"
        action: keep
```

## Alerting Rules

### alerts.yml

```yaml
groups:
  - name: shadowmesh-gateway
    rules:
      # High error rate
      - alert: GatewayHighErrorRate
        expr: |
          sum(rate(shadowmesh_gateway_requests_total{status=~"5.."}[5m]))
          /
          sum(rate(shadowmesh_gateway_requests_total[5m])) > 0.05
        for: 5m
        labels:
          severity: critical
        annotations:
          summary: "High error rate on gateway"
          description: "Gateway error rate is {{ $value | humanizePercentage }} over the last 5 minutes"

      # High latency
      - alert: GatewayHighLatency
        expr: |
          histogram_quantile(0.95,
            sum(rate(shadowmesh_gateway_request_duration_seconds_bucket[5m])) by (le)
          ) > 2
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "High latency on gateway"
          description: "95th percentile latency is {{ $value }}s"

      # Circuit breaker open
      - alert: CircuitBreakerOpen
        expr: shadowmesh_circuit_breaker_state == 2
        for: 1m
        labels:
          severity: critical
        annotations:
          summary: "Circuit breaker is open for {{ $labels.backend }}"
          description: "Backend {{ $labels.backend }} circuit breaker has been open for over 1 minute"

      # Rate limiting triggered
      - alert: RateLimitExceeded
        expr: increase(shadowmesh_rate_limit_exceeded_total[5m]) > 100
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "High rate limit violations"
          description: "{{ $value }} rate limit violations in the last 5 minutes"

  - name: shadowmesh-nodes
    rules:
      # Low peer count
      - alert: LowPeerCount
        expr: shadowmesh_peers_connected < 3
        for: 10m
        labels:
          severity: warning
        annotations:
          summary: "Low peer count on {{ $labels.instance }}"
          description: "Node {{ $labels.instance }} has only {{ $value }} connected peers"

      # No peers
      - alert: NoPeers
        expr: shadowmesh_peers_connected == 0
        for: 5m
        labels:
          severity: critical
        annotations:
          summary: "No peers connected on {{ $labels.instance }}"
          description: "Node {{ $labels.instance }} has no connected peers"

      # Storage full
      - alert: StorageNearlyFull
        expr: |
          shadowmesh_storage_used_bytes
          /
          (shadowmesh_storage_used_bytes + shadowmesh_storage_available_bytes) > 0.9
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Storage nearly full on {{ $labels.instance }}"
          description: "Storage is {{ $value | humanizePercentage }} full"

      # High bandwidth usage
      - alert: HighBandwidthUsage
        expr: |
          shadowmesh_bandwidth_outbound_bytes_per_second > 100000000  # 100 MB/s
        for: 10m
        labels:
          severity: warning
        annotations:
          summary: "High bandwidth usage on {{ $labels.instance }}"
          description: "Outbound bandwidth is {{ $value | humanize1024 }}B/s"

  - name: shadowmesh-signaling
    rules:
      # Signaling server down
      - alert: SignalingServerDown
        expr: up{job="shadowmesh-gateway"} == 0
        for: 1m
        labels:
          severity: critical
        annotations:
          summary: "Signaling server is down"
          description: "The signaling server has been down for over 1 minute"

      # High session count
      - alert: HighSignalingSessions
        expr: shadowmesh_signaling_active_sessions > 900
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "High number of signaling sessions"
          description: "{{ $value }} active signaling sessions (approaching limit)"
```

## Grafana Dashboards

### Dashboard JSON Import

Import the following dashboards into Grafana:

1. **ShadowMesh Overview** - High-level health metrics
2. **Gateway Performance** - Request rates, latencies, errors
3. **P2P Network** - Peer connections, content distribution
4. **WebRTC Signaling** - WebRTC session metrics

### Key Panels

#### Request Rate Panel
```json
{
  "title": "Request Rate",
  "type": "graph",
  "targets": [
    {
      "expr": "sum(rate(shadowmesh_gateway_requests_total[5m])) by (status)",
      "legendFormat": "{{status}}"
    }
  ]
}
```

#### Peer Count Panel
```json
{
  "title": "Connected Peers",
  "type": "stat",
  "targets": [
    {
      "expr": "sum(shadowmesh_peers_connected)"
    }
  ]
}
```

## Incident Response

### High Error Rate

1. Check the error logs:
   ```bash
   kubectl logs -l app=shadowmesh-gateway --tail=100 | grep ERROR
   ```

2. Check backend health:
   ```bash
   kubectl get pods -l app=shadowmesh-node
   ```

3. Check circuit breaker status:
   ```bash
   curl http://gateway:3000/health | jq .circuit_breakers
   ```

4. If backends are down, scale up:
   ```bash
   kubectl scale deployment shadowmesh-node --replicas=5
   ```

### No Peers Connected

1. Check network connectivity:
   ```bash
   kubectl exec -it shadowmesh-node-0 -- ping -c 3 shadowmesh-node-1
   ```

2. Check bootstrap peers configuration:
   ```bash
   kubectl get configmap shadowmesh-config -o yaml | grep bootstrap
   ```

3. Restart affected nodes:
   ```bash
   kubectl rollout restart deployment shadowmesh-node
   ```

### Storage Full

1. Check storage usage:
   ```bash
   kubectl exec -it shadowmesh-node-0 -- df -h /data
   ```

2. Clear old cache:
   ```bash
   kubectl exec -it shadowmesh-node-0 -- ./shadowmesh-node cache clear --older-than 7d
   ```

3. Increase PVC size (if supported):
   ```bash
   kubectl patch pvc shadowmesh-data -p '{"spec":{"resources":{"requests":{"storage":"100Gi"}}}}'
   ```

### Circuit Breaker Open

1. Identify the failing backend:
   ```bash
   curl http://gateway:3000/metrics | grep circuit_breaker
   ```

2. Check backend health:
   ```bash
   kubectl logs -l app=shadowmesh-node --tail=50
   ```

3. Force circuit breaker reset (if backend is healthy):
   ```bash
   curl -X POST http://gateway:3000/admin/circuit-breaker/reset
   ```

## Log Aggregation

### Loki Configuration

```yaml
# loki-config.yaml
scrape_configs:
  - job_name: shadowmesh
    static_configs:
      - targets:
          - localhost
        labels:
          job: shadowmesh
          __path__: /var/log/shadowmesh/*.log
```

### Log Queries

```logql
# All errors
{job="shadowmesh"} |= "ERROR"

# WebRTC signaling errors
{job="shadowmesh"} | json | level="error" | component="signaling"

# Slow requests (>1s)
{job="shadowmesh"} | json | duration > 1000

# Rate limit events
{job="shadowmesh"} |= "rate_limit_exceeded"
```

## Health Checks

### Kubernetes Probes

```yaml
livenessProbe:
  httpGet:
    path: /health
    port: 3000
  initialDelaySeconds: 10
  periodSeconds: 10

readinessProbe:
  httpGet:
    path: /ready
    port: 3000
  initialDelaySeconds: 5
  periodSeconds: 5
```

### Health Endpoint Response

```json
{
  "status": "healthy",
  "components": {
    "signaling": "healthy",
    "cache": "healthy",
    "circuit_breakers": {
      "ipfs_backend": "closed"
    }
  },
  "version": "0.1.0",
  "uptime_seconds": 86400
}
```

## SLOs and SLIs

### Service Level Indicators (SLIs)

| SLI | Target | Measurement |
|-----|--------|-------------|
| Availability | 99.9% | `up{job="shadowmesh-gateway"}` |
| Latency (p95) | < 500ms | `histogram_quantile(0.95, ...)` |
| Error Rate | < 0.1% | `rate(requests{status=~"5.."})/rate(requests)` |
| Content Delivery | 99% | Successful fetches / Total fetches |

### SLO Dashboard

Create burn rate alerts based on error budget:

```yaml
- alert: ErrorBudgetBurnRate
  expr: |
    sum(rate(shadowmesh_gateway_requests_total{status=~"5.."}[1h]))
    /
    sum(rate(shadowmesh_gateway_requests_total[1h])) > 14.4 * 0.001
  for: 5m
  labels:
    severity: critical
  annotations:
    summary: "Error budget burning too fast"
```

## Contact and Escalation

| Level | Contact | Trigger |
|-------|---------|---------|
| L1 | On-call engineer | Warning alerts |
| L2 | Team lead | Critical alerts > 15min |
| L3 | Engineering manager | Outage > 30min |

## Useful Commands

```bash
# Check all pod status
kubectl get pods -l app=shadowmesh -o wide

# View recent events
kubectl get events --sort-by='.lastTimestamp' | grep shadowmesh

# Port forward to metrics
kubectl port-forward svc/shadowmesh-gateway 3000:3000

# View real-time logs
kubectl logs -f -l app=shadowmesh-gateway

# Check resource usage
kubectl top pods -l app=shadowmesh
```
