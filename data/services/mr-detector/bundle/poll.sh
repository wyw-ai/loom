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

emit_status_diff() {
    # $1 = current_state JSON, $2 = previous_state JSON or "null", $3 = delta JSON, $4 = actionable_summary
    local cur="$1" prev="$2" delta="$3" summary="$4"
    local fp
    fp=$(printf '%s' "$cur" | shasum -a 256 | awk '{print $1}')
    jq -nc \
        --arg sv "1" \
        --arg producer "svc_mr_detector" \
        --arg mr_id "$MR_URL" \
        --arg captured_at "$(now_iso)" \
        --arg fp "sha256:$fp" \
        --arg summary "$summary" \
        --arg ak "mr-status-diff.v1" \
        --argjson cur "$cur" \
        --argjson prev "$prev" \
        --argjson delta "$delta" \
        '{
          artifact_kind: $ak,
          schema_version: $sv,
          producer: $producer,
          mr_id: $mr_id,
          captured_at: $captured_at,
          current_state: $cur,
          delta: $delta,
          actionable_summary: $summary,
          raw_event_fingerprint: $fp
        } + (if $prev == null then {} else {previous_state: $prev} end)'
}

emit_merged() {
    # $1 = payload JSON
    local payload="$1"
    local fp
    fp=$(printf '%s' "$payload" | shasum -a 256 | awk '{print $1}')
    jq -nc \
        --arg sv "1" \
        --arg producer "svc_mr_detector" \
        --arg mr_id "$MR_URL" \
        --arg captured_at "$(now_iso)" \
        --arg fp "sha256:$fp" \
        --arg ak "mr-merged.v1" \
        --argjson body "$payload" \
        '{
          artifact_kind: $ak,
          schema_version: $sv,
          producer: $producer,
          mr_id: $mr_id,
          captured_at: $captured_at,
          raw_event_fingerprint: $fp
        } + ($body | {title, head_sha, base_sha, author, labels, merged_at})'
}

