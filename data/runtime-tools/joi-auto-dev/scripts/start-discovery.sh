#!/usr/bin/env bash
# start-discovery.sh — create an isolated discovery thread for one task.
#
# Each discovery task gets its own Joi thread to avoid long-lived
# discovery-desk context pollution. The thread workspace still receives a
# read-only reference entrance to the channel repo library:
#   ~/joi-workspaces/thread/<thread_id>/shared/repos
#     -> ~/.agentx/channels/<channel_id>/shared/repos

set -euo pipefail

CHANNEL_ID=""
TITLE=""
MESSAGE=""
AS_ACTOR="${JOI_ACTOR:-actor_router}"
TARGET_ACTOR="${TARGET_ACTOR:-actor_discovery}"
JQ_BIN="${JQ_BIN:-jq}"
if [[ -n "${JOI_BIN:-}" ]]; then
    JOI_BIN="$JOI_BIN"
elif command -v joi >/dev/null 2>&1; then
    JOI_BIN="$(command -v joi)"
elif [[ -x /home/canfeng/joi-apps/joi ]]; then
    JOI_BIN="/home/canfeng/joi-apps/joi"
else
    JOI_BIN="joi"
fi

usage() {
    cat <<'EOF'
usage: start-discovery.sh --channel-id <chan> --title <title> --message <message>
                          [--as <actor>] [--target-actor <actor>]

Creates a per-task discovery thread, links channel shared/repos into the
thread workspace, handoffs actor_discovery, and prints a JSON summary.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --channel-id) CHANNEL_ID="$2"; shift 2;;
        --title) TITLE="$2"; shift 2;;
        --message) MESSAGE="$2"; shift 2;;
        --as) AS_ACTOR="$2"; shift 2;;
        --target-actor) TARGET_ACTOR="$2"; shift 2;;
        -h|--help) usage; exit 0;;
        *) echo "start-discovery: unknown arg: $1" >&2; usage >&2; exit 2;;
    esac
done

[[ -n "$CHANNEL_ID" ]] || { echo "start-discovery: --channel-id required" >&2; exit 2; }
[[ -n "$MESSAGE" ]] || { echo "start-discovery: --message required" >&2; exit 2; }
command -v "$JQ_BIN" >/dev/null || { echo "start-discovery: jq required" >&2; exit 3; }
[[ -x "$JOI_BIN" ]] || { echo "start-discovery: joi binary not executable: $JOI_BIN" >&2; exit 3; }

joi() { "$JOI_BIN" --as "$AS_ACTOR" "$@"; }

safe_title() {
    local text="$1"
    text=$(printf '%s' "$text" | tr '\n\r\t' '   ' | sed -E 's/[[:space:]]+/ /g; s/^ //; s/ $//')
    printf '%s' "${text:0:90}"
}

lock_key() {
    local text="$1"
    printf '%s' "$text" | sed -E 's/[^A-Za-z0-9._-]+/_/g; s/^_+//; s/_+$//' | cut -c1-160
}

event_id_from_send() {
    "$JQ_BIN" -r '.event.id // .id // empty'
}

thread_id_from_create() {
    "$JQ_BIN" -r '.thread.id // .thread_id // .id // empty'
}

TITLE=$(safe_title "${TITLE:-discovery-task}")
[[ -n "$TITLE" ]] || TITLE="discovery-task"
THREAD_TITLE="[discovery] ${TITLE}"

LOCK_ROOT="${TMPDIR:-/tmp}/joi-start-discovery-locks"
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
[[ "$LOCK_ACQUIRED" == "1" ]] || { echo "start-discovery: timed out waiting for lock: $THREAD_TITLE" >&2; exit 5; }
cleanup_lock() {
    rmdir "$LOCK_DIR" 2>/dev/null || true
}
trap cleanup_lock EXIT

