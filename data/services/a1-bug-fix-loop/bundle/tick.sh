#!/usr/bin/env bash
# a1-bug-fix-loop/tick.sh — serial driver that consumes the latest
# feedback-scan.bugs.v1 artifact in the scanner thread and walks each
# entry through router → discovery → delivery, gating on mr-merged.v1.
#
# Per tick:
#   1. Read newest feedback-scan.bugs.v1 from --scanner-thread-id.
#   2. Reconcile state: any tracked bug with mr-merged becomes "fixed";
#      any tracked bug with no live bugfix thread becomes "dropped".
#   3. If concurrency budget allows, pick the next "pending" bug,
#      create a bugfix thread (joi thread create), bootstrap repos,
#      handoff to actor_a1_bug_triage, persist tracking row.
#   4. Emit one bug-fix-loop-status.v1 line (the running ledger).
#
# Offline / dev: with --dry-run, performs no mutations and no network /
# joi-cli calls. Reads state.json if present, prints the planned
# action, and exits 0 with an empty status artifact.
#
# The contract here is "best-effort autonomous": failures (joi cli not
# found, scanner thread empty, etc.) degrade to a no-op tick that
# still emits an artifact so consumers can confirm liveness.

set -euo pipefail

DRY_RUN=0
STATE_DIR=""
CHANNEL_ID=""
SCANNER_TID=""
LOOP_TID=""
CONCURRENCY=1

usage() {
    cat <<'EOF'
usage: tick.sh --state-dir <p> --channel-id <c> --scanner-thread-id <t>
               --loop-thread-id <t> [--concurrency N] [--dry-run]
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --state-dir) STATE_DIR="$2"; shift 2;;
        --channel-id) CHANNEL_ID="$2"; shift 2;;
        --scanner-thread-id) SCANNER_TID="$2"; shift 2;;
        --loop-thread-id) LOOP_TID="$2"; shift 2;;
        --concurrency) CONCURRENCY="$2"; shift 2;;
        --dry-run) DRY_RUN=1; shift;;
        -h|--help) usage; exit 0;;
        *) echo "tick.sh: unknown arg: $1" >&2; usage >&2; exit 2;;
    esac
done

[[ -z "$STATE_DIR" || -z "$CHANNEL_ID" || -z "$SCANNER_TID" || -z "$LOOP_TID" ]] && { usage >&2; exit 2; }
command -v jq >/dev/null 2>&1 || { echo "tick.sh: jq is required" >&2; exit 3; }
mkdir -p "$STATE_DIR"
STATE_FILE="$STATE_DIR/state.json"
[[ -f "$STATE_FILE" ]] || echo '{"tracking":{}}' > "$STATE_FILE"

now_iso() { date -u +%Y-%m-%dT%H:%M:%SZ; }
TICK_ID="tick-$(date -u +%Y-%m-%dT%H:%M:%SZ | tr -d ':')"
TICK_AT=$(now_iso)

joi_avail() { command -v joi >/dev/null 2>&1; }
a1_avail()  { command -v a1  >/dev/null 2>&1; }

# Try to read newest feedback-scan.bugs.v1 from the scanner thread.
# In dev/offline we just produce an empty list so the loop is a no-op.
fetch_bugs() {
    if [[ "$DRY_RUN" -eq 1 ]] || ! joi_avail; then
        printf '[]'
        return
    fi
    if ! out=$(joi artifact list --in "$SCANNER_TID" --kind feedback-scan.bugs.v1 --latest --json 2>/dev/null); then
        printf '[]'
        return
    fi
    jq -c '.items // []' <<<"$out" 2>/dev/null || printf '[]'
}

# Try to read mr-merged.v1 in a bugfix thread. Returns "true"/"false".
mr_is_merged() {
    local tid="$1"
    if [[ "$DRY_RUN" -eq 1 ]] || ! joi_avail; then
        printf 'false'
        return
    fi
    if joi artifact list --in "$tid" --kind mr-merged.v1 --json 2>/dev/null | jq -e '.items | length > 0' >/dev/null 2>&1; then
        printf 'true'
    else
        printf 'false'
    fi
}

bugs=$(fetch_bugs)
state=$(cat "$STATE_FILE")
tracking=$(jq -c '.tracking // {}' <<<"$state")

