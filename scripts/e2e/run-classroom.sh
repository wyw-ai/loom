#!/usr/bin/env bash
# scripts/e2e/run-classroom.sh — real-LLM classroom剧情 (M7 leg 1, 1.4b).
#
# Drives the full classroom loop with `actor_teacher` running real Claude
# (via ~/.claude/settings-glm.json by default, linked into the actor
# profile by local-up.sh). Validates that teacher emits a lesson-plan
# artifact carrying the `spec_apply` frontmatter block per
# docs/artifact-contracts.md §4 and a matching
# `action.request type=approval.spec_apply`. The driver then accepts as
# human, runs `joi spec apply`, and asserts the spec/bundle/reload-epoch
# side-effects.
#
# Target of the patch: a synthesized throwaway agent `lesson-target`
# (same as run-spec-apply.sh) — zero blast radius on real specs.
#
# Pre-req: scripts/e2e/local-up.sh start.
#
# Tunables:
#   JOI_E2E_CLASSROOM_TIMEOUT  seconds to wait for action.request (default 600)

set -euo pipefail

ROOT="${JOI_E2E_ROOT:-/tmp/joi-e2e}"
PORT="${JOI_E2E_PORT:-7900}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BIN_JOI="$REPO_ROOT/target/release/joi"
TIMEOUT="${JOI_E2E_CLASSROOM_TIMEOUT:-600}"

export JOI_SERVER="ws://127.0.0.1:$PORT/rpc"
export JOI_AGENT_DATA_ROOT="$ROOT/agent-data"
export JOI_SERVICE_HOST_DATA="$ROOT/service-data"
export JOI_AGENT_SPECS="$ROOT/agent-specs"
export JOI_SERVICE_SPECS="$ROOT/service-specs"

step() { echo; echo "=== $* ==="; }
j()    { "$BIN_JOI" --json "$@"; }

# ---- 0. seed lesson-target spec ---------------------------------------------
step "0. seed lesson-target agent spec"
target_dir="$JOI_AGENT_SPECS/lesson-target"
mkdir -p "$target_dir/bundle"
cat > "$target_dir/spec.json" <<'EOF'
{
  "actor": { "id": "actor_lesson_target", "kind": "agent", "displayName": "before-classroom" },
  "transport": { "kind": "noop" },
  "autostart": false
}
EOF
echo "seeded -> $target_dir/spec.json"

# ---- 1. channel + thread ----------------------------------------------------
step "1. create channel + thread + invite teacher"
ch=$(j channel create --title "classroom-real-llm" --as actor_e2e_human | jq -r '.channel.id')
j channel invite "$ch" actor_teacher --as actor_e2e_human >/dev/null
th=$(j thread create --channel "$ch" --as actor_e2e_human --title "rename-lesson-target" | jq -r '.thread.id')
echo "channel = $ch"
echo "thread  = $th"

# ---- 2. publish task-goal + DoD as human -----------------------------------
step "2. publish task-goal.json + definition-of-done.json"
goal_file=$(mktemp /tmp/task-goal.XXXXXX).json
cat > "$goal_file" <<EOF
{
  "schema_version": "1",
  "task_id": "e2e-classroom-rename-lesson-target",
  "summary": "Rename actor_lesson_target's displayName from 'before-classroom' to 'after-via-llm', and write a NOTE.md into its bundle/.",
  "target": { "kind": "agent", "id": "lesson-target" },
  "constraints": [
    "Use the spec_apply block in lesson-plan.md frontmatter — do not edit spec.json directly.",
    "Bundle write path NOTE.md is relative to the spec's bundle/ sibling."
  ]
}
EOF
goal_id=$(j artifact publish --as actor_e2e_human --file "$goal_file" \
            --media-type application/json --name "task-goal.json" \
          | jq -r '.artifact.id')
echo "task-goal = $goal_id"

dod_file=$(mktemp /tmp/dod.XXXXXX).json
cat > "$dod_file" <<EOF
{
  "schema_version": "1",
  "task_id": "e2e-classroom-rename-lesson-target",
  "criteria": [
    { "id": "dod-1", "must": "lesson-plan.md frontmatter spec_apply.target = {kind:'agent', id:'lesson-target'}" },
    { "id": "dod-2", "must": "spec_apply.spec_patch sets actor.displayName to 'after-via-llm'" },
    { "id": "dod-3", "must": "spec_apply.bundle_writes contains a NOTE.md with non-empty contents" },
    { "id": "dod-4", "must": "teacher emits action.request type=approval.spec_apply attaching the lesson-plan artifact" }
  ]
}
EOF
dod_id=$(j artifact publish --as actor_e2e_human --file "$dod_file" \
            --media-type application/json --name "definition-of-done.json" \
          | jq -r '.artifact.id')
