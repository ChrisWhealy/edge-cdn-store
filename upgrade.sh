#!/usr/bin/env bash
set -euo pipefail

RUNTIME_DIR="/tmp/edge-cdn-store"
CONF_FILE="$RUNTIME_DIR/conf.yaml"
PID="$RUNTIME_DIR/server.pid"
OLD_PID=$(cat "$PID")

# Prepare old process to hand over to new process
kill -QUIT "$OLD_PID"

exec cargo run --release -- --conf "$CONF_FILE" --upgrade
