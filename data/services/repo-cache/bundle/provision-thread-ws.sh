#!/usr/bin/env bash
# provision-thread-ws.sh — set up a fresh per-thread workspace.
#
# Reads a clone-manifest (JSON) and creates ${HOME}/joi-workspaces/thread/<thread_id>/repos/<basename>/
# for every entry. Behavior per entry:
#   - "worktree":  fresh git clone using the channel's bare mirror as
#                  --reference, then `git checkout -B <task_branch>
#                  origin/<HEAD>` (or origin/<pickup_branch> if pickup).
#   - "ro_link":   read-only clone of the mirror at upstream default branch.
#
# Manifest schema (clone-manifest v2):
# {
#   "schema_version": "2",
#   "thread_id": "thread_xxx",
#   "channel_id": "chan_xxx",
#   "task_branch": "feat/foo",          # default branch for "worktree" entries
#   "pickup": false,                    # if true: do not create new branch,
#                                       # checkout existing remote pickup_branch
#                                       # repos[].pickup_branch overrides the
#                                       # global pickup_branch for multi-repo
#                                       # pickup tasks.
#   "repos": [
#     { "repo": "aone/a1", "role": "worktree", "url": "git@.../a1.git" },
#     { "repo": "aone/a1-server", "role": "worktree" },
#     { "repo": "aone/aone-pipeline", "role": "ro_link" },
#     ...
#   ]
# }
#
# Exit non-zero on any error.

set -euo pipefail

MANIFEST=""
CHAN_ID="${JOI_CHAN_ID:-}"

usage() {
    cat <<'EOF'
usage: provision-thread-ws.sh --manifest <path> [--chan <chan_id>]
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --manifest) MANIFEST="$2"; shift 2;;
        --chan)     CHAN_ID="$2"; shift 2;;
        -h|--help)  usage; exit 0;;
        *) echo "provision: unknown arg: $1" >&2; usage >&2; exit 2;;
    esac
done

[[ -f "$MANIFEST" ]] || { echo "manifest not found: $MANIFEST" >&2; exit 2; }
command -v jq >/dev/null || { echo "jq required" >&2; exit 3; }

THREAD_ID=$(jq -r '.thread_id' "$MANIFEST")
CHAN_ID=${CHAN_ID:-$(jq -r '.channel_id // empty' "$MANIFEST")}
TASK_BRANCH=$(jq -r '.task_branch // empty' "$MANIFEST")
PICKUP=$(jq -r '.pickup // false' "$MANIFEST")
PICKUP_BRANCH=$(jq -r '.pickup_branch // empty' "$MANIFEST")

[[ -n "$THREAD_ID" ]] || { echo "manifest missing thread_id" >&2; exit 2; }
[[ -n "$CHAN_ID" ]]   || { echo "missing channel_id (manifest or --chan)" >&2; exit 2; }

SHARED_ROOT="${HOME}/.agentx/channels/${CHAN_ID}/shared/repos"
WS_ROOT="${HOME}/joi-workspaces/thread/${THREAD_ID}/repos"
mkdir -p "$WS_ROOT"

basename_of() {
    local repo="$1"; local last="${repo##*/}"
    [[ "$last" == *.git ]] && last="${last%.git}"
    printf '%s' "$last"
}

emit() { printf '%s\n' "$1"; }

resolve_default() {
    local mirror="$1"
    local r
    r=$(GIT_DIR="$mirror" git ls-remote --symref origin HEAD 2>/dev/null \
            | awk '/^ref:/ { print $2; exit }') || true
    if [[ -n "$r" ]]; then printf '%s' "${r#refs/heads/}"; return 0; fi
    for cand in master main develop; do
        GIT_DIR="$mirror" git rev-parse --verify "refs/heads/$cand" >/dev/null 2>&1 \
            && { printf '%s' "$cand"; return 0; }
    done
    return 1
}

