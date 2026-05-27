#!/usr/bin/env bash
# Materialise a bare git repo with one seed commit on main.
# Usage: make-bare-repo.sh <dest-dir>
# Idempotent: skips work if dest-dir/HEAD already exists.

set -euo pipefail

DEST="${1:-}"
[ -n "$DEST" ] || { echo "usage: make-bare-repo.sh <dest-dir>" >&2; exit 2; }

if [ -f "$DEST/HEAD" ]; then
  echo "bare repo already present at $DEST"
  exit 0
fi

mkdir -p "$DEST"
git init --bare -b main "$DEST" >/dev/null

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
git -C "$WORK" init -b main >/dev/null
git -C "$WORK" config user.email e2e@loom.local
git -C "$WORK" config user.name "e2e"
mkdir -p "$WORK/openspec/changes"
cat > "$WORK/README.md" <<EOF
# loom-e2e seed
Synthetic repo used by scripts/e2e/. Not for production.
EOF
cat > "$WORK/openspec/AGENTS.md" <<EOF
# OpenSpec workflow (seed)
Channels: a1-auto-dev, classroom.
EOF
git -C "$WORK" add -A >/dev/null
git -C "$WORK" commit -m "seed: e2e bare repo" >/dev/null
git -C "$WORK" remote add origin "$DEST"
git -C "$WORK" push -u origin main >/dev/null 2>&1

echo "bare repo materialised: $DEST"
