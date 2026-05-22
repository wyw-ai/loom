#!/usr/bin/env bash
# Reproducible Joi core terminal smoke.
#
# This is intentionally LLM-free. It starts an isolated joi-server and daemon,
# then exercises the task primitives that a1-dev-canfeng relies on, including
# real concurrent ref/fact/assignment/lease races and restart durability.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ROOT="${JOI_CORE_E2E_ROOT:-/tmp/joi-core-terminal-e2e}"
PORT="${JOI_CORE_E2E_PORT:-17982}"
JOI="${JOI_BIN:-$REPO_ROOT/target/debug/joi}"
SERVER_BIN="${JOI_SERVER_BIN:-$REPO_ROOT/target/debug/joi-server}"
WS="ws://127.0.0.1:$PORT/rpc"
SERVER_PID=""
DAEMON_PID=""
BG_PIDS=()
export JOI_CONFIG_DIR="$ROOT/config"

need() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "missing required command: $1" >&2
    exit 2
  }
}

stop_processes() {
  if [ -n "${DAEMON_PID:-}" ] && kill -0 "$DAEMON_PID" 2>/dev/null; then
    kill "$DAEMON_PID" 2>/dev/null || true
    wait "$DAEMON_PID" 2>/dev/null || true
  fi
  if [ -n "${SERVER_PID:-}" ] && kill -0 "$SERVER_PID" 2>/dev/null; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
}

start_server() {
  "$SERVER_BIN" --bind "127.0.0.1:$PORT" --data-dir "$ROOT/server-data" \
    >"$ROOT/logs/server.log" 2>&1 &
  SERVER_PID=$!
  for _ in $(seq 1 50); do
    if "$JOI" --server "$WS" --as actor_core_human --json actor list >/dev/null 2>&1; then
      return
    fi
    sleep 0.1
  done
  echo "joi-server did not become reachable; log follows" >&2
  tail -80 "$ROOT/logs/server.log" >&2 || true
  exit 1
}

restart_server() {
  kill "$SERVER_PID" 2>/dev/null || true
  wait "$SERVER_PID" 2>/dev/null || true
  SERVER_PID=""
  start_server
}

j() {
  "$JOI" --server "$WS" --as actor_core_human --display "Core E2E Human" --json "$@"
}

run_bg_json() {
  local out="$1"
  shift
  (
    set +e
    "$@" >"$out" 2>"$out.err"
    echo $? >"$out.rc"
  ) &
  BG_PIDS+=("$!")
}

wait_bg_json() {
  local pid
  for pid in "${BG_PIDS[@]}"; do
    wait "$pid"
  done
  BG_PIDS=()
}

assert_rc_ok() {
  local file="$1"
  local rc
  rc="$(cat "$file.rc")"
  if [ "$rc" != "0" ]; then
    echo "command failed rc=$rc: $file" >&2
    cat "$file.err" >&2 || true
    exit 1
  fi
}

need jq
[ -x "$JOI" ] || { echo "missing $JOI; run cargo build -p joi-cli -p joi-server" >&2; exit 2; }
[ -x "$SERVER_BIN" ] || { echo "missing $SERVER_BIN; run cargo build -p joi-cli -p joi-server" >&2; exit 2; }

rm -rf "$ROOT"
mkdir -p "$ROOT"/{logs,tmp} "$ROOT/machines/ws_core_terminal/actor_core_human/local"
trap stop_processes EXIT

start_server
"$JOI" --server "$WS" daemon \
  --machine-id machine_core_terminal_e2e \
  --data-root "$ROOT/machines/ws_core_terminal/actor_core_human/local" \
  --allow-actors actor_none \
  --no-services \
  --no-ipc \
  >"$ROOT/logs/daemon.log" 2>&1 &
DAEMON_PID=$!
sleep 0.5

j actor upsert actor_core_owner --kind agent --display "Core Owner" \
  --capabilities-json '{"capabilities":["task.own","task.assign"],"revision":"owner-rev"}' >/dev/null
