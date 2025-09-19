#!/usr/bin/env bash
set -euo pipefail

RUNTIME_DIR="/tmp/edge-cdn-store"
CONF_SRC="${1:-conf.yaml}"

mkdir -p "$RUNTIME_DIR/cache"
mkdir -p "$RUNTIME_DIR/keys"
chmod -R 700 $RUNTIME_DIR

if [[ ! -f "$CONF_SRC" ]]; then
  echo "⚠️   Config file not found: $CONF_SRC"
  echo "    Pass a path explicitly: ./start.sh path/to/conf.yaml"
else
  cargo build --release
  export RUST_LOG=edge_cdn_store=debug,pingora=info

  # Clean up old files
  [ -S "$RUNTIME_DIR/upgrade.sock" ] && rm -f "$RUNTIME_DIR/upgrade.sock"
  [ -f "$RUNTIME_DIR/server.pid" ] && rm -f "$RUNTIME_DIR/server.pid"
  [ -e "$RUNTIME_DIR/conf.yaml" ] && rm -f "$RUNTIME_DIR/conf.yaml"

  cp "$PWD"/keys/server.* "$RUNTIME_DIR/keys/"
  cp "$CONF_SRC" "$RUNTIME_DIR/conf.yaml"
  exec "$(pwd)/target/release/edge-cdn-store" --conf "$RUNTIME_DIR/conf.yaml"
fi
