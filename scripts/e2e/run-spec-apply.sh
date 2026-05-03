#!/usr/bin/env bash
# scripts/e2e/run-spec-apply.sh — deterministic M5 smoke for the
# classroom application gate (`approval.spec_apply`). Skips the real
# LLM (covered by run-classroom.sh) and just exercises the CLI path
# from lesson-plan artifact → action.request → action.response →
# `joi spec apply` → spec.json patched + reload epoch bumped + receipt
# artifact + status.update.
#
# Target: a throwaway agent `lesson-target` materialised on the fly
# under $JOI_AGENT_SPECS. We never touch data/agents/* on disk.
#
# Pre-req: scripts/e2e/local-up.sh start.

set -euo pipefail

ROOT="${JOI_E2E_ROOT:-/tmp/joi-e2e}"
PORT="${JOI_E2E_PORT:-7900}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BIN_JOI="$REPO_ROOT/target/release/joi"

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
  "actor": { "id": "actor_lesson_target", "kind": "agent", "displayName": "before" },
  "transport": { "kind": "noop" },
  "autostart": false
}
EOF
echo "seeded -> $target_dir/spec.json"

# ---- 1. channel + thread as a human ----------------------------------------
step "1. create channel + thread + invite teacher"
ch=$(j channel create --title "spec-apply-smoke" --as actor_e2e_human | jq -r '.channel.id')
j channel invite "$ch" actor_teacher --as actor_e2e_human >/dev/null
th=$(j thread create --channel "$ch" --as actor_e2e_human --title "spec-apply" | jq -r '.thread.id')
echo "channel = $ch"
echo "thread  = $th"

# ---- 2. publish lesson-plan artifact --------------------------------------
step "2. publish lesson-plan artifact (with spec_apply frontmatter)"
plan=$(mktemp /tmp/lesson-plan.XXXXXX).md
cat > "$plan" <<'EOF'
```json
{
  "schema_version": "1",
  "producer": "teacher",
  "task_id": "e2e-smoke-spec-apply",
  "skills": [],
  "spec_apply": {
    "target": { "kind": "agent", "id": "lesson-target" },
    "spec_patch": {
      "actor": { "displayName": "after" }
    },
    "bundle_writes": [
      { "path": "NOTE.md", "contents": "patched by spec-apply smoke\n" }
    ]
  }
}
```

# Lesson plan: rename lesson-target to "after"
EOF
art_id=$(j artifact publish --as actor_teacher --file "$plan" --media-type text/markdown --name "lesson-plan.md" \
          | jq -r '.artifact.id')
echo "artifact = $art_id"

# ---- 3. emit action.request via event append ------------------------------
step "3. teacher emits action.request"
req_payload=$(cat <<EOF
{"requestType":"approval.spec_apply","title":"Apply lesson-plan","choices":[{"id":"approve","label":"Approve"},{"id":"reject","label":"Reject"}]}
EOF
)
req_id=$(j event append --in "$th" --as actor_teacher \
          --type action.request --content-type application/json \
          --text "$req_payload" \
          --artifact-link "$art_id" \
          | jq -r '.event.id')
echo "action.request = $req_id"

# ---- 4. human accepts ------------------------------------------------------
step "4. human accepts the request"
j --as actor_e2e_human action accept "$req_id" --option approve >/dev/null
# Find the action.response we just created.
resp_id=$(j --as actor_e2e_human event list --in "$th" --limit 50 \
  | jq -r --arg req "$req_id" \
      '.events[]? | select(.type=="action.response") | select(.relations[]?.target.id==$req) | .id' \
  | tail -n1)
[ -n "$resp_id" ] || { echo "FAIL: action.response not found"; exit 1; }
echo "action.response = $resp_id"

# ---- 5. apply ---------------------------------------------------------------
step "5. joi spec apply --action <resp>"
out=$(j --as actor_e2e_human spec apply --action "$resp_id")
echo "$out"

# ---- 6. assert ---------------------------------------------------------------
step "6. assert spec patched + receipt artifact"
fail=0

after_name=$(jq -r '.actor.displayName' "$target_dir/spec.json")
[ "$after_name" = "after" ] || { echo "FAIL: spec.actor.displayName=$after_name (expected 'after')"; fail=1; }

[ -f "$target_dir/bundle/NOTE.md" ] || { echo "FAIL: bundle write didn't land"; fail=1; }

[ -d "$target_dir/.backups" ] && backup_count=$(find "$target_dir/.backups" -name spec.json | wc -l | tr -d ' ') || backup_count=0
[ "$backup_count" -ge 1 ] || { echo "FAIL: no backup written"; fail=1; }

epoch_marker="$JOI_AGENT_DATA_ROOT/agents/lesson-target/reload-epoch.json"
[ -f "$epoch_marker" ] || { echo "FAIL: reload epoch marker missing at $epoch_marker"; fail=1; }
if [ -f "$epoch_marker" ]; then
  epoch_val=$(jq -r '.epoch_ms // 0' "$epoch_marker" 2>/dev/null || echo 0)
  [ "$epoch_val" -gt 0 ] || { echo "FAIL: reload epoch is $epoch_val (expected >0) at $epoch_marker"; fail=1; }
fi

# receipt artifact + spec_apply.completed status.update
events=$(j --as actor_e2e_human event list --in "$th" --limit 100)
echo "$events" | jq -e --arg resp "$resp_id" \
  '.events[]? | select(.type=="status.update")
   | select(.payload.kind=="spec_apply.completed")
   | select(.relations[]?.target.id==$resp)' >/dev/null \
  || { echo "FAIL: no status.update spec_apply.completed event responding to $resp_id"; fail=1; }

echo "$events" | jq -e \
  '.events[]? | select(.type=="status.update")
   | select(.payload.kind=="spec_apply.completed")
   | .relations[]? | select(.kind=="attaches_artifact")' >/dev/null \
  || { echo "FAIL: spec_apply.completed event has no attaches_artifact relation (receipt missing)"; fail=1; }

if [ $fail -ne 0 ]; then
  echo "--- recent events ---"
  echo "$events" | jq '.events[]? | {type, actor: .actorId, payload, relations}'
  exit 1
fi

echo
echo "PASS: spec_apply smoke (lesson-plan -> action.request -> accept -> spec patched + receipt + status.update)"
