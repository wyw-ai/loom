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
SKIP_UPLOAD=1

usage() {
  cat <<'EOF'
Usage: scripts/package-release.sh [options]

Build and package Joi release artifacts in one command.

Options:
  --skip-build       Package existing dist/release binaries without rebuilding.
  --skip-gui         Do not build/copy the macOS arm64 GUI dmg.
  --upload, --publish Upload release artifacts to OSS and refresh portal release data.
  --skip-upload      Do not upload release artifacts to OSS. This is the default.
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
    --upload | --publish)
      SKIP_UPLOAD=0
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

package_gui_dmg() {
  if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "GUI dmg packaging requires macOS; rerun on macOS or pass --skip-gui" >&2
    exit 1
  fi

  local target="aarch64-apple-darwin"
  local src_joi="$DIST_DIR/$PROFILE/$target/joi"
  ensure_file "$src_joi"

  log "building GUI app bundle for $target"
  (
    cd crates/gui
    "$CARGO" tauri build --target "$target" --bundles app --ci
  )

  local app="target/$target/release/bundle/macos/Joi Desktop.app"
  if [[ ! -d "$app" ]]; then
    echo "Tauri build finished but no app bundle was found at $app" >&2
    exit 1
  fi

  local out="$PACKAGE_OUT_DIR/joi-gui-$VERSION-aarch64-apple-darwin.dmg"
  local resources_dir="$app/Contents/Resources/bin"
  mkdir -p "$resources_dir"
  cp "$src_joi" "$resources_dir/joi"
  chmod 0755 "$resources_dir/joi"

  if command -v codesign >/dev/null 2>&1; then
    codesign --force --deep --sign - "$app"
  fi

  local stage_dir
  stage_dir="$(mktemp -d "/private/tmp/joi-gui-dmg.XXXXXX")"
  if command -v ditto >/dev/null 2>&1; then
    ditto "$app" "$stage_dir/Joi Desktop.app"
  else
    cp -R "$app" "$stage_dir/Joi Desktop.app"
  fi
  ln -s /Applications "$stage_dir/Applications"
  local dmg_dir="target/$target/release/bundle/dmg"
  local dmg="$dmg_dir/Joi Desktop_${VERSION}_aarch64.dmg"
  local tmp_dmg="/private/tmp/joi-gui-dmg-output-${VERSION}-$$.dmg"
  mkdir -p "$dmg_dir"
  rm -f "$dmg" "$out" "$tmp_dmg"
  local attempt=1
  local max_attempts=5
  until hdiutil create \
      -volname "Joi Desktop" \
      -srcfolder "$stage_dir" \
      -ov \
      -format UDRO \
      "$tmp_dmg"; do
    if [[ "$attempt" -ge "$max_attempts" ]]; then
      rm -f "$tmp_dmg"
      echo "hdiutil create failed after $max_attempts attempts" >&2
      exit 1
    fi
    rm -f "$tmp_dmg"
    sleep "$((attempt * 2))"
    attempt=$((attempt + 1))
  done
  cp "$tmp_dmg" "$dmg"
  cp "$tmp_dmg" "$out"
  rm -rf "$stage_dir"
  rm -f "$tmp_dmg"
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
    find "$PACKAGE_OUT_DIR" -maxdepth 1 -type f \( -name '*.tar.gz' -o -name '*.dmg' -o -name 'install.sh' \) \
      -exec basename {} \; | sort | sed 's/^/- /'
  } >"$manifest"
  log "wrote $manifest"
}

artifact_download_url() {
  local file_name="$1"
  printf '%s/api/v1/%s/%s' "${OSS_BASE_URL%/}" "$OSS_GROUP" "$file_name"
}

