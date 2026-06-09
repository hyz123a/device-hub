#!/usr/bin/env sh
set -eu

test_dir="../target/playwright-e2e"

mkdir -p "$test_dir"

cat > "$test_dir/devices.yaml" <<'YAML'
devices:
  - id: board-default
    label: Default Target
    serial:
      port: /dev/ttyDEFAULT0
      baud: 115200
device_registry_path: device-registry.yaml
artifacts_dir: artifacts
YAML

cat > "$test_dir/device-registry.yaml" <<'YAML'
devices: []
YAML

cd "$test_dir"

DEVICE_HUB_BIND_ADDR=127.0.0.1:3100 \
DEVICE_HUB_CONFIG=devices.yaml \
cargo run --manifest-path ../../Cargo.toml -p device-hub-server
