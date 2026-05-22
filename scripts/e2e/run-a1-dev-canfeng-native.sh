#!/usr/bin/env bash
# Deterministic a1-dev-canfeng Joi-native smoke.
#
# Starts an isolated server + daemon, then proves that the practice layer can
# create validated artifacts, enforce assignment contracts, write task facts
# through services, recompute task-summary.v1, and run replay cases without LLMs.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ROOT="${A1_DEV_NATIVE_E2E_ROOT:-/tmp/a1-dev-canfeng-native-e2e}"
PORT="${A1_DEV_NATIVE_E2E_PORT:-17983}"
JOI="${JOI_BIN:-$REPO_ROOT/target/debug/joi}"
SERVER_BIN="${JOI_SERVER_BIN:-$REPO_ROOT/target/debug/joi-server}"
WS="ws://127.0.0.1:$PORT/rpc"
SERVER_PID=""
DAEMON_PID=""
export JOI_CONFIG_DIR="$ROOT/config"

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
    if "$JOI" --server "$WS" --as actor_router --json actor list >/dev/null 2>&1; then
      return
    fi
    sleep 0.1
  done
  echo "joi-server did not become reachable; log follows" >&2
  tail -80 "$ROOT/logs/server.log" >&2 || true
  exit 1
}

j() {
  "$JOI" --server "$WS" --as actor_router --display "A1 Native Router" --json "$@"
}

publish_validate_attach() {
  local schema="$1"
  local file="$2"
  local role="$3"
  local artifact
  local validation
  local fact_kind
  local fact_payload
  local fact_sig
  artifact="$(j artifact publish --channel --in "$ch" --name "$schema.json" --media-type application/json --file "$file" | jq -r '.artifact.id')"
  j task artifact attach "$task" --artifact-id "$artifact" --schema "$schema" --role "$role" --status active \
    --binding-json "{\"target_key\":\"repo:aone/a1\",\"purpose\":\"$role\",\"head\":\"h1\"}" >/dev/null
  validation="$(python3 "$REPO_ROOT/data/a1-dev-canfeng/contracts/validator.py" validate-artifact --schema "$schema" --file "$file" --artifact-id "$artifact" --task-id "$task")"
  fact_kind="$(printf '%s' "$validation" | jq -r '.fact.kind')"
  fact_payload="$(printf '%s' "$validation" | jq -c '.fact.payload')"
  fact_sig="$(printf '%s' "$validation" | jq -r '.fact.signature')"
  j task fact append "$task" \
    --target-key "$artifact" \
    --kind "$fact_kind" \
    --type status \
    --signature "$fact_sig" \
    --authority contract-validator \
    --producer-id svc_a1_contract_validator \
    --artifact-id "$artifact" \
    --payload-schema contract-validation-result.v1 \
    --payload-json "$fact_payload" \
    --summary "contract validated: $schema" | jq -r '.fact.id'
}

need() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "missing required command: $1" >&2
    exit 2
  }
}

need jq
[ -x "$JOI" ] || { echo "missing $JOI; run cargo build -p joi-cli -p joi-server" >&2; exit 2; }
[ -x "$SERVER_BIN" ] || { echo "missing $SERVER_BIN; run cargo build -p joi-cli -p joi-server" >&2; exit 2; }

rm -rf "$ROOT"
mkdir -p "$ROOT"/{logs,tmp} "$ROOT/machines/ws_a1_native/actor_router/local"
trap stop_processes EXIT

start_server
"$JOI" --server "$WS" daemon \
  --machine-id machine_a1_native_e2e \
  --data-root "$ROOT/machines/ws_a1_native/actor_router/local" \
  --allow-actors actor_none \
  --no-services \
  --no-ipc \
  >"$ROOT/logs/daemon.log" 2>&1 &
DAEMON_PID=$!
sleep 0.5

j actor upsert actor_router --kind agent --display "路由" \
  --capabilities-json '{"capabilities":["task.own","task.assign","action.request","fact.append","artifact.link","projection.read"],"revision":"actor_router@2026-05-19"}' >/dev/null
j actor upsert actor_discovery --kind agent --display "仓库发现" \
  --capabilities-json '{"capabilities":["artifact.publish","workspace.read","fact.append"],"revision":"actor_discovery@2026-05-19"}' >/dev/null
j actor upsert actor_engineering_standards --kind agent --display "研发规范" \
  --capabilities-json '{"capabilities":["artifact.publish","workspace.read","kbase.read","kbase.propose","fact.append"],"revision":"actor_engineering_standards@2026-05-19"}' >/dev/null
