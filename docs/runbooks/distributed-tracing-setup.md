# Guide: Distributed Tracing with Jaeger

## Overview
ShadowMesh Gateway supports distributed tracing via OpenTelemetry Protocol (OTLP). This guide explains how to configure Jaeger to receive traces.

## Prerequisites
- Jaeger 1.35+ (supports OTLP natively)
- Or any OTLP-compatible backend (Tempo, Honeycomb, etc.)

## Deployment Options

### Option 1: Jaeger All-in-One (Development)

```bash
docker run -d --name jaeger \
  -p 16686:16686 \
  -p 4317:4317 \
  -p 4318:4318 \
  jaegertracing/all-in-one:latest
```

### Option 2: Jaeger on Kubernetes

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: jaeger
spec:
  replicas: 1
  selector:
    matchLabels:
      app: jaeger
  template:
    metadata:
      labels:
        app: jaeger
    spec:
      containers:
      - name: jaeger
        image: jaegertracing/all-in-one:latest
        ports:
        - containerPort: 16686  # UI
        - containerPort: 4317   # OTLP gRPC
        - containerPort: 4318   # OTLP HTTP
---
apiVersion: v1
kind: Service
metadata:
  name: jaeger
spec:
  selector:
    app: jaeger
  ports:
  - name: ui
    port: 16686
  - name: otlp-grpc
    port: 4317
  - name: otlp-http
    port: 4318
```

## Gateway Configuration

### Environment Variables

```bash
# Enable telemetry
OTEL_EXPORTER_OTLP_ENDPOINT=http://jaeger:4317

# Or via config.toml
```

### config.toml

```toml
[telemetry]
enabled = true
otlp_endpoint = "http://jaeger:4317"
service_name = "shadowmesh-gateway"
```

## Verification

1. **Start the gateway with telemetry enabled**

2. **Make some requests**
   ```bash
   curl http://localhost:8081/health
   curl http://localhost:8081/dashboard
   ```

3. **Open Jaeger UI**
   - Navigate to http://localhost:16686
   - Select service: "shadowmesh-gateway"
   - Click "Find Traces"

4. **Verify spans appear**
   - Each HTTP request should create a trace
   - Look for spans like:
     - `HTTP GET /health`
     - `HTTP GET /dashboard`

## Troubleshooting

### No traces appearing

1. Check gateway logs for OTLP errors:
   ```bash
   kubectl logs -l app=shadowmesh-gateway | grep -i otel
   ```

2. Verify endpoint connectivity:
   ```bash
   kubectl exec -it deploy/shadowmesh-gateway -- nc -zv jaeger 4317
   ```

3. Check Jaeger is receiving data:
   ```bash
   kubectl logs -l app=jaeger | grep -i "received"
   ```

### High latency from tracing

- Reduce sampling rate in production
- Use async batch export (default)
- Consider using a collector for buffering

## Production Recommendations

1. **Use Jaeger Collector + Storage backend**
   - Elasticsearch or Cassandra for persistence
   - Collector for buffering and processing

2. **Set appropriate sampling**
   - 100% sampling in dev/staging
   - 1-10% sampling in production (adjust based on volume)

3. **Add trace context propagation**
   - Traces will automatically propagate via W3C Trace Context headers
   - Ensure downstream services also support OTLP
