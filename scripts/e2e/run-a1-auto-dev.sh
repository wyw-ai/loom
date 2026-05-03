#!/usr/bin/env bash
# scripts/e2e/run-a1-auto-dev.sh — 1.4c real-LLM a1-auto-dev 剧情 driver.
#
# Exercises router → discovery → delivery → thread-bound mr-detector with
# real LLMs (claude CLI under actor_profile, GLM/Minimax via
# ~/.claude/settings-*.json). Driver pre-publishes task-goal +
# definition-of-done (classmaster's job) so the chain we exercise is the
# critical 3-hop one called out in design §1.5.2.
#
# Pre-req: scripts/e2e/local-up.sh start  (joi-server + agent serve +
# service serve up, fixtures seeded, MR_DETECTOR_FETCH_CMD pointing at
# tests/e2e/fixtures/mr-fetch-merged.sh).
#
# Usage:
#   scripts/e2e/run-a1-auto-dev.sh                 # creates fresh channel
#   TIMEOUT=900 scripts/e2e/run-a1-auto-dev.sh
#
# Exits 0 on green. On red, dumps recent events + agent-host log tail.

set -euo pipefail

ROOT="${JOI_E2E_ROOT:-/tmp/joi-e2e}"
PORT="${JOI_E2E_PORT:-7900}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
JOI="$REPO_ROOT/target/release/joi"
TIMEOUT="${TIMEOUT:-600}"

export JOI_SERVER="ws://127.0.0.1:$PORT/rpc"
export JOI_AGENT_DATA_ROOT="$ROOT/agent-data"
export JOI_SERVICE_HOST_DATA="$ROOT/service-data"
USER_ACTOR="actor_e2e_human"
USER_DISPLAY="E2E Human"

[ -x "$JOI" ] || { echo "missing $JOI; run scripts/e2e/local-up.sh build" >&2; exit 2; }
"$JOI" who >/dev/null 2>&1 || { echo "joi-server not reachable at $JOI_SERVER" >&2; exit 2; }

j() { "$JOI" --as "$USER_ACTOR" --display "$USER_DISPLAY" --json "$@"; }
step() { printf '\n=== %s ===\n' "$*"; }

# ------------------------------------------------------------------
# 1. channel + resident discovery thread + invitations
# ------------------------------------------------------------------
step "1. create channel + resident discovery thread"
ch=$(j channel create --title "e2e-a1-auto-dev" | jq -r '.channel.id')
echo "channel = $ch"

for a in actor_router actor_discovery actor_delivery svc_mr_detector; do
  j channel invite "$ch" "$a" >/dev/null 2>&1 || true
done

dth=$(j thread create --channel "$ch" --resident-as discovery \
        --title "discovery-resident" | jq -r '.thread.id')
echo "discovery thread = $dth"

# ------------------------------------------------------------------
# 2. pre-stage task-goal.json + definition-of-done.json
#    (skipping classmaster's LLM hop on purpose — design §1.5.2 puts
#     classmaster only at task-definition entry, and the migration smoke
#     focuses on router→discovery→delivery+mr-detector.)
# ------------------------------------------------------------------
step "2. publish task-goal + dod"
goal_file=$(mktemp /tmp/a1-goal.XXXXXX).json
cat > "$goal_file" <<'EOF'
{
  "schema_version": "1",
  "task_id": "e2e-a1-auto-dev-rename-greeting",
  "producer": "actor_e2e_human",
  "title": "Rename greeting in fixture repo",
  "summary": "In the fixture repo, change the greeting string in greet.txt from 'hello' to 'hello-from-e2e' and open an MR.",
  "scope": "single-repo bug fix"
}
EOF
goal_id=$(j artifact publish --as actor_e2e_human --file "$goal_file" \
            --media-type application/json --name "task-goal.json" \
          | jq -r '.artifact.id')
echo "task-goal = $goal_id"

