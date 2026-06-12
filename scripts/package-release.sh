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
DOWNLOAD_BASE_URL="${DOWNLOAD_BASE_URL:-}"
PROFILE="release"
SKIP_BUILD=0
SKIP_GUI=0

usage() {
  cat <<'EOF'
Usage: scripts/package-release.sh [options]

Build and package Loom release artifacts into a local directory.

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
  DOWNLOAD_BASE_URL  Optional release URL embedded into install.sh for remote downloads.
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
TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/loom-package.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT

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

runtime_target_available() {
  local target="$1"
  local src_dir="$DIST_DIR/$PROFILE/$target"
  [[ -f "$src_dir/loom" && -f "$src_dir/loom-daemon" && -f "$src_dir/loom-server" ]]
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
  local package_name="loom-runtime-$VERSION-$target"
  local stage_dir="$TMP_DIR/$package_name"
  local archive="$PACKAGE_OUT_DIR/$package_name.tar.gz"

  ensure_file "$src_dir/loom"
  ensure_file "$src_dir/loom-daemon"
  ensure_file "$src_dir/loom-server"

  rm -rf "$stage_dir"
  mkdir -p "$stage_dir/bin"
  cp "$src_dir/loom" "$stage_dir/bin/loom"
  cp "$src_dir/loom-daemon" "$stage_dir/bin/loom-daemon"
  cp "$src_dir/loom-server" "$stage_dir/bin/loom-server"
  chmod 0755 "$stage_dir/bin/loom" "$stage_dir/bin/loom-daemon" "$stage_dir/bin/loom-server"

  cat >"$stage_dir/README.txt" <<EOF
Loom runtime package

Version: $VERSION
Git SHA: $GIT_SHA
Target: $target

Contents:
- bin/loom: operator CLI.
- bin/loom-daemon: machine-scoped agent and service host.
- bin/loom-server: WebSocket collaboration server.
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
  local src_loom="$DIST_DIR/$PROFILE/$target/loom"
  local src_daemon="$DIST_DIR/$PROFILE/$target/loom-daemon"
  ensure_file "$src_loom"
  ensure_file "$src_daemon"

  log "building GUI app bundle for $target"
  (
    cd crates/gui
    "$CARGO" tauri build --target "$target" --bundles app --ci
  )

  local app="target/$target/release/bundle/macos/Loom Desktop.app"
  if [[ ! -d "$app" ]]; then
    echo "Tauri build finished but no app bundle was found at $app" >&2
    exit 1
  fi

  local out="$PACKAGE_OUT_DIR/loom-gui-$VERSION-aarch64-apple-darwin.dmg"
  local resources_dir="$app/Contents/Resources/bin"
  mkdir -p "$resources_dir"
  cp "$src_loom" "$resources_dir/loom"
  cp "$src_daemon" "$resources_dir/loom-daemon"
  chmod 0755 "$resources_dir/loom" "$resources_dir/loom-daemon"

  if command -v codesign >/dev/null 2>&1; then
    codesign --force --deep --sign - "$app"
  fi

  local stage_dir tmp_dmg dmg_dir dmg
  stage_dir="$(mktemp -d "${TMPDIR:-/tmp}/loom-gui-dmg.XXXXXX")"
  tmp_dmg="$(mktemp "${TMPDIR:-/tmp}/loom-gui-dmg-output.XXXXXX").dmg"
  rm -f "$tmp_dmg"
  if command -v ditto >/dev/null 2>&1; then
    ditto "$app" "$stage_dir/Loom Desktop.app"
  else
    cp -R "$app" "$stage_dir/Loom Desktop.app"
  fi
  ln -s /Applications "$stage_dir/Applications"

  dmg_dir="target/$target/release/bundle/dmg"
  dmg="$dmg_dir/Loom Desktop_${VERSION}_aarch64.dmg"
  mkdir -p "$dmg_dir"
  rm -f "$dmg" "$out"

  local attempt=1
  local max_attempts=5
  until hdiutil create \
      -volname "Loom Desktop" \
      -srcfolder "$stage_dir" \
      -ov \
      -format UDRO \
      "$tmp_dmg"; do
    if [[ "$attempt" -ge "$max_attempts" ]]; then
      rm -rf "$stage_dir"
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
  local artifacts=()
  local path name

  for path in "$PACKAGE_OUT_DIR"/*; do
    [[ -f "$path" ]] || continue
    name="$(basename "$path")"
    case "$name" in
      *.tar.gz | *.dmg | install.sh)
        artifacts+=("$name")
        ;;
    esac
  done

  {
    printf 'version=%s\n' "$VERSION"
    printf 'git_sha=%s\n' "$GIT_SHA"
    printf 'generated_at=%s\n' "$GENERATED_AT"
    printf 'profile=%s\n' "$PROFILE"
    printf 'package_dir=%s\n' "$PACKAGE_OUT_DIR"
    if [[ -n "$DOWNLOAD_BASE_URL" ]]; then
      printf 'download_base_url=%s\n' "$DOWNLOAD_BASE_URL"
    fi
    printf '\nartifacts:\n'
    if [[ "${#artifacts[@]}" -gt 0 ]]; then
      printf '%s\n' "${artifacts[@]}" | sort | sed 's/^/- /'
    fi
  } >"$manifest"
  log "wrote $manifest"
}

archive_name_if_exists() {
  local name="$1"
  if [[ -f "$PACKAGE_OUT_DIR/$name" ]]; then
    printf '%s' "$name"
  fi
}

archive_sha_if_exists() {
  local name="$1"
  if [[ -n "$name" && -f "$PACKAGE_OUT_DIR/$name" ]]; then
    checksum_cmd "$PACKAGE_OUT_DIR/$name" | awk '{print $1}'
  fi
}

write_installer() {
  local installer="$PACKAGE_OUT_DIR/install.sh"
  local runtime_mac="loom-runtime-$VERSION-universal-apple-darwin.tar.gz"
  local runtime_linux_x86="loom-runtime-$VERSION-x86_64-unknown-linux-musl.tar.gz"
  local runtime_linux_arm="loom-runtime-$VERSION-aarch64-unknown-linux-musl.tar.gz"
  local sha_mac sha_linux_x86 sha_linux_arm

  runtime_mac="$(archive_name_if_exists "$runtime_mac")"
  runtime_linux_x86="$(archive_name_if_exists "$runtime_linux_x86")"
  runtime_linux_arm="$(archive_name_if_exists "$runtime_linux_arm")"
  sha_mac="$(archive_sha_if_exists "$runtime_mac")"
  sha_linux_x86="$(archive_sha_if_exists "$runtime_linux_x86")"
  sha_linux_arm="$(archive_sha_if_exists "$runtime_linux_arm")"

  cat >"$installer" <<EOF
#!/usr/bin/env sh
set -eu

VERSION='$VERSION'
GIT_SHA='$GIT_SHA'
DOWNLOAD_BASE_URL='$DOWNLOAD_BASE_URL'
DEFAULT_BIN_DIR="\${HOME}/.local/bin"
DEFAULT_PACKAGE_DIR="\$(CDPATH= cd "\$(dirname "\$0")" && pwd)"

PKG_UNIVERSAL_APPLE_DARWIN='$runtime_mac'
SHA_UNIVERSAL_APPLE_DARWIN='$sha_mac'
PKG_X86_64_UNKNOWN_LINUX_MUSL='$runtime_linux_x86'
SHA_X86_64_UNKNOWN_LINUX_MUSL='$sha_linux_x86'
PKG_AARCH64_UNKNOWN_LINUX_MUSL='$runtime_linux_arm'
SHA_AARCH64_UNKNOWN_LINUX_MUSL='$sha_linux_arm'

usage() {
  cat <<'USAGE'
Usage: install.sh [options]

Install Loom runtime binaries from a local package directory.

Options:
  -m, --module MODULE   Module to install: all, loom, loom-daemon, loom-server.
                        Can be repeated or comma-separated. Default: all.
  -t, --target TARGET   Override target package:
                        universal-apple-darwin,
                        x86_64-unknown-linux-musl,
                        aarch64-unknown-linux-musl.
      --package-dir DIR Directory containing loom-runtime-*.tar.gz.
                        Default: the directory containing install.sh.
      --bin-dir DIR     Install binaries into DIR. Default: \$HOME/.local/bin.
      --dry-run         Print the selected package and modules without installing.
  -y, --yes             Accepted for non-interactive automation.
  -h, --help            Show this help.

Examples:
  sh install.sh
  sh install.sh --module loom
  sh install.sh --module loom-server --bin-dir /usr/local/bin
  sh install.sh --package-dir ./dist/packages --target x86_64-unknown-linux-musl -y
USAGE
}

die() {
  printf 'install.sh: %s\n' "\$*" >&2
  exit 1
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

runtime_package_name() {
  case "\$1" in
    universal-apple-darwin) printf '%s\n' "\$PKG_UNIVERSAL_APPLE_DARWIN" ;;
    x86_64-unknown-linux-musl) printf '%s\n' "\$PKG_X86_64_UNKNOWN_LINUX_MUSL" ;;
    aarch64-unknown-linux-musl) printf '%s\n' "\$PKG_AARCH64_UNKNOWN_LINUX_MUSL" ;;
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
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "\$url" -o "\$out"
  elif command -v wget >/dev/null 2>&1; then
    wget -qO "\$out" "\$url"
  else
    die "missing downloader: install curl or wget"
  fi
}

normalize_modules() {
  modules=""
  for module in \$(printf '%s' "\$1" | tr ',' ' '); do
    case "\$module" in
      all) modules="loom loom-daemon loom-server" ;;
      loom) modules="\$modules loom" ;;
      loom-daemon) modules="\$modules loom-daemon" ;;
      loom-server) modules="\$modules loom-server" ;;
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
package_dir="\$DEFAULT_PACKAGE_DIR"
dry_run=0

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
    --package-dir)
      [ "\$#" -ge 2 ] || die "missing value for \$1"
      package_dir="\$2"
      shift 2
      ;;
    --package-dir=*)
      package_dir="\${1#*=}"
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
package_name="\$(runtime_package_name "\$target")"
package_path="\$package_dir/\$package_name"
expected_sha="\$(runtime_sha256 "\$target")"
modules="\$(normalize_modules "\$module_spec")"
download_url=""

[ -n "\$package_name" ] || die "no runtime package is available for target \$target"
[ -n "\$expected_sha" ] || die "no checksum is available for target \$target"

if [ -n "\$DOWNLOAD_BASE_URL" ]; then
  download_url="\${DOWNLOAD_BASE_URL%/}/\$package_name"
fi

printf 'Loom %s (%s)\n' "\$VERSION" "\$GIT_SHA"
printf 'target: %s\n' "\$target"
printf 'modules:%s\n' "\$modules"
printf 'bin dir: %s\n' "\$bin_dir"
printf 'package: %s\n' "\$package_path"
if [ -n "\$download_url" ]; then
  printf 'download: %s\n' "\$download_url"
fi

if [ "\$dry_run" -eq 1 ]; then
  exit 0
fi

command -v tar >/dev/null 2>&1 || die "missing required command: tar"

download_tmp_dir=""
extract_tmp_dir=""
cleanup() {
  if [ -n "\$download_tmp_dir" ]; then
    rm -rf "\$download_tmp_dir"
  fi
  if [ -n "\$extract_tmp_dir" ]; then
    rm -rf "\$extract_tmp_dir"
  fi
}
trap cleanup EXIT INT TERM

if [ ! -f "\$package_path" ]; then
  [ -n "\$download_url" ] || die "missing runtime package: \$package_path"
  download_tmp_dir="\$(mktemp -d "\${TMPDIR:-/tmp}/loom-install-download.XXXXXX")"
  package_path="\$download_tmp_dir/\$package_name"
  printf 'downloading %s\n' "\$download_url"
  download_file "\$download_url" "\$package_path"
fi

actual_sha="\$(sha256_file "\$package_path")"
[ "\$actual_sha" = "\$expected_sha" ] || die "checksum mismatch for \$package_path"

extract_tmp_dir="\$(mktemp -d "\${TMPDIR:-/tmp}/loom-install.XXXXXX")"
tar -xzf "\$package_path" -C "\$extract_tmp_dir"
package_root=""
for candidate in "\$extract_tmp_dir"/loom-runtime-*; do
  [ -d "\$candidate" ] || continue
  package_root="\$candidate"
  break
done
[ -n "\$package_root" ] || die "runtime package did not extract correctly"

for module in \$modules; do
  install_one "\$module" "\$package_root/bin/\$module" "\$bin_dir"
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
    for path in *; do
      [[ -f "$path" ]] || continue
      case "$path" in
        *.tar.gz | *.dmg | install.sh)
          artifacts+=("$path")
          ;;
      esac
    done

    if [[ "${#artifacts[@]}" -eq 0 ]]; then
      echo "no package artifacts found for checksum generation" >&2
      exit 1
    fi

    printf '%s\n' "${artifacts[@]}" | sort | while IFS= read -r artifact; do
      checksum_cmd "$artifact"
    done
  ) >"$sums"
  log "wrote $sums"
}

mkdir -p "$PACKAGE_OUT_DIR"
rm -f "$PACKAGE_OUT_DIR"/loom-runtime-*.tar.gz \
  "$PACKAGE_OUT_DIR"/loom-gui-*.dmg \
  "$PACKAGE_OUT_DIR"/install.sh \
  "$PACKAGE_OUT_DIR"/SHA256SUMS \
  "$PACKAGE_OUT_DIR"/manifest.txt

if [[ "$SKIP_BUILD" -eq 0 ]]; then
  log "building CLI/daemon/server release binaries for macOS and Linux targets"
  run_make all-release
else
  log "skipping binary build; using existing $DIST_DIR/$PROFILE artifacts"
fi

runtime_package_count=0
for target in "${RUNTIME_TARGETS[@]}"; do
  if ! runtime_target_available "$target"; then
    log "skipping $target; missing runtime artifacts"
    continue
  fi
  package_runtime_target "$target"
  runtime_package_count=$((runtime_package_count + 1))
done

if [[ "$runtime_package_count" -eq 0 ]]; then
  echo "no runtime artifacts found under $DIST_DIR/$PROFILE" >&2
  exit 1
fi

if [[ "$SKIP_GUI" -eq 0 ]]; then
  package_gui_dmg
else
  log "skipping GUI dmg"
fi

write_installer
write_manifest
write_checksums

log "done: $PACKAGE_OUT_DIR"
