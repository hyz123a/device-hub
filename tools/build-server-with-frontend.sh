#!/bin/sh
set -eu

ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
BUNDLE_PATH=$(bash "$ROOT_DIR/tools/build-frontend-bundle.sh")

DEVICE_HUB_FRONTEND_BUNDLE="$BUNDLE_PATH" cargo build -p device-hub-server "$@"
