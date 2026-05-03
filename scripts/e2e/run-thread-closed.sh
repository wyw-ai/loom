#!/usr/bin/env bash
# scripts/e2e/run-thread-closed.sh — M6 smoke: assert the
# thread-bound service watcher reaps an instance when its bound
# thread is deleted (auto_stop_on=thread.closed, §4.7.3).
#
# Strategy: relaunch the harness with the *open* fixture (mr-detector
# never self-completes), start an instance, delete the thread, and
# expect request.json to disappear within a short window. The
# scheduler's `await_responds_to` path keeps the plugin task running
# indefinitely against the open fixture, so any reap we observe is
# strictly attributable to the thread.closed branch.
#
# Restores the merged fixture on exit so subsequent smokes (M3) work.
#
# Exits 0 on green.

set -euo pipefail

ROOT="${JOI_E2E_ROOT:-/tmp/joi-e2e}"
PORT="${JOI_E2E_PORT:-7900}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
JOI="$REPO_ROOT/target/release/joi"

USER_ACTOR="actor_e2e_human"
USER_DISPLAY="E2E Human"

[ -x "$JOI" ] || { echo "missing $JOI; run scripts/e2e/local-up.sh build" >&2; exit 2; }

restore_merged_fixture() {
  echo
  echo "=== restoring merged fixture for subsequent smokes ==="
  bash "$REPO_ROOT/scripts/e2e/local-up.sh" nuke >/dev/null 2>&1 || true
  bash "$REPO_ROOT/scripts/e2e/local-up.sh" start >/dev/null
}
trap restore_merged_fixture EXIT

step() { printf '\n=== %s ===\n' "$*"; }

step "1. relaunch harness with open fixture"
bash "$REPO_ROOT/scripts/e2e/local-up.sh" nuke >/dev/null 2>&1 || true
MR_DETECTOR_FETCH_CMD="$REPO_ROOT/tests/e2e/fixtures/mr-fetch-open.sh" \
  bash "$REPO_ROOT/scripts/e2e/local-up.sh" start >/dev/null

export JOI_SERVER="ws://127.0.0.1:$PORT/rpc"
export JOI_AGENT_DATA_ROOT="$ROOT/agent-data"
export JOI_SERVICE_HOST_DATA="$ROOT/service-data"

j() { "$JOI" --as "$USER_ACTOR" --display "$USER_DISPLAY" --json "$@"; }

step "2. create channel + thread"
ch=$(j channel create --title "e2e-thread-closed" | jq -r '.channel.id // .id')
th=$(j thread create --channel "$ch" --title "thread-closed-smoke" | jq -r '.thread.id // .id')
echo "channel=$ch thread=$th"

step "3. invite svc_mr_detector + start instance"
j channel invite "$ch" svc_mr_detector >/dev/null 2>&1 || true
j service start --spec mr-detector --in "$th" --channel "$ch" \
  --params '{"mr_url":"fixture://open"}' \
  --specs "$ROOT/service-specs" >/dev/null

req="$ROOT/service-data/services/mr-detector/instances/$th/request.json"

step "4. wait up to 10s for request.json to land"
for _ in 1 2 3 4 5 6 7 8 9 10; do
  [ -e "$req" ] && break
  sleep 1
done
[ -e "$req" ] || { echo "FAIL: request.json never appeared at $req" >&2; exit 1; }

step "5. let one poll cycle run (open fixture won't self-complete)"
sleep 4
[ -e "$req" ] || { echo "FAIL: request.json vanished before we deleted the thread" >&2; exit 1; }

step "6. delete the thread"
j thread delete "$th" >/dev/null
echo "thread $th deleted; watcher should reap within ~3 ticks"

step "7. wait up to 30s for watcher to reap request.json"
saw_reap=0
for i in $(seq 1 30); do
  if [ ! -e "$req" ]; then
    saw_reap=1
    echo "reaped after ${i}s"
    break
  fi
  sleep 1
done

step "8. assert"
if [ "$saw_reap" -ne 1 ]; then
  echo "FAIL: request.json still present 30s after thread.delete" >&2
  echo "--- service-host log tail ---"
  tail -60 "$ROOT/logs/service-host.log" || true
  exit 1
fi

echo "PASS: thread-bound watcher reaps on thread.closed (M6) — channel=$ch thread=$th"
