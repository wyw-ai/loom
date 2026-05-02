#!/usr/bin/env bash
# repo-provision/provision.sh — thread bootstrap hook. Consumes a
# clone-manifest.json artifact (docs/artifact-contracts.md §3) and
# materializes the listed repos as worktrees / shared clones under
# <thread_workspace>/repos/.
#
# Emits one repo-provision-receipt.json on stdout (single JSON object,
# not JSON-lines) describing what was provisioned, suitable for
# `joi artifact publish`.
#
# Offline contract: with --dry-run, prints the receipt with
# status="planned" entries and exits 0, no FS or git ops.

set -euo pipefail

DRY_RUN=0
MANIFEST=""
THREAD_WS=""
CACHE_ROOT=""

usage() {
    cat <<'EOF'
usage: provision.sh --manifest <clone-manifest.json>
                    --thread-workspace <path>
                    --cache-root <path>
                    [--dry-run]
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --manifest) MANIFEST="$2"; shift 2;;
        --thread-workspace) THREAD_WS="$2"; shift 2;;
        --cache-root) CACHE_ROOT="$2"; shift 2;;
        --dry-run) DRY_RUN=1; shift;;
        -h|--help) usage; exit 0;;
        *) echo "provision.sh: unknown arg: $1" >&2; usage >&2; exit 2;;
    esac
done

[[ -z "$MANIFEST" || -z "$THREAD_WS" || -z "$CACHE_ROOT" ]] && { usage >&2; exit 2; }
command -v jq >/dev/null 2>&1 || { echo "provision.sh: jq is required" >&2; exit 3; }

if [[ ! -f "$MANIFEST" ]]; then
    echo "provision.sh: manifest not found: $MANIFEST" >&2
    exit 3
fi

task_id=$(jq -r '.task_id // ""' "$MANIFEST")
producer="repo-provision"
captured_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)

urlencode() { printf '%s' "$1" | sed 's|/|%2F|g'; }

repo_results='[]'
REPO_LINES=()
while IFS= read -r line; do
    REPO_LINES+=("$line")
done < <(jq -c '.repos[]?' "$MANIFEST")

for line in "${REPO_LINES[@]}"; do
    repo_id=$(jq -rn --argjson r "$line" '$r.repo_id')
    ref=$(jq -rn --argjson r "$line" '$r.ref // "main"')
    readonly_flag=$(jq -rn --argjson r "$line" '$r.readonly // false')
    purpose=$(jq -rn --argjson r "$line" '$r.purpose // "reference"')
    enc=$(urlencode "$repo_id")
    base=$(basename "$repo_id")
    from=$(jq -rn --argjson r "$line" --arg def "service://repo-cache/cache/$enc" '$r.from // $def')
    to=$(jq -rn --argjson r "$line" --arg def "repos/$base" '$r.to // $def')
    target="$THREAD_WS/$to"
    cache_path="$CACHE_ROOT/$enc"
    sha=""
    status="planned"

    if [[ "$DRY_RUN" -eq 0 ]]; then
        if [[ ! -d "$cache_path" ]]; then
            status="missing-cache"
        else
            mkdir -p "$(dirname "$target")"
            if [[ -d "$target/.git" || -d "$target" && -n "$(ls -A "$target" 2>/dev/null || true)" ]]; then
                status="already-present"
            else
                # Use --shared to keep deltas small; falls back to plain clone
                # when --shared is unsupported (e.g., across filesystems).
                git clone --shared --quiet --branch "$ref" "$cache_path" "$target" 2>/dev/null \
                    || git clone --quiet --branch "$ref" "$cache_path" "$target"
                status="provisioned"
            fi
            sha=$(git -C "$target" rev-parse HEAD 2>/dev/null || echo "")
        fi
    fi

    entry=$(jq -nc \
        --arg repo_id "$repo_id" \
        --arg ref "$ref" \
        --arg from "$from" \
        --arg to "$to" \
        --argjson readonly "$readonly_flag" \
        --arg purpose "$purpose" \
        --arg sha "$sha" \
        --arg status "$status" \
        '{repo_id:$repo_id, ref:$ref, from:$from, to:$to, readonly:$readonly, purpose:$purpose, sha:$sha, status:$status}')
    repo_results=$(jq -nc --argjson acc "$repo_results" --argjson e "$entry" '$acc + [$e]')
done

jq -nc \
    --arg schema_version "1" \
    --arg producer "$producer" \
    --arg task_id "$task_id" \
    --arg manifest "$MANIFEST" \
    --arg thread_workspace "$THREAD_WS" \
    --arg captured_at "$captured_at" \
    --argjson repos "$repo_results" \
    '{schema_version:$schema_version, producer:$producer, task_id:$task_id, manifest:$manifest, thread_workspace:$thread_workspace, repos:$repos, captured_at:$captured_at}'
