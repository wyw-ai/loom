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
BUGFIX_SOURCE="auto"
TITLE=""
SOURCE_THREAD_ID=""
WORK_ITEM_IDS=""
AS_ACTOR="${JOI_ACTOR:-actor_discovery}"
JQ_BIN="${JQ_BIN:-jq}"
if [[ -n "${A1_BIN:-}" ]]; then
    A1_BIN="$A1_BIN"
elif command -v a1 >/dev/null 2>&1; then
    A1_BIN="$(command -v a1)"
elif [[ -x /home/canfeng/canfeng-projects/a1/a1/a1 ]]; then
    A1_BIN="/home/canfeng/canfeng-projects/a1/a1/a1"
else
    A1_BIN="a1"
fi
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
                         [--bugfix-source auto|loop|direct]
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
        --bugfix-source) BUGFIX_SOURCE="$2"; shift 2;;
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
case "$BUGFIX_SOURCE" in
    auto|loop|direct) ;;
    *) echo "start-delivery: --bugfix-source must be auto, loop, or direct" >&2; exit 2;;
esac
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

strip_thread_prefixes() {
    local text="$1"
    printf '%s' "$text" | sed -E 's/^(\[(bugfixloop|bugfix):[^]]+\][[:space:]]*|\[delivery\][[:space:]]*)+//'
}

lock_key() {
    local text="$1"
    printf '%s' "$text" | sed -E 's/[^A-Za-z0-9._-]+/_/g; s/^_+//; s/_+$//' | cut -c1-160
}

thread_id_from_create() {
    "$JQ_BIN" -r '.thread.id // .thread_id // .id // empty'
}

event_id_from_send() {
    "$JQ_BIN" -r '.event.id // .id // empty'
}

