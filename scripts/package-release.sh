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
OSS_BASE_URL="${OSS_BASE_URL:-https://pre-ai.aone.alibaba-inc.com}"
OSS_GROUP="${OSS_GROUP:-}"
PORTAL_RELEASE_DATA="${PORTAL_RELEASE_DATA:-pages/portal/release-downloads.js}"
PROFILE="release"
SKIP_BUILD=0
SKIP_GUI=0
SKIP_UPLOAD=0

usage() {
  cat <<'EOF'
Usage: scripts/package-release.sh [options]

Build and package Joi release artifacts in one command.

Options:
  --skip-build       Package existing dist/release binaries without rebuilding.
  --skip-gui         Do not build/copy the macOS arm64 GUI dmg.
  --skip-upload      Do not upload release artifacts to OSS.
  --dist-dir DIR     Source dist directory. Defaults to $DIST_DIR or dist.
  --out-dir DIR      Package output directory. Defaults to $PACKAGE_OUT_DIR or dist/packages.
  --oss-base-url URL OSS manager origin. Defaults to $OSS_BASE_URL or pre-ai.
  --oss-group GROUP  OSS group. Defaults to joi-apps-releases-<version>-<git-sha>.
  -h, --help         Show this help.

Environment:
  CARGO              Cargo executable. Defaults to cargo.
  MAKE               Make executable. Defaults to make.
  PNPM               pnpm executable used by Tauri beforeBuildCommand. Defaults to pnpm.
  LINUX_BUILDER      Builder for Linux Rust targets. Defaults to $CARGO.
  DIST_DIR           Dist directory. Defaults to dist.
  PACKAGE_OUT_DIR    Package output directory. Defaults to dist/packages.
  OSS_BASE_URL       OSS manager origin.
  OSS_GROUP          OSS grouped upload path.
  PORTAL_RELEASE_DATA Portal release data JS path.
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
    --skip-upload)
      SKIP_UPLOAD=1
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
    --oss-base-url)
      OSS_BASE_URL="${2:?missing value for --oss-base-url}"
      shift 2
      ;;
    --oss-group)
      OSS_GROUP="${2:?missing value for --oss-group}"
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
sanitize_oss_group_part() {
  printf '%s' "$1" | tr -c 'A-Za-z0-9_-' '-'
}

if [[ -z "$OSS_GROUP" ]]; then
  OSS_GROUP="joi-apps-releases-$(sanitize_oss_group_part "$VERSION")-$(sanitize_oss_group_part "$GIT_SHA")"
fi

RUNTIME_TARGETS=(
  "aarch64-apple-darwin"
  "x86_64-apple-darwin"
  "universal-apple-darwin"
  "aarch64-unknown-linux-musl"
  "x86_64-unknown-linux-musl"
)

log() {
  printf '[package-release] %s\n' "$*" >&2
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

upload_one_artifact() {
  local path="$1"
  local file_name
  file_name="$(basename "$path")"

  log "uploading $file_name to OSS group $OSS_GROUP"
  env -u HTTP_PROXY -u HTTPS_PROXY -u ALL_PROXY \
    -u http_proxy -u https_proxy -u all_proxy \
    curl -fsSL \
      -F "file=@$path" \
      -F "group=$OSS_GROUP" \
      -F "downloadFileName=$file_name" \
      "$OSS_BASE_URL/api/v1/oss/grouped/upload" \
    >"$TMP_DIR/upload-$file_name.json"

  python3 - "$TMP_DIR/upload-$file_name.json" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as f:
    payload = json.load(f)

if isinstance(payload, dict) and payload.get("success") is False:
    raise SystemExit(payload.get("message") or "upload failed")

data = payload.get("data", payload) if isinstance(payload, dict) else payload
if not isinstance(data, dict):
    raise SystemExit("upload response is not an object")

url = data.get("downloadUrl")
if not url:
    raise SystemExit("upload response missing downloadUrl")

print(url)
PY
}

artifact_kind() {
  case "$1" in
    joi-runtime-*.tar.gz) printf 'runtime' ;;
    joi-gui-*.dmg) printf 'gui' ;;
    SHA256SUMS) printf 'checksums' ;;
    manifest.txt) printf 'manifest' ;;
    *) printf 'file' ;;
  esac
}

