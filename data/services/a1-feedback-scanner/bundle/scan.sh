#!/usr/bin/env bash
# a1-feedback-scanner/scan.sh — pull recent a1 feedback, run them
# through a1-bug-triage classification (best-effort, via the a1 CLI),
# and emit two artifacts per tick:
#   feedback-scan.bugs.v1    -> existing-bug bucket (input to bug-fix loop)
#   feedback-scan.others.v1  -> everything else + diff vs. previous scan
#
# Output: two JSON lines on stdout (one per artifact), wrapped in the
# top-level shape so the scheduler's `artifact_per_json_line` mode
# publishes each as its own artifact. State (previous scan id +
# previous others bucket fingerprints) lives under <state-dir>.
#
# Offline / dry-run: when --dry-run is set OR the `a1` binary is not on
# PATH, emits a structurally-valid stub artifact pair with zero items.
# This keeps the scheduler happy in dev environments and CI.

set -euo pipefail

DRY_RUN=0
STATE_DIR=""
SINCE_HOURS=24
A1_FILTER="${A1_FILTER:-}"

usage() {
    cat <<'EOF'
usage: scan.sh --state-dir <path> [--since-hours N] [--dry-run]

Scan recent a1 feedback and emit feedback-scan.bugs.v1 +
feedback-scan.others.v1 artifacts on stdout.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --state-dir) STATE_DIR="$2"; shift 2;;
        --since-hours) SINCE_HOURS="$2"; shift 2;;
        --a1-filter) A1_FILTER="$2"; shift 2;;
        --dry-run) DRY_RUN=1; shift;;
        -h|--help) usage; exit 0;;
        *) echo "scan.sh: unknown arg: $1" >&2; usage >&2; exit 2;;
    esac
done

[[ -z "$STATE_DIR" ]] && { usage >&2; exit 2; }
command -v jq >/dev/null 2>&1 || { echo "scan.sh: jq is required" >&2; exit 3; }

mkdir -p "$STATE_DIR"
STATE_FILE="$STATE_DIR/state.json"
[[ -f "$STATE_FILE" ]] || echo '{"last_scan_id":null,"last_others_fp":{}}' > "$STATE_FILE"

now_iso() { date -u +%Y-%m-%dT%H:%M:%SZ; }
since_iso() {
    if date -u -v-"${SINCE_HOURS}"H +%Y-%m-%dT%H:%M:%SZ 2>/dev/null; then return; fi
    date -u -d "${SINCE_HOURS} hours ago" +%Y-%m-%dT%H:%M:%SZ
}

SCAN_ID="scan-$(date -u +%Y-%m-%dT%H:%M:%SZ | tr -d ':')"
SCANNED_AT=$(now_iso)
WINDOW_SINCE=$(since_iso)
WINDOW_UNTIL="$SCANNED_AT"

# Best-effort fetch via the a1 CLI. Each implementation MUST emit an
# array of {feedback_id,title,summary,url,category,severity} objects
# on stdout. When unavailable we degrade to an empty array.
fetch_feedbacks() {
    if [[ "$DRY_RUN" -eq 1 ]] || ! command -v a1 >/dev/null 2>&1; then
        printf '[]'
        return
    fi
    # The a1 binary is expected to provide:
    #   a1 feedback list --since <iso> [--filter <expr>] --json
    # If the schema differs in production, adapt the jq pipeline below.
    if ! a1 feedback list --since "$WINDOW_SINCE" ${A1_FILTER:+--filter "$A1_FILTER"} --json 2>/dev/null; then
        printf '[]'
    fi
}

raw=$(fetch_feedbacks)
[[ -z "$raw" ]] && raw='[]'

