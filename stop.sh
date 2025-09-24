#!/usr/bin/env bash
set -euo pipefail

RUNTIME_DIR="/tmp/edge-cdn-store"
PID_FILE="$RUNTIME_DIR/server.pid"

if [[ ! -f "$PID_FILE" ]]; then
  echo "⚠️  Unable to stop server: Missing file $PID_FILE"
else
  PID=$(cat "$PID_FILE")
  rm "$PID_FILE"
  kill -TERM $PID
fi

