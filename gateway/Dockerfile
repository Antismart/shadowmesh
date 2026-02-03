# ShadowMesh Gateway Dockerfile
# Multi-stage build for minimal production image

# Build stage
FROM rust:1.75-bookworm as builder

WORKDIR /app

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY protocol ./protocol
COPY gateway ./gateway
COPY node-runner ./node-runner
COPY benchmarks ./benchmarks

# Build release binary
RUN cargo build --release -p gateway

# Runtime stage
FROM debian:bookworm-slim

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