dod_file=$(mktemp /tmp/a1-dod.XXXXXX).json
cat > "$dod_file" <<'EOF'
{
  "schema_version": "1",
  "task_id": "e2e-a1-auto-dev-rename-greeting",
  "producer": "actor_e2e_human",
  "items": [
    { "id": "dod-1", "text": "greet.txt contains 'hello-from-e2e'" },
    { "id": "dod-2", "text": "MR opened on a topic branch" },
    { "id": "dod-3", "text": "mr-detector reports merged status" }
  ]
}
EOF
dod_id=$(j artifact publish --as actor_e2e_human --file "$dod_file" \
            --media-type application/json --name "definition-of-done.json" \
          | jq -r '.artifact.id')
echo "dod = $dod_id"

# ------------------------------------------------------------------
# 3. trigger router (hop 1)  — handoff to discovery
# ------------------------------------------------------------------
step "3. handoff human → router (hop 1)"
discovery_instr_f=$(mktemp /tmp/disc-instr.XXXXXX).txt
cat > "$discovery_instr_f" <<DISC
你是 discovery agent。任务：发布一份 clone-manifest.json artifact，然后
handoff 回 router。**不要 clone、不要研究代码内容、不要分析 DoD 之外的
仓库**——本任务只有一个仓库，路径已经给定。

Step 1. 把以下 JSON 原样写入 /tmp/manifest-${dth}.json：

JSONFENCE
{"schema_version":"1","task_id":"e2e-a1-auto-dev-rename-greeting","producer":"discovery","created_at":"2026-01-01T00:00:00Z","repos":[{"repo_id":"fixture","clone_url":"file://${ROOT}/fixtures/repo.git","ref":"main","readonly":false,"purpose":"target"}]}
JSONFENCEEND

(JSONFENCE / JSONFENCEEND 是占位符，不要照写——直接把里面的 JSON 对象写进去。)

Step 2. 真的执行（不是描述）：

  joi --json artifact publish --file /tmp/manifest-${dth}.json --media-type application/json --name clone-manifest.json

捕获 .artifact.id（设为 MANIFEST_ID）。

Step 3. 把 manifest handoff 回 router：

  joi --json event append --in $dth --type message --handoff actor_router --text "manifest ready" --artifact-link MANIFEST_ID

完成后单独一行输出 __JOI_DONE__。
DISC
discovery_instr=$(cat "$discovery_instr_f")

router_prompt_f=$(mktemp /tmp/router-prompt.XXXXXX).txt
cat > "$router_prompt_f" <<PROMPT
你是 a1-auto-dev 频道的 router。用户给了 task-goal + DoD，下一步按路由表
应该 handoff 给 discovery（仓库尚未 bootstrap）。

只做一件事：把下面这段 \$DISCOVERY_TEXT 原文（包括占位符）通过 joi event
append 转交给 actor_discovery。**不要自己写 manifest、不要修改文本。**

执行（真的执行，不是打印）：

  TEXT=\$(cat <<'DELIM'
${discovery_instr}
DELIM
  )
  joi --json event append --in $dth --type message --handoff actor_discovery --text "\$TEXT" --artifact-link $goal_id --artifact-link $dod_id

完成后单独一行输出 __JOI_DONE__。
PROMPT
router_text=$(cat "$router_prompt_f")
j event append --in "$dth" --as actor_e2e_human \
  --type message \
  --handoff actor_router \
  --text "$router_text" \
  --artifact-link "$goal_id" \
  --artifact-link "$dod_id" >/dev/null
echo "router triggered."