# Partition into existing-bug / others by `category` field. When the
# upstream item carries no category, we tag it `unclear` and let the
# triage agent re-classify downstream.
bugs_items=$(jq -c '
  [ .[]
    | select((.category // "unclear") == "existing_bug")
    | {
        feedback_id: (.feedback_id // .id // ""),
        feedback_url: (.url // ""),
        title: (.title // ""),
        triage_artifact_uri: null,
        severity: (.severity // "minor"),
        summary: (.summary // .title // ""),
        fix_status: "pending"
      }
  ]
' <<<"$raw")

others_items=$(jq -c '
  reduce .[] as $f (
    {"new_request": [], "unclear": [], "duplicate": [], "not_actionable": []};
    . as $acc
    | ($f.category // "unclear") as $cat
    | if $cat == "existing_bug" then $acc
      elif ($acc | has($cat)) then ($acc | .[$cat] += [{feedback_id: ($f.feedback_id // $f.id // ""), title: ($f.title // ""), summary: ($f.summary // $f.title // "")}])
      else ($acc | .unclear += [{feedback_id: ($f.feedback_id // $f.id // ""), title: ($f.title // ""), summary: ($f.summary // $f.title // "")}])
      end
  )
' <<<"$raw")

prev_scan_id=$(jq -r '.last_scan_id // ""' "$STATE_FILE")
prev_fp=$(jq -c '.last_others_fp // {}' "$STATE_FILE")

# Compute diff_vs_previous_scan based on feedback_id presence per bucket.
new_fp=$(jq -c '
  reduce (to_entries[]) as $kv ({};
    .[$kv.key] = ($kv.value | map(.feedback_id))
  )
' <<<"$others_items")

diff_block="null"
if [[ "$prev_scan_id" != "" ]]; then
    diff_block=$(jq -nc \
        --argjson prev "$prev_fp" \
        --argjson cur "$new_fp" \
        --arg prev_id "$prev_scan_id" \
        '
        def flatten_ids(o): [o | to_entries[] | .key as $b | .value[] | {feedback_id: ., bucket: $b}];
        ($prev | flatten_ids(.)) as $p
        | ($cur  | flatten_ids(.)) as $c
        | {
            previous_scan_id: $prev_id,
            added:   [ $c[] | select( . as $x | ($p | map(.feedback_id) | index($x.feedback_id)) | not ) ],
            removed: [ $p[] | select( . as $x | ($c | map(.feedback_id) | index($x.feedback_id)) | not ) ]
          }
        ')
fi

# Emit bugs artifact (always)
jq -nc \
  --arg sv "1" \
  --arg producer "service_a1_feedback_scanner" \
  --arg scan_id "$SCAN_ID" \
  --arg scanned_at "$SCANNED_AT" \
  --arg since "$WINDOW_SINCE" \
  --arg until "$WINDOW_UNTIL" \
  --argjson items "$bugs_items" \
  '{
    schema_version: $sv,
    producer: $producer,
    bucket: "bugs",
    scan_id: $scan_id,
    scanned_at: $scanned_at,
    window: { since: $since, until: $until },
    items: $items
  }'

# Emit others artifact (always)
jq -nc \
  --arg sv "1" \
  --arg producer "service_a1_feedback_scanner" \
  --arg scan_id "$SCAN_ID" \
  --arg scanned_at "$SCANNED_AT" \
  --arg since "$WINDOW_SINCE" \
  --arg until "$WINDOW_UNTIL" \
  --argjson buckets "$others_items" \
  --argjson diff "$diff_block" \
  '{
    schema_version: $sv,
    producer: $producer,
    bucket: "others",
    scan_id: $scan_id,
    scanned_at: $scanned_at,
    window: { since: $since, until: $until },
    buckets: $buckets
  } + (if $diff == null then {} else { diff_vs_previous_scan: $diff } end)'

# Persist scan state for diffing on the next tick.
tmp=$(mktemp)
jq -nc --arg id "$SCAN_ID" --argjson fp "$new_fp" '{last_scan_id:$id,last_others_fp:$fp}' >"$tmp"
mv "$tmp" "$STATE_FILE"
