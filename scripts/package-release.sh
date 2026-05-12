#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

CARGO="${CARGO:-cargo}"
MAKE="${MAKE:-make}"
PNPM="${PNPM:-pnpm}"
LINUX_BUILDER="${LINUX_BUILDER:-$CARGO}"
DIST_DIR="${DIST_DIR:-dist}"
PACKAGE_OUT_DIR="${PACKAGE_OUT_DIR:-$DIST_DIR/packages}"
PROFILE="release"
SKIP_BUILD=0
SKIP_GUI=0

usage() {
  cat <<'EOF'
Usage: scripts/package-release.sh [options]

Build and package Joi release artifacts in one command.

Options:
  --skip-build       Package existing dist/release binaries without rebuilding.
  --skip-gui         Do not build/copy the macOS arm64 GUI dmg.
  --dist-dir DIR     Source dist directory. Defaults to $DIST_DIR or dist.
  --out-dir DIR      Package output directory. Defaults to $PACKAGE_OUT_DIR or dist/packages.
  -h, --help         Show this help.

Environment:
  CARGO              Cargo executable. Defaults to cargo.
  MAKE               Make executable. Defaults to make.
  PNPM               pnpm executable used by Tauri beforeBuildCommand. Defaults to pnpm.
  LINUX_BUILDER      Builder for Linux Rust targets. Defaults to $CARGO.
  DIST_DIR           Dist directory. Defaults to dist.
  PACKAGE_OUT_DIR    Package output directory. Defaults to dist/packages.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-build)
      SKIP_BUILD=1
      shift
      ;;
    --skip-gui)
      SKIP_GUI=1
      shift
      ;;
    --dist-dir)
      DIST_DIR="${2:?missing value for --dist-dir}"
      shift 2
      ;;
    --out-dir)
      PACKAGE_OUT_DIR="${2:?missing value for --out-dir}"
      shift 2
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      echo "unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

VERSION="$(awk -F\" '/^version/ { print $2; exit }' Cargo.toml)"
if [[ -z "$VERSION" ]]; then
  echo "failed to read workspace version from Cargo.toml" >&2
  exit 1
fi

GIT_SHA="$(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
GENERATED_AT="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/joi-package.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT

RUNTIME_TARGETS=(
  "aarch64-apple-darwin"
  "x86_64-apple-darwin"
  "universal-apple-darwin"
  "aarch64-unknown-linux-musl"
  "x86_64-unknown-linux-musl"
)

log() {
  printf '[package-release] %s\n' "$*"
}

run_make() {
  "$MAKE" "$@" \
    CARGO="$CARGO" \
    PNPM="$PNPM" \
    DIST_DIR="$DIST_DIR" \
    LINUX_BUILDER="$LINUX_BUILDER"
}

ensure_file() {
  local path="$1"
  if [[ ! -f "$path" ]]; then
    echo "missing expected artifact: $path" >&2
    exit 1
  fi
}

checksum_cmd() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$@"
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$@"
  else
    echo "no checksum command found: install shasum or sha256sum" >&2
    exit 1
  fi
}

package_runtime_target() {
  local target="$1"
  local src_dir="$DIST_DIR/$PROFILE/$target"
  local package_name="joi-runtime-$VERSION-$target"
  local stage_dir="$TMP_DIR/$package_name"
  local archive="$PACKAGE_OUT_DIR/$package_name.tar.gz"

  ensure_file "$src_dir/joi"
  ensure_file "$src_dir/joi-server"

  rm -rf "$stage_dir"
  mkdir -p "$stage_dir/bin"
  cp "$src_dir/joi" "$stage_dir/bin/joi"
  cp "$src_dir/joi-server" "$stage_dir/bin/joi-server"
  chmod 0755 "$stage_dir/bin/joi" "$stage_dir/bin/joi-server"

  cat >"$stage_dir/README.txt" <<EOF
Joi runtime package

Version: $VERSION
Git SHA: $GIT_SHA
Target: $target

Contents:
- bin/joi: CLI and daemon host. Run the daemon with "joi daemon".
- bin/joi-server: WebSocket collaboration server.
EOF

  tar -C "$TMP_DIR" -czf "$archive" "$package_name"
  log "wrote $archive"
}

