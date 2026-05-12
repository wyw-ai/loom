#!/usr/bin/env bash
# start-delivery.sh — deterministic delivery launcher for a1-auto-dev.
#
# Turns discovery artifacts into an implementation thread. It creates or reuses
# a delivery thread, provisions its workspace, handoffs actor_delivery, and
# prints one JSON summary for discovery/router bookkeeping.

set -euo pipefail

CHANNEL_ID=""
TASK_GOAL=""
DOD=""
CLONE_MANIFEST=""
FEEDBACK_ID=""
TITLE=""
SOURCE_THREAD_ID=""
WORK_ITEM_IDS=""
AS_ACTOR="${JOI_ACTOR:-actor_discovery}"
JQ_BIN="${JQ_BIN:-jq}"
A1_BIN="${A1_BIN:-a1}"
if [[ -n "${JOI_BIN:-}" ]]; then
    JOI_BIN="$JOI_BIN"
elif command -v joi >/dev/null 2>&1; then
    JOI_BIN="$(command -v joi)"
elif [[ -x /home/canfeng/joi-apps/joi ]]; then
    JOI_BIN="/home/canfeng/joi-apps/joi"
else
    JOI_BIN="joi"
fi
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TOOLS_ROOT="${JOI_AUTO_DEV_TOOLS_ROOT:-$(cd "$SCRIPT_DIR/.." && pwd)}"
PROVISION_SCRIPT="${PROVISION_SCRIPT:-$TOOLS_ROOT/scripts/provision-thread-ws.sh}"

usage() {
    cat <<'EOF'
usage: start-delivery.sh --channel-id <chan> --task-goal <art> --dod <art>
                         --clone-manifest <art> [--feedback-id <id>]
                         [--title <title>] [--work-item-ids <ids>]
                         [--source-thread-id <thread>] [--as <actor>]

Creates/reuses a delivery thread, provisions its workspace, and handoffs
actor_delivery. Prints a JSON object with delivery_thread_id and handoff_event_id.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --channel-id) CHANNEL_ID="$2"; shift 2;;
        --task-goal) TASK_GOAL="$2"; shift 2;;
        --dod|--definition-of-done) DOD="$2"; shift 2;;
        --clone-manifest) CLONE_MANIFEST="$2"; shift 2;;
        --feedback-id) FEEDBACK_ID="$2"; shift 2;;
        --title) TITLE="$2"; shift 2;;
        --source-thread-id) SOURCE_THREAD_ID="$2"; shift 2;;
        --work-item-ids) WORK_ITEM_IDS="$2"; shift 2;;
        --as) AS_ACTOR="$2"; shift 2;;
        -h|--help) usage; exit 0;;
        *) echo "start-delivery: unknown arg: $1" >&2; usage >&2; exit 2;;
    esac
done

[[ -n "$CHANNEL_ID" ]] || { echo "start-delivery: --channel-id required" >&2; exit 2; }
[[ -n "$TASK_GOAL" ]] || { echo "start-delivery: --task-goal required" >&2; exit 2; }
[[ -n "$DOD" ]] || { echo "start-delivery: --dod required" >&2; exit 2; }
[[ -n "$CLONE_MANIFEST" ]] || { echo "start-delivery: --clone-manifest required" >&2; exit 2; }
command -v "$JQ_BIN" >/dev/null || { echo "start-delivery: jq required" >&2; exit 3; }
[[ -x "$JOI_BIN" ]] || { echo "start-delivery: joi binary not executable: $JOI_BIN" >&2; exit 3; }
[[ -x "$PROVISION_SCRIPT" ]] || { echo "start-delivery: provision script not executable: $PROVISION_SCRIPT" >&2; exit 3; }

joi() { "$JOI_BIN" --as "$AS_ACTOR" "$@"; }

artifact_body() {
    local art="$1"
    joi artifact read "$art" --max-bytes 1048576
}

safe_title() {
    local text="$1"
    text=$(printf '%s' "$text" | tr '\n\r\t' '   ' | sed -E 's/[[:space:]]+/ /g; s/^ //; s/ $//')
    printf '%s' "${text:0:90}"
}

thread_id_from_create() {
    "$JQ_BIN" -r '.thread.id // .thread_id // .id // empty'
}

event_id_from_send() {
    "$JQ_BIN" -r '.event.id // .id // empty'
}

if [[ -z "$TITLE" ]]; then
    goal_tmp=$(mktemp)
    artifact_body "$TASK_GOAL" >"$goal_tmp" || true
    TITLE=$("$JQ_BIN" -r '.title // .task.title // .summary // empty' "$goal_tmp" 2>/dev/null || true)
    rm -f "$goal_tmp"
fi
[[ -n "$TITLE" ]] || TITLE="delivery-task"
TITLE=$(safe_title "$TITLE")