echo "dod = $dod_id"

# ---- 3. trigger teacher -----------------------------------------------------
step "3. handoff to teacher (hands_off_to relation)"
prompt_file=$(mktemp /tmp/classroom-prompt.XXXXXX).txt
cat > "$prompt_file" <<PROMPT
Run phase A. You MUST execute the joi CLI commands yourself — do not just describe them.

Step 1. Write a lesson-plan.md whose VERY FIRST non-blank lines are a fenced JSON block (literal triple-backtick + the word json), NOT yaml. Schema:

JSONFENCE
{
  "schema_version": "1",
  "producer": "teacher",
  "task_id": "e2e-classroom-rename-lesson-target",
  "skills": [],
  "spec_apply": {
    "target": { "kind": "agent", "id": "lesson-target" },
    "spec_patch": { "actor": { "displayName": "after-via-llm" } },
    "bundle_writes": [
      { "path": "NOTE.md", "contents": "# updated by teacher\n" }
    ]
  }
}
JSONFENCEEND

(Replace JSONFENCE with three backticks followed by the word json on a single line, and JSONFENCEEND with three backticks on a single line.)

Note: spec_patch is actor.displayName (nested under "actor"), NOT a top-level "displayName". Frontmatter MUST be inside a triple-backtick json fence — no YAML --- blocks.

Step 2. Publish it via:
  joi --json artifact publish --file <local lesson-plan.md path> --media-type text/markdown --name lesson-plan.md
Capture the returned artifact id (jq -r '.artifact.id').

Step 3. Emit an action.request event yourself (do NOT just print it). There is NO 'joi action request' subcommand — use 'joi event append --type action.request'. Replace LP with the artifact id from step 2:

  joi --json event append --in $th --type action.request --content-type application/json --text '{"requestType":"approval.spec_apply","title":"Apply lesson-plan","choices":[{"id":"approve","label":"Approve"},{"id":"reject","label":"Reject"}]}' --artifact-link LP

The payload JSON MUST use exactly these camelCase fields: requestType, title, choices. requestType MUST equal "approval.spec_apply".

Thread id (literal): $th

Stop after the action.request runs successfully. Do not run 'joi spec apply' — that's the human's job.
PROMPT
trigger_text=$(cat "$prompt_file")
j event append --in "$th" --as actor_e2e_human \
  --type message \
  --handoff actor_teacher \
  --text "$trigger_text" \
  --artifact-link "$goal_id" \
  --artifact-link "$dod_id" >/dev/null
echo "trigger event appended; teacher should pick up within a few seconds."