provision_one() {
    local repo="$1"; local role="$2"; local url="$3"; local repo_pickup_branch="${4:-}"
    local base; base=$(basename_of "$repo")
    local mirror="$SHARED_ROOT/${base}.git"
    local dest="$WS_ROOT/$base"

    if [[ ! -d "$mirror" ]]; then
        if [[ -n "$url" ]]; then
            echo "[provision] mirror missing for $repo; bootstrapping shared mirror first" >&2
            git clone --mirror --quiet "$url" "$mirror"
        else
            echo "[provision] no mirror and no clone URL for $repo" >&2
            return 1
        fi
    fi

    local default
    default=$(resolve_default "$mirror") || { echo "[provision] cannot resolve default for $repo" >&2; return 1; }

    rm -rf "$dest"

    if [[ "$role" == "ro_link" ]]; then
        # Lightweight RO clone: --shared so objects are shared with the bare mirror.
        git clone --shared --branch "$default" --quiet "$mirror" "$dest"
        chmod -R a-w "$dest" 2>/dev/null || true
        emit "{\"repo\":\"$repo\",\"role\":\"ro_link\",\"branch\":\"$default\",\"path\":\"$dest\"}"
        return 0
    fi

    # worktree role: clone + reset upstream URL + checkout target branch.
    git clone --reference "$mirror" --dissociate --quiet "$mirror" "$dest"
    pushd "$dest" >/dev/null

    # Replace origin URL with upstream so push works against gitlab, not the mirror.
    local upstream
    upstream=$(GIT_DIR="$mirror" git config --get remote.origin.url || echo "")
    [[ -n "$upstream" ]] && git remote set-url origin "$upstream"
    git fetch origin --prune --quiet

    local checkout_branch
    local effective_pickup_branch="${repo_pickup_branch:-$PICKUP_BRANCH}"
    if [[ "$PICKUP" == "true" ]]; then
        # Pick up an in-progress branch.
        if [[ -z "$effective_pickup_branch" ]]; then
            echo "[provision] pickup=true requires pickup_branch for $repo" >&2
            return 1
        fi
        if git rev-parse --verify "origin/$effective_pickup_branch" >/dev/null 2>&1; then
            git checkout -B "$effective_pickup_branch" "origin/$effective_pickup_branch"
            checkout_branch="$effective_pickup_branch"
        else
            echo "[provision] pickup_branch origin/$effective_pickup_branch not found for $repo" >&2
            return 1
        fi
    elif [[ -n "$TASK_BRANCH" ]]; then
        # Fresh branch from main trunk. Existing remote task branches are only
        # valid in pickup mode; otherwise reusing them can silently import
        # unrelated work from another delivery thread.
        if git rev-parse --verify "origin/$TASK_BRANCH" >/dev/null 2>&1; then
            echo "[provision] task_branch origin/$TASK_BRANCH already exists for $repo; use pickup=true to continue it, or choose a new task_branch" >&2
            return 1
        fi
        git checkout -B "$TASK_BRANCH" "origin/$default"
        checkout_branch="$TASK_BRANCH"
    else
        git checkout -B "$default" "origin/$default"
        checkout_branch="$default"
    fi
    popd >/dev/null

    emit "{\"repo\":\"$repo\",\"role\":\"worktree\",\"branch\":\"$checkout_branch\",\"trunk\":\"$default\",\"path\":\"$dest\"}"
}

# Iterate manifest entries.
jq -c '.repos[]' "$MANIFEST" | while read -r entry; do
    repo=$(jq -rn --argjson e "$entry" '$e.repo')
    role=$(jq -rn --argjson e "$entry" '$e.mode // $e.role // "worktree"')
    url=$(jq -rn  --argjson e "$entry" '$e.url // empty')
    repo_pickup_branch=$(jq -rn --argjson e "$entry" '$e.pickup_branch // empty')
    provision_one "$repo" "$role" "$url" "$repo_pickup_branch"
done

echo "{\"ok\":true,\"thread_id\":\"$THREAD_ID\",\"workspace\":\"$WS_ROOT\"}"
