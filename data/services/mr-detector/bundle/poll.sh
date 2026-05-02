#!/usr/bin/env bash
# mr-detector/poll.sh — poll a single MR URL and emit one
# mr-event-<event_kind>-<mr_id>.json line per *new* transition observed
# since the last tick. State (last seen labels, ci_status, merged/closed
# flags) is persisted under <state_dir>/state.json so restarts dedupe
# correctly.
#
# Output lines on stdout follow docs/artifact-contracts.md §6 exactly.
# When a "merged" or "closed" event fires, the script additionally writes
# {"service.self_complete":true,...} as the *last* line so the
# thread-bound service host can stop the instance.
#
# Offline contract: with --dry-run, performs no network I/O, reads the
# state file if it exists, prints what *would* be polled and exits 0.

set -euo pipefail

DRY_RUN=0
MR_URL=""
STATE_DIR=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --mr-url) MR_URL="$2"; shift 2;;
        --state-dir) STATE_DIR="$2"; shift 2;;
        --dry-run) DRY_RUN=1; shift;;
        -h|--help) echo "usage: poll.sh --mr-url <url> --state-dir <path> [--dry-run]"; exit 0;;
        *) echo "poll.sh: unknown arg: $1" >&2; exit 2;;
    esac
done

[[ -z "$MR_URL" || -z "$STATE_DIR" ]] && { echo "poll.sh: --mr-url and --state-dir are required" >&2; exit 2; }
command -v jq >/dev/null 2>&1 || { echo "poll.sh: jq is required" >&2; exit 3; }

STATE_FILE="$STATE_DIR/state.json"
mkdir -p "$STATE_DIR"
[[ -f "$STATE_FILE" ]] || echo '{"seen":{},"merged_emitted":false,"closed_emitted":false}' > "$STATE_FILE"

now_iso() { date -u +%Y-%m-%dT%H:%M:%SZ; }

emit_event() {
    # $1=event_kind  $2=payload_json
    local kind="$1" payload="$2"
    local fp
    fp=$(printf '%s' "$payload" | shasum -a 256 | awk '{print $1}')
    jq -nc \
        --arg sv "1" \
        --arg producer "mr-detector" \
        --arg kind "$kind" \
        --arg mr_id "$MR_URL" \
        --arg captured_at "$(now_iso)" \
        --arg fp "sha256:$fp" \
        --argjson body "$payload" \
        '{schema_version:$sv, producer:$producer, event_kind:$kind, mr_id:$mr_id, captured_at:$captured_at, raw_event_fingerprint:$fp} + $body'
}

if [[ "$DRY_RUN" -eq 1 ]]; then
    printf '{"schema_version":"1","producer":"mr-detector","op":"poll","mr_id":"%s","status":"planned","state_file":"%s"}\n' "$MR_URL" "$STATE_FILE"
    exit 0
fi

# Real polling needs an MR backend (gh / glab / a1). We keep the network
# layer deliberately swappable: callers may set MR_DETECTOR_FETCH_CMD to
# a command that prints the canonicalised payload JSON to stdout. When
# unset, we fall back to a stub payload so unit tests can exercise the
# emit path without a remote.
fetch_payload() {
    if [[ -n "${MR_DETECTOR_FETCH_CMD:-}" ]]; then
        eval "$MR_DETECTOR_FETCH_CMD" "$MR_URL"
    else
        printf '{"title":"(stub)","head_sha":"","base_sha":"","author":"","labels":[],"ci_status":"unknown","state":"open"}'
    fi
}

payload=$(fetch_payload)
state=$(cat "$STATE_FILE")

# diff labels & ci_status against last seen → emit "labeled" / status.update
prev_labels=$(jq -c '.seen.labels // []' <<<"$state")
new_labels=$(jq -c '.labels // []' <<<"$payload")
if [[ "$prev_labels" != "$new_labels" ]]; then
    emit_event "labeled" "$(jq -nc --argjson l "$new_labels" '{labels:$l}')"
fi

prev_ci=$(jq -r '.seen.ci_status // ""' <<<"$state")
new_ci=$(jq -r '.ci_status // ""' <<<"$payload")
if [[ -n "$new_ci" && "$prev_ci" != "$new_ci" ]]; then
    : # ci_status is captured implicitly inside the merged/closed payload below
fi

mr_state=$(jq -r '.state // "open"' <<<"$payload")
merged_emitted=$(jq -r '.merged_emitted' <<<"$state")
closed_emitted=$(jq -r '.closed_emitted' <<<"$state")

self_complete=0
if [[ "$mr_state" == "merged" && "$merged_emitted" == "false" ]]; then
    emit_event "merged" "$(jq -c '{title, head_sha, base_sha, author, labels, ci_status, merged_at:(.merged_at // empty)}' <<<"$payload")"
    state=$(jq -c '.merged_emitted = true' <<<"$state")
    self_complete=1
elif [[ "$mr_state" == "closed" && "$closed_emitted" == "false" ]]; then
    emit_event "closed" "$(jq -c '{title, head_sha, base_sha, author, labels, ci_status}' <<<"$payload")"
    state=$(jq -c '.closed_emitted = true' <<<"$state")
    self_complete=1
fi

# Persist the new "seen" snapshot regardless of which events fired.
state=$(jq -c --argjson p "$payload" '.seen = ($p | {labels, ci_status, head_sha, state})' <<<"$state")
printf '%s' "$state" > "$STATE_FILE"

if [[ "$self_complete" -eq 1 ]]; then
    printf '{"service.self_complete":true,"reason":"%s"}\n' "$mr_state"
fi