if [[ -n "$FEEDBACK_ID" ]]; then
    THREAD_TITLE="[bugfix:${FEEDBACK_ID}] ${TITLE}"
else
    THREAD_TITLE="$TITLE"
fi

existing_thread=""
if [[ -n "$FEEDBACK_ID" ]]; then
    existing_thread=$(joi thread list --channel "$CHANNEL_ID" --json 2>/dev/null \
        | "$JQ_BIN" -r --arg fid "$FEEDBACK_ID" '
            (.threads // .items // .)[]?
            | (.title // .name // "") as $title
            | select(($title | startswith("[bugfix:" + $fid + "]"))
                     or $title == ("delivery-bugfix-" + $fid)
                     or $title == ("bugfix-deliver-" + $fid)
                     or (($title | startswith("delivery-")) and ($title | contains($fid))))
            | (.id // .thread_id // .thread.id // empty)
        ' | head -n 1)
fi

if [[ -n "$existing_thread" ]]; then
    DELIVERY_THREAD_ID="$existing_thread"
else
    root_text="delivery-start: ${THREAD_TITLE}"
    root_out=$(joi event append --channel --in "$CHANNEL_ID" --type thread.opened --text "$root_text" --json)
    root_event_id=$(event_id_from_send <<<"$root_out")
    [[ -n "$root_event_id" ]] || { echo "start-delivery: failed to create root channel event" >&2; exit 4; }
    thread_out=$(joi thread create \
        --channel "$CHANNEL_ID" \
        --root-event "$root_event_id" \
        --title "$THREAD_TITLE" \
        --bootstrap-artifact "$CLONE_MANIFEST" \
        --json)
    DELIVERY_THREAD_ID=$(thread_id_from_create <<<"$thread_out")
    [[ -n "$DELIVERY_THREAD_ID" ]] || { echo "start-delivery: failed to create delivery thread" >&2; exit 4; }
fi

manifest_tmp=$(mktemp)
artifact_body "$CLONE_MANIFEST" >"$manifest_tmp"
manifest_norm=$(mktemp)
"$JQ_BIN" \
    --arg tid "$DELIVERY_THREAD_ID" \
    --arg cid "$CHANNEL_ID" \
    '. + {thread_id:$tid, channel_id:$cid, schema_version:(.schema_version // "2")}' \
    "$manifest_tmp" >"$manifest_norm"

provision_out=$("$PROVISION_SCRIPT" --manifest "$manifest_norm" --chan "$CHANNEL_ID")

kbase_lines=""
while IFS= read -r repo; do
    [[ -n "$repo" ]] || continue
    page="MISSING"
    if command -v "$A1_BIN" >/dev/null 2>&1; then
        page=$("$A1_BIN" -f json kbase search "$repo" --repo-ids 74121 --top 1 2>/dev/null \
            | "$JQ_BIN" -r '.items[0].page_id // .items[0].id // .data[0].page_id // empty' 2>/dev/null || true)
        [[ -n "$page" ]] || page="MISSING"
    fi
    kbase_lines+="- ${repo}: ${page}"$'\n'
done < <("$JQ_BIN" -r '.repos[]? | select((.mode // .role // "worktree") == "worktree") | .repo' "$manifest_norm")
[[ -n "$kbase_lines" ]] || kbase_lines="- MISSING: MISSING"$'\n'

msg=$(cat <<EOF
delivery 启动：feedback_id=${FEEDBACK_ID:-} work_item_ids=${WORK_ITEM_IDS:-${FEEDBACK_ID:-}} task-goal=${TASK_GOAL} DoD=${DOD} clone-manifest=${CLONE_MANIFEST}。
source_thread=${SOURCE_THREAD_ID:-}
workspace=~/joi-workspaces/thread/${DELIVERY_THREAD_ID}/repos/（已 provision，请 cd 进去干活；禁止动 shared/repos 与 channel-level workspace）
target-repos 开发规范（kbase 74121 page-id 列表）：
${kbase_lines}编码每个 repo 前先 a1 kbase page view 74121 <page-id> 读规范；MISSING 的请回报 router 补。
EOF
)
handoff_out=$(joi handoff actor_delivery --in "$DELIVERY_THREAD_ID" --message "$msg" --json)
handoff_event_id=$("$JQ_BIN" -r '.event.id // .id // empty' <<<"$handoff_out")

rm -f "$manifest_tmp" "$manifest_norm"

"$JQ_BIN" -nc \
    --arg delivery_thread_id "$DELIVERY_THREAD_ID" \
    --arg handoff_event_id "$handoff_event_id" \
    --arg thread_title "$THREAD_TITLE" \
    --argjson provision "$("$JQ_BIN" -s '.' <<<"$provision_out")" \
    '{ok:true, delivery_thread_id:$delivery_thread_id, handoff_event_id:$handoff_event_id, thread_title:$thread_title, provision:$provision}'