j actor upsert actor_delivery_a --kind agent --display "Delivery A" \
  --capabilities-json '{"capabilities":["workspace.write","artifact.publish"],"revision":"rev-a"}' >/dev/null
j actor upsert actor_delivery_b --kind agent --display "Delivery B" \
  --capabilities-json '{"capabilities":["workspace.write","artifact.publish"],"revision":"rev-a"}' >/dev/null

ch="$(j channel create --title "joi-core-terminal-e2e" | jq -r '.channel.id')"
for actor in actor_core_owner actor_delivery_a actor_delivery_b; do
  j channel invite "$ch" "$actor" >/dev/null
done

root="$(j event append --channel --in "$ch" --type content.add --text "core terminal root" | jq -r '.event.id')"
root_text="$(j event get "$root" | jq -r '.event.payload.text')"
[ "$root_text" = "core terminal root" ] || { echo "event get returned unexpected root text: $root_text" >&2; exit 1; }
task="$(j task create --source-event "$root" --title "core terminal root" --owner actor_core_owner --status in_progress --practice-contract-epoch core-terminal@2026-05-19 | jq -r '.task.id')"

child_a_root="$(j event append --channel --in "$ch" --type content.add --text "child A" | jq -r '.event.id')"
child_b_root="$(j event append --channel --in "$ch" --type content.add --text "child B" | jq -r '.event.id')"
child_a="$(j task create --source-event "$child_a_root" --title "child A" --owner actor_core_owner --parent-source-event "$root" --parent-task "$task" | jq -r '.task.id')"
child_b="$(j task create --source-event "$child_b_root" --title "child B" --owner actor_core_owner --parent-source-event "$root" --parent-task "$task" | jq -r '.task.id')"

# Concurrent confirmed ref attach: exactly one non-terminal task may own it.
run_bg_json "$ROOT/tmp/ref-a.json" "$JOI" --server "$WS" --as actor_core_human --json \
  task ref attach "$child_a" --kind branch --subtype git_branch --value repo#race --normalized repo#race --confidence confirmed
run_bg_json "$ROOT/tmp/ref-b.json" "$JOI" --server "$WS" --as actor_core_human --json \
  task ref attach "$child_b" --kind branch --subtype git_branch --value repo#race --normalized repo#race --confidence confirmed
wait_bg_json
ref_success=0
for f in "$ROOT"/tmp/ref-*.json; do
  [ "$(cat "$f.rc")" = "0" ] && ref_success=$((ref_success + 1))
done
[ "$ref_success" = "1" ] || { echo "expected exactly one ref attach success, got $ref_success" >&2; exit 1; }
[ "$(j task ref find --channel "$ch" --kind branch --subtype git_branch --normalized repo#race --confidence confirmed --status active | jq '.refs | length')" = "1" ]

# Concurrent duplicate fact append: one created=true, the rest are idempotent.
for i in $(seq 1 8); do
  run_bg_json "$ROOT/tmp/fact-$i.json" "$JOI" --server "$WS" --as actor_core_human --json \
    task fact append "$task" --target-key mr:1 --kind ci.status --type status \
    --signature ci:p1 --authority code --authority-binding-json '{"source":"e2e"}' \
    --source-cursor 100 --source-snapshot-id snap-100 \
    --observed-field state --snapshot-completeness complete \
    --producer-id actor_ci --payload-schema ci-status.v1 --payload-json '{"state":"passed"}'
done
wait_bg_json
created_count="$(jq -s '[.[] | select(.created == true)] | length' "$ROOT"/tmp/fact-*.json)"
[ "$created_count" = "1" ] || { echo "expected one created fact, got $created_count" >&2; exit 1; }

