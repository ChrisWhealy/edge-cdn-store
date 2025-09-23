#!/usr/bin/env bash
set -euo pipefail

RUNTIME_DIR="/tmp/edge-cdn-store"
PID_FILE="$RUNTIME_DIR/server.pid"

if [[ ! -f "$PID_FILE" ]]; then
  echo "⚠️   Failed to stop server: Missing file $PID_FILE"
else
  kill -TERM $(cat "$PID_FILE")
  rm "$PID_FILE"
fi