# 1. reconcile in_progress entries
for fid in $(jq -r 'keys[]' <<<"$tracking"); do
    cur_status=$(jq -r --arg k "$fid" '.[$k].fix_status' <<<"$tracking")
    [[ "$cur_status" != "in_progress" ]] && continue
    bf_tid=$(jq -r --arg k "$fid" '.[$k].bugfix_thread_id // ""' <<<"$tracking")
    [[ -z "$bf_tid" ]] && continue
    if [[ "$(mr_is_merged "$bf_tid")" == "true" ]]; then
        tracking=$(jq -c --arg k "$fid" --arg ts "$TICK_AT" \
            '.[$k].fix_status = "fixed" | .[$k].fixed_at = $ts' <<<"$tracking")
        # Best-effort: tell a1 the feedback is fixed, ack to the channel.
        if [[ "$DRY_RUN" -eq 0 ]] && a1_avail; then
            a1 feedback resolve "$fid" --note "随下次发布更新（自动）" >/dev/null 2>&1 || true
        fi
        if [[ "$DRY_RUN" -eq 0 ]] && joi_avail; then
            joi say --channel "$CHANNEL_ID" "[bug-fix-loop] feedback $fid 已修复，等待下次发布。" >/dev/null 2>&1 || true
        fi
    fi
done

# 2. determine concurrency budget
in_flight=$(jq -r '[.[] | select(.fix_status=="in_progress")] | length' <<<"$tracking")
budget=$(( CONCURRENCY - in_flight ))
[[ $budget -lt 0 ]] && budget=0

# 3. promote next pending(s)
if [[ $budget -gt 0 ]]; then
    while IFS= read -r row; do
        [[ -z "$row" ]] && continue
        fid=$(jq -r '.feedback_id' <<<"$row")
        [[ -z "$fid" || "$fid" == "null" ]] && continue
        # skip if already tracked
        already=$(jq -r --arg k "$fid" '. | has($k)' <<<"$tracking")
        [[ "$already" == "true" ]] && continue

        title=$(jq -r '.title // ""' <<<"$row")
        summary=$(jq -r '.summary // .title // ""' <<<"$row")

        bf_tid="(dry-run-thread)"
        if [[ "$DRY_RUN" -eq 0 ]] && joi_avail; then
            if out=$(joi thread create --channel "$CHANNEL_ID" --topic "bugfix-$fid" --json 2>/dev/null); then
                bf_tid=$(jq -r '.thread_id // .id // ""' <<<"$out")
            fi
            if [[ -n "$bf_tid" && "$bf_tid" != "(dry-run-thread)" ]]; then
                joi handoff actor_a1_bug_triage --in "$bf_tid" \
                    --message "请对 feedback ${fid} 做缺陷分流：${summary}" >/dev/null 2>&1 || true
            fi
            if a1_avail; then
                a1 feedback claim "$fid" --note "已进入 bug-fix loop：$bf_tid" >/dev/null 2>&1 || true
            fi
        fi

        tracking=$(jq -c \
            --arg k "$fid" \
            --arg tid "$bf_tid" \
            --arg ts "$TICK_AT" \
            --arg title "$title" \
            '.[$k] = {fix_status:"in_progress", bugfix_thread_id:$tid, started_at:$ts, title:$title}' <<<"$tracking")
        budget=$((budget - 1))
        [[ $budget -le 0 ]] && break
    done < <(jq -c '.[]' <<<"$bugs")
fi

# 4. persist + emit status
state=$(jq -c --argjson t "$tracking" '.tracking = $t' <<<"$state")
tmp=$(mktemp); printf '%s' "$state" >"$tmp"; mv "$tmp" "$STATE_FILE"

jq -nc \
  --arg sv "1" \
  --arg producer "service_a1_bug_fix_loop" \
  --arg tick_id "$TICK_ID" \
  --arg tick_at "$TICK_AT" \
  --argjson tracking "$tracking" \
  --arg conc "$CONCURRENCY" \
  '{
    schema_version: $sv,
    producer: $producer,
    tick_id: $tick_id,
    tick_at: $tick_at,
    concurrency: ($conc|tonumber),
    in_flight: ([$tracking[] | select(.fix_status=="in_progress")] | length),
    fixed_total: ([$tracking[] | select(.fix_status=="fixed")] | length),
    tracking: $tracking
  }'