write_installer() {
  local installer="$PACKAGE_OUT_DIR/install.sh"
  local runtime_mac="joi-runtime-$VERSION-universal-apple-darwin.tar.gz"
  local runtime_linux_x86="joi-runtime-$VERSION-x86_64-unknown-linux-musl.tar.gz"
  local runtime_linux_arm="joi-runtime-$VERSION-aarch64-unknown-linux-musl.tar.gz"
  local sha_mac sha_linux_x86 sha_linux_arm

  ensure_file "$PACKAGE_OUT_DIR/$runtime_mac"
  ensure_file "$PACKAGE_OUT_DIR/$runtime_linux_x86"
  ensure_file "$PACKAGE_OUT_DIR/$runtime_linux_arm"

  sha_mac="$(checksum_cmd "$PACKAGE_OUT_DIR/$runtime_mac" | awk '{print $1}')"
  sha_linux_x86="$(checksum_cmd "$PACKAGE_OUT_DIR/$runtime_linux_x86" | awk '{print $1}')"
  sha_linux_arm="$(checksum_cmd "$PACKAGE_OUT_DIR/$runtime_linux_arm" | awk '{print $1}')"

  cat >"$installer" <<EOF
#!/usr/bin/env sh
set -eu

VERSION='$VERSION'
GIT_SHA='$GIT_SHA'
DEFAULT_BIN_DIR="\${HOME}/.local/bin"

URL_UNIVERSAL_APPLE_DARWIN='$(artifact_download_url "$runtime_mac")'
SHA_UNIVERSAL_APPLE_DARWIN='$sha_mac'
URL_X86_64_UNKNOWN_LINUX_MUSL='$(artifact_download_url "$runtime_linux_x86")'
SHA_X86_64_UNKNOWN_LINUX_MUSL='$sha_linux_x86'
URL_AARCH64_UNKNOWN_LINUX_MUSL='$(artifact_download_url "$runtime_linux_arm")'
SHA_AARCH64_UNKNOWN_LINUX_MUSL='$sha_linux_arm'

usage() {
  cat <<'USAGE'
Usage: install.sh [options]

Install Joi runtime binaries from the current release.

Options:
  -m, --module MODULE   Module to install: all, joi, joi-server, server.
                        Can be repeated or comma-separated. Default: all.
  -t, --target TARGET   Override target package:
                        universal-apple-darwin,
                        x86_64-unknown-linux-musl,
                        aarch64-unknown-linux-musl.
      --bin-dir DIR     Install binaries into DIR. Default: \$HOME/.local/bin.
      --dry-run         Print the selected package and modules without installing.
      --print-url       Print the selected runtime package URL and exit.
  -y, --yes             Accepted for non-interactive automation.
  -h, --help            Show this help.

Examples:
  sh install.sh
  sh install.sh --module joi
  sh install.sh --module joi-server --bin-dir /usr/local/bin
  sh install.sh --target x86_64-unknown-linux-musl --module joi,joi-server -y
USAGE
}

die() {
  printf 'install.sh: %s\n' "\$*" >&2
  exit 1
}

need_cmd() {
  command -v "\$1" >/dev/null 2>&1 || die "missing required command: \$1"
}

detect_target() {
  os="\$(uname -s 2>/dev/null || true)"
  arch="\$(uname -m 2>/dev/null || true)"
  case "\$os:\$arch" in
    Darwin:*) printf '%s\n' universal-apple-darwin ;;
    Linux:x86_64|Linux:amd64) printf '%s\n' x86_64-unknown-linux-musl ;;
    Linux:aarch64|Linux:arm64) printf '%s\n' aarch64-unknown-linux-musl ;;
    *) die "unsupported platform: \$os \$arch; pass --target explicitly" ;;
  esac
}

runtime_url() {
  case "\$1" in
    universal-apple-darwin) printf '%s\n' "\$URL_UNIVERSAL_APPLE_DARWIN" ;;
    x86_64-unknown-linux-musl) printf '%s\n' "\$URL_X86_64_UNKNOWN_LINUX_MUSL" ;;
    aarch64-unknown-linux-musl) printf '%s\n' "\$URL_AARCH64_UNKNOWN_LINUX_MUSL" ;;
    *) die "unknown target: \$1" ;;
  esac
}

runtime_sha256() {
  case "\$1" in
    universal-apple-darwin) printf '%s\n' "\$SHA_UNIVERSAL_APPLE_DARWIN" ;;
    x86_64-unknown-linux-musl) printf '%s\n' "\$SHA_X86_64_UNKNOWN_LINUX_MUSL" ;;
    aarch64-unknown-linux-musl) printf '%s\n' "\$SHA_AARCH64_UNKNOWN_LINUX_MUSL" ;;
    *) die "unknown target: \$1" ;;
  esac
}

sha256_file() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "\$1" | awk '{print \$1}'
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum "\$1" | awk '{print \$1}'
  else
    die "missing checksum command: install shasum or sha256sum"
  fi
}

download_file() {
  url="\$1"
  out="\$2"
  need_cmd curl
  if [ "\${JOI_INSTALL_KEEP_PROXY:-}" = "1" ]; then
    curl -fL --retry 3 --connect-timeout 20 -o "\$out" "\$url"
  else
    env -u HTTP_PROXY -u HTTPS_PROXY -u ALL_PROXY \\
      -u http_proxy -u https_proxy -u all_proxy \\
      curl -fL --retry 3 --connect-timeout 20 -o "\$out" "\$url"
  fi
}

normalize_modules() {
  modules=""
  for module in \$(printf '%s' "\$1" | tr ',' ' '); do
    case "\$module" in
      all) modules="joi joi-server" ;;
      joi|cli) modules="\$modules joi" ;;
      server|joi-server) modules="\$modules joi-server" ;;
      "") ;;
      *) die "unknown module: \$module" ;;
    esac
  done
  printf '%s\n' "\$modules"
}

