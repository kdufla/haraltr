#!/usr/bin/env bash
set -euo pipefail

BINARY_NAME="daemon"
BINARY_PATH="$(pwd)/target/debug/$BINARY_NAME"
UNIT="bt-proximity-dev"
LOG_LEVEL="${1:-${RUST_LOG:-info}}"

echo "building"
cargo build

if [ ! -f "$BINARY_PATH" ]; then
    echo "error: $BINARY_PATH not found"
    exit 1
fi

trap 'sudo systemctl stop "$UNIT" 2>/dev/null || true; sudo systemctl reset-failed "$UNIT" 2>/dev/null || true' EXIT

# Stop previous instance and clear failed state
sudo systemctl stop "$UNIT" 2>/dev/null || true
sudo systemctl reset-failed "$UNIT" 2>/dev/null || true

echo "launching $BINARY_NAME"
START=$(date "+%Y-%m-%d %H:%M:%S")
sudo systemd-run \
    --system \
    --unit="$UNIT" \
    -E RUST_LOG="$LOG_LEVEL" \
    "$BINARY_PATH" run

echo "following journal (Ctrl+C to stop)"
journalctl -f --since="$START"