artifact_label() {
  local file_name="$1"
  local label="$file_name"
  label="${label#joi-runtime-$VERSION-}"
  label="${label#joi-gui-$VERSION-}"
  label="${label%.tar.gz}"
  label="${label%.dmg}"
  case "$file_name" in
    SHA256SUMS) label="SHA256 checksums" ;;
    manifest.txt) label="Release manifest" ;;
  esac
  printf '%s' "$label"
}

write_portal_release_data() {
  local uploads_json="$1"
  mkdir -p "$(dirname "$PORTAL_RELEASE_DATA")"
  python3 - "$uploads_json" "$PORTAL_RELEASE_DATA" "$VERSION" "$GIT_SHA" "$GENERATED_AT" "$OSS_GROUP" <<'PY'
import json
import sys

uploads_path, out_path, version, git_sha, generated_at, group = sys.argv[1:7]
with open(uploads_path, "r", encoding="utf-8") as f:
    uploads = json.load(f)

payload = {
    "version": version,
    "gitSha": git_sha,
    "generatedAt": generated_at,
    "group": group,
    "artifacts": uploads,
}

with open(out_path, "w", encoding="utf-8") as f:
    f.write("window.JOI_RELEASE_DOWNLOADS = ")
    json.dump(payload, f, ensure_ascii=False, indent=2)
    f.write(";\n")
PY
  log "wrote $PORTAL_RELEASE_DATA"
  if [[ -d pages/build ]]; then
    cp "$PORTAL_RELEASE_DATA" pages/build/release-downloads.js
    log "updated pages/build/release-downloads.js"
  fi
}

upload_artifacts() {
  local uploads_json="$TMP_DIR/uploads.json"
  local first=1
  printf '[\n' >"$uploads_json"

  local artifacts=()
  while IFS= read -r artifact; do
    artifacts+=("$artifact")
  done < <(find "$PACKAGE_OUT_DIR" -maxdepth 1 -type f \
    \( -name '*.tar.gz' -o -name '*.dmg' -o -name 'SHA256SUMS' -o -name 'manifest.txt' \) \
    | sort)

  if [[ "${#artifacts[@]}" -eq 0 ]]; then
    echo "no package artifacts found for upload" >&2
    exit 1
  fi

  for artifact in "${artifacts[@]}"; do
    local file_name url size sha kind label
    file_name="$(basename "$artifact")"
    url="$(upload_one_artifact "$artifact")"
    size="$(wc -c <"$artifact" | tr -d ' ')"
    sha="$(checksum_cmd "$artifact" | awk '{print $1}')"
    kind="$(artifact_kind "$file_name")"
    label="$(artifact_label "$file_name")"

    if [[ "$first" -eq 0 ]]; then
      printf ',\n' >>"$uploads_json"
    fi
    first=0
    python3 - "$uploads_json" "$file_name" "$label" "$kind" "$size" "$sha" "$url" <<'PY'
import json
import sys

_, file_name, label, kind, size, sha, url = sys.argv
item = {
    "fileName": file_name,
    "label": label,
    "kind": kind,
    "size": int(size),
    "sha256": sha,
    "downloadUrl": url,
}
with open(sys.argv[1], "a", encoding="utf-8") as f:
    f.write("  ")
    json.dump(item, f, ensure_ascii=False)
PY
  done

  printf '\n]\n' >>"$uploads_json"
  write_portal_release_data "$uploads_json"
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

if [[ "$SKIP_UPLOAD" -eq 0 ]]; then
  upload_artifacts
else
  log "skipping OSS upload"
fi

log "done: $PACKAGE_OUT_DIR"