# Active artifact + validation fact guard.
art="$(j artifact publish --channel --in "$ch" --name effective-context.json --media-type application/json --text '{"schema":"effective-context.v1"}' | jq -r '.artifact.id')"
link="$(j task artifact attach "$task" --artifact-id "$art" --schema effective-context.v1 --role current --status active --binding-json '{"target_key":"repo#main","purpose":"context"}' | jq -r '.link.id')"
validation_fact="$(j task fact append "$task" --target-key "$art" --kind artifact.contract_validated --type status --signature "validate:$art" --artifact-id "$art" --payload-schema contract-validation-result.v1 --payload-json '{"valid":true}' | jq -r '.fact.id')"

# Concurrent idempotent assignment create.
contract_idem="$ROOT/tmp/contract-idem.json"
cat >"$contract_idem" <<JSON
{"idempotency_key":"idem-core","target":{"target_key":"repo#main","head":"h1"},"effects":{"authorized":[]},"context":{}}
JSON
for i in 1 2; do
  run_bg_json "$ROOT/tmp/assign-idem-$i.json" "$JOI" --server "$WS" --as actor_core_human --json \
    task assign "$task" --to actor_delivery_a --type investigate --instruction "same" --contract-file "$contract_idem" --idempotency-key idem-core
done
wait_bg_json
for f in "$ROOT"/tmp/assign-idem-*.json; do assert_rc_ok "$f"; done
[ "$(jq -r '.assignment.id' "$ROOT"/tmp/assign-idem-*.json | sort -u | wc -l | tr -d ' ')" = "1" ]

contract_a="$ROOT/tmp/contract-a.json"
contract_b="$ROOT/tmp/contract-b.json"
cat >"$contract_a" <<JSON
{"target":{"target_key":"repo#main","head":"h1"},"effects":{"authorized":["repo.push"]},"workspace":{"resource_key":"worktree:core","write_mode":"write"},"context":{"required_artifacts":["$art"],"required_validation_facts":["$validation_fact"]},"required_capabilities":["workspace.write"],"versions":{"target_actor_spec_revision":"rev-a"},"idempotency_key":"lease-a"}
JSON
cat >"$contract_b" <<JSON
{"target":{"target_key":"repo#main","head":"h1"},"effects":{"authorized":["repo.push"]},"workspace":{"resource_key":"worktree:core","write_mode":"write"},"context":{"required_artifacts":["$art"],"required_validation_facts":["$validation_fact"]},"required_capabilities":["workspace.write"],"versions":{"target_actor_spec_revision":"rev-a"},"idempotency_key":"lease-b"}
JSON
asgn_a="$(j task assign "$task" --to actor_delivery_a --type fix --instruction "fix A" --contract-file "$contract_a" | jq -r '.assignment.id')"
asgn_b="$(j task assign "$task" --to actor_delivery_b --type fix --instruction "fix B" --contract-file "$contract_b" | jq -r '.assignment.id')"

# Concurrent write lease acquire: one holder, one visible conflict.
run_bg_json "$ROOT/tmp/lease-a.json" "$JOI" --server "$WS" --as actor_delivery_a --json \
  task workspace lease acquire "$asgn_a" --resource-key worktree:core --mode write --ttl-seconds 60
run_bg_json "$ROOT/tmp/lease-b.json" "$JOI" --server "$WS" --as actor_delivery_b --json \
  task workspace lease acquire "$asgn_b" --resource-key worktree:core --mode write --ttl-seconds 60
wait_bg_json
for f in "$ROOT"/tmp/lease-*.json; do assert_rc_ok "$f"; done
lease_count="$(jq -s '[.[] | select(.lease != null)] | length' "$ROOT"/tmp/lease-*.json)"
conflict_count="$(jq -s '[.[] | select((.conflicts // []) | length > 0)] | length' "$ROOT"/tmp/lease-*.json)"
[ "$lease_count" = "1" ] || { echo "expected one lease holder, got $lease_count" >&2; exit 1; }
[ "$conflict_count" = "1" ] || { echo "expected one lease conflict, got $conflict_count" >&2; exit 1; }
holder="$(jq -s -r '.[] | select(.lease != null) | .lease.holderAssignmentId' "$ROOT"/tmp/lease-*.json)"
lease_id="$(jq -s -r '.[] | select(.lease != null) | .lease.id' "$ROOT"/tmp/lease-*.json)"

