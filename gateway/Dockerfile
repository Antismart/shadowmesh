# ShadowMesh Gateway Dockerfile
# Multi-stage build: Node (dashboard) → Rust (gateway) → minimal runtime

# Stage 1: Build React dashboard
# Pinned 2025-12-17 – update periodically from https://hub.docker.com/_/node
FROM node:20.18.1-slim AS dashboard-builder

WORKDIR /app/dashboard
COPY dashboard/package.json dashboard/package-lock.json* ./
RUN npm ci
COPY dashboard/ ./
RUN npm run build

# Stage 2: Build Rust gateway (embeds dashboard/dist via rust-embed)
# Pinned 2025-12-17 – update periodically from https://hub.docker.com/_/rust
FROM rust:1.93.1-bookworm AS builder

WORKDIR /app

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY protocol ./protocol
COPY gateway ./gateway
COPY node-runner ./node-runner
COPY benchmarks ./benchmarks
COPY sdk-browser ./sdk-browser

# Copy built dashboard into expected location for rust-embed
COPY --from=dashboard-builder /app/dashboard/dist ./dashboard/dist

# Build release binary
RUN cargo build --release -p gateway

# Stage 3: Minimal runtime
# Pinned 2025-12-17 – update periodically from https://hub.docker.com/_/debian
FROM debian:bookworm-20250113-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -r -s /bin/false shadowmesh

WORKDIR /app

# Copy binary from builder
COPY --from=builder /app/target/release/gateway /usr/local/bin/gateway

# Copy default config
COPY gateway/config.toml /app/config.toml

# Set ownership
RUN chown -R shadowmesh:shadowmesh /app

# Switch to non-root user
USER shadowmesh

# Expose port
EXPOSE 8081

# Health check
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8081/health || exit 1

# Environment variables
ENV RUST_LOG=info
ENV SHADOWMESH_SERVER_HOST=0.0.0.0
ENV SHADOWMESH_SERVER_PORT=8081

# Run gateway
CMD ["gateway"]
