#!/bin/sh
set -eu

ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT_DIR"

cargo xwin check \
  -p device-hub-server \
  --no-default-features \
  --features serial \
  --target x86_64-pc-windows-msvc

cargo xwin check \
  -p device-hub-server \
  --no-default-features \
  --features full \
  --target x86_64-pc-windows-msvc

cargo xwin test \
  -p device-hub-server \
  --no-default-features \
  --features full,test-fixtures \
  --target x86_64-pc-windows-msvc \
  --no-run

cargo xwin test \
  -p device-hub-adapter-ssh \
  -p device-hub-adapter-jlink \
  -p device-hub-adapter-adb \
  --all-features \
  --target x86_64-pc-windows-msvc \
  --no-run
