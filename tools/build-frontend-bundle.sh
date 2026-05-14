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

TRUNK_BIN=${TRUNK_BIN:-}
if [ -z "$TRUNK_BIN" ]; then
  if command -v trunk >/dev/null 2>&1; then
    TRUNK_BIN=trunk
  elif [ -x "$HOME/.cargo/bin/trunk" ]; then
    TRUNK_BIN="$HOME/.cargo/bin/trunk"
  else
    printf '%s\n' 'trunk not found. Install it with `cargo install trunk --locked` or source ~/.cargo/env.' >&2
    exit 127
  fi
fi

"$TRUNK_BIN" build --config "$FRONTEND_DIR/Trunk.toml" --release --dist "$DIST_DIR" >&2
tar -cf "$OUTPUT_BUNDLE" -C "$DIST_DIR" .

printf '%s\n' "$OUTPUT_BUNDLE"
