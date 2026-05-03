#!/usr/bin/env bash
# scripts/e2e/run-all.sh — 1.5: sequential full-run of every local smoke.
#
# Order chosen to share the long-lived joi-server / agent-host / service-
# host launched by local-up.sh, and to put the synthetic smokes first so
# any infra regression fails fast before paying for a real-LLM run.
#
# Usage:
#   scripts/e2e/run-all.sh             # boots fresh harness, runs all
#   SKIP_LLM=1 scripts/e2e/run-all.sh  # skip the two real-LLM smokes
#
# Exits non-zero on the first failure with the failing script's tail
# already printed by that script itself.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

# fresh state every run -- some smokes assert "no prior request.json"
# style invariants and would false-flag against carried-over data.
rm -rf /tmp/joi-e2e
bash scripts/e2e/local-up.sh start

run() {
  local name="$1"; shift
  printf '\n########## %s ##########\n' "$name"
  "$@"
}

run "mr-detector (M3)"   bash scripts/e2e/run-mr-detector.sh
run "spec-apply (M5/O6)" bash scripts/e2e/run-spec-apply.sh

if [ "${SKIP_LLM:-0}" = "1" ]; then
  echo
  echo "SKIP_LLM=1 set; skipping classroom + a1-auto-dev real-LLM smokes."
else
  run "classroom (real LLM)"     bash scripts/e2e/run-classroom.sh
  run "a1-auto-dev (real LLM)"   bash scripts/e2e/run-a1-auto-dev.sh
fi

# thread-closed relaunches the harness with the open mr-fetch fixture
# (mr-detector never self-completes), so it must run AFTER the merged-
# fixture smokes.
run "thread-closed (M6)" bash scripts/e2e/run-thread-closed.sh

echo
echo "ALL GREEN — 1.5 full-run passed."