thread_has_target_handoff() {
    local tid="$1"
    joi event list --in "$tid" --limit 20 --json 2>/dev/null \
        | "$JQ_BIN" -e --arg target "$TARGET_ACTOR" '
            (.events // .items // .)[]?
            | select((.actorId // .actor_id // "") == $target
                or any((.relations // [])[]?; (.kind // "") == "hands_off_to"
                    and ((.target.id // .target.actorId // "") == $target)))
          ' >/dev/null 2>&1
}

pick_existing_thread() {
    local first="" tid="" handoff_match=""
    while IFS= read -r tid; do
        [[ -n "$tid" ]] || continue
        [[ -n "$first" ]] || first="$tid"
        if [[ -z "$handoff_match" ]] && thread_has_target_handoff "$tid"; then
            handoff_match="$tid"
        fi
    done
    printf '%s' "${handoff_match:-$first}"
}

existing_thread=$(joi thread list --channel "$CHANNEL_ID" --json 2>/dev/null \
    | "$JQ_BIN" -r --arg title "$THREAD_TITLE" --arg legacy "$TITLE" '
        (.threads // .items // .)[]?
        | select(((.title // .name // "") == $title) or ((.title // .name // "") == $legacy))
        | (.id // .thread_id // .thread.id // empty)
    ' | pick_existing_thread)

root_event_id=""
handoff_event_id=""
REUSED_THREAD=0
SKIP_HANDOFF=0
if [[ -n "$existing_thread" ]]; then
    DISCOVERY_THREAD_ID="$existing_thread"
    REUSED_THREAD=1
    if thread_has_target_handoff "$DISCOVERY_THREAD_ID"; then
        SKIP_HANDOFF=1
    fi
else
    root_out=$(joi event append --channel --in "$CHANNEL_ID" --type thread.opened --text "discovery-start: ${TITLE}" --json)
    root_event_id=$(event_id_from_send <<<"$root_out")
    [[ -n "$root_event_id" ]] || { echo "start-discovery: failed to create root channel event" >&2; exit 4; }

    thread_out=$(joi thread create --channel "$CHANNEL_ID" --root-event "$root_event_id" --title "$THREAD_TITLE" --json)
    DISCOVERY_THREAD_ID=$(thread_id_from_create <<<"$thread_out")
    [[ -n "$DISCOVERY_THREAD_ID" ]] || { echo "start-discovery: failed to create discovery thread" >&2; exit 4; }
fi

thread_ws="${HOME}/joi-workspaces/thread/${DISCOVERY_THREAD_ID}"
shared_root="${HOME}/.agentx/channels/${CHANNEL_ID}/shared/repos"
mkdir -p "${thread_ws}/shared" "${thread_ws}/.joi"
ln -sfn "$shared_root" "${thread_ws}/shared/repos"

cat >"${thread_ws}/.joi/discovery-scope.json" <<EOF
{
  "schema_version": "1",
  "channel_id": "${CHANNEL_ID}",
  "thread_id": "${DISCOVERY_THREAD_ID}",
  "thread_title": "${THREAD_TITLE}",
  "shared_repos": "${thread_ws}/shared/repos",
  "shared_repos_source": "${shared_root}",
  "shared_repos_mode": "read_only_reference"
}
EOF

if [[ "$SKIP_HANDOFF" != "1" ]]; then
    handoff_out=$(joi handoff "$TARGET_ACTOR" --in "$DISCOVERY_THREAD_ID" --message "$MESSAGE" --json)
    handoff_event_id=$(event_id_from_send <<<"$handoff_out")
    [[ -n "$handoff_event_id" ]] || { echo "start-discovery: failed to handoff ${TARGET_ACTOR}" >&2; exit 4; }
fi

"$JQ_BIN" -nc \
    --arg thread_id "$DISCOVERY_THREAD_ID" \
    --arg root_event_id "$root_event_id" \
    --arg handoff_event_id "$handoff_event_id" \
    --arg title "$THREAD_TITLE" \
    --arg shared_repos "${thread_ws}/shared/repos" \
    --argjson reused "$REUSED_THREAD" \
    --argjson skipped_handoff "$SKIP_HANDOFF" \
    '{
      ok: true,
      discovery_thread_id: $thread_id,
      root_event_id: $root_event_id,
      handoff_event_id: $handoff_event_id,
      title: $title,
      shared_repos: $shared_repos,
      reused_thread: $reused,
      skipped_handoff: $skipped_handoff
    }'
