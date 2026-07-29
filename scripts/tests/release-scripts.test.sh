#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TEST_DIR="$(mktemp -d "${TMPDIR:-/tmp}/loom-release-test.XXXXXX")"
trap 'rm -rf "$TEST_DIR"' EXIT

DIST_DIR="$TEST_DIR/dist"
OUT_DIR="$TEST_DIR/packages"
LINUX_DIR="$DIST_DIR/release/x86_64-unknown-linux-gnu"
WINDOWS_DIR="$DIST_DIR/release/x86_64-pc-windows-msvc"
INSTALL_DIR="$TEST_DIR/install"

mkdir -p "$LINUX_DIR" "$WINDOWS_DIR"
for binary in loom loom-daemon loom-server; do
  printf 'fake %s\n' "$binary" >"$LINUX_DIR/$binary"
  printf 'fake %s.exe\n' "$binary" >"$WINDOWS_DIR/$binary.exe"
done

DOWNLOAD_BASE_URL="https://github.com/wyw-ai/loom/releases/download/v0.1.1" \
  bash "$ROOT_DIR/scripts/package-release.sh" \
    --skip-build \
    --skip-gui \
    --dist-dir "$DIST_DIR" \
    --out-dir "$OUT_DIR"

test -f "$OUT_DIR/loom-runtime-0.1.1-x86_64-unknown-linux-gnu.tar.gz"
test -f "$OUT_DIR/loom-runtime-0.1.1-x86_64-pc-windows-msvc.zip"

node "$ROOT_DIR/scripts/write-release-downloads.mjs" \
  --package-dir "$OUT_DIR" \
  --out "$TEST_DIR/release-downloads.js" \
  --version 0.1.1 \
  --tag v0.1.1 \
  --download-base-url "https://github.com/wyw-ai/loom/releases/download/v0.1.1"

grep -q '"label": "x86_64-pc-windows-msvc"' "$TEST_DIR/release-downloads.js"
grep -q 'loom-runtime-0.1.1-x86_64-pc-windows-msvc.zip' "$TEST_DIR/release-downloads.js"

sh "$OUT_DIR/install.sh" \
  --target x86_64-pc-windows-msvc \
  --module loom \
  --package-dir "$OUT_DIR" \
  --bin-dir "$INSTALL_DIR"

test -f "$INSTALL_DIR/loom.exe"
printf 'release script tests passed\n'
