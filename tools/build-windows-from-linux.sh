#!/bin/sh
set -eu

ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if [ -n "${DEVICE_HUB_FRONTEND_BUNDLE:-}" ]; then
  BUNDLE_PATH=$DEVICE_HUB_FRONTEND_BUNDLE
  if [ ! -f "$BUNDLE_PATH" ]; then
    printf 'frontend bundle not found: %s\n' "$BUNDLE_PATH" >&2
    exit 1
  fi
else
  BUNDLE_PATH=$("$ROOT_DIR/tools/build-frontend-bundle.sh")
fi

DEVICE_HUB_FRONTEND_BUNDLE="$BUNDLE_PATH" cargo xwin build \
  -p device-hub-server \
  --no-default-features \
  --features serial \
  --target x86_64-pc-windows-msvc \
  --release

PACKAGE_DIR="$ROOT_DIR/target/packages/device-hub-windows-serial"
PACKAGE_ZIP="$ROOT_DIR/target/packages/device-hub-windows-serial.zip"
mkdir -p "$PACKAGE_DIR"
cp "$ROOT_DIR/target/x86_64-pc-windows-msvc/release/device-hub-server.exe" "$PACKAGE_DIR/"
cp "$ROOT_DIR/devices.example.yaml" "$PACKAGE_DIR/"
cp "$ROOT_DIR/devices.windows.yaml" "$PACKAGE_DIR/"
cp "$ROOT_DIR/README.md" "$PACKAGE_DIR/"
cp "$ROOT_DIR/docs/windows-linux.md" "$PACKAGE_DIR/"
cp "$ROOT_DIR/tools/windows-serial-smoke.ps1" "$PACKAGE_DIR/"
cp "$ROOT_DIR/tools/windows-runtime-smoke.ps1" "$PACKAGE_DIR/"
cp "$ROOT_DIR/tools/run-windows-from-wsl.sh" "$PACKAGE_DIR/"
cp "$ROOT_DIR/tools/windows-server-control.ps1" "$PACKAGE_DIR/"
(
  cd "$PACKAGE_DIR"
  zip -q -FS "$PACKAGE_ZIP" \
    device-hub-server.exe \
    devices.example.yaml \
    devices.windows.yaml \
    README.md \
    windows-linux.md \
    windows-serial-smoke.ps1 \
    windows-runtime-smoke.ps1 \
    run-windows-from-wsl.sh \
    windows-server-control.ps1
)

printf '%s\n' "$PACKAGE_ZIP"