j actor upsert actor_delivery --kind agent --display "交付" \
  --capabilities-json '{"capabilities":["workspace.read","repo.write","artifact.publish","fact.append","platform.mr.write"],"revision":"actor_delivery@2026-05-19"}' >/dev/null
j actor upsert actor_examiner --kind agent --display "审查员" \
  --capabilities-json '{"capabilities":["workspace.read","repo.read","artifact.publish","fact.append","platform.mr.comment"],"revision":"actor_examiner@2026-05-19"}' >/dev/null
j actor upsert svc_task_projection --kind service --display "Task Projection" \
  --capabilities-json '{"capabilities":["fact.read","artifact.read","projection.put"],"revision":"task-projection@2026-05-19"}' >/dev/null
j actor upsert mr-watcher --kind service --display "mr-watcher" \
  --capabilities-json '{"capabilities":["platform.mr.read","fact.append","task.ref.find"],"revision":"mr-watcher@2026-05-19"}' >/dev/null
j actor upsert feedback-scanner --kind service --display "feedback-scanner" \
  --capabilities-json '{"capabilities":["feedback.read","task.candidate.emit","fact.append"],"revision":"feedback-scanner@2026-05-19"}' >/dev/null

ch="$(j channel create --title "a1-dev-canfeng-native-e2e" | jq -r '.channel.id')"
for actor in actor_discovery actor_engineering_standards actor_delivery actor_examiner svc_task_projection mr-watcher feedback-scanner; do
  j channel invite "$ch" "$actor" >/dev/null
done

root="$(j event append --channel --in "$ch" --type content.add --text "修复自定义机器人 AM 凭证绑定，并确保反馈闭环" | jq -r '.event.id')"
task="$(j task create --source-event "$root" --title "修复自定义机器人 AM 凭证绑定" --owner actor_router --status in_progress --practice-contract-epoch a1-dev-canfeng@2026-05-19 | jq -r '.task.id')"
j task ref attach "$task" --kind branch --subtype git_branch --value "aone/a1#joi/custom-bot-binding" --normalized "aone/a1#joi/custom-bot-binding" --confidence confirmed >/dev/null
j task ref attach "$task" --kind external_url --subtype feedback --value "BT-001" --normalized "feedback:BT-001" --confidence confirmed >/dev/null

cp "$REPO_ROOT/data/a1-dev-canfeng/replay/cases/normal-task-goal.json" "$ROOT/tmp/task-goal.json"
cp "$REPO_ROOT/data/a1-dev-canfeng/replay/cases/normal-validation-plan.json" "$ROOT/tmp/validation-plan.json"
cp "$REPO_ROOT/data/a1-dev-canfeng/replay/cases/normal-effective-context.json" "$ROOT/tmp/effective-context.json"

goal_fact="$(publish_validate_attach task-goal.v1 "$ROOT/tmp/task-goal.json" current-goal)"
plan_fact="$(publish_validate_attach validation-plan.v1 "$ROOT/tmp/validation-plan.json" current-validation-plan)"
ctx_fact="$(publish_validate_attach effective-context.v1 "$ROOT/tmp/effective-context.json" current-effective-context)"

contract="$ROOT/tmp/delivery-contract.json"
cat >"$contract" <<JSON
{
  "target": {"target_key":"repo:aone/a1","head":"h1"},
  "effects": {"authorized":["repo.push","platform.mr.write"]},
  "workspace": {"resource_key":"worktree:aone/a1:joi/custom-bot-binding","write_mode":"write"},
  "context": {
    "required_validation_facts": ["$goal_fact", "$plan_fact", "$ctx_fact"]
  },
  "required_capabilities": ["workspace.read","repo.write","artifact.publish","fact.append","platform.mr.write"],
  "versions": {
    "target_actor_spec_revision": "actor_delivery@2026-05-19",
    "contract_schema_version": "a1-dev-assignment-contract.v1",
    "practice_contract_version": "a1-dev-canfeng@2026-05-19"
  },
  "idempotency_key": "delivery-native-e2e"
}
JSON

asgn="$(j task assign "$task" --to actor_delivery --type fix --instruction "按 effective-context 实现并写 validation evidence" --contract-file "$contract" | jq -r '.assignment.id')"
pending_allowed="$(j task assignment preflight "$asgn" --target-key repo:aone/a1 --head h1 --effect repo.push | jq -r '.preflight.allowed')"
[ "$pending_allowed" = "false" ] || { echo "pending assignment should not pass preflight" >&2; exit 1; }
j task assignment update "$asgn" --status running >/dev/null
lease_id="$(j task workspace lease acquire "$asgn" --resource-key "worktree:aone/a1:joi/custom-bot-binding" --mode write --ttl-seconds 60 | jq -r '.lease.id')"
[ -n "$lease_id" ] && [ "$lease_id" != "null" ]

