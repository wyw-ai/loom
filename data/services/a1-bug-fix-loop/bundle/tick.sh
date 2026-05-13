#!/usr/bin/env bash
# a1-bug-fix-loop/tick.sh — serial driver that consumes the persisted
# feedback queue written by feedback-scanner and walks each stored bug
# through router → discovery → delivery, gating on MR terminal state.
#
# Per tick:
#   1. Check whether the scanner queue is stale; if last scan is >=24h old,
#      trigger feedback-scanner in the resident scanner thread.
#   2. Read `<feedback-triage-state>/feedback-queues/thread-*/bugs.json`.
#   2. Reconcile state: tracked bugs with mr-merged / mr.final(merged) become
#      "fixed"; tracked bugs whose MR / feedback reached a non-fixed terminal
#      state (closed, withdrawn, not-a-bug, already-covered) become "closed".
#   3. If concurrency budget allows, pick the next "pending" bug,
#      create a bugfix thread (joi thread create), bootstrap repos,
#      handoff to actor_a1_bug_triage, persist tracking row.
#   4. If no pending bug exists and the scanner thread is stale (>=24h),
#      handoff feedback-scanner inside the resident scanner thread.
#   5. Emit one bug-fix-loop-status.v1 line (the running ledger).
#
# Offline / dev: with --dry-run, performs no mutations and no network /
# joi-cli calls. Reads state.json if present, prints the planned
# action, and exits 0 with an empty status artifact.
#
# The contract here is "best-effort autonomous": failures (joi cli not
# found, scanner thread empty, etc.) degrade to a no-op tick that
# still emits an artifact so consumers can confirm liveness.

set -euo pipefail

export PATH="$HOME/.cargo/bin:$HOME/.local/bin:$HOME/bin:$HOME/.nvm/versions/node/v24.14.1/bin:$HOME/joi-apps:$HOME/canfeng-projects/a1/a1:$PATH"

DRY_RUN=0
STATE_DIR=""
CHANNEL_ID=""
SCANNER_TID=""
LOOP_TID=""
CONCURRENCY=1
SCAN_STALE_HOURS=24
FEEDBACK_TRIAGE_STATE_DIR="${FEEDBACK_TRIAGE_STATE_DIR:-$HOME/.local/state/joi-agent/services/feedback-triage}"

usage() {
    cat <<'EOF'
usage: tick.sh --state-dir <p> --channel-id <c> --scanner-thread-id <t>
               --loop-thread-id <t> [--concurrency N] [--scan-stale-hours N] [--dry-run]
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --state-dir) STATE_DIR="$2"; shift 2;;
        --channel-id) CHANNEL_ID="$2"; shift 2;;
        --scanner-thread-id) SCANNER_TID="$2"; shift 2;;
        --loop-thread-id) LOOP_TID="$2"; shift 2;;
        --concurrency) CONCURRENCY="$2"; shift 2;;
        --scan-stale-hours) SCAN_STALE_HOURS="$2"; shift 2;;
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

LOCK_FILE="$STATE_DIR/loop.lock"
exec 9>"$LOCK_FILE"
flock 9

QUEUE_DIR="${FEEDBACK_QUEUE_DIR:-$FEEDBACK_TRIAGE_STATE_DIR/feedback-queues/thread-$SCANNER_TID}"
BUGS_FILE="$QUEUE_DIR/bugs.json"
QUEUE_META_FILE="$QUEUE_DIR/scan-meta.json"

now_iso() { date -u +%Y-%m-%dT%H:%M:%SZ; }
TICK_ID="tick-$(date -u +%Y-%m-%dT%H:%M:%SZ | tr -d ':')"
TICK_AT=$(now_iso)

joi_avail() { command -v joi >/dev/null 2>&1; }
a1_avail()  { command -v a1  >/dev/null 2>&1; }

