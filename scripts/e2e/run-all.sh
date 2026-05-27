#!/usr/bin/env bash
# scripts/e2e/run-all.sh — Loom local smoke suite.
#
# Usage:
#   scripts/e2e/run-all.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

run() {
  local name="$1"; shift
  printf '\n########## %s ##########\n' "$name"
  "$@"
}

run "loom-message-core" bash scripts/e2e/run-loom-message-core.sh

echo
echo "ALL GREEN — Loom smoke suite passed."