install_one() {
  name="\$1"
  src="\$2"
  dst="\$3"
  [ -f "\$src" ] || die "archive does not contain \$name"
  mkdir -p "\$dst"
  cp "\$src" "\$dst/\$name"
  chmod 0755 "\$dst/\$name"
  printf 'installed %s -> %s/%s\n' "\$name" "\$dst" "\$name"
}

target=""
module_spec="all"
bin_dir="\$DEFAULT_BIN_DIR"
dry_run=0
print_url=0

while [ "\$#" -gt 0 ]; do
  case "\$1" in
    -m|--module)
      [ "\$#" -ge 2 ] || die "missing value for \$1"
      if [ "\$module_spec" = "all" ]; then module_spec="\$2"; else module_spec="\$module_spec,\$2"; fi
      shift 2
      ;;
    --module=*)
      value="\${1#*=}"
      if [ "\$module_spec" = "all" ]; then module_spec="\$value"; else module_spec="\$module_spec,\$value"; fi
      shift
      ;;
    -t|--target)
      [ "\$#" -ge 2 ] || die "missing value for \$1"
      target="\$2"
      shift 2
      ;;
    --target=*)
      target="\${1#*=}"
      shift
      ;;
    --bin-dir)
      [ "\$#" -ge 2 ] || die "missing value for \$1"
      bin_dir="\$2"
      shift 2
      ;;
    --bin-dir=*)
      bin_dir="\${1#*=}"
      shift
      ;;
    --dry-run)
      dry_run=1
      shift
      ;;
    --print-url)
      print_url=1
      shift
      ;;
    -y|--yes)
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown argument: \$1"
      ;;
  esac
done

[ -n "\$target" ] || target="\$(detect_target)"
url="\$(runtime_url "\$target")"
expected_sha="\$(runtime_sha256 "\$target")"
modules="\$(normalize_modules "\$module_spec")"

if [ "\$print_url" -eq 1 ]; then
  printf '%s\n' "\$url"
  exit 0
fi

printf 'Joi %s (%s)\n' "\$VERSION" "\$GIT_SHA"
printf 'target: %s\n' "\$target"
printf 'modules:%s\n' "\$modules"
printf 'bin dir: %s\n' "\$bin_dir"
printf 'package: %s\n' "\$url"

if [ "\$dry_run" -eq 1 ]; then
  exit 0
fi

need_cmd tar
tmp_dir="\$(mktemp -d "\${TMPDIR:-/tmp}/joi-install.XXXXXX")"
trap 'rm -rf "\$tmp_dir"' EXIT INT TERM
archive="\$tmp_dir/joi-runtime.tar.gz"

download_file "\$url" "\$archive"
actual_sha="\$(sha256_file "\$archive")"
[ "\$actual_sha" = "\$expected_sha" ] || die "checksum mismatch for \$url"

tar -xzf "\$archive" -C "\$tmp_dir"
package_dir="\$(find "\$tmp_dir" -maxdepth 1 -type d -name "joi-runtime-*" | head -1)"
[ -n "\$package_dir" ] || die "runtime package did not extract correctly"

for module in \$modules; do
  install_one "\$module" "\$package_dir/bin/\$module" "\$bin_dir"
done

printf 'done. Add %s to PATH if needed.\n' "\$bin_dir"
EOF

  chmod 0755 "$installer"
  log "wrote $installer"
}

write_checksums() {
  local sums="$PACKAGE_OUT_DIR/SHA256SUMS"
  (
    cd "$PACKAGE_OUT_DIR"
    artifacts=()
    while IFS= read -r artifact; do
      artifacts+=("$artifact")
    done < <(find . -maxdepth 1 -type f \( -name '*.tar.gz' -o -name '*.dmg' -o -name 'install.sh' \) \
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
    install.sh) printf 'installer' ;;
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
    install.sh) label="Install script" ;;
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
    \( -name '*.tar.gz' -o -name '*.dmg' -o -name 'install.sh' -o -name 'SHA256SUMS' -o -name 'manifest.txt' \) \
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

_, uploads_json, file_name, label, kind, size, sha, url = sys.argv
item = {
    "fileName": file_name,
    "label": label,
    "kind": kind,
    "size": int(size),
    "sha256": sha,
    "downloadUrl": url,
}
with open(uploads_json, "a", encoding="utf-8") as f:
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
  "$PACKAGE_OUT_DIR"/install.sh \
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

write_installer
write_manifest
write_checksums

if [[ "$SKIP_UPLOAD" -eq 0 ]]; then
  upload_artifacts
else
  log "skipping OSS upload"
fi

log "done: $PACKAGE_OUT_DIR"