# Helper: scan recent events in a scope, surface the most recent artifact
# whose body has the requested $kind (schema_version/producer/etc.). Echoes
# the artifact body JSON or empty string.
fetch_latest_artifact_body() {
    local scope="$1" kind="$2" channel_flag="${3:-}"
    if [[ "$DRY_RUN" -eq 1 ]] || ! joi_avail; then
        return 0
    fi
    local list
    if [[ "$channel_flag" == "--channel" ]]; then
        list=$(joi event list --in "$scope" --channel --limit 100 --json 2>/dev/null) || return 0
    else
        list=$(joi event list --in "$scope" --limit 100 --json 2>/dev/null) || return 0
    fi
    # Pull artifact ids from any event relations of kind attaches_artifact;
    # newest-first iteration since the list is reverse-chronological.
    local ids
    ids=$(jq -r '
        (.events // .items // .) | reverse |
        .[] | (.relations // []) | .[] |
        select((.kind // .type) == "attaches_artifact") |
        (.target_id // .target // .id // empty)
    ' <<<"$list" 2>/dev/null) || return 0
    while IFS= read -r aid; do
        [[ -z "$aid" ]] && continue
        meta=$(joi artifact get "$aid" --json 2>/dev/null) || continue
        akind=$(jq -r '.kind // .schema // ""' <<<"$meta")
        if [[ "$akind" == "$kind" ]]; then
            joi artifact read "$aid" 2>/dev/null
            return 0
        fi
        body=$(joi artifact read "$aid" 2>/dev/null) || continue
        bkind=$(jq -r '.kind // .schema // ""' <<<"$body" 2>/dev/null || true)
        if [[ "$bkind" == "$kind" ]]; then
            printf '%s' "$body"
            return 0
        fi
    done <<<"$ids"
}

fetch_latest_feedback_scan_text() {
    if [[ "$DRY_RUN" -eq 1 ]] || ! joi_avail; then
        return 0
    fi
    local list
    list=$(joi event list --in "$SCANNER_TID" --limit 100 --json 2>/dev/null) || return 0
    jq -r '
      [
          (.events // .items // .)[]
          | select((.type // "") == "content.add")
          | select((.actorId // .actor_id // "") == "feedback-scanner")
          | select((.payload.text // "") | contains("[feedback-scan v1]"))
        ]
      | sort_by(.occurredAt // .createdAt // "")
      | reverse
      | .[0].payload.text // ""
    ' <<<"$list" 2>/dev/null || true
}

fetch_bugs_from_feedback_scan_event() {
    local text
    text=$(fetch_latest_feedback_scan_text) || true
    if [[ -z "$text" ]]; then
        printf '[]'
        return
    fi
    printf '%s\n' "$text" \
      | perl -ne 'if (/^- `([^`]+)` \[([^\]]*)\] (.*)$/) { print "$1\t$2\t$3\n" }' \
      | jq -R -s -c '
          split("\n")
          | map(select(length > 0) | split("\t"))
          | map({
              feedback_id: .[0],
              status: .[1],
              title: .[2],
              summary: .[2],
              fix_status: "pending"
            })
        ' 2>/dev/null || printf '[]'
}

fetch_bugs() {
    local classification_status
    classification_status=$(jq -r '.classification_status // ""' "$QUEUE_META_FILE" 2>/dev/null || true)
    if [[ "$classification_status" != "done" ]]; then
        printf '[]'
        return
    fi
    if [[ -f "$BUGS_FILE" ]]; then
        jq -c '[.items[]? | select((.fix_status // "pending") == "pending")]' "$BUGS_FILE" 2>/dev/null || printf '[]'
        return
    fi
    printf '[]'
}

latest_scan_epoch() {
    local body scanned_at text
    if [[ -f "$QUEUE_META_FILE" ]]; then
        scanned_at=$(jq -r '.scanned_at // ""' "$QUEUE_META_FILE" 2>/dev/null || true)
    fi
    if [[ -z "$scanned_at" || "$scanned_at" == "null" ]]; then
        text=$(fetch_latest_feedback_scan_text) || true
        scanned_at=$(grep -E '^scanned_at:' <<<"${text:-}" | head -n 1 | sed 's/^scanned_at:[[:space:]]*//' || true)
    fi
    [[ -z "$scanned_at" || "$scanned_at" == "null" ]] && return 1
    if date -u -j -f "%Y-%m-%dT%H:%M:%SZ" "$scanned_at" +%s 2>/dev/null; then return 0; fi
    date -u -d "$scanned_at" +%s 2>/dev/null
}

maybe_trigger_scan_refresh() {
    local last_epoch now_epoch age_hours
    [[ "$DRY_RUN" -eq 1 ]] && return 0
    joi_avail || return 0

    now_epoch=$(date -u +%s)
    if last_epoch=$(latest_scan_epoch); then
        age_hours=$(( (now_epoch - last_epoch) / 3600 ))
        [[ "$age_hours" -lt "$SCAN_STALE_HOURS" ]] && return 0
    else
        age_hours="$SCAN_STALE_HOURS"
    fi

    local last_trigger
    last_trigger=$(jq -r '.last_scan_refresh_triggered_at // ""' <<<"$state")
    if [[ -n "$last_trigger" && "$last_trigger" != "null" ]]; then
        local last_trigger_epoch trigger_age
        if last_trigger_epoch=$(date -u -d "$last_trigger" +%s 2>/dev/null); then
            trigger_age=$(( (now_epoch - last_trigger_epoch) / 3600 ))
            [[ "$trigger_age" -lt "$SCAN_STALE_HOURS" ]] && return 0
        fi
    fi

    joi handoff --as svc_a1_bug_fix_loop feedback-scanner --in "$SCANNER_TID" \
        --message "feedback_scan refresh：上次扫描已超过 ${SCAN_STALE_HOURS} 小时，请刷新并维护本地 bugs.json / others.json；扫描记录只写本 thread。" >/dev/null 2>&1 || true
    state=$(jq -c --arg ts "$TICK_AT" '.last_scan_refresh_triggered_at = $ts' <<<"$state")
}

update_bug_queue_item() {
    local fid="$1" patch="$2" tmp
    [[ -f "$BUGS_FILE" ]] || return 0
    tmp=$(mktemp)
    jq --arg fid "$fid" --argjson patch "$patch" '
      .items = ((.items // []) | map(if (.feedback_id // .id // "" | tostring) == $fid then . + $patch else . end))
    ' "$BUGS_FILE" >"$tmp" && mv "$tmp" "$BUGS_FILE"
}

find_delivery_thread_for_feedback() {
    local fid="$1" list found
    [[ "$DRY_RUN" -eq 1 ]] && return 0
    joi_avail || return 0

    find_in_thread_json() {
      jq -r --arg fid "$fid" '
      (.threads // .items // .)[]?
      | (.title // .name // "") as $title
      | select($title | startswith("[bugfixloop:" + $fid + "]"))
      | (.id // .thread_id // .thread.id // empty)
    ' 2>/dev/null | head -n 1
    }

    list=$(joi thread list --channel "$CHANNEL_ID" --json 2>/dev/null) || list=""
    found=$(find_in_thread_json <<<"$list")
    if [[ -n "$found" ]]; then
        printf '%s\n' "$found"
        return 0
    fi

    list=$(joi thread archive-list --channel "$CHANNEL_ID" --json 2>/dev/null) || list=""
    find_in_thread_json <<<"$list"
}

find_bugfix_thread_for_feedback() {
    local fid="$1" list found
    [[ "$DRY_RUN" -eq 1 ]] && return 0
    joi_avail || return 0

    find_in_thread_json() {
      jq -r --arg fid "$fid" '
      (.threads // .items // .)[]?
      | (.title // .name // "") as $title
      | select($title == ("bugfix-" + $fid) or ($title | startswith("[bugfix:" + $fid + "]")))
      | (.id // .thread_id // .thread.id // empty)
    ' 2>/dev/null | head -n 1
    }

    list=$(joi thread list --channel "$CHANNEL_ID" --json 2>/dev/null) || list=""
    found=$(find_in_thread_json <<<"$list")
    if [[ -n "$found" ]]; then
        printf '%s\n' "$found"
        return 0
    fi

    list=$(joi thread archive-list --channel "$CHANNEL_ID" --json 2>/dev/null) || list=""
    find_in_thread_json <<<"$list"
}

mr_is_merged() {
    local tid="$1"
    local body
    body=$(fetch_latest_artifact_body "$tid" "mr-merged.v1") || true
    if [[ -n "$body" ]]; then printf 'true'; else printf 'false'; fi
}

mr_is_final_merged() {
    local tid="$1"
    local body
    body=$(fetch_latest_artifact_body "$tid" "mr-final.v1") || true
    if [[ -n "$body" ]] && jq -e '(.terminal_kind // .kind // "") == "merged"' >/dev/null 2>&1 <<<"$body"; then
        printf 'true'
        return
    fi
    if [[ "$DRY_RUN" -eq 1 ]] || ! joi_avail; then
        printf 'false'
        return
    fi
    local list
    list=$(joi event list --in "$tid" --limit 100 --json 2>/dev/null) || { printf 'false'; return; }
    if jq -e '
        (.events // .items // .)[]? as $event
        | ($event.payload.text // "") as $text
        | ($event.actorId // $event.actor_id // "") as $actor
        | select($actor == "mr-watcher")
        | select(
            (($text | contains("terminal=true")) and ($text | test("已合并|terminal_kind[:= ]+merged|terminal_kind.*merged")))
            or ($text | test("state:[[:space:]]*merged|state: merged|MR 已合并|已合并 → router|已合并 →"))
          )
    ' >/dev/null 2>&1 <<<"$list"; then
        printf 'true'
    else
        printf 'false'
    fi
}

mr_is_final_closed() {
    local tid="$1"
    local body
    body=$(fetch_latest_artifact_body "$tid" "mr-final.v1") || true
    if [[ -n "$body" ]] && jq -e '(.terminal_kind // .kind // "") == "closed"' >/dev/null 2>&1 <<<"$body"; then
        printf 'true'
        return
    fi
    if [[ "$DRY_RUN" -eq 1 ]] || ! joi_avail; then
        printf 'false'
        return
    fi
    local list
    list=$(joi event list --in "$tid" --limit 160 --json 2>/dev/null) || { printf 'false'; return; }
    if jq -e '
        (.events // .items // .)[]? as $event
        | ($event.payload.text // "") as $text
        | ($event.actorId // $event.actor_id // "") as $actor
        | select($actor == "mr-watcher" or $actor == "actor_router" or $actor == "actor_delivery")
        | select(
            ($text | test("terminal_kind[:= ]+closed|state:[[:space:]]*closed|state: closed|MR 已关闭|已关闭/废弃|MR .*关闭|已关闭 MR|任务已完成关闭|关闭此任务|human 要求关闭"))
          )
    ' >/dev/null 2>&1 <<<"$list"; then
        printf 'true'
    else
        printf 'false'
    fi
}

feedback_is_fixed() {
    local fid="$1" status
    status=$(feedback_status_value "$fid" || true)
    if [[ "$status" == "Fixed" || "$status" == "已修复" ]]; then
        printf 'true'
    else
        printf 'false'
    fi
}

feedback_is_nonfixed_terminal() {
    local fid="$1" status
    status=$(feedback_status_value "$fid" || true)
    case "$status" in
        Closed|已关闭|Close|关闭|Won\'tfix|Won’tfix|Wontfix|Won\'t\ Fix|Won’t\ Fix|Wont\ Fix|不修复|无需修复|Not\ a\ Bug|Invalid|无效)
            printf 'true'
            ;;
        *)
            printf 'false'
            ;;
    esac
}

extract_mr_close_summary() {
    local tid="$1" list text mr_id repo branch
    if [[ "$DRY_RUN" -eq 1 ]] || ! joi_avail; then
        return 0
    fi
    list=$(joi event list --in "$tid" --limit 120 --json 2>/dev/null) || return 0
    text=$(jq -r '
      (.events // .items // .)[]?
      | select((.actorId // .actor_id // "") == "mr-watcher")
      | (.payload.text // "")
      | select(test("terminal=true|state:[[:space:]]*(merged|closed)|MR 已(合并|关闭)|terminal_kind.*(merged|closed)"))
    ' <<<"$list" 2>/dev/null | tail -n 1)
    [[ -z "$text" ]] && return 0
    mr_id=$(grep -Eo 'codereview/[0-9]+' <<<"$text" | head -n 1 | cut -d/ -f2 || true)
    [[ -z "$mr_id" ]] && mr_id=$(grep -Eo 'MR[ #!]*[0-9]+' <<<"$text" | head -n 1 | grep -Eo '[0-9]+' || true)
    repo=$(grep -Eo 'repo: [^[:space:]]+' <<<"$text" | head -n 1 | sed 's/^repo: //' || true)
    branch=$(grep -Eo 'branch: [^[:space:]]+' <<<"$text" | head -n 1 | sed 's/^branch: //' || true)
    jq -nc --arg mr "$mr_id" --arg repo "$repo" --arg branch "$branch" '{mr_id:$mr,repo:$repo,branch:$branch}'
}

extract_nonfixed_outcome() {
    local tid="$1" list
    if [[ "$DRY_RUN" -eq 1 ]] || ! joi_avail; then
        printf 'closed'
        return 0
    fi
    list=$(joi event list --in "$tid" --limit 200 --json 2>/dev/null) || { printf 'closed'; return; }
    if jq -e '(.events // .items // .)[]? | (.payload.text // "") | select(test("bugfix-rescope|reproduced_cross_repo|scope_correction|target_repos|问题真实.*仓库|责任仓库|重新.*clone-manifest"))' >/dev/null 2>&1 <<<"$list"; then
        printf 'rescope'
    elif jq -e '(.events // .items // .)[]? | (.payload.text // "") | select(test("not_reproduced|无法复现|未复现|没有复现|不复现"))' >/dev/null 2>&1 <<<"$list"; then
        printf 'not_reproduced'
    elif jq -e '(.events // .items // .)[]? | (.payload.text // "") | select(test("不是[[:space:]]*bug|不是缺陷|缺陷不成立|not a bug|误修|瞎修"))' >/dev/null 2>&1 <<<"$list"; then
        printf 'not_a_bug'
    elif jq -e '(.events // .items // .)[]? | (.payload.text // "") | select(test("已有修复|已经修复|already fixed|release .*正常|现网.*正常|当前版本.*正常|已覆盖"))' >/dev/null 2>&1 <<<"$list"; then
        printf 'already_covered'
    elif jq -e '(.events // .items // .)[]? | (.payload.text // "") | select(test("撤回|关闭此任务|human 要求关闭|放弃|废弃|withdraw"))' >/dev/null 2>&1 <<<"$list"; then
        printf 'withdrawn'
    elif jq -e '(.events // .items // .)[]? | (.payload.text // "") | select(test("不修复|无需修复|won.?t fix|wontfix"))' >/dev/null 2>&1 <<<"$list"; then
        printf 'wontfix'
    else
        printf 'closed'
    fi
}

feedback_status_value() {
    local fid="$1"
    a1 --format json project workitem get "$fid" 2>/dev/null \
      | jq -r '
          ((.status // empty) | if type == "object" then (.displayName // .name // .nameEn // empty) else . end),
          ((.fields // [])[]?
            | select(.identifier == "status" or .name == "status")
            | (.value | if type == "object" then (.displayName // .name // empty) else . end))
        ' 2>/dev/null | head -n 1
}

close_feedback_fixed() {
    local fid="$1" tid="$2" summary status mr_id repo branch msg
    [[ "$DRY_RUN" -eq 0 ]] || return 0
    a1_avail || return 0

    status=$(feedback_status_value "$fid" || true)
    if [[ "$status" == "Fixed" || "$status" == "已修复" ]]; then
        printf 'already-fixed'
        return 0
    fi

    summary=$(extract_mr_close_summary "$tid" || true)
    mr_id=$(jq -r '.mr_id // ""' <<<"${summary:-{}}" 2>/dev/null || true)
    repo=$(jq -r '.repo // ""' <<<"${summary:-{}}" 2>/dev/null || true)
    branch=$(jq -r '.branch // ""' <<<"${summary:-{}}" 2>/dev/null || true)

    msg="已完成处理：对应 MR"
    [[ -n "$repo" ]] && msg+=" ${repo}"
    [[ -n "$mr_id" ]] && msg+=" !${mr_id}"
    [[ -n "$branch" ]] && msg+="（分支 ${branch}）"
    msg+=" 已合并到目标分支。修复会随下一次版本发布生效。"

    a1 project workitem comment create "$fid" -m "$msg" >/dev/null 2>&1 || true
    if a1 project workitem update "$fid" --status Fixed >/dev/null 2>&1; then
        printf 'closed'
        return 0
    fi
    if a1 project workitem update "$fid" --status 已修复 >/dev/null 2>&1; then
        printf 'closed'
        return 0
    fi
    printf 'status-update-failed'
}

close_feedback_nonfixed() {
    local fid="$1" tid="$2" outcome="$3" summary status mr_id repo branch msg status_result
    [[ "$DRY_RUN" -eq 0 ]] || return 0
    a1_avail || return 0

    status=$(feedback_status_value "$fid" || true)
    if [[ "$(feedback_is_nonfixed_terminal "$fid")" == "true" ]]; then
        printf 'already-closed'
        return 0
    fi
    if [[ "$status" == "Fixed" || "$status" == "已修复" ]]; then
        printf 'already-fixed'
        return 0
    fi

    summary=$(extract_mr_close_summary "$tid" || true)
    mr_id=$(jq -r '.mr_id // ""' <<<"${summary:-{}}" 2>/dev/null || true)
    repo=$(jq -r '.repo // ""' <<<"${summary:-{}}" 2>/dev/null || true)
    branch=$(jq -r '.branch // ""' <<<"${summary:-{}}" 2>/dev/null || true)

    msg="经复核，此反馈本轮不按 Fixed 收口"
    case "$outcome" in
        not_a_bug) msg+="：当前证据表明原缺陷不成立。" ;;
        not_reproduced) msg+="：当前验证未能复现用户问题。" ;;
        already_covered) msg+="：当前/发布版本已有能力覆盖，无需本轮代码修复。" ;;
        withdrawn) msg+="：任务已按评审或 human 决策撤回。" ;;
        wontfix) msg+="：本轮结论为无需修复 / Won't Fix。" ;;
        *) msg+="：对应交付已关闭，未产生可合并修复。" ;;
    esac
    msg+=" 对应 MR"
    [[ -n "$repo" ]] && msg+=" ${repo}"
    [[ -n "$mr_id" ]] && msg+=" !${mr_id}"
    [[ -n "$branch" ]] && msg+="（分支 ${branch}）"
    msg+=" 已关闭或终止；不会按“下次版本发布生效”的修复口径处理。"

    a1 project workitem comment create "$fid" -m "$msg" >/dev/null 2>&1 || true
    for status_result in Won\'tfix Invalid ByDesign Worksforme Closed 已关闭 "Won't Fix" "Won’t Fix" 无需修复; do
        if a1 project workitem update "$fid" --status "$status_result" >/dev/null 2>&1; then
            printf 'closed'
            return 0
        fi
    done
    printf 'status-update-failed'
}

state=$(cat "$STATE_FILE")
tracking=$(jq -c '.tracking // {}' <<<"$state")

maybe_trigger_scan_refresh
bugs=$(fetch_bugs)

# 1. reconcile in_progress entries
for fid in $(jq -r 'keys[]' <<<"$tracking"); do
    cur_status=$(jq -r --arg k "$fid" '.[$k].fix_status' <<<"$tracking")
    [[ "$cur_status" != "in_progress" ]] && continue
    bf_tid=$(jq -r --arg k "$fid" '.[$k].bugfix_thread_id // ""' <<<"$tracking")
    delivery_tid=$(jq -r --arg k "$fid" '.[$k].delivery_thread_id // ""' <<<"$tracking")
    if [[ -z "$delivery_tid" || "$delivery_tid" == "null" ]]; then
        delivery_tid=$(find_delivery_thread_for_feedback "$fid" || true)
        if [[ -n "$delivery_tid" ]]; then
            tracking=$(jq -c --arg k "$fid" --arg tid "$delivery_tid" '.[$k].delivery_thread_id = $tid' <<<"$tracking")
        fi
    fi
    terminal_tid="$bf_tid"
    [[ -n "${delivery_tid:-}" && "$delivery_tid" != "null" ]] && terminal_tid="$delivery_tid"
    [[ -z "$bf_tid" ]] && continue
    if [[ "$(mr_is_merged "$bf_tid")" == "true" || "$(mr_is_final_merged "$bf_tid")" == "true" || "$(mr_is_merged "$terminal_tid")" == "true" || "$(mr_is_final_merged "$terminal_tid")" == "true" || "$(feedback_is_fixed "$fid")" == "true" ]]; then
        close_result=$(close_feedback_fixed "$fid" "$terminal_tid" || true)
        update_bug_queue_item "$fid" "$(jq -nc --arg ts "$TICK_AT" --arg close_result "${close_result:-skipped}" '{fix_status:"fixed", fixed_at:$ts, archived_at:$ts, feedback_close_result:$close_result}')" || true
        tracking=$(jq -c --arg k "$fid" --arg ts "$TICK_AT" --arg close_result "${close_result:-skipped}" \
            '.[$k].fix_status = "fixed"
             | .[$k].fixed_at = $ts
             | .[$k].feedback_closed_at = $ts
             | .[$k].feedback_close_result = $close_result' <<<"$tracking")
        if [[ "$DRY_RUN" -eq 0 ]] && joi_avail; then
            joi say --as svc_a1_bug_fix_loop --in "$SCANNER_TID" "[bug-fix-loop] feedback $fid 已按 MR 终态收口：已回评并更新为 Fixed（result=${close_result:-skipped}）。" >/dev/null 2>&1 || true
        fi
    elif [[ "$(mr_is_final_closed "$bf_tid")" == "true" || "$(mr_is_final_closed "$terminal_tid")" == "true" || "$(feedback_is_nonfixed_terminal "$fid")" == "true" ]]; then
        outcome=$(extract_nonfixed_outcome "$terminal_tid" || true)
        [[ -z "$outcome" ]] && outcome="closed"
        if [[ "$outcome" == "rescope" ]]; then
            tracking=$(jq -c --arg k "$fid" --arg ts "$TICK_AT" --arg delivery_tid "${delivery_tid:-}" \
                '.[$k].fix_status = "in_progress"
                 | .[$k].outcome = "rescope"
                 | .[$k].rescope_at = $ts
                 | (if $delivery_tid != "" then .[$k].delivery_thread_id = $delivery_tid else . end)' <<<"$tracking")
            update_bug_queue_item "$fid" "$(jq -nc --arg ts "$TICK_AT" --arg delivery_tid "${delivery_tid:-}" '{fix_status:"in_progress", outcome:"rescope", rescope_at:$ts} + (if $delivery_tid != "" then {delivery_thread_id:$delivery_tid} else {} end)')" || true
            if [[ "$DRY_RUN" -eq 0 ]] && joi_avail; then
                joi say --as svc_a1_bug_fix_loop --in "$SCANNER_TID" "[bug-fix-loop] feedback $fid 检测到 rescope：问题仍需在正确仓库继续处理，暂不归档、不推进下一条。" >/dev/null 2>&1 || true
            fi
            continue
        fi
        close_result=$(close_feedback_nonfixed "$fid" "$terminal_tid" "$outcome" || true)
        update_bug_queue_item "$fid" "$(jq -nc --arg ts "$TICK_AT" --arg close_result "${close_result:-skipped}" --arg outcome "$outcome" --arg delivery_tid "${delivery_tid:-}" '{fix_status:"closed", outcome:$outcome, closed_at:$ts, archived_at:$ts, feedback_close_result:$close_result} + (if $delivery_tid != "" then {delivery_thread_id:$delivery_tid} else {} end)')" || true
        tracking=$(jq -c --arg k "$fid" --arg ts "$TICK_AT" --arg close_result "${close_result:-skipped}" --arg outcome "$outcome" \
            '.[$k].fix_status = "closed"
             | .[$k].outcome = $outcome
             | .[$k].closed_at = $ts
             | .[$k].archived_at = $ts
             | .[$k].feedback_closed_at = $ts
             | .[$k].feedback_close_result = $close_result' <<<"$tracking")
        if [[ "$DRY_RUN" -eq 0 ]] && joi_avail; then
            joi say --as svc_a1_bug_fix_loop --in "$SCANNER_TID" "[bug-fix-loop] feedback $fid 已按非 Fixed 终态归档：outcome=${outcome}（result=${close_result:-skipped}），继续处理下一条队列。" >/dev/null 2>&1 || true
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
            existing_tid=$(find_bugfix_thread_for_feedback "$fid" || true)
            if [[ -n "$existing_tid" ]]; then
                bf_tid="$existing_tid"
            else
                root_event=""
                if anchor=$(joi event append --as svc_a1_bug_fix_loop --channel --in "$CHANNEL_ID" \
                    --type thread.opened --text "anchor: bugfix-$fid" --json 2>/dev/null); then
                    root_event=$(jq -r '.event.id // ""' <<<"$anchor")
                fi
                if [[ -n "$root_event" ]] && out=$(joi thread create --channel "$CHANNEL_ID" \
                    --root-event "$root_event" --title "bugfix-$fid" --json 2>/dev/null); then
                    bf_tid=$(jq -r '.thread.id // .thread_id // .id // ""' <<<"$out")
                fi
            fi
            if [[ -n "$bf_tid" && "$bf_tid" != "(dry-run-thread)" ]]; then
                joi handoff --as svc_a1_bug_fix_loop actor_router --in "$bf_tid" \
                    --message "bugfix-loop next：feedback_id=${fid}
title=${title}
summary=${summary}
请按存量 bug 修复闭环推进：先让 discovery 产出 task-goal/DoD/clone-manifest，再新建/启动 delivery，后续 MR watcher 终态由 delivery 回评并更新 feedback 状态；loop 只负责监工和归档。" >/dev/null 2>&1 || true
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
        update_bug_queue_item "$fid" "$(jq -nc --arg ts "$TICK_AT" --arg tid "$bf_tid" '{fix_status:"in_progress", started_at:$ts, bugfix_thread_id:$tid}')" || true
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
    closed_total: ([$tracking[] | select(.fix_status=="closed")] | length),
    tracking: $tracking
  }'
