#!/usr/bin/env bash
#
# Deploy the ShadowMesh dashboard as static content on the network.
#
# Usage:
#   ./scripts/deploy-dashboard.sh [--gateway URL]
#
# Prerequisites:
#   - Node.js 18+ (for dashboard build)
#   - A running IPFS daemon  OR  a reachable gateway with /api/upload
#
set -euo pipefail

GATEWAY="${GATEWAY:-http://localhost:8081}"
DASHBOARD_DIR="$(cd "$(dirname "$0")/../dashboard" && pwd)"
DIST_DIR="$DASHBOARD_DIR/dist"

# Parse flags
while [[ $# -gt 0 ]]; do
  case "$1" in
    --gateway) GATEWAY="$2"; shift 2 ;;
    *) echo "Unknown option: $1"; exit 1 ;;
  esac
done

echo "==> Building dashboard..."
(cd "$DASHBOARD_DIR" && npm run build)

echo "==> Dashboard built to $DIST_DIR"

# ── Upload via IPFS CLI (if available) ───────────────────────────────────
if command -v ipfs &>/dev/null; then
  echo "==> Uploading to IPFS (local daemon)..."
  CID=$(ipfs add -r -Q "$DIST_DIR")
  echo "    Root CID: $CID"

  # Pin on gateways
  echo "==> Pinning on gateway ($GATEWAY)..."
  curl -sf -X POST "$GATEWAY/api/pin/$CID" || echo "    (pin failed or gateway unreachable)"

  echo ""
  echo "Dashboard deployed!"
  echo ""
  echo "Access via:"
  echo "  Gateway:  $GATEWAY/$CID"
  echo "  Local:    http://localhost:8081/$CID"
  echo ""
  echo "CID: $CID"
  exit 0
fi

# ── Fallback: upload through gateway /api/deploy endpoint ────────────────
echo "==> ipfs CLI not found — uploading via gateway..."

# Zip the dist directory and upload
TMPZIP=$(mktemp /tmp/dashboard-XXXXXX.zip)
(cd "$DIST_DIR" && zip -r -q "$TMPZIP" .)

RESPONSE=$(curl -sf -X POST "$GATEWAY/api/deploy" -F "file=@$TMPZIP")
rm -f "$TMPZIP"

CID=$(echo "$RESPONSE" | python3 -c "import sys,json; print(json.load(sys.stdin).get('cid',''))" 2>/dev/null || true)

if [[ -z "$CID" ]]; then
  echo "Upload failed. Response:"
  echo "$RESPONSE"
  exit 1
fi

# Pin it
echo "==> Pinning CID $CID..."
curl -sf -X POST "$GATEWAY/api/pin/$CID" || true

echo ""
echo "Dashboard deployed!"
echo ""
echo "Access via:"
echo "  Gateway:  $GATEWAY/$CID"
echo "  Local:    http://localhost:8081/$CID"
echo ""
echo "CID: $CID"
