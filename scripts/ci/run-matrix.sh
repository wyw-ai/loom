#!/usr/bin/env bash
# Local CI matrix entry point for macOS / Linux.
#
# Runs the full local quality gate: cargo fmt --check, cargo clippy,
# cargo test --workspace --exclude loom-gui. Test target directory is
# redirected so the host project cache is not polluted; override via
# $LOOM_MATRIX_TARGET_DIR.
#
# Exit codes:
#   0  all stages green
#   1  argument / environment error
#   2  fmt failed
#   3  clippy failed
#   4  test failed
#
# Pair script: scripts/ci/run-matrix.ps1 (PowerShell for Windows).
# See docs/ci/local-matrix.md for the design rationale.

set -u
set -o pipefail

SKIP_FMT=0
SKIP_CLIPPY=0
SKIP_TEST=0
QUICK=0
STRICT_CLIPPY=0

usage() {
    cat <<'USAGE'
Usage: scripts/ci/run-matrix.sh [--skip-fmt] [--skip-clippy] [--skip-test] [--quick] [--strict-clippy] [-h|--help]

  --skip-fmt        skip cargo fmt --check
  --skip-clippy     skip cargo clippy
  --skip-test       skip cargo test
  --quick           alias for --skip-clippy (faster local iteration)
  --strict-clippy   promote clippy warnings to errors (-D warnings); target end-state
  -h, --help        show this help
USAGE
}

while [ $# -gt 0 ]; do
    case "$1" in
        --skip-fmt) SKIP_FMT=1 ;;
        --skip-clippy) SKIP_CLIPPY=1 ;;
        --skip-test) SKIP_TEST=1 ;;
        --quick) QUICK=1 ;;
        --strict-clippy) STRICT_CLIPPY=1 ;;
        -h|--help) usage; exit 0 ;;
        *) echo "unknown arg: $1" >&2; usage; exit 1 ;;
    esac
    shift
done

if [ "$QUICK" -eq 1 ]; then SKIP_CLIPPY=1; fi

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../.." && pwd)"
cd "$REPO_ROOT"

if [ -z "${LOOM_MATRIX_TARGET_DIR:-}" ]; then
    case "$(uname -s)" in
        Darwin) LOOM_MATRIX_TARGET_DIR="$HOME/Library/Caches/loom-test/target" ;;
        Linux)  LOOM_MATRIX_TARGET_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/loom-test/target" ;;
        *)      LOOM_MATRIX_TARGET_DIR="$HOME/.loom-test/target" ;;
    esac
fi
mkdir -p "$LOOM_MATRIX_TARGET_DIR"
export CARGO_TARGET_DIR="$LOOM_MATRIX_TARGET_DIR"

c_red=$'\033[31m'; c_grn=$'\033[32m'; c_ylw=$'\033[33m'
c_cyn=$'\033[36m'; c_mag=$'\033[35m'; c_rst=$'\033[0m'

if [ ! -t 1 ]; then
    c_red=""; c_grn=""; c_ylw=""; c_cyn=""; c_mag=""; c_rst=""
fi

stage() { printf '\n%s==> %s%s\n' "$c_cyn" "$1" "$c_rst"; }

run_stage() {
    local name="$1"; shift
    local fail_code="$1"; shift
    local start
    start=$(date +%s)
    if ! "$@"; then
        local rc=$?
        local end; end=$(date +%s)
        printf '%s[FAIL] %s (%ss, exit %s)%s\n' "$c_red" "$name" "$((end-start))" "$rc" "$c_rst"
        exit "$fail_code"
    fi
    local end; end=$(date +%s)
    printf '%s[OK]   %s (%ss)%s\n' "$c_grn" "$name" "$((end-start))" "$c_rst"
}

printf '%s=== loom local CI matrix (%s) ===%s\n' "$c_mag" "$(uname -s)" "$c_rst"
printf '    repo:           %s\n' "$REPO_ROOT"
printf '    target dir:     %s\n' "$CARGO_TARGET_DIR"
printf '    rustc:          %s\n' "$(rustc --version 2>&1)"
printf '    cargo:          %s\n\n' "$(cargo --version 2>&1)"

total_start=$(date +%s)

if [ "$SKIP_FMT" -eq 0 ]; then
    stage 'cargo fmt --check'
    run_stage fmt 2 cargo fmt --all -- --check
else
    printf '%sSKIP fmt%s\n' "$c_ylw" "$c_rst"
fi

if [ "$SKIP_CLIPPY" -eq 0 ]; then
    # P0-Lint PAL guardrail — runs across the FULL workspace (including loom-gui)
    # with only `clippy::disallowed_methods` denied. Catches any new
    # std::process::Command::new / tokio::process::Command::new regression in
    # gui/ without forcing the broader baseline rewrite that is still excluded
    # for loom-gui (see Iter#3 §8.9 Known Limitation).
    stage 'cargo clippy --workspace --all-targets -- -D clippy::disallowed_methods  [PAL guardrail]'
    run_stage clippy 3 cargo clippy --workspace --all-targets -- -D clippy::disallowed_methods

    if [ "$STRICT_CLIPPY" -eq 1 ]; then
        stage 'cargo clippy --workspace --exclude loom-gui --all-targets -- -D warnings  [strict]'
        run_stage clippy 3 cargo clippy --workspace --exclude loom-gui --all-targets -- -D warnings
    else
        stage 'cargo clippy --workspace --exclude loom-gui --all-targets  [informational]'
        run_stage clippy 3 cargo clippy --workspace --exclude loom-gui --all-targets
    fi
else
    printf '%sSKIP clippy%s\n' "$c_ylw" "$c_rst"
fi

if [ "$SKIP_TEST" -eq 0 ]; then
    stage 'cargo test --workspace --exclude loom-gui --no-fail-fast'
    run_stage test 4 cargo test --workspace --exclude loom-gui --no-fail-fast
else
    printf '%sSKIP test%s\n' "$c_ylw" "$c_rst"
fi

total_end=$(date +%s)
printf '\n%s=== matrix OK (%ss total) ===%s\n' "$c_grn" "$((total_end-total_start))" "$c_rst"
exit 0
