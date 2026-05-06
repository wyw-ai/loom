#!/usr/bin/env bash
# repo-cache/sync.sh — refresh git mirrors declared in a channel
# `.joi/repos/manifest.json` into the service host data dir.
#
# Reads:    <manifest> 
# Writes:   <cache_root>/<urlencoded(repo_id)>/  (bare or --shared clone)
# Emits:    one JSON line per repo on stdout (consumed by scheduler as the
#           service event body); see schema below.
#
# Offline contract: with --dry-run, performs no network I/O and no FS
# mutations. Prints the planned actions and exits 0 even when the
# manifest is absent (treated as "no repos to sync").

set -euo pipefail

DRY_RUN=0
MANIFEST=""
CACHE_ROOT=""

usage() {
    cat <<'EOF'
usage: sync.sh --manifest <path> --cache-root <path> [--dry-run]

Refresh every repo listed in <manifest> into <cache-root>. With
--dry-run, prints a plan and exits without touching disk or network.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --manifest) MANIFEST="$2"; shift 2;;
        --cache-root) CACHE_ROOT="$2"; shift 2;;
        --dry-run) DRY_RUN=1; shift;;
        -h|--help) usage; exit 0;;
        *) echo "sync.sh: unknown arg: $1" >&2; usage >&2; exit 2;;
    esac
done

if [[ -z "$MANIFEST" || -z "$CACHE_ROOT" ]]; then
    usage >&2
    exit 2
fi

emit_event() {
    # repo_id, status, action — printed as one JSON object per line.
    printf '{"schema_version":"1","producer":"repo-cache","repo_id":%s,"status":%s,"action":%s,"cache_path":%s}\n' \
        "$(jq -nc --arg v "$1" '$v')" \
        "$(jq -nc --arg v "$2" '$v')" \
        "$(jq -nc --arg v "$3" '$v')" \
        "$(jq -nc --arg v "$4" '$v')"
}

if [[ ! -f "$MANIFEST" ]]; then
    echo "sync.sh: manifest absent ($MANIFEST); nothing to do" >&2
    emit_event "" "skip" "no-manifest" ""
    exit 0
fi

if ! command -v jq >/dev/null 2>&1; then
    echo "sync.sh: jq is required" >&2
    exit 3
fi

REPO_LINES=()
while IFS= read -r line; do
    REPO_LINES+=("$line")
done < <(jq -c '.repos[]?' "$MANIFEST")

if [[ ${#REPO_LINES[@]} -eq 0 ]]; then
    emit_event "" "skip" "empty-manifest" ""
    exit 0
fi

urlencode() {
    # Minimal: replace '/' with '%2F'; sufficient for github.com/<org>/<repo>.
    printf '%s' "$1" | sed 's|/|%2F|g'
}

for line in "${REPO_LINES[@]}"; do
    repo_id=$(jq -rn --argjson r "$line" '$r.repo_id')
    clone_url=$(jq -rn --argjson r "$line" '$r.clone_url // empty')
    target="$CACHE_ROOT/$(urlencode "$repo_id")"

    if [[ "$DRY_RUN" -eq 1 ]]; then
        emit_event "$repo_id" "planned" "fetch-or-clone" "$target"
        continue
    fi

    mkdir -p "$CACHE_ROOT"
    if [[ -d "$target/.git" || -d "$target/objects" ]]; then
        (cd "$target" && git fetch --all --prune --quiet)
        emit_event "$repo_id" "ok" "fetched" "$target"
    else
        if [[ -z "$clone_url" ]]; then
            emit_event "$repo_id" "fail" "missing-clone-url" "$target"
            continue
        fi
        git clone --mirror --quiet "$clone_url" "$target"
        emit_event "$repo_id" "ok" "cloned" "$target"
    fi
done
