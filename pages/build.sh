#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BUILD_DIR="$SCRIPT_DIR/build"
PORTAL_DIR="$SCRIPT_DIR/portal"

echo "==> Cleaning build directory..."
rm -rf "$BUILD_DIR"
mkdir -p "$BUILD_DIR"

echo "==> Copying Loom portal..."
cp "$PORTAL_DIR/index.html" "$BUILD_DIR/index.html"
cp "$PORTAL_DIR/style.css" "$BUILD_DIR/style.css"
cp "$PORTAL_DIR/app.js" "$BUILD_DIR/app.js"
if [ -f "$PORTAL_DIR/release-downloads.js" ]; then
  cp "$PORTAL_DIR/release-downloads.js" "$BUILD_DIR/release-downloads.js"
fi

echo "==> Build complete! Output in $BUILD_DIR"
echo "    Portal page: build/index.html"