# Legacy emit retained for any consumer that still subscribes to
# mr-event-<kind>; preferred path is mr-status-diff.v1 / mr-merged.v1.
emit_event() {
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
        --arg ak "mr-event" \
        --argjson body "$payload" \
        '{artifact_kind:$ak, schema_version:$sv, producer:$producer, event_kind:$kind, mr_id:$mr_id, captured_at:$captured_at, raw_event_fingerprint:$fp} + $body'
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

# Snapshot current vs previous to compute the diff artifact.
prev_seen=$(jq -c '.seen // {}' <<<"$state")
prev_labels=$(jq -c '.labels // []' <<<"$prev_seen")
new_labels=$(jq -c '.labels // []' <<<"$payload")
prev_ci=$(jq -r '.ci_status // ""' <<<"$prev_seen")
new_ci=$(jq -r '.ci_status // ""' <<<"$payload")
prev_comments=$(jq -r '.open_comments // 0' <<<"$prev_seen")
new_comments=$(jq -r '.open_comments // 0' <<<"$payload")
prev_behind=$(jq -r '.behind_target // false' <<<"$prev_seen")
new_behind=$(jq -r '.behind_target // false' <<<"$payload")

# labels added / removed
labels_added=$(jq -nc --argjson p "$prev_labels" --argjson c "$new_labels" '[$c[] | select( . as $x | $p | index($x) | not )]')
labels_removed=$(jq -nc --argjson p "$prev_labels" --argjson c "$new_labels" '[$p[] | select( . as $x | $c | index($x) | not )]')

ci_changed=false; ci_failed=false
[[ -n "$new_ci" && "$prev_ci" != "$new_ci" ]] && ci_changed=true
[[ "$new_ci" == "failed" || "$new_ci" == "failure" || "$new_ci" == "error" ]] && ci_failed=true
new_comments_count=$(( new_comments - prev_comments ))
[[ $new_comments_count -lt 0 ]] && new_comments_count=0
behind_changed=false
[[ "$prev_behind" != "$new_behind" ]] && behind_changed=true

cur_state_json=$(jq -nc \
    --arg ci "$new_ci" \
    --argjson labels "$new_labels" \
    --arg head "$(jq -r '.head_sha // ""' <<<"$payload")" \
    --argjson oc "$new_comments" \
    --argjson bt "$new_behind" \
    '{ci_status:$ci, labels:$labels, head_sha:$head, open_comments:$oc, behind_target:$bt}')

prev_state_json="null"
if [[ "$prev_seen" != "{}" ]]; then
    prev_state_json=$(jq -nc \
        --arg ci "$prev_ci" \
        --argjson labels "$prev_labels" \
        --arg head "$(jq -r '.head_sha // ""' <<<"$prev_seen")" \
        --argjson oc "$prev_comments" \
        --argjson bt "$prev_behind" \
        '{ci_status:$ci, labels:$labels, head_sha:$head, open_comments:$oc, behind_target:$bt}')
fi

delta_json=$(jq -nc \
    --argjson ci_c "$ci_changed" \
    --argjson ci_f "$ci_failed" \
    --argjson nc "$new_comments_count" \
    --argjson bc "$behind_changed" \
    --argjson la "$labels_added" \
    --argjson lr "$labels_removed" \
    '{ci_status_changed:$ci_c, ci_failed:$ci_f, new_comments:$nc, behind_target_changed:$bc, labels_added:$la, labels_removed:$lr}')

# Build a Chinese actionable summary from the delta
summary_parts=()
[[ "$ci_failed" == "true" ]] && summary_parts+=("CI 红了")
[[ "$behind_changed" == "true" && "$new_behind" == "true" ]] && summary_parts+=("落后 origin/master，需要 rebase")
[[ $new_comments_count -gt 0 ]] && summary_parts+=("新增 ${new_comments_count} 条评论")
summary=""
if [[ ${#summary_parts[@]} -gt 0 ]]; then
    summary=$(IFS='；'; echo "${summary_parts[*]}")
fi

# Emit a status-diff artifact only when something material changed
# (avoid republishing identical state every tick).
material_change="false"
if [[ "$ci_changed" == "true" || "$behind_changed" == "true" || $new_comments_count -gt 0 || "$labels_added" != "[]" || "$labels_removed" != "[]" ]]; then
    material_change="true"
fi

mr_state=$(jq -r '.state // "open"' <<<"$payload")
merged_emitted=$(jq -r '.merged_emitted' <<<"$state")
closed_emitted=$(jq -r '.closed_emitted' <<<"$state")

self_complete=0
if [[ "$mr_state" == "merged" && "$merged_emitted" == "false" ]]; then
    emit_merged "$payload"
    state=$(jq -c '.merged_emitted = true' <<<"$state")
    self_complete=1
elif [[ "$mr_state" == "closed" && "$closed_emitted" == "false" ]]; then
    emit_event "closed" "$(jq -c '{title, head_sha, base_sha, author, labels, ci_status}' <<<"$payload")"
    state=$(jq -c '.closed_emitted = true' <<<"$state")
    self_complete=1
elif [[ "$material_change" == "true" ]]; then
    emit_status_diff "$cur_state_json" "$prev_state_json" "$delta_json" "$summary"
fi

# Persist the new "seen" snapshot regardless of which events fired.
state=$(jq -c --argjson p "$payload" \
    '.seen = {labels: ($p.labels // []), ci_status: ($p.ci_status // ""), head_sha: ($p.head_sha // ""), open_comments: ($p.open_comments // 0), behind_target: ($p.behind_target // false), state: ($p.state // "open")}' <<<"$state")
printf '%s' "$state" > "$STATE_FILE"

if [[ "$self_complete" -eq 1 ]]; then
    printf '{"service.self_complete":true,"reason":"%s"}\n' "$mr_state"
fi