pre_pending="$(j task assignment preflight "$holder" --target-key repo#main --head h1 --effect repo.push | jq -r '.preflight.allowed')"
[ "$pre_pending" = "false" ] || { echo "pending assignment unexpectedly passed preflight" >&2; exit 1; }
j task assignment update "$holder" --status running >/dev/null
pre_allowed="$(j task assignment preflight "$holder" --target-key repo#main --head h1 --effect repo.push | jq -r '.preflight.allowed')"
[ "$pre_allowed" = "true" ] || { echo "running leased assignment failed preflight" >&2; exit 1; }

set +e
j task assignment update "$holder" --status completed >/dev/null 2>"$ROOT/tmp/complete-no-envelope.err"
complete_rc=$?
set -e
[ "$complete_rc" != "0" ] || { echo "completion without envelope unexpectedly succeeded" >&2; exit 1; }
j task assignment update "$holder" --status completed \
  --result-envelope-json "{\"assignment_id\":\"$holder\",\"status\":\"completed\",\"verdict\":\"pass\",\"evidence_refs\":[\"$lease_id\"]}" >/dev/null
post_preflight="$(j task assignment preflight "$holder" --target-key repo#main --head h1 --effect repo.push | jq -r '.preflight.allowed')"
[ "$post_preflight" = "false" ] || { echo "completed assignment unexpectedly passed preflight" >&2; exit 1; }

# Change ack must carry a real disposition result or reason.
j task projection put "$task" --type summary --health fresh --watermark-json "{\"fact_ids\":[\"$validation_fact\"]}" --payload-schema task-summary.v1 --payload-json '{"summary":"ready"}' >/dev/null
change_id="$(j task change list --task-id "$task" | jq -r '.deliveries[] | select(.change.changeType=="projection") | .change.id' | tail -n1)"
[ -n "$change_id" ] && [ "$change_id" != "null" ]
set +e
j task change ack "$change_id" --disposition noop_recorded >/dev/null 2>"$ROOT/tmp/ack-empty.err"
ack_rc=$?
set -e
[ "$ack_rc" != "0" ] || { echo "empty task change ack unexpectedly succeeded" >&2; exit 1; }
j task change ack "$change_id" --disposition noop_recorded --reason "projection observed" >/dev/null

# Durable backlog survives server restart.
j task fact append "$task" --target-key mr:1 --kind reviewer.status --type status --signature reviewer:1 --payload-schema reviewer-status.v1 --payload-json '{"state":"approved"}' >/dev/null
pending_before="$(j task change list --task-id "$task" | jq '.deliveries | length')"
[ "$pending_before" -gt 0 ] || { echo "expected pending task changes before restart" >&2; exit 1; }
restart_server
pending_after="$(j task change list --task-id "$task" | jq '.deliveries | length')"
[ "$pending_after" -gt 0 ] || { echo "pending task changes lost after restart" >&2; exit 1; }

# MCP memory cannot bypass accepted-source guard.
profile="$ROOT/tmp/profile"
mkdir -p "$profile"
bad_mcp="$(printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"memory.append","arguments":{"summary":"bad accepted","status":"accepted","channelId":"ch1"}}}' | "$JOI" mcp memory --actor-id actor_core_owner --profile-dir "$profile")"
echo "$bad_mcp" | jq -e '.error.message | contains("accepted memory requires")' >/dev/null
good_mcp="$(printf '%s\n' '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"memory.append","arguments":{"summary":"pending lesson","channelId":"ch1"}}}' | "$JOI" mcp memory --actor-id actor_core_owner --profile-dir "$profile")"
echo "$good_mcp" | jq -e '.result.content[0].text | contains("recorded")' >/dev/null

echo "JOI_CORE_TERMINAL_E2E_OK task=$task holder=$holder pending_after_restart=$pending_after"