source_thread_is_loop() {
    [[ -n "$SOURCE_THREAD_ID" ]] || return 1
    joi event list --in "$SOURCE_THREAD_ID" --limit 40 --json 2>/dev/null \
        | "$JQ_BIN" -e '
            (.events // .items // .)[]?
            | select((.actorId // .actor_id // "") == "svc_a1_bug_fix_loop"
                or ((.payload.text // "") | contains("bugfix-loop next"))
                or ((.payload.text // "") | contains("bugfix_loop_item")))
          ' >/dev/null 2>&1
}

if [[ -z "$TITLE" ]]; then
    goal_tmp=$(mktemp)
    artifact_body "$TASK_GOAL" >"$goal_tmp" || true
    TITLE=$("$JQ_BIN" -r '.title // .task.title // .summary // empty' "$goal_tmp" 2>/dev/null || true)
    rm -f "$goal_tmp"
fi
[[ -n "$TITLE" ]] || TITLE="delivery-task"
TITLE=$(safe_title "$TITLE")
TITLE=$(strip_thread_prefixes "$TITLE")
[[ -n "$TITLE" ]] || TITLE="delivery-task"

if [[ -n "$FEEDBACK_ID" ]]; then
    if [[ "$BUGFIX_SOURCE" == "auto" ]]; then
        if source_thread_is_loop; then
            BUGFIX_SOURCE="loop"
        else
            BUGFIX_SOURCE="direct"
        fi
    fi
    if [[ "$BUGFIX_SOURCE" == "loop" ]]; then
        THREAD_TITLE="[bugfixloop:${FEEDBACK_ID}] ${TITLE}"
    else
        THREAD_TITLE="[bugfix:${FEEDBACK_ID}] ${TITLE}"
    fi
else
    THREAD_TITLE="[delivery] ${TITLE}"
fi

LOCK_ROOT="${TMPDIR:-/tmp}/joi-start-delivery-locks"
mkdir -p "$LOCK_ROOT"
LOCK_DIR="${LOCK_ROOT}/$(lock_key "${CHANNEL_ID}_${THREAD_TITLE}").lock"
LOCK_ACQUIRED=0
for _ in $(seq 1 60); do
    if mkdir "$LOCK_DIR" 2>/dev/null; then
        LOCK_ACQUIRED=1
        break
    fi
    sleep 1
done
[[ "$LOCK_ACQUIRED" == "1" ]] || { echo "start-delivery: timed out waiting for lock: $THREAD_TITLE" >&2; exit 5; }
cleanup_lock() {
    rmdir "$LOCK_DIR" 2>/dev/null || true
}
trap cleanup_lock EXIT

thread_has_delivery_handoff() {
    local tid="$1"
    joi event list --in "$tid" --limit 20 --json 2>/dev/null \
        | "$JQ_BIN" -e '
            (.events // .items // .)[]?
            | select((.actorId // .actor_id // "") == "actor_delivery"
                or any((.relations // [])[]?; (.kind // "") == "hands_off_to"
                    and ((.target.id // .target.actorId // "") == "actor_delivery")))
          ' >/dev/null 2>&1
}

thread_has_delivery_activity() {
    local tid="$1"
    joi event list --in "$tid" --limit 20 --json 2>/dev/null \
        | "$JQ_BIN" -e '
            (.events // .items // .)[]?
            | select((.actorId // .actor_id // "") == "actor_delivery")
          ' >/dev/null 2>&1
}

pick_existing_thread() {
    local first="" tid="" activity_match="" handoff_match=""
    while IFS= read -r tid; do
        [[ -n "$tid" ]] || continue
        [[ -n "$first" ]] || first="$tid"
        if [[ -z "$activity_match" ]] && thread_has_delivery_activity "$tid"; then
            activity_match="$tid"
        fi
        if [[ -z "$handoff_match" ]] && thread_has_delivery_handoff "$tid"; then
            handoff_match="$tid"
        fi
    done
    printf '%s' "${activity_match:-${handoff_match:-$first}}"
}

existing_thread=""
if [[ -n "$FEEDBACK_ID" ]]; then
    if [[ "$BUGFIX_SOURCE" == "loop" ]]; then
        existing_prefix="[bugfixloop:${FEEDBACK_ID}]"
    else
        existing_prefix="[bugfix:${FEEDBACK_ID}]"
    fi
    existing_thread=$(joi thread list --channel "$CHANNEL_ID" --json 2>/dev/null \
        | "$JQ_BIN" -r --arg prefix "$existing_prefix" '
            (.threads // .items // .)[]?
            | (.title // .name // "") as $title
            | select($title | startswith($prefix))
            | (.id // .thread_id // .thread.id // empty)
        ' | pick_existing_thread)
else
    existing_thread=$(joi thread list --channel "$CHANNEL_ID" --json 2>/dev/null \
        | "$JQ_BIN" -r --arg title "$THREAD_TITLE" --arg legacy "$TITLE" '
            (.threads // .items // .)[]?
            | select(((.title // .name // "") == $title) or ((.title // .name // "") == $legacy))
            | (.id // .thread_id // .thread.id // empty)
        ' | pick_existing_thread)
fi

REUSED_THREAD=0
SKIP_HANDOFF=0
if [[ -n "$existing_thread" ]]; then
    DELIVERY_THREAD_ID="$existing_thread"
    REUSED_THREAD=1
    if thread_has_delivery_handoff "$DELIVERY_THREAD_ID"; then
        SKIP_HANDOFF=1
    fi
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
    pages="MISSING"
    if [[ -x "$A1_BIN" ]] || command -v "$A1_BIN" >/dev/null 2>&1; then
        pages=$("$A1_BIN" -f json kbase search "$repo 开发规范" --repo-ids 74121 --top 50 2>/dev/null \
            | "$JQ_BIN" -r --arg repo "$repo" --arg prefix "[$repo] " '
                [(.items // .data // .results // [])[]
                 | {id:(.page_id // .pageId // .pageID // .id // ""),
                    title:(.title // .name // .page_title // .pageTitle // .page_name // .pageName // "")}
                 | select(.id != "")
                 | select((.title == $repo) or (.title | startswith($prefix)))]
                | if length == 0 then empty
                  else map(.id + "(" + (.title | gsub("[\r\n]"; " ") | gsub("[()]"; " ")) + ")") | join(" ")
                  end
              ' 2>/dev/null || true)
        [[ -n "$pages" ]] || pages="MISSING"
    fi
    kbase_lines+="- ${repo}: ${pages}"$'\n'
done < <("$JQ_BIN" -r '.repos[]? | select((.mode // .role // "worktree") == "worktree") | .repo' "$manifest_norm")
[[ -n "$kbase_lines" ]] || kbase_lines="- MISSING: MISSING"$'\n'

msg=$(cat <<EOF
delivery 启动：feedback_id=${FEEDBACK_ID:-} work_item_ids=${WORK_ITEM_IDS:-${FEEDBACK_ID:-}} task-goal=${TASK_GOAL} DoD=${DOD} clone-manifest=${CLONE_MANIFEST}。
source_thread=${SOURCE_THREAD_ID:-}
workspace=~/joi-workspaces/thread/${DELIVERY_THREAD_ID}/repos/（已 provision，请 cd 进去干活；禁止动 shared/repos 与 channel-level workspace）
target-repos 开发规范（kbase 74121 page-id 列表）：
${kbase_lines}编码每个 repo 前先逐个执行 a1 kbase page view 74121 <page-id> 读取该 repo 的全部研发规范，并保存到 workspace 的 repo-specs/<group>__<repo>/；MISSING 的请回报 router 补。规范源头始终是 kbase，workspace 文件只是本次读取快照。
EOF
)
handoff_event_id=""
if [[ "$SKIP_HANDOFF" != "1" ]]; then
    handoff_out=$(joi handoff actor_delivery --in "$DELIVERY_THREAD_ID" --message "$msg" --json)
    handoff_event_id=$("$JQ_BIN" -r '.event.id // .id // empty' <<<"$handoff_out")
fi

rm -f "$manifest_tmp" "$manifest_norm"

"$JQ_BIN" -nc \
    --arg delivery_thread_id "$DELIVERY_THREAD_ID" \
    --arg handoff_event_id "$handoff_event_id" \
    --arg thread_title "$THREAD_TITLE" \
    --argjson reused "$REUSED_THREAD" \
    --argjson skipped_handoff "$SKIP_HANDOFF" \
    --argjson provision "$("$JQ_BIN" -s '.' <<<"$provision_out")" \
    '{ok:true, delivery_thread_id:$delivery_thread_id, handoff_event_id:$handoff_event_id, thread_title:$thread_title, reused_thread:$reused, skipped_handoff:$skipped_handoff, provision:$provision}'