# ---- 4. poll for action.request type=approval.spec_apply --------------------
step "4. wait up to ${TIMEOUT}s for teacher's action.request"
deadline=$(( $(date +%s) + TIMEOUT ))
req_id=""
while [ "$(date +%s)" -lt "$deadline" ]; do
  events=$(j --as actor_e2e_human event list --in "$th" --limit 200 || echo '{"events":[]}')
  req_id=$(echo "$events" | jq -r '
    .events[]?
    | select(.type=="action.request")
    | select(.payload.requestType=="approval.spec_apply" or .payload.type=="approval.spec_apply")
    | .id' | tail -n1)
  if [ -n "$req_id" ] && [ "$req_id" != "null" ]; then
    break
  fi
  sleep 5
done
[ -n "$req_id" ] && [ "$req_id" != "null" ] || {
  echo "FAIL: no approval.spec_apply action.request within ${TIMEOUT}s"
  echo "--- last event types ---"
  echo "$events" | jq -r '.events[]? | "\(.type)\t\(.actorId // .actor_id)"'
  exit 1
}
echo "action.request = $req_id"

# ---- 5. fetch attached lesson-plan artifact and sanity-check ---------------
step "5. inspect lesson-plan attached to action.request"
plan_artifact=$(echo "$events" | jq -r --arg req "$req_id" '
  .events[]? | select(.id==$req) | .relations[]?
  | select(.kind=="attaches_artifact") | .target.id' | head -n1)
[ -n "$plan_artifact" ] && [ "$plan_artifact" != "null" ] || {
  echo "FAIL: action.request $req_id has no attaches_artifact relation"
  exit 1
}
echo "lesson-plan artifact = $plan_artifact"

plan_body=$("$BIN_JOI" artifact read "$plan_artifact" --as actor_e2e_human)
[ -n "$plan_body" ] || {
  echo "FAIL: could not read lesson-plan artifact body"
  exit 1
}

# Extract first triple-backtick json fence and verify structure roughly.
fence=$(printf '%s\n' "$plan_body" | awk '/^```json/{flag=1; next} /^```/{flag=0} flag')
[ -n "$fence" ] || { echo 'FAIL: lesson-plan has no fenced json frontmatter'; echo "$plan_body" | head -40; exit 1; }
target_kind=$(echo "$fence" | jq -r '.spec_apply.target.kind // ""')
target_id=$(echo "$fence" | jq -r '.spec_apply.target.id // ""')
patch_name=$(echo "$fence" | jq -r '.spec_apply.spec_patch.actor.displayName // ""')
[ "$target_kind" = "agent" ] || { echo "FAIL: spec_apply.target.kind=$target_kind"; exit 1; }
[ "$target_id" = "lesson-target" ] || { echo "FAIL: spec_apply.target.id=$target_id"; exit 1; }
[ "$patch_name" = "after-via-llm" ] || { echo "FAIL: spec_patch.actor.displayName=$patch_name"; exit 1; }
echo "lesson-plan structure OK (target=$target_kind:$target_id, displayName=$patch_name)"

# ---- 6. human accepts -------------------------------------------------------
step "6. human accepts approval.spec_apply"
j --as actor_e2e_human action accept "$req_id" --option approve >/dev/null
events_after=$(j --as actor_e2e_human event list --in "$th" --limit 200)
resp_id=$(echo "$events_after" | jq -r --arg req "$req_id" '
  .events[]? | select(.type=="action.response")
  | select(.relations[]?.target.id==$req) | .id' | tail -n1)
[ -n "$resp_id" ] && [ "$resp_id" != "null" ] || { echo "FAIL: action.response not found"; exit 1; }
echo "action.response = $resp_id"

# ---- 7. joi spec apply -----------------------------------------------------
step "7. joi spec apply --action <resp>"
j --as actor_e2e_human spec apply --action "$resp_id"

# ---- 8. assert ---------------------------------------------------------------
step "8. assert spec patched + bundle write + reload-epoch + receipt"
fail=0
after_name=$(jq -r '.actor.displayName' "$target_dir/spec.json")
[ "$after_name" = "after-via-llm" ] || { echo "FAIL: displayName=$after_name (expected after-via-llm)"; fail=1; }
[ -f "$target_dir/bundle/NOTE.md" ] || { echo "FAIL: bundle/NOTE.md missing"; fail=1; }

epoch_marker="$JOI_AGENT_DATA_ROOT/agents/lesson-target/reload-epoch.json"
[ -f "$epoch_marker" ] || { echo "FAIL: reload-epoch missing at $epoch_marker"; fail=1; }
if [ -f "$epoch_marker" ]; then
  epoch_val=$(jq -r '.epoch_ms // 0' "$epoch_marker")
  [ "$epoch_val" -gt 0 ] || { echo "FAIL: epoch_ms=$epoch_val"; fail=1; }
fi

events_final=$(j --as actor_e2e_human event list --in "$th" --limit 200)
echo "$events_final" | jq -e --arg resp "$resp_id" '
  .events[]? | select(.type=="status.update")
  | select(.payload.kind=="spec_apply.completed")
  | select(.relations[]?.target.id==$resp)' >/dev/null \
  || { echo "FAIL: spec_apply.completed status.update missing"; fail=1; }
echo "$events_final" | jq -e '
  .events[]? | select(.type=="status.update")
  | select(.payload.kind=="spec_apply.completed")
  | .relations[]? | select(.kind=="attaches_artifact")' >/dev/null \
  || { echo "FAIL: spec_apply.completed has no receipt artifact relation"; fail=1; }

if [ $fail -ne 0 ]; then
  echo "--- recent events ---"
  echo "$events_final" | jq '.events[]? | {type, actor: (.actorId // .actor_id), payload, relations}'
  exit 1
fi

echo
echo "PASS: real-LLM classroom剧情 (1.4b) — channel=$ch thread=$th"