gateway_out="$(JOI_BIN="$JOI" JOI_SERVER="$WS" JOI_ACTOR=actor_delivery "$REPO_ROOT/data/services/a1-side-effect-gateway/bundle/preflight-exec.sh" --assignment-id "$asgn" --target-key repo:aone/a1 --head h1 --effect repo.push --preflight-only)"
printf '%s' "$gateway_out" | jq -e '.preflight.allowed == true' >/dev/null
unexpected_head_allowed="$(j task assignment preflight "$asgn" --target-key repo:aone/a1 --head h2 --effect repo.push | jq -r '.preflight.allowed')"
[ "$unexpected_head_allowed" = "false" ] || { echo "unexpected head should not pass preflight" >&2; exit 1; }

cat >"$ROOT/tmp/mr-payload.json" <<JSON
{
  "repo": "aone/a1",
  "mr": "1",
  "target_key": "repo:aone/a1",
  "state": "opened",
  "columns": {"mr":"!1","ci":"pass","review":"quality_pass","ready":"yes","merged":"no"},
  "observed_fields": ["mr","ci","review","ready","merged"],
  "snapshot_completeness": "complete",
  "source_cursor": "mr-snapshot-1",
  "source_snapshot_id": "mr-snapshot-1",
  "source_refs": ["mr:aone/a1!1"],
  "authority_binding": {
    "external_system": "aone-code",
    "external_subject": "reviewer-1",
    "role": "human_reviewer",
    "is_author": false,
    "source_kind": "api",
    "source_refs": ["mr:aone/a1!1"],
    "validated_by": "mr-watcher",
    "validator_revision": "mr-watcher@2026-05-19"
  }
}
JSON
JOI_BIN="$JOI" JOI_SERVER="$WS" JOI_ACTOR=mr-watcher "$REPO_ROOT/data/services/mr-watcher/bundle/poll.py" --task-id "$task" --fixture "$ROOT/tmp/mr-payload.json" >/dev/null

j task fact append "$task" \
  --target-key feedback:BT-001 \
  --kind target.observed \
  --type status \
  --signature feedback:BT-001:snapshot-1 \
  --authority feedback-scanner \
  --authority-binding-json '{"external_system":"feedback","external_subject":"BT-001","role":"feedback_system","is_author":false,"source_kind":"api","source_refs":["feedback:BT-001"],"validated_by":"feedback-scanner","validator_revision":"feedback-scanner@2026-05-19"}' \
  --source-cursor feedback-snapshot-1 \
  --source-snapshot-id feedback-snapshot-1 \
  --observed-field status \
  --unobserved-field deploy \
  --snapshot-completeness partial \
  --producer-id feedback-scanner \
  --payload-schema a1-dev-target-observation.v1 \
  --payload-json '{"target_kind":"feedback","source_refs":["feedback:BT-001"],"terminal_condition":"closed_fixed","columns":{"status":"waiting_fixed"}}' >/dev/null

JOI_BIN="$JOI" JOI_SERVER="$WS" JOI_ACTOR=svc_task_projection "$REPO_ROOT/data/a1-dev-canfeng/projection/recompute.py" --task-id "$task" --put >/dev/null
projection="$(j task projection get "$task" --type summary)"
printf '%s' "$projection" | jq -e '.health == "fresh"' >/dev/null
printf '%s' "$projection" | jq -e '.projection.payload.targets[] | select(.key=="repo:aone/a1") | .columns.ci == "pass"' >/dev/null
printf '%s' "$projection" | jq -e '.projection.payload.targets[] | select(.key=="feedback:BT-001") | .next_owner == "delivery"' >/dev/null

candidate="$(FEEDBACK_SCANNER_ITEMS='[{"feedback_id":"BT-002","source_refs":["feedback:BT-002"]}]' "$REPO_ROOT/data/services/feedback-scanner/bundle/scan.py")"
printf '%s' "$candidate" | jq -e '._meta.taskRefCandidates[0].normalized == "BT-002" and .hands_off_to == "actor_router"' >/dev/null

python3 "$REPO_ROOT/data/a1-dev-canfeng/replay/run.py" >/dev/null

j task assignment update "$asgn" --status completed \
  --evidence-ref "$lease_id" \
  --result-fact-id "$ctx_fact" \
  --result-envelope-json "{\"assignment_id\":\"$asgn\",\"status\":\"completed\",\"evidence_refs\":[\"$lease_id\",\"$ctx_fact\"]}" >/dev/null

echo "A1_DEV_CANFENG_NATIVE_E2E_OK task=$task assignment=$asgn projection=fresh"