# ------------------------------------------------------------------
# 4. wait for router → discovery handoff
# ------------------------------------------------------------------
step "4. wait up to ${TIMEOUT}s for router→discovery handoff"
deadline=$(( $(date +%s) + TIMEOUT ))
router_handoff=""
while [ "$(date +%s)" -lt "$deadline" ]; do
  events=$(j event list --in "$dth" --limit 200 || echo '{"events":[]}')
  router_handoff=$(echo "$events" | jq -r '
    .events[]?
    | select(.actorId=="actor_router")
    | select(.relations[]?.kind=="hands_off_to" and .relations[]?.target.id=="actor_discovery")
    | .id' | tail -n1)
  [ -n "$router_handoff" ] && [ "$router_handoff" != "null" ] && break
  sleep 5
done
[ -n "$router_handoff" ] && [ "$router_handoff" != "null" ] || {
  echo "FAIL: router did not handoff to discovery in ${TIMEOUT}s" >&2
  echo "--- agent-host log tail ---"; tail -60 "$ROOT/logs/agent-host.log"
  exit 1
}
echo "router→discovery handoff = $router_handoff"

# ------------------------------------------------------------------
# 5. wait for discovery's clone-manifest.json artifact (hop 2)
#    NB: real discovery LLM is launched by agent-host on the trigger
#    above; we just need to wait for the manifest.
# ------------------------------------------------------------------
step "5. wait up to ${TIMEOUT}s for discovery's clone-manifest"
deadline=$(( $(date +%s) + TIMEOUT ))
manifest_id=""
while [ "$(date +%s)" -lt "$deadline" ]; do
  events=$(j event list --in "$dth" --limit 200 || echo '{"events":[]}')
  manifest_id=$(echo "$events" | jq -r '
    .events[]?
    | select(.actorId=="actor_discovery")
    | (.relations[]? | select(.kind=="attaches_artifact") | .target.id)' \
    | tail -n1)
  if [ -n "$manifest_id" ] && [ "$manifest_id" != "null" ]; then
    body=$(j artifact read "$manifest_id" 2>/dev/null | jq -r '.content // empty' || true)
    if echo "$body" | jq -e '.repos[0].clone_url' >/dev/null 2>&1; then
      break
    fi
    manifest_id=""
  fi
  sleep 5
done
[ -n "$manifest_id" ] || {
  echo "FAIL: discovery did not publish clone-manifest in ${TIMEOUT}s" >&2
  echo "--- agent-host log tail ---"; tail -60 "$ROOT/logs/agent-host.log"
  exit 1
}
echo "clone-manifest = $manifest_id"

# ------------------------------------------------------------------
# 6. driver creates delivery thread bootstrapped from manifest;
#    invites delivery + mr-detector service actor.
# ------------------------------------------------------------------
step "6. create delivery thread (bootstrap-artifact)"
delth=$(j thread create --channel "$ch" \
          --bootstrap-artifact "$manifest_id" \
          --title "delivery-rename-greeting" | jq -r '.thread.id')
echo "delivery thread = $delth"

# ------------------------------------------------------------------
# 7. handoff to delivery (hop 3) — do code change + publish MR descriptor
# ------------------------------------------------------------------
step "7. handoff human → delivery (hop 3)"
mr_desc_marker="MR_DESCRIPTOR_HERE"
deliv_prompt_f=$(mktemp /tmp/deliv-prompt.XXXXXX).txt
cat > "$deliv_prompt_f" <<PROMPT
你是 delivery agent。task-goal + DoD + clone-manifest 已挂在本条 event。
本 e2e 用 fixture 仓库（bare），不需要真做代码改动 / push。你只需发布
MR descriptor + 一条 status.update，演示交付链的 artifact 路径。

Step 1. 把这段 JSON 原样写入 /tmp/mr-descriptor-${delth}.json（不要修改字段）：

JSONFENCE
{"schema_version":"1","task_id":"e2e-a1-auto-dev-rename-greeting","mr_url":"fixture://merged","head_branch":"joi/e2e-a1-auto-dev/rename-greeting","base_branch":"main","title":"feat: rename greeting","producer":"actor_delivery"}
JSONFENCEEND

(JSONFENCE/JSONFENCEEND 是占位符，写文件时直接写里面的对象。)

Step 2. 真的执行：

  joi --json artifact publish --file /tmp/mr-descriptor-${delth}.json --media-type application/json --name mr-descriptor.json

捕获返回的 .artifact.id（设为 MR_ART）。

Step 3. 真的执行：

  joi --json event append --in $delth --type status.update --content-type application/json --text '{"phase":"mr_opened","summary":"fixture MR ready"}' --artifact-link MR_ART

完成后单独一行输出 __JOI_DONE__。
PROMPT
deliv_text=$(cat "$deliv_prompt_f")
j event append --in "$delth" --as actor_e2e_human \
  --type message \
  --handoff actor_delivery \
  --text "$deliv_text" \
  --artifact-link "$goal_id" \
  --artifact-link "$dod_id" \
  --artifact-link "$manifest_id" >/dev/null
echo "delivery triggered."

# ------------------------------------------------------------------
# 8. wait for delivery's mr-descriptor artifact + status.update
# ------------------------------------------------------------------
step "8. wait up to ${TIMEOUT}s for delivery's mr-descriptor + status.update"
deadline=$(( $(date +%s) + TIMEOUT ))
saw_status=0
mr_art=""
while [ "$(date +%s)" -lt "$deadline" ]; do
  events=$(j event list --in "$delth" --limit 200 || echo '{"events":[]}')
  if echo "$events" | jq -e '.events[]? | select(.type=="status.update" and .actorId=="actor_delivery")' >/dev/null 2>&1; then
    saw_status=1
  fi
  cand=$(echo "$events" | jq -r '
    .events[]?
    | select(.actorId=="actor_delivery")
    | (.relations[]? | select(.kind=="attaches_artifact") | .target.id)' \
    | tail -n1)
  if [ -n "$cand" ] && [ "$cand" != "null" ]; then
    body=$(j artifact read "$cand" 2>/dev/null | jq -r '.content // empty' || true)
    if echo "$body" | jq -e '.mr_url' >/dev/null 2>&1; then
      mr_art="$cand"
    fi
  fi
  if [ "$saw_status" = 1 ] && [ -n "$mr_art" ]; then break; fi
  sleep 5
done
[ "$saw_status" = 1 ] || { echo "FAIL: delivery missing status.update"; tail -60 "$ROOT/logs/agent-host.log"; exit 1; }
[ -n "$mr_art" ]      || { echo "FAIL: delivery missing mr-descriptor artifact"; tail -60 "$ROOT/logs/agent-host.log"; exit 1; }
echo "mr-descriptor = $mr_art"

# ------------------------------------------------------------------
# 9. start mr-detector service for the delivery thread
# ------------------------------------------------------------------
step "9. start mr-detector service (thread-bound)"
j service start --spec mr-detector --in "$delth" --channel "$ch" \
  --params '{"mr_url":"fixture://merged"}' \
  --specs "$ROOT/service-specs" >/dev/null
echo "mr-detector instance requested."

# ------------------------------------------------------------------
# 10. wait for mr-detector status.update + self_complete
# ------------------------------------------------------------------
step "10. wait up to 180s for mr-detector status.update + self_complete"
deadline=$(( $(date +%s) + 180 ))
saw_md_status=0
saw_self_complete=0
while [ "$(date +%s)" -lt "$deadline" ]; do
  events=$(j event list --in "$delth" --limit 200 || echo '{"events":[]}')
  if echo "$events" | jq -e '.events[]? | select(.type=="status.update" and .actorId=="svc_mr_detector")' >/dev/null 2>&1; then
    saw_md_status=1
  fi
  if echo "$events" | jq -e '.events[]? | select(.type=="service.self_complete")' >/dev/null 2>&1; then
    saw_self_complete=1
  fi
  if [ "$saw_md_status" = 1 ] && [ "$saw_self_complete" = 1 ]; then break; fi
  sleep 5
done
[ "$saw_md_status" = 1 ]   || { echo "FAIL: mr-detector status.update missing"; tail -40 "$ROOT/logs/service-host.log"; exit 1; }
[ "$saw_self_complete" = 1 ] || { echo "FAIL: mr-detector did not self-complete"; tail -40 "$ROOT/logs/service-host.log"; exit 1; }

# ------------------------------------------------------------------
# 11. final assert
# ------------------------------------------------------------------
step "11. assert"
echo "PASS: real-LLM a1-auto-dev剧情 (1.4c) — channel=$ch"
echo "  discovery thread = $dth"
echo "  delivery thread  = $delth"
echo "  manifest         = $manifest_id"
echo "  mr-descriptor    = $mr_art"
