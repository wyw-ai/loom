#!/usr/bin/env bash
# cache-ctl.sh — channel-level shared/repos cache controller.
#
# Maintains `~/.local/share/loom/agents/channels/<chan>/shared/repos/<basename>.git` as bare
# mirrors with HEAD pinned to the upstream default branch (master / main / ...).
#
# Subcommands:
#   add     <git-url>           ensure mirror exists; clone if missing; pin HEAD
#   refresh <repo|--all>        git remote update --prune + repin HEAD
#   verify  [--json]            list mirrors with bare/HEAD/last-fetch
#   prune   <repo> [--yes]      remove a mirror
#   heal    [--all]             repair: convert non-bare → bare, repin HEAD
#
# Requires: --chan <chan_id> (or env LOOM_CHAN_ID).

set -euo pipefail

CHAN_ID="${LOOM_CHAN_ID:-}"
SHARED_ROOT=""

log()  { printf '[cache-ctl] %s\n' "$*" >&2; }
die()  { log "ERROR: $*"; exit 1; }

resolve_root() {
    [[ -n "$CHAN_ID" ]] || die "--chan <chan_id> (or env LOOM_CHAN_ID) required"
    SHARED_ROOT="${LOOM_AGENT_DATA_ROOT:-${HOME}/.local/share/loom/agents}/channels/${CHAN_ID}/shared/repos"
    mkdir -p "$SHARED_ROOT"
}

# Extract <basename>.git from a git URL.
# git@host:group/project.git  → project.git
# https://host/group/project(.git) → project.git
url_to_basename() {
    local u="$1"; local last
    last="${u##*/}"
    last="${last##*:}"
    [[ "$last" == *.git ]] || last="${last}.git"
    printf '%s' "$last"
}

detect_default_branch() {
    # Prints the branch name (no refs/heads/ prefix).
    local mirror_dir="$1"
    local ref
    ref=$(GIT_DIR="$mirror_dir" git ls-remote --symref origin HEAD 2>/dev/null \
            | awk '/^ref:/ { print $2; exit }') || true
    if [[ -n "$ref" ]]; then
        printf '%s' "${ref#refs/heads/}"
        return 0
    fi
    # Fallback: try common defaults that exist locally.
    for cand in master main develop; do
        if GIT_DIR="$mirror_dir" git rev-parse --verify "refs/heads/$cand" >/dev/null 2>&1; then
            printf '%s' "$cand"
            return 0
        fi
    done
    return 1
}

pin_head() {
    local mirror_dir="$1"
    local default
    if default=$(detect_default_branch "$mirror_dir"); then
        GIT_DIR="$mirror_dir" git symbolic-ref HEAD "refs/heads/$default"
        log "pinned HEAD of $(basename "$mirror_dir") -> refs/heads/$default"
    else
        log "WARN: cannot detect default branch for $(basename "$mirror_dir")"
    fi
}

cmd_add() {
    local url="$1"
    [[ -n "$url" ]] || die "add: <git-url> required"
    resolve_root
    local base; base=$(url_to_basename "$url")
    local target="$SHARED_ROOT/$base"

    if [[ -d "$target" ]] && GIT_DIR="$target" git rev-parse --is-bare-repository >/dev/null 2>&1; then
        log "$base already present; refreshing"
        GIT_DIR="$target" git remote update --prune
        pin_head "$target"
        printf '{"ok":true,"repo":"%s","action":"refreshed","path":"%s"}\n' "$base" "$target"
        return 0
    fi
    if [[ -e "$target" ]]; then
        log "$base exists but is not bare; backing up to ${target}.bak.$(date +%s) and re-cloning"
        mv "$target" "${target}.bak.$(date +%s)"
    fi
    git clone --mirror --quiet "$url" "$target"
    pin_head "$target"
    printf '{"ok":true,"repo":"%s","action":"added","path":"%s"}\n' "$base" "$target"
}

