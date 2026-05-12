#!/usr/bin/env bash
# repo-notes/push.sh — upload an updated note record back to a1 kbase.
# Source of truth is kbase; this is the write side of the mirror.
#
# Offline contract: with --dry-run, prints planned ops and exits 0
# without invoking `a1`.

set -euo pipefail

DRY_RUN=0
REPO=""
FILE=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --repo) REPO="$2"; shift 2;;
        --file) FILE="$2"; shift 2;;
        --dry-run) DRY_RUN=1; shift;;
        -h|--help) echo "usage: push.sh --repo <repo_id> --file <path> [--dry-run]"; exit 0;;
        *) echo "push.sh: unknown arg: $1" >&2; exit 2;;
    esac
done

[[ -z "$REPO" || -z "$FILE" ]] && { echo "push.sh: --repo and --file are required" >&2; exit 2; }

if [[ "$DRY_RUN" -eq 1 ]]; then
    printf '{"schema_version":"1","producer":"repo-notes","op":"push","repo_id":"%s","file":"%s","status":"planned"}\n' "$REPO" "$FILE"
    exit 0
fi

[[ -f "$FILE" ]] || { echo "push.sh: file not found: $FILE" >&2; exit 3; }

if ! command -v a1 >/dev/null 2>&1; then
    echo "push.sh: 'a1' CLI not found on PATH" >&2
    exit 3
fi

a1 kbase repo-stg-put --repo "$REPO" --input "$FILE"

printf '{"schema_version":"1","producer":"repo-notes","op":"push","repo_id":"%s","file":"%s","status":"ok"}\n' "$REPO" "$FILE"
