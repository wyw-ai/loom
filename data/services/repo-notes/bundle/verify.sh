#!/usr/bin/env bash
# repo-notes/verify.sh — sanity check that the kbase entry for <repo>
# matches the expected clone URL. Used pre-cutover to catch
# misconfigured manifests.
#
# Offline contract: with --dry-run, treats the check as inconclusive
# (status="planned") instead of failing.

set -euo pipefail

DRY_RUN=0
REPO=""
EXPECTED=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --repo) REPO="$2"; shift 2;;
        --expected-clone-url) EXPECTED="$2"; shift 2;;
        --dry-run) DRY_RUN=1; shift;;
        -h|--help) echo "usage: verify.sh --repo <repo_id> --expected-clone-url <url> [--dry-run]"; exit 0;;
        *) echo "verify.sh: unknown arg: $1" >&2; exit 2;;
    esac
done

[[ -z "$REPO" || -z "$EXPECTED" ]] && { echo "verify.sh: --repo and --expected-clone-url are required" >&2; exit 2; }

if [[ "$DRY_RUN" -eq 1 ]]; then
    printf '{"schema_version":"1","producer":"repo-notes","op":"verify","repo_id":"%s","expected_clone_url":"%s","status":"planned"}\n' "$REPO" "$EXPECTED"
    exit 0
fi

if ! command -v a1 >/dev/null 2>&1; then
    echo "verify.sh: 'a1' CLI not found on PATH" >&2
    exit 3
fi

actual=$(a1 kbase repo-stg-get --repo "$REPO" --field clone_url 2>/dev/null || true)

if [[ "$actual" == "$EXPECTED" ]]; then
    printf '{"schema_version":"1","producer":"repo-notes","op":"verify","repo_id":"%s","status":"ok"}\n' "$REPO"
    exit 0
fi

printf '{"schema_version":"1","producer":"repo-notes","op":"verify","repo_id":"%s","status":"fail","expected":"%s","actual":"%s"}\n' "$REPO" "$EXPECTED" "$actual"
exit 4
