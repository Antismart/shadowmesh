#!/bin/bash
# Run CI checks locally before pushing
# Usage: ./scripts/pre-push.sh

set -e  # Exit on first error

echo "=== Running CI checks locally ==="
echo ""

# 1. Format check
echo "1/4 Checking formatting..."
cargo fmt --all -- --check
echo "✓ Formatting OK"
echo ""

# 2. Clippy
echo "2/4 Running clippy..."
cargo clippy --workspace --all-targets -- -D warnings -A dead_code -A unused
echo "✓ Clippy OK"
echo ""

# 3. Tests
echo "3/4 Running tests..."
cargo test --workspace
echo "✓ Tests OK"
echo ""

# 4. Build
echo "4/4 Building release..."
cargo build --release --workspace
echo "✓ Build OK"
echo ""

echo "=== All CI checks passed! Safe to push. ==="