find_latest_dmg() {
  local roots=(
    "target/aarch64-apple-darwin/release/bundle/dmg"
    "target/release/bundle/dmg"
    "crates/gui/target/aarch64-apple-darwin/release/bundle/dmg"
    "crates/gui/target/release/bundle/dmg"
  )
  local latest=""
  local latest_mtime=0

  for root in "${roots[@]}"; do
    [[ -d "$root" ]] || continue
    while IFS= read -r -d '' path; do
      local mtime
      mtime="$(stat -f '%m' "$path" 2>/dev/null || stat -c '%Y' "$path")"
      if [[ "$mtime" -gt "$latest_mtime" ]]; then
        latest="$path"
        latest_mtime="$mtime"
      fi
    done < <(find "$root" -type f -name '*.dmg' -print0)
  done

  [[ -n "$latest" ]] || return 1
  printf '%s\n' "$latest"
}

package_gui_dmg() {
  if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "GUI dmg packaging requires macOS; rerun on macOS or pass --skip-gui" >&2
    exit 1
  fi

  log "building GUI dmg for aarch64-apple-darwin"
  run_make gui-dmg-mac-arm

  local dmg
  if ! dmg="$(find_latest_dmg)"; then
    echo "Tauri build finished but no dmg was found" >&2
    exit 1
  fi

  local out="$PACKAGE_OUT_DIR/joi-gui-$VERSION-aarch64-apple-darwin.dmg"
  cp "$dmg" "$out"
  log "wrote $out"
}

write_manifest() {
  local manifest="$PACKAGE_OUT_DIR/manifest.txt"
  {
    printf 'version=%s\n' "$VERSION"
    printf 'git_sha=%s\n' "$GIT_SHA"
    printf 'generated_at=%s\n' "$GENERATED_AT"
    printf 'profile=%s\n' "$PROFILE"
    printf '\nartifacts:\n'
    find "$PACKAGE_OUT_DIR" -maxdepth 1 -type f \( -name '*.tar.gz' -o -name '*.dmg' \) \
      -exec basename {} \; | sort | sed 's/^/- /'
  } >"$manifest"
  log "wrote $manifest"
}

write_checksums() {
  local sums="$PACKAGE_OUT_DIR/SHA256SUMS"
  (
    cd "$PACKAGE_OUT_DIR"
    artifacts=()
    while IFS= read -r artifact; do
      artifacts+=("$artifact")
    done < <(find . -maxdepth 1 -type f \( -name '*.tar.gz' -o -name '*.dmg' \) \
      -exec basename {} \; | sort)

    if [[ "${#artifacts[@]}" -eq 0 ]]; then
      echo "no package artifacts found for checksum generation" >&2
      exit 1
    fi

    checksum_cmd "${artifacts[@]}"
  ) >"$sums"
  log "wrote $sums"
}

mkdir -p "$PACKAGE_OUT_DIR"
rm -f "$PACKAGE_OUT_DIR"/joi-runtime-*.tar.gz \
  "$PACKAGE_OUT_DIR"/joi-gui-*.dmg \
  "$PACKAGE_OUT_DIR"/SHA256SUMS \
  "$PACKAGE_OUT_DIR"/manifest.txt

if [[ "$SKIP_BUILD" -eq 0 ]]; then
  log "building CLI/daemon/server release binaries for macOS and Linux targets"
  run_make all-release
else
  log "skipping binary build; using existing $DIST_DIR/$PROFILE artifacts"
fi

for target in "${RUNTIME_TARGETS[@]}"; do
  package_runtime_target "$target"
done

if [[ "$SKIP_GUI" -eq 0 ]]; then
  package_gui_dmg
else
  log "skipping GUI dmg"
fi

write_manifest
write_checksums

log "done: $PACKAGE_OUT_DIR"
