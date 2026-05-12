#!/usr/bin/env bash
# repo-notes/pull.sh — mirror an a1 kbase repo-note record into a
# channel-local file. The kbase remains the source of truth; this
# script writes a side-effect-free mirror suitable for skill lookup.
#
# Offline contract: with --dry-run, prints planned ops and exits 0 even
# without `a1` on PATH.

set -euo pipefail

DRY_RUN=0
REPO=""
MIRROR=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --repo) REPO="$2"; shift 2;;
        --mirror) MIRROR="$2"; shift 2;;
        --dry-run) DRY_RUN=1; shift;;
        -h|--help) echo "usage: pull.sh --repo <repo_id> --mirror <path> [--dry-run]"; exit 0;;
        *) echo "pull.sh: unknown arg: $1" >&2; exit 2;;
    esac
done

[[ -z "$REPO" || -z "$MIRROR" ]] && { echo "pull.sh: --repo and --mirror are required" >&2; exit 2; }

if [[ "$DRY_RUN" -eq 1 ]]; then
    printf '{"schema_version":"1","producer":"repo-notes","op":"pull","repo_id":"%s","mirror":"%s","status":"planned"}\n' "$REPO" "$MIRROR"
    exit 0
fi

mkdir -p "$(dirname "$MIRROR")"

if ! command -v a1 >/dev/null 2>&1; then
    echo "pull.sh: 'a1' CLI not found on PATH" >&2
    exit 3
fi

a1 kbase repo-stg-get --repo "$REPO" --output "$MIRROR"

printf '{"schema_version":"1","producer":"repo-notes","op":"pull","repo_id":"%s","mirror":"%s","status":"ok"}\n' "$REPO" "$MIRROR"