each_mirror() {
    local fn="$1"
    for d in "$SHARED_ROOT"/*; do
        [[ -d "$d" ]] || continue
        "$fn" "$d"
    done
}

cmd_refresh() {
    resolve_root
    local arg="${1:-}"
    if [[ "$arg" == "--all" || -z "$arg" ]]; then
        each_mirror _refresh_one
    else
        local target="$SHARED_ROOT/$arg"
        [[ "$arg" == *.git ]] || target="$SHARED_ROOT/${arg}.git"
        [[ -d "$target" ]] || die "no such mirror: $target"
        _refresh_one "$target"
    fi
}
_refresh_one() {
    local d="$1"
    if ! GIT_DIR="$d" git rev-parse --is-bare-repository >/dev/null 2>&1; then
        log "skipping $(basename "$d") (not bare)"
        return 0
    fi
    GIT_DIR="$d" git remote update --prune --quiet 2>&1 | sed 's/^/  /' >&2 || true
    pin_head "$d"
    printf '{"repo":"%s","status":"refreshed"}\n' "$(basename "$d")"
}

cmd_verify() {
    resolve_root
    local fmt="${1:-text}"
    local rows=()
    for d in "$SHARED_ROOT"/*; do
        [[ -d "$d" ]] || continue
        local name bare head last
        name=$(basename "$d")
        bare=$(GIT_DIR="$d" git config --bool core.bare 2>/dev/null || echo unknown)
        head=$(GIT_DIR="$d" git symbolic-ref HEAD 2>/dev/null || echo none)
        last=$(stat -c %Y "$d/FETCH_HEAD" 2>/dev/null || echo 0)
        rows+=("$name|$bare|$head|$last")
    done
    if [[ "$fmt" == "--json" ]]; then
        printf '['
        local first=1
        for r in "${rows[@]}"; do
            IFS='|' read -r n b h l <<<"$r"
            [[ $first -eq 1 ]] || printf ','
            printf '{"repo":"%s","bare":"%s","head":"%s","last_fetch_ts":%s}' "$n" "$b" "$h" "$l"
            first=0
        done
        printf ']\n'
    else
        printf '%-40s %-6s %-50s %s\n' REPO BARE HEAD LAST_FETCH
        for r in "${rows[@]}"; do
            IFS='|' read -r n b h l <<<"$r"
            local ts="-"
            [[ "$l" != "0" ]] && ts=$(date -d "@$l" '+%F %T' 2>/dev/null || echo "$l")
            printf '%-40s %-6s %-50s %s\n' "$n" "$b" "$h" "$ts"
        done
    fi
}

cmd_prune() {
    resolve_root
    local repo="${1:-}"; local yes="${2:-}"
    [[ -n "$repo" ]] || die "prune: <repo> required"
    [[ "$repo" == *.git ]] || repo="${repo}.git"
    local target="$SHARED_ROOT/$repo"
    [[ -d "$target" ]] || die "no such mirror: $target"
    if [[ "$yes" != "--yes" ]]; then
        die "refusing to delete without --yes"
    fi
    rm -rf "$target"
    printf '{"ok":true,"repo":"%s","action":"pruned"}\n' "$repo"
}

cmd_heal() {
    resolve_root
    for d in "$SHARED_ROOT"/*; do
        [[ -d "$d" ]] || continue
        local name; name=$(basename "$d")
        if ! GIT_DIR="$d" git rev-parse --is-bare-repository >/dev/null 2>&1; then
            log "$name is not bare; converting"
            local origin
            origin=$(cd "$d" && git config --get remote.origin.url 2>/dev/null || true)
            [[ -n "$origin" ]] || { log "WARN: $name has no origin url, skipping"; continue; }
            local backup="${d}.bak.$(date +%s)"
            mv "$d" "$backup"
            local target_name="$name"
            [[ "$target_name" == *.git ]] || target_name="${target_name}.git"
            git clone --mirror --quiet "$origin" "$SHARED_ROOT/$target_name"
            d="$SHARED_ROOT/$target_name"
        fi
        pin_head "$d"
    done
}

main() {
    local cmd="${1:-}"; shift || true
    # Parse global --chan flag wherever it appears.
    local pos=()
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --chan) CHAN_ID="$2"; shift 2;;
            *) pos+=("$1"); shift;;
        esac
    done
    set -- "${pos[@]+"${pos[@]}"}"

    case "$cmd" in
        add)     cmd_add "$@" ;;
        refresh) cmd_refresh "$@" ;;
        verify)  cmd_verify "$@" ;;
        prune)   cmd_prune "$@" ;;
        heal)    cmd_heal "$@" ;;
        ""|-h|--help)
            cat <<'EOF'
usage: cache-ctl.sh <subcommand> [args] --chan <chan_id>

  add <git-url>           clone mirror; pin HEAD to upstream default
  refresh [<repo>|--all]  git remote update --prune + repin HEAD
  verify  [--json]        list mirrors
  prune   <repo> --yes    delete one mirror
  heal    [--all]         convert non-bare → bare; repin HEAD

Repo argument may be either "<basename>" or "<basename>.git".
EOF
            ;;
        *) die "unknown subcommand: $cmd" ;;
    esac
}

main "$@"
