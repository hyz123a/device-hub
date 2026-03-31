#!/bin/sh
set -eu

ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
FRONTEND_DIR="$ROOT_DIR/frontend"
OUTPUT_DIR="${1:-$ROOT_DIR/target/frontend-bundle}"
DIST_DIR="$FRONTEND_DIR/dist"
OUTPUT_BUNDLE="$OUTPUT_DIR/frontend.bundle.tar"

mkdir -p "$OUTPUT_DIR"

if [ ! -d "$FRONTEND_DIR/node_modules" ]; then
  npm --prefix "$FRONTEND_DIR" ci >&2
fi

trunk build --config "$FRONTEND_DIR/Trunk.toml" --release --dist "$DIST_DIR" >&2
tar -cf "$OUTPUT_BUNDLE" -C "$DIST_DIR" .

printf '%s\n' "$OUTPUT_BUNDLE"
